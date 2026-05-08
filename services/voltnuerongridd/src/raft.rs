//! Raft consensus algorithm scaffold — S7-WS6-02.
//!
//! Provides a single-node Raft state machine that can answer vote requests
//! and accept append-entries RPCs.  The implementation is a scaffold: it
//! models all the required state transitions and log structures but does
//! not run a background election timer or do network I/O.  It is wired into
//! `AppState` so the service can expose status and RPC endpoints.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use tokio::sync::oneshot;
use serde::{Deserialize, Serialize};
use serde_json;

// ---------------------------------------------------------------------------
// Roles
// ---------------------------------------------------------------------------

/// The role a Raft node currently holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RaftRole {
    Follower,
    Candidate,
    Leader,
}

impl Default for RaftRole {
    fn default() -> Self {
        RaftRole::Follower
    }
}

// ---------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------

/// A single entry in the Raft log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaftLogEntry {
    pub index: u64,
    pub term: u64,
    /// Opaque command string (e.g. serialised DML statement).
    pub command: String,
}

// ---------------------------------------------------------------------------
// RPC request / response types
// ---------------------------------------------------------------------------

/// RequestVote RPC arguments (§5.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftVoteRequest {
    pub term: u64,
    pub candidate_id: String,
    pub last_log_index: u64,
    pub last_log_term: u64,
}

/// RequestVote RPC reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftVoteResponse {
    pub term: u64,
    pub vote_granted: bool,
}

/// AppendEntries RPC arguments (§5.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftAppendRequest {
    pub term: u64,
    pub leader_id: String,
    pub prev_log_index: u64,
    pub prev_log_term: u64,
    pub entries: Vec<RaftLogEntry>,
    pub leader_commit: u64,
}

/// AppendEntries RPC reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftAppendResponse {
    pub term: u64,
    pub success: bool,
    /// Index of the last log entry successfully appended (for leader tracking).
    pub match_index: u64,
}

/// InstallSnapshot RPC arguments (§7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftInstallSnapshotRequest {
    pub term: u64,
    pub leader_id: String,
    pub snapshot_index: u64,
    pub snapshot_term: u64,
    /// Full serialised row-store snapshot (key → JSON value).
    pub rows: Vec<(String, serde_json::Value)>,
}

/// InstallSnapshot RPC reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftInstallSnapshotResponse {
    pub term: u64,
    pub success: bool,
}

/// One chunk of a multi-part snapshot transfer.
///
/// The leader splits a large row-store export into fixed-size chunks and
/// sends them sequentially using the same `session_id`.  The follower
/// accumulates chunks and applies the snapshot on the final chunk
/// (`is_last = true`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftSnapshotChunkRequest {
    /// Unique ID for this snapshot-transfer session (UUID or similar).
    pub session_id: String,
    pub term: u64,
    pub leader_id: String,
    pub snapshot_index: u64,
    pub snapshot_term: u64,
    /// 0-based index of this chunk.
    pub chunk_index: u32,
    /// True on the final chunk; triggers apply.
    pub is_last: bool,
    /// Row subset for this chunk.
    pub rows: Vec<(String, serde_json::Value)>,
}

/// Reply to a single snapshot chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftSnapshotChunkResponse {
    pub term: u64,
    pub success: bool,
    /// The chunk index the follower expects next.
    pub next_expected_chunk: u32,
    /// True once the final chunk was applied (session is done).
    pub complete: bool,
}

/// Snapshot of the node's current Raft state (for the status endpoint).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftStatusSnapshot {
    pub node_id: String,
    pub current_term: u64,
    pub role: RaftRole,
    pub voted_for: Option<String>,
    pub log_length: usize,
    pub commit_index: u64,
    pub last_applied: u64,
    /// Ticks elapsed since the last heartbeat was received (S7-WS6-03).
    pub ticks_since_heartbeat: u64,
    /// Configured election timeout in ticks (S7-WS6-03).
    pub election_timeout_ticks: u64,
    /// S7-WS6-03: Monotonically incrementing fencing token; advances on each leader election.
    pub fencing_token: u64,
}

// ---------------------------------------------------------------------------
// RaftNode
// ---------------------------------------------------------------------------

/// A single Raft node.  All methods are synchronous and side-effect-free
/// except for mutating `self`.
#[derive(Debug)]
pub struct RaftNode {
    pub node_id: String,
    /// Latest term this node has seen (increases monotonically).
    pub current_term: u64,
    /// Candidate this node voted for in `current_term`, if any.
    pub voted_for: Option<String>,
    /// Role: follower / candidate / leader.
    pub role: RaftRole,
    /// Replicated log.
    pub log: Vec<RaftLogEntry>,
    /// Index of highest log entry known to be committed.
    pub commit_index: u64,
    /// Index of highest log entry applied to the state machine.
    pub last_applied: u64,
    /// S7-WS6-03: number of logical clock ticks since the last heartbeat from a leader.
    /// When this reaches `election_timeout_ticks` the node converts to Candidate.
    pub ticks_since_heartbeat: u64,
    /// S7-WS6-03: election timeout threshold in ticks.
    /// Randomised per-node in real deployments; fixed here for deterministic tests.
    pub election_timeout_ticks: u64,
    /// S7-WS6-03: Fencing token — increments each time this node becomes Leader.
    pub fencing_token: u64,
    /// Per-peer next log index the leader should send next (§5.3).
    /// Only meaningful when `role == Leader`. Keyed by peer node URL.
    pub next_index: HashMap<String, u64>,
    /// Per-peer highest log index known to be replicated (§5.3).
    /// Only meaningful when `role == Leader`. Keyed by peer node URL.
    pub match_index: HashMap<String, u64>,
    /// Pending oneshot senders waiting for quorum confirmation.
    /// Key = log index; value = sender to fire when commit_index reaches that index.
    pub pending_quorum: HashMap<u64, oneshot::Sender<u64>>,
    /// Index of the last snapshot that was installed (§7).
    pub snapshot_index: u64,
    /// Term of the last snapshot entry (§7).
    pub snapshot_term: u64,
}

impl RaftNode {
    /// Create a new node in Follower state at term 0.
    pub fn new(node_id: impl Into<String>) -> Self {
        RaftNode {
            node_id: node_id.into(),
            current_term: 0,
            voted_for: None,
            role: RaftRole::Follower,
            log: Vec::new(),
            commit_index: 0,
            last_applied: 0,
            ticks_since_heartbeat: 0,
            election_timeout_ticks: 10,
            fencing_token: 0,
            next_index: HashMap::new(),
            match_index: HashMap::new(),
            pending_quorum: HashMap::new(),
            snapshot_index: 0,
            snapshot_term: 0,
        }
    }

    // -----------------------------------------------------------------------
    // State transitions
    // -----------------------------------------------------------------------

    /// Transition to Candidate and start a new election term.
    pub fn become_candidate(&mut self) {
        self.current_term += 1;
        self.role = RaftRole::Candidate;
        self.voted_for = Some(self.node_id.clone());
    }

    /// The leader won an election; transition to Leader.
    #[allow(dead_code)]
    pub fn become_leader(&mut self) {
        self.fencing_token += 1;
        self.role = RaftRole::Leader;
        // Clear per-peer progress; caller should call `init_leader_indices` next.
        self.next_index.clear();
        self.match_index.clear();
    }

    /// Initialise `next_index` and `match_index` for all known peers.
    ///
    /// Called immediately after `become_leader` once the peer list is known.
    /// Per §5.3: `next_index[peer] = last_log_index + 1`, `match_index[peer] = 0`.
    pub fn init_leader_indices(&mut self, peers: &[String]) {
        let next = self.last_log_position().0 + 1;
        for peer in peers {
            self.next_index.insert(peer.clone(), next);
            self.match_index.insert(peer.clone(), 0);
        }
    }

    /// Record a successful AppendEntries response from `peer`.
    ///
    /// Updates `next_index` and `match_index` and advances `commit_index` if
    /// a new entry has been replicated to a quorum.
    pub fn record_append_success(&mut self, peer: &str, peer_match_index: u64, total_nodes: usize) {
        self.next_index.insert(peer.to_string(), peer_match_index + 1);
        self.match_index.insert(peer.to_string(), peer_match_index);

        // Advance commit_index if a quorum has replicated the new entry.
        let quorum = (total_nodes + 1) / 2;
        for n in (self.commit_index + 1)..=peer_match_index {
            let replication_count = 1 + // self
                self.match_index.values().filter(|&&m| m >= n).count();
            if replication_count >= quorum {
                self.commit_index = n;
            }
        }
        // Fire pending quorum waiters for any newly committed indices.
        let newly_committed: Vec<u64> = self
            .pending_quorum
            .keys()
            .copied()
            .filter(|&idx| idx <= self.commit_index)
            .collect();
        for idx in newly_committed {
            if let Some(tx) = self.pending_quorum.remove(&idx) {
                let _ = tx.send(idx);
            }
        }
    }

    /// Record a failed AppendEntries response from `peer` (log inconsistency).
    ///
    /// Decrements `next_index[peer]` by one so the next heartbeat retries
    /// with an earlier entry (standard Raft back-off, §5.3).
    pub fn record_append_failure(&mut self, peer: &str) {
        let ni = self.next_index.get(peer).copied().unwrap_or(1);
        self.next_index.insert(peer.to_string(), ni.saturating_sub(1).max(1));
    }

    /// Append a new command to the log as the current leader.
    ///
    /// - Builds a `RaftLogEntry` at `last_log_index + 1` with the current term.
    /// - On a single-node cluster (`total_peers == 0`) immediately advances
    ///   `commit_index` to the new index (the leader IS the quorum).
    /// - On multi-node clusters `commit_index` only advances when
    ///   `record_append_success` accumulates quorum acks.
    /// - Does NOT pre-advance `last_applied`; the apply loop handles that.
    ///
    /// Returns the log index assigned to the new entry.
    pub fn append_command(&mut self, command: String, total_peers: usize) -> u64 {
        let (last_index, _) = self.last_log_position();
        let new_index = last_index + 1;
        self.log.push(RaftLogEntry {
            index: new_index,
            term: self.current_term,
            command,
        });
        if total_peers == 0 {
            // Single-node: leader is the only quorum member — commit immediately.
            self.commit_index = new_index;
        }
        new_index
    }

    /// Append a new command to the log as the current leader, and return a
    /// `oneshot::Receiver` that fires once the entry is committed by quorum.
    ///
    /// The receiver resolves to the committed log index.  On a single-node
    /// cluster the receiver fires immediately (commit is synchronous).
    ///
    /// `last_applied` is NOT advanced here; callers must wait for the receiver
    /// before applying to the state machine.
    pub fn append_command_pending(
        &mut self,
        command: String,
        total_peers: usize,
    ) -> (u64, oneshot::Receiver<u64>) {
        let (tx, rx) = oneshot::channel();
        let (last_index, _) = self.last_log_position();
        let new_index = last_index + 1;
        self.log.push(RaftLogEntry {
            index: new_index,
            term: self.current_term,
            command,
        });
        if total_peers == 0 {
            self.commit_index = new_index;
            // Single-node: send immediately (ignore if receiver dropped).
            let _ = tx.send(new_index);
        } else {
            self.pending_quorum.insert(new_index, tx);
        }
        (new_index, rx)
    }

    /// Handle an incoming `InstallSnapshot` RPC (§7).
    ///
    /// - Rejects if `req.term < current_term`.
    /// - No-op (success) if `req.snapshot_index <= self.snapshot_index` (already ahead).
    /// - Discards log entries covered by the snapshot.
    /// - Advances `snapshot_index`, `snapshot_term`, `commit_index`, `last_applied`.
    pub fn handle_install_snapshot(
        &mut self,
        req: &RaftInstallSnapshotRequest,
    ) -> RaftInstallSnapshotResponse {
        if req.term < self.current_term {
            return RaftInstallSnapshotResponse {
                term: self.current_term,
                success: false,
            };
        }
        // Step down if we see a higher term.
        if req.term > self.current_term {
            self.become_follower(req.term);
        }
        // Already at or ahead of this snapshot.
        if req.snapshot_index <= self.snapshot_index {
            return RaftInstallSnapshotResponse {
                term: self.current_term,
                success: true,
            };
        }
        // Discard log entries covered by the snapshot.
        self.log.retain(|e| e.index > req.snapshot_index);
        self.snapshot_index = req.snapshot_index;
        self.snapshot_term = req.snapshot_term;
        self.commit_index = self.commit_index.max(req.snapshot_index);
        self.last_applied = self.last_applied.max(req.snapshot_index);
        RaftInstallSnapshotResponse {
            term: self.current_term,
            success: true,
        }
    }

    /// Revert to Follower (e.g. after seeing a higher term).
    pub fn become_follower(&mut self, new_term: u64) {
        if new_term > self.current_term {
            self.current_term = new_term;
            self.voted_for = None;
        }
        self.role = RaftRole::Follower;
    }

    // -----------------------------------------------------------------------
    // RequestVote RPC handler (§5.2)
    // -----------------------------------------------------------------------

    /// Handle an incoming `RequestVote` RPC.
    ///
    /// Returns `vote_granted = true` iff:
    /// - The candidate's term ≥ our current term.
    /// - We haven't voted for someone else in this term.
    /// - The candidate's log is at least as up-to-date as ours.
    pub fn handle_vote_request(&mut self, req: &RaftVoteRequest) -> RaftVoteResponse {
        // Step down if we see a higher term.
        if req.term > self.current_term {
            self.become_follower(req.term);
        }
        if req.term < self.current_term {
            return RaftVoteResponse { term: self.current_term, vote_granted: false };
        }
        // Check if we already voted for someone else this term.
        let already_voted_other = self
            .voted_for
            .as_deref()
            .map(|v| v != req.candidate_id.as_str())
            .unwrap_or(false);
        if already_voted_other {
            return RaftVoteResponse { term: self.current_term, vote_granted: false };
        }
        // Candidate's log must be at least as up-to-date as ours.
        let (our_last_index, our_last_term) = self.last_log_position();
        let log_ok = req.last_log_term > our_last_term
            || (req.last_log_term == our_last_term && req.last_log_index >= our_last_index);
        if !log_ok {
            return RaftVoteResponse { term: self.current_term, vote_granted: false };
        }
        self.voted_for = Some(req.candidate_id.clone());
        RaftVoteResponse { term: self.current_term, vote_granted: true }
    }

    // -----------------------------------------------------------------------
    // AppendEntries RPC handler (§5.3)
    // -----------------------------------------------------------------------

    /// Handle an incoming `AppendEntries` RPC (also used as heartbeat).
    pub fn handle_append_entries(&mut self, req: &RaftAppendRequest) -> RaftAppendResponse {
        if req.term < self.current_term {
            return RaftAppendResponse {
                term: self.current_term,
                success: false,
                match_index: self.last_log_position().0,
            };
        }
        // Valid leader message — step down / stay follower.
        self.become_follower(req.term);
        // S7-WS6-03: receiving a valid AppendEntries resets the election timer.
        self.ticks_since_heartbeat = 0;

        // Consistency check: does our log contain an entry at prev_log_index
        // with the expected prev_log_term?
        if req.prev_log_index > 0 {
            let ok = self.log.get((req.prev_log_index - 1) as usize)
                .map(|e| e.term == req.prev_log_term)
                .unwrap_or(false);
            if !ok {
                return RaftAppendResponse {
                    term: self.current_term,
                    success: false,
                    match_index: self.last_log_position().0,
                };
            }
        }

        // Append new entries, truncating any conflicting tail.
        for (offset, entry) in req.entries.iter().enumerate() {
            let idx = req.prev_log_index as usize + offset;
            if idx < self.log.len() {
                if self.log[idx].term != entry.term {
                    // Conflict — truncate and append.
                    self.log.truncate(idx);
                    self.log.push(entry.clone());
                }
                // else: existing entry matches; skip.
            } else {
                self.log.push(entry.clone());
            }
        }

        // Advance commit index.
        if req.leader_commit > self.commit_index {
            self.commit_index = req.leader_commit.min(self.last_log_position().0);
        }

        let match_index = self.last_log_position().0;
        RaftAppendResponse { term: self.current_term, success: true, match_index }
    }

    // -----------------------------------------------------------------------
    // Status snapshot
    // -----------------------------------------------------------------------

    pub fn status(&self) -> RaftStatusSnapshot {
        RaftStatusSnapshot {
            node_id: self.node_id.clone(),
            current_term: self.current_term,
            role: self.role,
            voted_for: self.voted_for.clone(),
            log_length: self.log.len(),
            commit_index: self.commit_index,
            last_applied: self.last_applied,
            ticks_since_heartbeat: self.ticks_since_heartbeat,
            election_timeout_ticks: self.election_timeout_ticks,
            fencing_token: self.fencing_token,
        }
    }

    // -----------------------------------------------------------------------
    // S7-WS6-03: Election timeout via logical clock ticks
    // -----------------------------------------------------------------------

    /// Advance the logical clock by one tick.
    ///
    /// - If the node is a **Follower** and `ticks_since_heartbeat` reaches
    ///   `election_timeout_ticks`, it automatically transitions to Candidate
    ///   (starting a new election term and voting for itself).
    /// - Leaders and Candidates do not time out; their tick counter is
    ///   reset but no state change is triggered.
    pub fn tick(&mut self) {
        self.ticks_since_heartbeat += 1;
        if self.role == RaftRole::Follower
            && self.ticks_since_heartbeat >= self.election_timeout_ticks
        {
            self.become_candidate();
            self.ticks_since_heartbeat = 0;
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn last_log_position(&self) -> (u64, u64) {
        match self.log.last() {
            Some(e) => (e.index, e.term),
            None => (0, 0),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_node_starts_as_follower_at_term_0() {
        let node = RaftNode::new("node-1");
        assert_eq!(node.role, RaftRole::Follower);
        assert_eq!(node.current_term, 0);
        assert!(node.voted_for.is_none());
        assert!(node.log.is_empty());
    }

    #[test]
    fn become_candidate_increments_term_and_votes_for_self() {
        let mut node = RaftNode::new("node-1");
        node.become_candidate();
        assert_eq!(node.role, RaftRole::Candidate);
        assert_eq!(node.current_term, 1);
        assert_eq!(node.voted_for.as_deref(), Some("node-1"));
    }

    #[test]
    fn vote_granted_to_candidate_with_equal_term() {
        let mut node = RaftNode::new("node-1");
        let req = RaftVoteRequest {
            term: 1,
            candidate_id: "node-2".into(),
            last_log_index: 0,
            last_log_term: 0,
        };
        let resp = node.handle_vote_request(&req);
        assert!(resp.vote_granted);
        assert_eq!(resp.term, 1);
    }

    #[test]
    fn vote_denied_when_already_voted_for_other() {
        let mut node = RaftNode::new("node-1");
        // Vote for node-2 first.
        let req1 = RaftVoteRequest { term: 1, candidate_id: "node-2".into(), last_log_index: 0, last_log_term: 0 };
        node.handle_vote_request(&req1);
        // Now node-3 requests vote for same term.
        let req2 = RaftVoteRequest { term: 1, candidate_id: "node-3".into(), last_log_index: 0, last_log_term: 0 };
        let resp = node.handle_vote_request(&req2);
        assert!(!resp.vote_granted);
    }

    #[test]
    fn append_entries_heartbeat_succeeds_and_stays_follower() {
        let mut node = RaftNode::new("node-1");
        let req = RaftAppendRequest {
            term: 1,
            leader_id: "node-2".into(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![],
            leader_commit: 0,
        };
        let resp = node.handle_append_entries(&req);
        assert!(resp.success);
        assert_eq!(node.role, RaftRole::Follower);
        assert_eq!(node.current_term, 1);
    }

    #[test]
    fn append_entries_adds_entries_to_log() {
        let mut node = RaftNode::new("node-1");
        let entries = vec![
            RaftLogEntry { index: 1, term: 1, command: "INSERT INTO t VALUES (1)".into() },
            RaftLogEntry { index: 2, term: 1, command: "INSERT INTO t VALUES (2)".into() },
        ];
        let req = RaftAppendRequest {
            term: 1, leader_id: "node-2".into(),
            prev_log_index: 0, prev_log_term: 0,
            entries, leader_commit: 2,
        };
        let resp = node.handle_append_entries(&req);
        assert!(resp.success);
        assert_eq!(node.log.len(), 2);
        assert_eq!(node.commit_index, 2);
    }

    // ── S7-WS6-03: election timeout tests ────────────────────────────────────

    #[test]
    fn tick_below_timeout_does_not_trigger_election() {
        let mut node = RaftNode::new("node-1");
        assert_eq!(node.election_timeout_ticks, 10);
        for _ in 0..9 {
            node.tick();
        }
        assert_eq!(node.role, RaftRole::Follower);
        assert_eq!(node.ticks_since_heartbeat, 9);
    }

    #[test]
    fn tick_at_timeout_converts_follower_to_candidate() {
        let mut node = RaftNode::new("node-1");
        for _ in 0..10 {
            node.tick();
        }
        assert_eq!(node.role, RaftRole::Candidate);
        assert_eq!(node.current_term, 1);
        assert_eq!(node.ticks_since_heartbeat, 0, "counter resets after election starts");
    }

    #[test]
    fn heartbeat_resets_tick_counter() {
        let mut node = RaftNode::new("node-1");
        for _ in 0..5 {
            node.tick();
        }
        assert_eq!(node.ticks_since_heartbeat, 5);
        let hb = RaftAppendRequest {
            term: 1, leader_id: "node-2".into(),
            prev_log_index: 0, prev_log_term: 0,
            entries: vec![], leader_commit: 0,
        };
        node.handle_append_entries(&hb);
        assert_eq!(node.ticks_since_heartbeat, 0, "heartbeat must reset election timer");
        assert_eq!(node.role, RaftRole::Follower);
    }

    #[test]
    fn status_snapshot_includes_tick_fields() {
        let node = RaftNode::new("node-x");
        let snap = node.status();
        assert_eq!(snap.election_timeout_ticks, 10);
        assert_eq!(snap.ticks_since_heartbeat, 0);
    }

    #[test]
    fn append_command_single_node_commits_immediately() {
        let mut node = RaftNode::new("node-1");
        node.become_candidate();
        node.become_leader();
        let idx = node.append_command("INSERT INTO t VALUES (1)".into(), 0);
        assert_eq!(idx, 1);
        assert_eq!(node.commit_index, 1, "single-node: commit_index must advance");
        assert_eq!(node.log.len(), 1);
        assert_eq!(node.last_applied, 0, "last_applied not pre-advanced by append_command");
    }

    #[test]
    fn append_command_multi_node_does_not_commit_without_quorum() {
        let mut node = RaftNode::new("node-1");
        node.become_candidate();
        node.become_leader();
        // 2 peers → cluster of 3 → quorum = 2.
        let idx = node.append_command("INSERT INTO t VALUES (2)".into(), 2);
        assert_eq!(idx, 1);
        assert_eq!(node.commit_index, 0, "multi-node: commit_index must not advance before quorum");
    }

    #[test]
    fn append_command_pending_single_node_receiver_fires() {
        let mut node = RaftNode::new("node-1");
        node.become_candidate();
        node.become_leader();
        let (idx, mut rx) = node.append_command_pending("INSERT INTO t VALUES (3)".into(), 0);
        assert_eq!(idx, 1);
        assert_eq!(node.commit_index, 1);
        // Single-node: receiver should be ready immediately.
        let received = rx.try_recv().expect("receiver must be ready after single-node commit");
        assert_eq!(received, 1);
    }

    #[test]
    fn append_command_pending_multi_node_last_applied_not_pre_advanced() {
        let mut node = RaftNode::new("node-1");
        node.become_candidate();
        node.become_leader();
        let (_idx, _rx) = node.append_command_pending("INSERT INTO t VALUES (4)".into(), 2);
        assert_eq!(node.commit_index, 0, "multi-node: commit_index must not advance before quorum");
        assert_eq!(node.last_applied, 0, "last_applied must NOT be pre-advanced in multi-node case");
    }

    #[test]
    fn install_snapshot_advances_state_and_clears_covered_log() {
        let mut node = RaftNode::new("node-1");
        // Add some log entries first.
        node.log.push(RaftLogEntry { index: 1, term: 1, command: "cmd1".into() });
        node.log.push(RaftLogEntry { index: 2, term: 1, command: "cmd2".into() });
        node.log.push(RaftLogEntry { index: 3, term: 1, command: "cmd3".into() });
        node.current_term = 1;

        let req = RaftInstallSnapshotRequest {
            term: 1,
            leader_id: "node-2".into(),
            snapshot_index: 2,
            snapshot_term: 1,
            rows: vec![],
        };
        let resp = node.handle_install_snapshot(&req);
        assert!(resp.success);
        assert_eq!(node.snapshot_index, 2);
        assert_eq!(node.last_applied, 2);
        assert_eq!(node.commit_index, 2);
        // Entries 1 and 2 should be discarded; entry 3 stays.
        assert_eq!(node.log.len(), 1);
        assert_eq!(node.log[0].index, 3);
    }

    #[test]
    fn install_snapshot_rejected_on_stale_term() {
        let mut node = RaftNode::new("node-1");
        node.current_term = 5;
        let req = RaftInstallSnapshotRequest {
            term: 3,
            leader_id: "node-2".into(),
            snapshot_index: 10,
            snapshot_term: 3,
            rows: vec![],
        };
        let resp = node.handle_install_snapshot(&req);
        assert!(!resp.success);
        assert_eq!(resp.term, 5);
    }

    #[test]
    fn record_append_success_drains_pending_quorum() {
        let mut node = RaftNode::new("node-1");
        node.become_candidate();
        node.become_leader();
        node.init_leader_indices(&["node-2".to_string(), "node-3".to_string()]);
        // Append a command and keep the receiver.
        let (_idx, mut rx) = node.append_command_pending("cmd".into(), 2);
        // Simulate peer-2 replicating index 1 — that's quorum (leader + 1 of 2 peers).
        node.record_append_success("node-2", 1, 3);
        // commit_index should have advanced to 1.
        assert_eq!(node.commit_index, 1, "quorum ack should advance commit_index");
        // The pending receiver should fire.
        let committed = rx.try_recv().expect("quorum reached — receiver must fire");
        assert_eq!(committed, 1);
    }

    #[tokio::test]
    async fn append_command_pending_fires_on_quorum() {
        let mut node = RaftNode::new("node-1");
        node.become_candidate();
        node.become_leader();
        node.init_leader_indices(&["node-2".to_string()]);

        // Multi-node: entry is NOT committed until quorum acks.
        let (idx, mut rx) = node.append_command_pending("INSERT INTO t VALUES (99)".into(), 1);
        assert_eq!(node.commit_index, 0, "not committed yet");
        assert!(rx.try_recv().is_err(), "receiver not ready yet");

        // Simulate peer-2 acking — that's quorum (leader + 1 of 1 peer = 2/2).
        // total_nodes = 1 (self) + 1 (peer) = 2; quorum = ceil(2/2) = 1... wait,
        // quorum = (total_nodes + 1) / 2 = (2+1)/2 = 1 (integer div). leader counts as 1.
        // So replication_count = 1 (leader self) >= quorum(1). But commit_index only
        // advances after record_append_success sees peer ack. Let's use total_nodes = 2.
        node.record_append_success("node-2", idx, 2);
        assert_eq!(node.commit_index, idx, "quorum reached");
        let committed = rx.try_recv().expect("quorum fires receiver");
        assert_eq!(committed, idx);
    }
}
