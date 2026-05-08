//! Raft background loop — drives the election timer and handles peer fanout.
//!
//! One `run_raft_tick_loop` task is spawned at startup. It:
//!   1. Calls `RaftNode::tick()` every 150 ms to advance the logical clock.
//!   2. When a Follower times out and becomes a Candidate, runs an election
//!      by sending RequestVote RPCs to all configured peers.  On a single-node
//!      cluster (no peers) the node wins immediately and becomes Leader.
//!   3. While Leader, sends a periodic empty AppendEntries heartbeat to every
//!      peer every ~450 ms (3 ticks) to suppress their election timers.

use std::time::Duration;
use reqwest::Client;
use crate::{AppState, RaftAppendRequest, RaftRole, RaftVoteRequest, RaftVoteResponse};

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

        let (became_candidate, is_leader, term, node_id, last_log_index, last_log_term, commit_index) = {
            let mut node = state.raft_state.lock().expect("raft tick lock");
            let role_before = node.role;
            node.tick();
            let became_candidate =
                node.role == RaftRole::Candidate && role_before == RaftRole::Follower;
            let is_leader = node.role == RaftRole::Leader;
            let last_log_term = node.log.last().map(|e| e.term).unwrap_or(0);
            let snap = node.status();
            (
                became_candidate,
                is_leader,
                snap.current_term,
                snap.node_id.clone(),
                snap.log_length as u64,
                last_log_term,
                snap.commit_index,
            )
        };

        if became_candidate {
            run_election(&state, &client, term, &node_id, last_log_index, last_log_term).await;
        }

        if is_leader && tick_count % HEARTBEAT_EVERY_N_TICKS == 0 {
            fanout_heartbeat(&state, &client, term, &node_id, commit_index).await;
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
            join_set.spawn(async move {
                let result = client.post(&url).json(&req).send().await;
                match result {
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
        }
    }
}


/// Send empty AppendEntries (heartbeat) to all peers in parallel (fire-and-forget).
async fn fanout_heartbeat(
    state: &AppState,
    client: &Client,
    term: u64,
    node_id: &str,
    commit_index: u64,
) {
    for peer_url in state.raft_peers.iter() {
        let url = format!("{}/api/v1/cluster/raft/append", peer_url);
        let client = client.clone();
        let req = RaftAppendRequest {
            term,
            leader_id: node_id.to_string(),
            prev_log_index: commit_index,
            prev_log_term: 0,
            entries: vec![],
            leader_commit: commit_index,
        };
        tokio::spawn(async move {
            let _ = client.post(&url).json(&req).send().await;
        });
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
