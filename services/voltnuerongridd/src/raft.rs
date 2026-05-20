//! Raft consensus algorithm scaffold — S7-WS6-02.
//!
//! Provides a single-node Raft state machine that can answer vote requests
//! and accept append-entries RPCs.  The implementation is a scaffold: it
//! models all the required state transitions and log structures but does
//! not run a background election timer or do network I/O.  It is wired into
//! `AppState` so the service can expose status and RPC endpoints.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

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

/// InstallSnapshot RPC arguments (§7 — sent when a follower is too far behind).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftInstallSnapshotRequest {
    pub term: u64,
    pub leader_id: String,
    /// Last log index included in this snapshot.
    pub snapshot_index: u64,
    /// Term of `snapshot_index`.
    pub snapshot_term: u64,
    /// Full row-store snapshot: key → column-value map serialised as JSON strings.
    pub rows: std::collections::HashMap<String, std::collections::HashMap<String, String>>,
}

/// InstallSnapshot RPC reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftInstallSnapshotResponse {
    pub term: u64,
    pub success: bool,
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
    /// Last log index included in the most recent compaction snapshot.
    /// Entries with `index <= snapshot_index` have been trimmed from `log`.
    pub snapshot_index: u64,
    /// Term of the `snapshot_index` entry (needed for consistency checks after trim).
    pub snapshot_term: u64,
}

impl RaftNode {
    /// Create a new node in Follower state at term 0.
    ///
    /// `election_timeout_ticks` is randomised in [10, 20) using a simple hash
    /// of `node_id` so that nodes in the same cluster have different timeouts,
    /// reducing the probability of split votes (§5.2).
    pub fn new(node_id: impl Into<String>) -> Self {
        let id: String = node_id.into();
        let timeout = election_timeout_for(&id);
        RaftNode {
            node_id: id,
            current_term: 0,
            voted_for: None,
            role: RaftRole::Follower,
            log: Vec::new(),
            commit_index: 0,
            last_applied: 0,
            ticks_since_heartbeat: 0,
            election_timeout_ticks: timeout,
            snapshot_index: 0,
            snapshot_term: 0,
            fencing_token: 0,
            next_index: HashMap::new(),
            match_index: HashMap::new(),
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
    }

    /// Record a failed AppendEntries response from `peer` (log inconsistency).
    ///
    /// Decrements `next_index[peer]` by one so the next heartbeat retries
    /// with an earlier entry (standard Raft back-off, §5.3).
    pub fn record_append_failure(&mut self, peer: &str) {
        let ni = self.next_index.get(peer).copied().unwrap_or(1);
        self.next_index.insert(peer.to_string(), ni.saturating_sub(1).max(1));
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
            // If prev_log_index is within the snapshot, use snapshot_term for match.
            let ok = if req.prev_log_index == self.snapshot_index {
                req.prev_log_term == self.snapshot_term
            } else {
                self.log_entry_at(req.prev_log_index)
                    .map(|e| e.term == req.prev_log_term)
                    .unwrap_or(false)
            };
            if !ok {
                return RaftAppendResponse {
                    term: self.current_term,
                    success: false,
                    match_index: self.last_log_position().0,
                };
            }
        }

        // Append new entries, truncating any conflicting tail.
        // Physical index = logical_index - snapshot_index - 1.
        for entry in req.entries.iter() {
            let phys = if entry.index <= self.snapshot_index {
                continue; // already compacted; skip
            } else {
                (entry.index - self.snapshot_index - 1) as usize
            };
            if phys < self.log.len() {
                if self.log[phys].term != entry.term {
                    self.log.truncate(phys);
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
    // Leader log append (§5.3)
    // -----------------------------------------------------------------------

    /// Append a new command to the leader's log and mark it as already applied
    /// locally (the caller has written it directly to the state machine).
    ///
    /// Returns the log index assigned to the new entry.
    ///
    /// `total_peers` is the number of Raft peers (excluding self).  When
    /// `total_peers == 0` (single-node cluster) the entry is immediately
    /// committed, since the leader alone forms a quorum.
    ///
    /// Should only be called when `role == Leader`.
    pub fn append_command(&mut self, command: String, total_peers: usize) -> u64 {
        let new_index = self.last_log_position().0 + 1;
        self.log.push(RaftLogEntry {
            index: new_index,
            term: self.current_term,
            command,
        });
        // The caller applied the command to the state machine directly; skip re-apply.
        if new_index > self.last_applied {
            self.last_applied = new_index;
        }
        // Single-node cluster: leader is the quorum, commit immediately.
        if total_peers == 0 && new_index > self.commit_index {
            self.commit_index = new_index;
        }
        new_index
    }

    /// Append a new command to the leader's log for the **linearisable write
    /// path** — without pre-advancing `last_applied`.
    ///
    /// The apply loop will write the command to the state machine once the
    /// entry reaches quorum commit, and then advance `last_applied`.  The
    /// calling handler waits for `last_applied >= returned_index` before
    /// acknowledging the client.
    ///
    /// On single-node clusters, advances `commit_index` immediately (leader
    /// is quorum), so the apply loop fires on the very next tick.
    ///
    /// Should only be called when `role == Leader`.
    pub fn append_command_pending(&mut self, command: String, total_peers: usize) -> u64 {
        let new_index = self.last_log_position().0 + 1;
        self.log.push(RaftLogEntry {
            index: new_index,
            term: self.current_term,
            command,
        });
        // Single-node cluster: leader is quorum; commit immediately so the
        // apply loop can fire without waiting for a heartbeat round.
        if total_peers == 0 && new_index > self.commit_index {
            self.commit_index = new_index;
        }
        new_index
    }

    // -----------------------------------------------------------------------
    // InstallSnapshot RPC handler (§7)
    // -----------------------------------------------------------------------

    /// Handle an incoming InstallSnapshot RPC from the leader.
    ///
    /// Replaces the local log and snapshot metadata with the leader's snapshot.
    /// The caller is responsible for replacing the row-store contents with the
    /// snapshot data in `req.rows`.
    pub fn handle_install_snapshot(&mut self, req: &RaftInstallSnapshotRequest) -> RaftInstallSnapshotResponse {
        if req.term < self.current_term {
            return RaftInstallSnapshotResponse { term: self.current_term, success: false };
        }
        self.become_follower(req.term);
        self.ticks_since_heartbeat = 0;
        // Accept the snapshot only if it advances our state.
        if req.snapshot_index <= self.snapshot_index {
            return RaftInstallSnapshotResponse { term: self.current_term, success: true };
        }
        // Discard any log entries covered by the snapshot.
        self.log.retain(|e| e.index > req.snapshot_index);
        self.snapshot_index = req.snapshot_index;
        self.snapshot_term = req.snapshot_term;
        // Advance commit and apply pointers so the apply loop skips the snapshot.
        if req.snapshot_index > self.commit_index {
            self.commit_index = req.snapshot_index;
        }
        if req.snapshot_index > self.last_applied {
            self.last_applied = req.snapshot_index;
        }
        RaftInstallSnapshotResponse { term: self.current_term, success: true }
    }

    // -----------------------------------------------------------------------
    // Log compaction (§7)
    // -----------------------------------------------------------------------

    /// Discard all log entries with `index <= up_to_index`.
    ///
    /// Records the last-trimmed entry's term in `snapshot_term` so the
    /// consistency check in `handle_append_entries` still works correctly
    /// after the trim.  The caller is responsible for persisting the
    /// associated state-machine snapshot before calling this.
    ///
    /// No-op if `up_to_index <= snapshot_index` or the log is already empty.
    pub fn compact_log(&mut self, up_to_index: u64) {
        if up_to_index <= self.snapshot_index {
            return;
        }
        // Find the term of the last entry we're about to trim.
        if let Some(last_kept_term) = self.log_entry_at(up_to_index).map(|e| e.term) {
            self.snapshot_term = last_kept_term;
        } else if up_to_index >= self.last_log_position().0 {
            // up_to_index covers the entire log.
            self.snapshot_term = self.last_log_position().1;
            self.log.clear();
            self.snapshot_index = up_to_index;
            return;
        }
        self.log.retain(|e| e.index > up_to_index);
        self.snapshot_index = up_to_index;
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Look up a log entry by logical index, accounting for the snapshot offset.
    fn log_entry_at(&self, index: u64) -> Option<&RaftLogEntry> {
        if index == 0 || index <= self.snapshot_index {
            return None;
        }
        let offset = (index - self.snapshot_index - 1) as usize;
        self.log.get(offset)
    }

    fn last_log_position(&self) -> (u64, u64) {
        match self.log.last() {
            Some(e) => (e.index, e.term),
            // Log is empty but we may have a non-empty snapshot.
            None if self.snapshot_index > 0 => (self.snapshot_index, self.snapshot_term),
            None => (0, 0),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Derive a per-node election timeout in the range [10, 20) ticks by hashing
/// the node_id string.  No external crate needed — just FNV-1a fold-fold.
///
/// Different nodes get different timeouts, reducing split-vote probability
/// in even-sized clusters (§5.2 of the Raft paper).
pub(crate) fn election_timeout_for(node_id: &str) -> u64 {
    // FNV-1a 64-bit hash over the UTF-8 bytes of node_id.
    const FNV_OFFSET: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;
    let hash = node_id.bytes().fold(FNV_OFFSET, |acc, b| {
        acc.wrapping_mul(FNV_PRIME) ^ (b as u64)
    });
    10 + (hash % 10)
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
        let timeout = node.election_timeout_ticks;
        assert!((10..20).contains(&timeout));
        for _ in 0..timeout - 1 {
            node.tick();
        }
        assert_eq!(node.role, RaftRole::Follower);
        assert_eq!(node.ticks_since_heartbeat, timeout - 1);
    }

    #[test]
    fn tick_at_timeout_converts_follower_to_candidate() {
        let mut node = RaftNode::new("node-1");
        let timeout = node.election_timeout_ticks;
        for _ in 0..timeout {
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
        // election_timeout_ticks is randomised per node_id; just check range [10, 20).
        assert!((10..20).contains(&snap.election_timeout_ticks),
            "expected timeout in [10,20) but got {}", snap.election_timeout_ticks);
        assert_eq!(snap.ticks_since_heartbeat, 0);
    }

    #[test]
    fn election_timeout_differs_by_node_id() {
        let t1 = election_timeout_for("node-1");
        let t2 = election_timeout_for("node-2");
        let t3 = election_timeout_for("node-3");
        for t in [t1, t2, t3] {
            assert!((10..20).contains(&t), "timeout {t} out of range");
        }
        // Not all three should be equal (they could collide but it is astronomically unlikely).
        assert!(!(t1 == t2 && t2 == t3), "all three nodes got the same timeout");
    }

    #[test]
    fn append_command_single_node_commits_immediately() {
        let mut node = RaftNode::new("leader-1");
        node.become_candidate();
        node.become_leader();
        let idx = node.append_command("INSERT INTO t VALUES (1)".to_string(), 0);
        assert_eq!(idx, 1);
        assert_eq!(node.log.len(), 1);
        assert_eq!(node.commit_index, 1, "single-node: commit_index must advance");
        assert_eq!(node.last_applied, 1, "single-node: last_applied must advance");
    }

    #[test]
    fn append_command_multi_node_does_not_commit_without_quorum() {
        let mut node = RaftNode::new("leader-1");
        node.become_candidate();
        node.become_leader();
        let idx = node.append_command("INSERT INTO t VALUES (2)".to_string(), 2);
        assert_eq!(idx, 1);
        assert_eq!(node.log.len(), 1);
        assert_eq!(node.commit_index, 0, "multi-node: commit_index must NOT advance without quorum");
        assert_eq!(node.last_applied, 1, "caller already applied — last_applied advances");
    }

    // ─── append_command_pending (linearisable write path) ────────────────────

    /// Single-node leader: commit_index advances immediately (leader is quorum),
    /// but last_applied stays behind so the apply loop handles the state-machine write.
    #[test]
    fn append_command_pending_single_node_commits_but_does_not_apply() {
        let mut node = RaftNode::new("leader-1");
        node.become_candidate();
        node.become_leader();
        let idx = node.append_command_pending("INSERT INTO t VALUES (99)".to_string(), 0);
        assert_eq!(idx, 1);
        assert_eq!(node.log.len(), 1);
        assert_eq!(node.commit_index, 1,
            "single-node: leader is quorum — commit_index must advance");
        assert_eq!(node.last_applied, 0,
            "last_applied must NOT advance — the apply loop is responsible");
    }

    /// Multi-node leader: commit_index stays at 0 until AppendEntries
    /// acknowledgements from a quorum arrive; last_applied also stays at 0.
    #[test]
    fn append_command_pending_multi_node_waits_for_quorum() {
        let mut node = RaftNode::new("leader-1");
        node.become_candidate();
        node.become_leader();
        let idx = node.append_command_pending("INSERT INTO t VALUES (99)".to_string(), 2);
        assert_eq!(idx, 1);
        assert_eq!(node.log.len(), 1);
        assert_eq!(node.commit_index, 0,
            "multi-node: commit_index must NOT advance until quorum");
        assert_eq!(node.last_applied, 0,
            "multi-node: last_applied must also stay at 0");
    }

    /// Multiple pending entries are assigned monotonically increasing indices.
    #[test]
    fn append_command_pending_indices_are_monotone() {
        let mut node = RaftNode::new("leader-1");
        node.become_candidate();
        node.become_leader();
        let idx1 = node.append_command_pending("INSERT INTO t VALUES (1)".to_string(), 0);
        let idx2 = node.append_command_pending("INSERT INTO t VALUES (2)".to_string(), 0);
        let idx3 = node.append_command_pending("INSERT INTO t VALUES (3)".to_string(), 0);
        assert_eq!(idx1, 1);
        assert_eq!(idx2, 2);
        assert_eq!(idx3, 3, "each pending command gets a strictly higher index");
        assert_eq!(node.commit_index, 3,
            "single-node: commit_index tracks the last pending index");
        assert_eq!(node.last_applied, 0,
            "apply loop has not run — last_applied stays at 0");
    }

    #[test]
    fn install_snapshot_advances_state_and_clears_covered_log() {
        let mut node = RaftNode::new("follower-1");
        node.log.push(RaftLogEntry { index: 1, term: 1, command: "INSERT INTO t VALUES (1)".into() });
        let req = RaftInstallSnapshotRequest {
            term: 2,
            leader_id: "leader-1".into(),
            snapshot_index: 5,
            snapshot_term: 2,
            rows: std::collections::HashMap::new(),
        };
        let resp = node.handle_install_snapshot(&req);
        assert!(resp.success);
        assert_eq!(node.snapshot_index, 5);
        assert_eq!(node.snapshot_term, 2);
        assert_eq!(node.commit_index, 5);
        assert_eq!(node.last_applied, 5);
        assert!(node.log.is_empty(), "entries covered by snapshot must be discarded");
    }

    #[test]
    fn install_snapshot_rejected_on_stale_term() {
        let mut node = RaftNode::new("follower-1");
        node.current_term = 3;
        let req = RaftInstallSnapshotRequest {
            term: 2,
            leader_id: "leader-1".into(),
            snapshot_index: 5,
            snapshot_term: 2,
            rows: std::collections::HashMap::new(),
        };
        let resp = node.handle_install_snapshot(&req);
        assert!(!resp.success);
        assert_eq!(node.snapshot_index, 0, "stale snapshot must not advance state");
    }

    #[test]
    fn compact_log_trims_entries_and_updates_snapshot_fields() {
        let mut node = RaftNode::new("node-1");
        for i in 1u64..=5 {
            node.log.push(RaftLogEntry { index: i, term: 1, command: format!("cmd{i}") });
        }
        node.commit_index = 3;
        node.last_applied = 3;
        node.compact_log(3);
        assert_eq!(node.snapshot_index, 3);
        assert_eq!(node.snapshot_term, 1);
        assert_eq!(node.log.len(), 2); // entries 4 and 5 remain
        assert_eq!(node.log[0].index, 4);
    }

    #[test]
    fn compact_log_full_trim_leaves_empty_log() {
        let mut node = RaftNode::new("node-1");
        for i in 1u64..=3 {
            node.log.push(RaftLogEntry { index: i, term: 2, command: String::new() });
        }
        node.compact_log(3);
        assert!(node.log.is_empty());
        assert_eq!(node.snapshot_index, 3);
        // last_log_position should fall back to (snapshot_index, snapshot_term).
        assert_eq!(node.last_log_position(), (3, 2));
    }
}
