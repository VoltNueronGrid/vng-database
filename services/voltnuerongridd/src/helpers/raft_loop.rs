//! Raft background loop — drives the election timer and handles peer fanout.
//!
//! One `run_raft_tick_loop` task is spawned at startup. It:
//!   1. Calls `RaftNode::tick()` every 150 ms to advance the logical clock.
//!   2. When a Follower times out and becomes a Candidate, runs an election
//!      by sending RequestVote RPCs to all configured peers.  On a single-node
//!      cluster (no peers) the node wins immediately and becomes Leader.
//!   3. While Leader, sends AppendEntries (with any uncommitted log entries)
//!      to every peer every ~450 ms (3 ticks) to replicate and suppress timers.
//!
//! When `VNG_CLUSTER_TOKEN` is set, every outgoing Raft RPC carries an
//! `Authorization: Bearer <token>` header so peers can reject unauthenticated
//! intra-cluster requests.

use std::time::Duration;
use reqwest::Client;
use crate::{AppState, RaftAppendRequest, RaftAppendResponse, RaftLogEntry, RaftRole, RaftSnapshotChunkRequest, RaftVoteRequest, RaftVoteResponse};

const TICK_INTERVAL_MS: u64 = 150;
const HEARTBEAT_EVERY_N_TICKS: u64 = 3;
const PEER_TIMEOUT_MS: u64 = 100;
/// Maximum rows per snapshot chunk.  Keeps individual HTTP request bodies small.
const SNAPSHOT_CHUNK_SIZE: usize = 500;


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
            let snapshot_index = node.snapshot_index;
            let snapshot_term = node.snapshot_term;
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
                snapshot_index,
                snapshot_term,
                per_peer,
                peers.len(),
            )
        };
        let (became_candidate, is_leader, term, node_id,
             last_log_index, last_log_term, commit_index, snapshot_index, snapshot_term, per_peer, total_peers) = tick_info;

        if became_candidate {
            run_election(&state, &client, term, &node_id, last_log_index, last_log_term).await;
        }

        if is_leader && tick_count % HEARTBEAT_EVERY_N_TICKS == 0 {
            fanout_heartbeat(&state, &client, term, &node_id, commit_index, snapshot_index, snapshot_term, per_peer, total_peers).await;
        }

        // Advance last_applied up to commit_index and broadcast the new value
        // so that any waiters in sql_execute (multi-node leader path) can unblock.
        apply_committed_entries(&state);
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
/// Peers whose `next_index` has fallen behind `snapshot_index` receive a
/// chunked snapshot transfer instead of log entries.  Each chunk is
/// `SNAPSHOT_CHUNK_SIZE` rows; the final chunk carries `is_last = true`.
///
/// `per_peer` is `(peer_url, prev_log_index, entries_from_next_index)`.
/// `total_peers` is used to compute quorum when advancing `commit_index`.
async fn fanout_heartbeat(
    state: &AppState,
    client: &Client,
    term: u64,
    node_id: &str,
    commit_index: u64,
    snapshot_index: u64,
    snapshot_term: u64,
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
        // If the peer's next_index has fallen behind the snapshot boundary,
        // send a chunked snapshot instead of log entries.
        let peer_next_index = prev_log_index + 1;
        if snapshot_index > 0 && peer_next_index <= snapshot_index {
            let client = client.clone();
            let token = token.clone();
            let peer_url_clone = peer_url.clone();
            let session_id = format!("snap-{}-{}", node_id, peer_url);
            let node_id_owned = node_id.to_string();
            // Export current row store for this snapshot.
            let all_rows: Vec<(String, serde_json::Value)> = {
                let rs = state.row_store.lock().expect("row_store snapshot lock");
                rs.export_rows_snapshot()
                    .into_iter()
                    .map(|(k, v)| {
                        let json_val = serde_json::Value::Object(
                            v.iter().map(|(ck, cv)| (ck.clone(), serde_json::Value::String(cv.clone()))).collect()
                        );
                        (k, json_val)
                    })
                    .collect()
            };
            let chunks: Vec<Vec<(String, serde_json::Value)>> = all_rows
                .chunks(SNAPSHOT_CHUNK_SIZE)
                .map(|c| c.to_vec())
                .collect();
            let total_chunks = chunks.len();
            // Fire-and-forget — send all chunks sequentially in a spawned task.
            tokio::spawn(async move {
                for (i, chunk) in chunks.into_iter().enumerate() {
                    let is_last = i + 1 == total_chunks;
                    let req = RaftSnapshotChunkRequest {
                        session_id: session_id.clone(),
                        term,
                        leader_id: node_id_owned.clone(),
                        snapshot_index,
                        snapshot_term,
                        chunk_index: i as u32,
                        is_last,
                        rows: chunk,
                    };
                    let url = format!("{peer_url_clone}/api/v1/cluster/raft/install_snapshot/chunk");
                    let mut builder = client.post(&url).json(&req);
                    if let Some(t) = &token {
                        builder = builder.header("Authorization", format!("Bearer {t}"));
                    }
                    match builder.send().await {
                        Ok(resp) if resp.status().is_success() => {}
                        _ => break, // abort on first error; next tick retries from chunk 0
                    }
                }
            });
            // Do not add to join_set — snapshot fanout is fire-and-forget for this tick.
            continue;
        }

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


/// Advance `last_applied` to match `commit_index` and broadcast the new value
/// on `raft_last_applied_tx`.
///
/// Called once per tick so followers and leaders both keep `last_applied`
/// current.  The watch channel fires to unblock any waiters in `sql_execute`
/// (multi-node leader linearisable-write path).
fn apply_committed_entries(state: &AppState) {
    let mut node = state.raft_state.lock().expect("raft apply_committed lock");
    if node.last_applied >= node.commit_index {
        return; // nothing to apply
    }
    node.last_applied = node.commit_index;
    let last_applied = node.last_applied;
    drop(node);
    let _ = state.raft_last_applied_tx.send(last_applied);
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
