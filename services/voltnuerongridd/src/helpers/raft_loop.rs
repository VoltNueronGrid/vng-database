//! Raft background loop — drives the election timer and handles peer fanout.
//!
//! One `run_raft_tick_loop` task is spawned at startup. It:
//!   1. Calls `RaftNode::tick()` every 150 ms to advance the logical clock.
//!   2. When a Follower times out and becomes a Candidate, runs an election
//!      by sending RequestVote RPCs to all configured peers.  On a single-node
//!      cluster (no peers) the node wins immediately and becomes Leader.
//!   3. While Leader, sends AppendEntries (with any uncommitted log entries)
//!      to every peer every ~450 ms (3 ticks) to replicate and suppress timers.
//!   4. Every tick: applies any newly committed log entries to `PagedRowStore`
//!      (§5.3 — advance `last_applied` up to `commit_index`).
//!   5. Every `COMPACT_EVERY_N_TICKS` ticks: trims log entries up to
//!      `last_applied` to bound memory usage (§7).
//!
//! When `VNG_CLUSTER_TOKEN` is set, every outgoing Raft RPC carries an
//! `Authorization: Bearer <token>` header so peers can reject unauthenticated
//! intra-cluster requests.

use std::time::Duration;
use reqwest::Client;
use crate::{AppState, RaftAppendRequest, RaftAppendResponse, RaftLogEntry, RaftRole, RaftVoteRequest, RaftVoteResponse};

/// Trim the Raft log once the log grows beyond this many entries.
const COMPACT_LOG_THRESHOLD: usize = 500;
/// Check compaction every N ticks (~450 ms at default tick rate).
const COMPACT_EVERY_N_TICKS: u64 = 3;

const TICK_INTERVAL_MS: u64 = 150;
const HEARTBEAT_EVERY_N_TICKS: u64 = 3;
const PEER_TIMEOUT_MS: u64 = 100;


pub(crate) async fn run_raft_tick_loop(state: AppState) {
    let client = Client::builder()
        .timeout(Duration::from_millis(PEER_TIMEOUT_MS))
        .build()
        .expect("raft peer http client");

    let mut interval = tokio::time::interval(Duration::from_millis(TICK_INTERVAL_MS));
    let mut tick_count: u64 = 0;

    loop {
        interval.tick().await;
        tick_count += 1;

        // Snapshot the state we need for this tick outside the lock.
        let tick_info = {
            let mut node = state.raft_state.lock().expect("raft tick lock");
            let role_before = node.role;
            node.tick();
            let became_candidate =
                node.role == RaftRole::Candidate && role_before == RaftRole::Follower;
            let is_leader = node.role == RaftRole::Leader;
            let last_log_term = node.log.last().map(|e| e.term).unwrap_or(0);
            let snap = node.status();
            let commit_idx = snap.commit_index;
            // Per-peer entries: for each peer send only entries from next_index[peer] onward.
            // If next_index is not yet initialised for a peer (just became leader), treat as
            // commit_idx + 1 so we send all pending entries on the first heartbeat.
            let peers: Vec<String> = state.raft_peers.as_ref().clone();
            let per_peer: Vec<(String, u64, Vec<RaftLogEntry>)> = peers
                .iter()
                .map(|peer| {
                    let ni = *node.next_index.get(peer).unwrap_or(&(commit_idx + 1));
                    let entries: Vec<RaftLogEntry> = node.log
                        .iter()
                        .filter(|e| e.index >= ni)
                        .cloned()
                        .collect();
                    (peer.clone(), ni.saturating_sub(1), entries)
                })
                .collect();
            (
                became_candidate,
                is_leader,
                snap.current_term,
                snap.node_id.clone(),
                snap.log_length as u64,
                last_log_term,
                commit_idx,
                per_peer,
                peers.len(),
            )
        };
        let (became_candidate, is_leader, term, node_id,
             last_log_index, last_log_term, commit_index, per_peer, total_peers) = tick_info;

        if became_candidate {
            run_election(&state, &client, term, &node_id, last_log_index, last_log_term).await;
        }

        if is_leader && tick_count % HEARTBEAT_EVERY_N_TICKS == 0 {
            fanout_heartbeat(&state, &client, term, &node_id, commit_index, per_peer, total_peers).await;
        }

        // Apply any newly committed log entries to the local state machine.
        apply_committed_entries(&state);

        // Periodically compact the log to bound memory usage.
        if tick_count % COMPACT_EVERY_N_TICKS == 0 {
            compact_if_needed(&state);
        }
    }
}


/// Collect votes from peers; promote self to Leader if quorum is reached.
///
/// Quorum = ceil((total_nodes) / 2) where total_nodes = 1 (self) + peers.len().
/// On a single-node cluster this is 1, so the self-vote alone wins.
async fn run_election(
    state: &AppState,
    client: &Client,
    term: u64,
    node_id: &str,
    last_log_index: u64,
    last_log_term: u64,
) {
    let peers = state.raft_peers.as_slice();
    let total_nodes = peers.len() + 1;
    let quorum = (total_nodes + 1) / 2;
    let token = state.cluster_token.as_deref().map(str::to_string);

    let mut votes_granted: usize = 1; // self-vote already cast in become_candidate()

    if !peers.is_empty() {
        let req = RaftVoteRequest {
            term,
            candidate_id: node_id.to_string(),
            last_log_index,
            last_log_term,
        };

        let mut join_set = tokio::task::JoinSet::new();
        for peer_url in peers.iter() {
            let url = format!("{}/api/v1/cluster/raft/vote", peer_url);
            let client = client.clone();
            let req = req.clone();
            let token = token.clone();
            join_set.spawn(async move {
                let mut builder = client.post(&url).json(&req);
                if let Some(t) = &token {
                    builder = builder.header("Authorization", format!("Bearer {t}"));
                }
                match builder.send().await {
                    Ok(resp) if resp.status().is_success() => {
                        resp.json::<RaftVoteResponse>().await
                            .map(|r| r.vote_granted)
                            .unwrap_or(false)
                    }
                    _ => false,
                }
            });
        }

        while let Some(result) = join_set.join_next().await {
            if result.unwrap_or(false) {
                votes_granted += 1;
            }
        }
    }

    if votes_granted >= quorum {
        let mut node = state.raft_state.lock().expect("raft leader lock");
        // Guard: only promote if we're still in the same term as a Candidate.
        if node.role == RaftRole::Candidate && node.current_term == term {
            node.become_leader();
            // Initialise per-peer progress indices (§5.3).
            let peer_urls: Vec<String> = state.raft_peers.as_ref().clone();
            node.init_leader_indices(&peer_urls);
        }
    }
}


/// Send per-peer AppendEntries RPCs in parallel, then process responses to
/// update `next_index` / `match_index` on the leader.
///
/// `per_peer` is `(peer_url, prev_log_index, entries_from_next_index)`.
/// `total_peers` is used to compute quorum when advancing `commit_index`.
async fn fanout_heartbeat(
    state: &AppState,
    client: &Client,
    term: u64,
    node_id: &str,
    commit_index: u64,
    per_peer: Vec<(String, u64, Vec<RaftLogEntry>)>,
    total_peers: usize,
) {
    if per_peer.is_empty() {
        return;
    }
    let token = state.cluster_token.as_deref().map(str::to_string);
    let total_nodes = total_peers + 1; // including self

    let mut join_set: tokio::task::JoinSet<(String, Result<RaftAppendResponse, ()>)> =
        tokio::task::JoinSet::new();

    for (peer_url, prev_log_index, entries) in per_peer {
        let url = format!("{}/api/v1/cluster/raft/append", peer_url);
        let client = client.clone();
        let token = token.clone();
        let req = RaftAppendRequest {
            term,
            leader_id: node_id.to_string(),
            prev_log_index,
            prev_log_term: 0,
            entries,
            leader_commit: commit_index,
        };
        let peer_url_owned = peer_url.clone();
        join_set.spawn(async move {
            let mut builder = client.post(&url).json(&req);
            if let Some(t) = &token {
                builder = builder.header("Authorization", format!("Bearer {t}"));
            }
            let result = match builder.send().await {
                Ok(resp) if resp.status().is_success() => {
                    resp.json::<RaftAppendResponse>().await.map_err(|_| ())
                }
                _ => Err(()),
            };
            (peer_url_owned, result)
        });
    }

    while let Some(join_result) = join_set.join_next().await {
        let Ok((peer_url, rpc_result)) = join_result else { continue };
        let mut node = state.raft_state.lock().expect("raft fanout response lock");
        // Only update progress if we're still leader in the same term.
        if node.role != RaftRole::Leader || node.current_term != term {
            break;
        }
        match rpc_result {
            Ok(resp) if resp.success => {
                node.record_append_success(&peer_url, resp.match_index, total_nodes);
            }
            Ok(_) => {
                // Follower rejected — log inconsistency; back off next_index.
                node.record_append_failure(&peer_url);
            }
            Err(_) => {} // network error; next tick will retry
        }
    }
}


/// Apply committed Raft log entries to the local `PagedRowStore`.
///
/// Processes every entry in `(last_applied, commit_index]`, interprets the
/// `command` field as a raw SQL DML statement (INSERT / UPDATE / DELETE),
/// applies it to the row store, and advances `last_applied`.
///
/// This function holds at most one lock at a time to avoid deadlocks with the
/// handler paths that hold `row_store` lock while calling helpers.
fn apply_committed_entries(state: &AppState) {
    // Step 1: collect the entries we need to apply (brief lock on raft_state).
    let entries_to_apply: Vec<crate::RaftLogEntry> = {
        let node = state.raft_state.lock().expect("raft apply read lock");
        let from = node.last_applied + 1;
        let to = node.commit_index;
        if from > to {
            return;
        }
        node.log
            .iter()
            .filter(|e| e.index >= from && e.index <= to)
            .cloned()
            .collect()
    };

    if entries_to_apply.is_empty() {
        return;
    }

    // Step 2: apply each command to the row store (holds row_store lock per entry).
    {
        let mut rs = state.row_store.lock().expect("raft apply row_store lock");
        let xid = rs.begin_xid();
        for entry in &entries_to_apply {
            apply_dml_command(&entry.command, &mut rs, xid);
        }
        rs.release_write_intents(xid);
    }

    // Step 3: advance last_applied (re-acquire raft_state lock briefly).
    if let Some(last) = entries_to_apply.last() {
        let mut node = state.raft_state.lock().expect("raft apply update lock");
        // Guard: only advance if still monotone (another path could have updated it).
        if last.index > node.last_applied {
            node.last_applied = last.index;
        }
    }
}

/// Parse a single DML command string and apply it to the row store.
///
/// Supports INSERT, UPDATE, and DELETE.  Unknown or unparseable commands are
/// silently skipped — the Raft log is the source of truth and we never want a
/// bad entry to stall the apply loop.
fn apply_dml_command(
    command: &str,
    rs: &mut voltnuerongrid_store::mvcc::PagedRowStore,
    xid: u64,
) {
    use crate::{extract_all_insert_rows, extract_update_row_from_sql, extract_delete_key_from_sql};
    let upper = command.trim_start().to_ascii_uppercase();
    if upper.starts_with("INSERT") {
        for (k, d, _) in extract_all_insert_rows(command) {
            let _ = rs.begin_write_intent(xid, &k);
            rs.insert(xid, &k, d);
        }
    } else if upper.starts_with("UPDATE") {
        if let Some((k, d)) = extract_update_row_from_sql(command) {
            let _ = rs.begin_write_intent(xid, &k);
            rs.insert(xid, &k, d);
        }
    } else if upper.starts_with("DELETE") {
        if let Some(k) = extract_delete_key_from_sql(command) {
            let _ = rs.begin_write_intent(xid, &k);
            rs.delete(xid, &k);
        }
    }
    // SELECT / DDL / unknown — no-op.
}

/// Compact the Raft log if it has grown past `COMPACT_LOG_THRESHOLD` entries.
///
/// Trims all entries with `index <= last_applied` since those have already
/// been applied to the state machine.  The current `PagedRowStore` contents
/// serve as the implicit snapshot.
fn compact_if_needed(state: &AppState) {
    let mut node = state.raft_state.lock().expect("raft compact lock");
    if node.log.len() < COMPACT_LOG_THRESHOLD {
        return;
    }
    let up_to = node.last_applied;
    if up_to > node.snapshot_index {
        node.compact_log(up_to);
        tracing::debug!(
            target: "vng.raft",
            snapshot_index = up_to,
            remaining_entries = node.log.len(),
            "raft log compacted"
        );
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn quorum_for(peer_count: usize) -> usize {
        let total = peer_count + 1;
        (total + 1) / 2
    }

    #[test]
    fn single_node_quorum_is_one() {
        assert_eq!(quorum_for(0), 1);
    }

    #[test]
    fn three_node_quorum_is_two() {
        assert_eq!(quorum_for(2), 2);
    }

    #[test]
    fn five_node_quorum_is_three() {
        assert_eq!(quorum_for(4), 3);
    }
}
