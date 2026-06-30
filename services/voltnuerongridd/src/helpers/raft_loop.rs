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
use crate::{AppState, RaftAppendRequest, RaftAppendResponse, RaftDurableState, RaftInstallSnapshotRequest, RaftInstallSnapshotResponse, RaftLogEntry, RaftRole, RaftVoteRequest, RaftVoteResponse};

/// O-1: Inject the current span's W3C TraceContext (`traceparent` / `tracestate`)
/// into an outbound Raft reqwest request so distributed traces stitch across the
/// cluster. No-op when no OTEL propagator/active span is configured.
pub(crate) fn inject_trace_context(builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    use opentelemetry::propagation::Injector;
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;

    struct HeaderInjector(Vec<(String, String)>);
    impl Injector for HeaderInjector {
        fn set(&mut self, key: &str, value: String) {
            self.0.push((key.to_string(), value));
        }
    }

    let ctx = tracing::Span::current().context();
    let mut injector = HeaderInjector(Vec::new());
    opentelemetry::global::get_text_map_propagator(|prop| {
        prop.inject_context(&ctx, &mut injector);
    });
    let mut builder = builder;
    for (k, v) in injector.0 {
        builder = builder.header(k, v);
    }
    builder
}

// ---------------------------------------------------------------------------
// H-2: Raft durable state persistence
// ---------------------------------------------------------------------------

/// Write durable Raft state atomically to `{data_dir}/raft_meta.json`.
///
/// Uses write-to-temp + rename for atomic replacement so a crash mid-write
/// never leaves a corrupt file.  No-op when `data_dir` is empty (in-memory /
/// test mode) or if the write fails (logs a warning and continues).
pub(crate) fn persist_raft_state(data_dir: &str, node: &crate::RaftNode) {
    if data_dir.is_empty() { return; }
    let durable = RaftDurableState::from_node(node);
    let json = match serde_json::to_string(&durable) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!(target: "vng.raft", "raft persist serialize error: {e}");
            return;
        }
    };
    let dir = std::path::Path::new(data_dir);
    let tmp_path = dir.join("raft_meta.json.tmp");
    let final_path = dir.join("raft_meta.json");
    if let Err(e) = std::fs::write(&tmp_path, json.as_bytes()) {
        tracing::warn!(target: "vng.raft", "raft persist write error: {e}");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp_path, &final_path) {
        tracing::warn!(target: "vng.raft", "raft persist rename error: {e}");
    }
}

/// Load persisted durable Raft state from `{data_dir}/raft_meta.json`.
/// Returns `None` if the file does not exist or cannot be parsed.
pub(crate) fn load_raft_state(data_dir: &str) -> Option<RaftDurableState> {
    if data_dir.is_empty() { return None; }
    let path = std::path::Path::new(data_dir).join("raft_meta.json");
    let bytes = std::fs::read(&path).ok()?;
    match serde_json::from_slice::<RaftDurableState>(&bytes) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!(target: "vng.raft", "raft_meta.json parse error: {e}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// M-3: Committed write-set persistence for cross-restart serializable isolation
// ---------------------------------------------------------------------------

/// Maximum number of committed write-set entries retained across restarts.
/// Older entries are evicted so the file doesn't grow unbounded.
const MAX_PERSISTED_WRITE_SETS: usize = 1_000;

/// A single committed serializable transaction's write/read set — the minimum
/// data needed for post-restart SSI conflict detection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PersistedWriteSet {
    pub tx_id: String,
    pub written_keys: Vec<String>,
    pub read_keys: Vec<String>,
    pub committed_at_ms: u128,
}

/// Persist the last N committed serializable write-sets to
/// `{data_dir}/acid_write_sets.json`.  Older entries beyond
/// `MAX_PERSISTED_WRITE_SETS` are evicted.  No-op when `data_dir` is empty.
pub(crate) fn persist_committed_write_sets(
    data_dir: &str,
    registry: &crate::AcidTransactionRegistry,
) {
    if data_dir.is_empty() { return; }

    let mut entries: Vec<PersistedWriteSet> = registry
        .transactions
        .values()
        .filter(|e| {
            e.isolation_level == "serializable"
                && e.state == crate::AcidTxState::Committed
                && !e.written_row_keys.is_empty()
        })
        .map(|e| PersistedWriteSet {
            tx_id: e.transaction_id.clone(),
            written_keys: e.written_row_keys.iter().cloned().collect(),
            read_keys: e.read_row_keys.iter().cloned().collect(),
            committed_at_ms: e.completed_at_unix_ms.unwrap_or(0),
        })
        .collect();

    // Keep only the most recent N entries.
    entries.sort_by_key(|e| e.committed_at_ms);
    if entries.len() > MAX_PERSISTED_WRITE_SETS {
        entries.drain(..entries.len() - MAX_PERSISTED_WRITE_SETS);
    }

    let path = std::path::Path::new(data_dir).join("acid_write_sets.json");
    let tmp_path = std::path::Path::new(data_dir).join("acid_write_sets.json.tmp");
    match serde_json::to_vec(&entries) {
        Ok(bytes) => {
            if std::fs::write(&tmp_path, &bytes).is_ok() {
                let _ = std::fs::rename(&tmp_path, &path);
            }
        }
        Err(e) => tracing::warn!(target: "vng.acid", "acid_write_sets serialize error: {e}"),
    }
}

/// Load previously persisted committed write-sets from
/// `{data_dir}/acid_write_sets.json`.  Returns an empty vec if the file is
/// missing or cannot be parsed.
pub(crate) fn load_committed_write_sets(data_dir: &str) -> Vec<PersistedWriteSet> {
    if data_dir.is_empty() { return Vec::new(); }
    let path = std::path::Path::new(data_dir).join("acid_write_sets.json");
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    match serde_json::from_slice::<Vec<PersistedWriteSet>>(&bytes) {
        Ok(sets) => sets,
        Err(e) => {
            tracing::warn!(target: "vng.acid", "acid_write_sets.json parse error: {e}");
            Vec::new()
        }
    }
}

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
            let mut node = state.cluster.raft_state.lock().expect("raft tick lock");
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
            // If next_index[peer] <= snapshot_index the peer is too far behind; mark it for
            // snapshot transfer (entries = empty sentinel, prev_log_index = 0 as flag).
            //
            // H-1: prev_log_term must be the actual term of the entry at prev_log_index,
            // not a hard-coded 0.  We use RaftNode::term_at() which handles the snapshot
            // boundary (prev_log_index == snapshot_index → snapshot_term) correctly.
            let peers: Vec<String> = state.cluster.raft_peers.as_ref().clone();
            let per_peer: Vec<(String, u64, u64, Vec<RaftLogEntry>, bool)> = peers
                .iter()
                .map(|peer| {
                    let ni = *node.next_index.get(peer).unwrap_or(&(commit_idx + 1));
                    let needs_snapshot = ni <= snapshot_index && snapshot_index > 0;
                    let prev_log_idx = ni.saturating_sub(1);
                    // H-1 fix: look up the real term at prev_log_idx.
                    let prev_log_term = node.term_at(prev_log_idx);
                    let entries: Vec<RaftLogEntry> = if needs_snapshot {
                        Vec::new()
                    } else {
                        node.log.iter().filter(|e| e.index >= ni).cloned().collect()
                    };
                    (peer.clone(), prev_log_idx, prev_log_term, entries, needs_snapshot)
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
                state.runtime_config.storage.data_dir.clone(),
            )
        };
        let (became_candidate, is_leader, term, node_id,
             last_log_index, last_log_term, commit_index,
             snapshot_index, snapshot_term, per_peer, total_peers,
             data_dir) = tick_info;

        if became_candidate {
            run_election(&state, &client, term, &node_id, last_log_index, last_log_term).await;
        }

        if is_leader && tick_count % HEARTBEAT_EVERY_N_TICKS == 0 {
            fanout_heartbeat(&state, &client, term, &node_id, commit_index, snapshot_index, snapshot_term, per_peer, total_peers).await;
        }

        // Apply any newly committed log entries to the local state machine.
        apply_committed_entries(&state);

        // Periodically compact the log to bound memory usage.
        if tick_count % COMPACT_EVERY_N_TICKS == 0 {
            compact_if_needed(&state);
        }

        // H-2: Persist durable Raft state (current_term, voted_for, log) once
        // per tick.  This covers state changes from tick(), compact_if_needed(),
        // and fanout_heartbeat() responses without holding any lock during I/O.
        if !data_dir.is_empty() {
            let node = state.cluster.raft_state.lock().expect("raft persist lock");
            persist_raft_state(&data_dir, &node);
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
    let peers = state.cluster.raft_peers.as_slice();
    let total_nodes = peers.len() + 1;
    let quorum = (total_nodes + 1) / 2;
    let token = state.cluster.cluster_token.as_deref().map(str::to_string);

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
                let mut builder = inject_trace_context(client.post(&url).json(&req));
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
        let mut node = state.cluster.raft_state.lock().expect("raft leader lock");
        // Guard: only promote if we're still in the same term as a Candidate.
        if node.role == RaftRole::Candidate && node.current_term == term {
            node.become_leader();
            // Initialise per-peer progress indices (§5.3).
            let peer_urls: Vec<String> = state.cluster.raft_peers.as_ref().clone();
            node.init_leader_indices(&peer_urls);
            let node_id = node.node_id.clone();
            let new_term = node.current_term;
            drop(node);
            // O-2: audit the leadership change (Raft leader promotion).
            crate::audit_helpers::append_audit_event(
                state,
                voltnuerongrid_audit::AuditEventKind::Failover,
                &node_id,
                "raft_leader_elected",
                "ok",
                &format!("{{\"new_leader_id\":\"{}\",\"term\":{}}}", node_id.replace('"', ""), new_term),
            );
        }
    }
}


/// Send per-peer AppendEntries (or InstallSnapshot) RPCs in parallel, then
/// process responses to update `next_index` / `match_index` on the leader.
///
/// `per_peer` is `(peer_url, prev_log_index, prev_log_term, entries, needs_snapshot)`.
/// H-1: `prev_log_term` is the actual term of the entry at `prev_log_index`
/// (looked up via `RaftNode::term_at`), not a hard-coded 0.
/// When `needs_snapshot` is true the peer is too far behind; the leader sends
/// an InstallSnapshot RPC instead of AppendEntries.
/// `total_peers` is used to compute quorum when advancing `commit_index`.
async fn fanout_heartbeat(
    state: &AppState,
    client: &Client,
    term: u64,
    node_id: &str,
    commit_index: u64,
    snapshot_index: u64,
    snapshot_term: u64,
    per_peer: Vec<(String, u64, u64, Vec<RaftLogEntry>, bool)>,
    total_peers: usize,
) {
    if per_peer.is_empty() {
        return;
    }
    let token = state.cluster.cluster_token.as_deref().map(str::to_string);
    let total_nodes = total_peers + 1; // including self

    // Snapshot the row-store once — only if any peer needs a full snapshot transfer.
    let needs_any_snapshot = per_peer.iter().any(|(_, _, _, _, ns)| *ns);
    let snapshot_rows: std::collections::HashMap<String, std::collections::HashMap<String, String>> =
        if needs_any_snapshot {
            let rs = state.storage.row_store.lock().expect("row_store snapshot export lock");
            rs.export_rows_snapshot()
                .into_iter()
                .map(|(k, v)| (k, v))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

    // Two separate JoinSets: one for AppendEntries, one for InstallSnapshot.
    let mut append_set: tokio::task::JoinSet<(String, Result<RaftAppendResponse, ()>)> =
        tokio::task::JoinSet::new();
    let mut snapshot_set: tokio::task::JoinSet<(String, Result<RaftInstallSnapshotResponse, ()>)> =
        tokio::task::JoinSet::new();

    for (peer_url, prev_log_index, prev_log_term, entries, needs_snapshot) in per_peer {
        let client = client.clone();
        let token = token.clone();
        let peer_url_owned = peer_url.clone();

        if needs_snapshot {
            let url = format!("{}/api/v1/cluster/raft/install_snapshot", peer_url);
            let req = RaftInstallSnapshotRequest {
                term,
                leader_id: node_id.to_string(),
                snapshot_index,
                snapshot_term,
                rows: snapshot_rows.clone(),
            };
            snapshot_set.spawn(async move {
                let mut builder = inject_trace_context(client.post(&url).json(&req));
                if let Some(t) = &token {
                    builder = builder.header("Authorization", format!("Bearer {t}"));
                }
                let result = match builder.send().await {
                    Ok(resp) if resp.status().is_success() => {
                        resp.json::<RaftInstallSnapshotResponse>().await.map_err(|_| ())
                    }
                    _ => Err(()),
                };
                (peer_url_owned, result)
            });
        } else {
            let url = format!("{}/api/v1/cluster/raft/append", peer_url);
            let req = RaftAppendRequest {
                term,
                leader_id: node_id.to_string(),
                prev_log_index,
                // H-1 fix: use the real term of the entry at prev_log_index,
                // not the previously hard-coded 0.
                prev_log_term,
                entries,
                leader_commit: commit_index,
            };
            append_set.spawn(async move {
                let mut builder = inject_trace_context(client.post(&url).json(&req));
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
    }

    // Process AppendEntries responses.
    while let Some(join_result) = append_set.join_next().await {
        let Ok((peer_url, rpc_result)) = join_result else { continue };
        let mut node = state.cluster.raft_state.lock().expect("raft fanout response lock");
        if node.role != RaftRole::Leader || node.current_term != term {
            break;
        }
        match rpc_result {
            Ok(resp) if resp.success => {
                node.record_append_success(&peer_url, resp.match_index, total_nodes);
            }
            Ok(_) => {
                node.record_append_failure(&peer_url);
            }
            Err(_) => {}
        }
    }

    // Process InstallSnapshot responses: on success, advance next_index to
    // snapshot_index + 1 so the next heartbeat resumes normal log replication.
    while let Some(join_result) = snapshot_set.join_next().await {
        let Ok((peer_url, rpc_result)) = join_result else { continue };
        let mut node = state.cluster.raft_state.lock().expect("raft snapshot response lock");
        if node.role != RaftRole::Leader || node.current_term != term {
            break;
        }
        if let Ok(resp) = rpc_result {
            if resp.success {
                node.next_index.insert(peer_url.clone(), snapshot_index + 1);
                node.match_index.insert(peer_url, snapshot_index);
            }
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
pub(crate) fn apply_committed_entries(state: &AppState) {
    // Q4: OTEL span for Raft apply loop.
    let _span = tracing::info_span!("raft.apply_committed_entries").entered();

    // Step 1: collect the entries we need to apply (brief lock on raft_state).
    let entries_to_apply: Vec<crate::RaftLogEntry> = {
        let node = state.cluster.raft_state.lock().expect("raft apply read lock");
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

    let entry_count = entries_to_apply.len();
    let apply_start = std::time::Instant::now();

    // Step 2: apply each command to the row store (holds row_store lock per entry).
    {
        let mut rs = state.storage.row_store.lock().expect("raft apply row_store lock");
        let xid = rs.begin_xid();
        for entry in &entries_to_apply {
            apply_dml_command(&entry.command, &mut rs, xid, state);
        }
        rs.release_write_intents(xid);
    }

    let apply_duration_ms = apply_start.elapsed().as_millis();
    tracing::debug!(
        entry_count = entry_count,
        apply_duration_ms = apply_duration_ms,
        "raft.apply_committed_entries complete"
    );

    // Step 3: advance last_applied (re-acquire raft_state lock briefly).
    if let Some(last) = entries_to_apply.last() {
        let new_last_applied = {
            let mut node = state.cluster.raft_state.lock().expect("raft apply update lock");
            // Guard: only advance if still monotone (another path could have updated it).
            if last.index > node.last_applied {
                node.last_applied = last.index;
            }
            node.last_applied
        };
        // Notify any handlers waiting for linearisable confirmation.
        let _ = state.cluster.raft_last_applied_tx.send(new_last_applied);
    }
}

/// Parse a single DML command string and apply it to the row store.
///
/// Supports INSERT, UPDATE, and DELETE.  Unknown or unparseable commands are
/// silently skipped — the Raft log is the source of truth and we never want a
/// bad entry to stall the apply loop.
///
/// M-1 fix: commands may be prefixed with `"__vng_db:<name>\n"` to carry the
/// originating database scope.  The prefix is stripped before parsing and the
/// database name is passed to `wal.store_row` so Raft-applied rows land in the
/// correct per-DB column family (or in-memory namespace), matching the
/// direct-write path that uses `db_prefix_key`.
fn apply_dml_command(
    command: &str,
    rs: &mut voltnuerongrid_store::mvcc::PagedRowStore,
    xid: u64,
    state: &AppState,
) {
    // T-3: a transaction's DML is grouped into ONE Raft log entry encoded as
    //   __vng_batch:<db>\n<stmt1>\n__vng_stmt__\n<stmt2>...
    // Applying the whole batch under a single `xid` here makes the transaction
    // atomic on followers — all statements land together or (on a crash before
    // commit) none do, since Raft only applies committed entries.
    if let Some(rest) = command.strip_prefix(crate::RAFT_BATCH_PREFIX) {
        let (db, body) = match rest.find('\n') {
            Some(nl) => (&rest[..nl], &rest[nl + 1..]),
            None => ("", ""),
        };
        for stmt in body.split(crate::RAFT_BATCH_STMT_SEP) {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                apply_single_dml(db, stmt, rs, xid, state);
            }
        }
        return;
    }

    // M-1: peel off the optional `__vng_db:<name>\n` scope prefix.
    let (db, sql) = if let Some(rest) = command.strip_prefix("__vng_db:") {
        if let Some(nl) = rest.find('\n') {
            (&rest[..nl], &rest[nl + 1..])
        } else {
            // Malformed prefix — treat as unscoped.
            ("", command)
        }
    } else {
        ("", command)
    };
    apply_single_dml(db, sql, rs, xid, state);
}

/// Apply a single (db-scoped) DML statement to the row store. Shared by the
/// single-statement and T-3 batch apply paths.
fn apply_single_dml(
    db: &str,
    sql: &str,
    rs: &mut voltnuerongrid_store::mvcc::PagedRowStore,
    xid: u64,
    state: &AppState,
) {
    use crate::{extract_all_insert_rows, extract_update_row_from_sql, extract_delete_key_from_sql};

    let upper = sql.trim_start().to_ascii_uppercase();
    if upper.starts_with("INSERT") {
        for (k, d, _) in extract_all_insert_rows(sql) {
            let _ = rs.begin_write_intent(xid, &k);
            { let mut wal = state.storage.wal_engine.lock().expect("wal store_row"); wal.store_row(db, &k, xid, Some(&d)); }
            rs.insert(xid, &k, d);
        }
    } else if upper.starts_with("UPDATE") {
        if let Some((k, d)) = extract_update_row_from_sql(sql) {
            let _ = rs.begin_write_intent(xid, &k);
            { let mut wal = state.storage.wal_engine.lock().expect("wal store_row"); wal.store_row(db, &k, xid, Some(&d)); }
            rs.insert(xid, &k, d);
        }
    } else if upper.starts_with("DELETE") {
        if let Some(k) = extract_delete_key_from_sql(sql) {
            let _ = rs.begin_write_intent(xid, &k);
            rs.delete(xid, &k);
            { let mut wal = state.storage.wal_engine.lock().expect("wal store_row"); wal.store_row(db, &k, xid, None); }
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
    let mut node = state.cluster.raft_state.lock().expect("raft compact lock");
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
    use crate::{RaftNode, RaftLogEntry};

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

    // ── H-1: term_at correctness ─────────────────────────────────────────────

    /// `term_at(0)` must always return 0 (no preceding entry).
    #[test]
    fn term_at_zero_returns_zero() {
        let node = RaftNode::new("node-1");
        assert_eq!(node.term_at(0), 0);
    }

    /// `term_at(snapshot_index)` must return `snapshot_term`.
    #[test]
    fn term_at_snapshot_index_returns_snapshot_term() {
        let mut node = RaftNode::new("node-1");
        node.snapshot_index = 5;
        node.snapshot_term = 3;
        assert_eq!(node.term_at(5), 3);
    }

    /// `term_at` must look up the term from the in-memory log correctly.
    #[test]
    fn term_at_log_entry_returns_correct_term() {
        let mut node = RaftNode::new("node-1");
        node.log.push(RaftLogEntry { index: 1, term: 2, command: "cmd1".into() });
        node.log.push(RaftLogEntry { index: 2, term: 4, command: "cmd2".into() });
        assert_eq!(node.term_at(1), 2);
        assert_eq!(node.term_at(2), 4);
    }

    /// `term_at` must return 0 for indices not in the log (missing entries).
    #[test]
    fn term_at_missing_entry_returns_zero() {
        let node = RaftNode::new("node-1");
        assert_eq!(node.term_at(99), 0);
    }

    /// After compaction the log entry for `snapshot_index` is removed; `term_at`
    /// must still return the correct term via `snapshot_term`.
    #[test]
    fn term_at_after_compaction_uses_snapshot_term() {
        let mut node = RaftNode::new("node-1");
        for i in 1u64..=5 {
            node.log.push(RaftLogEntry { index: i, term: i, command: String::new() });
        }
        node.compact_log(3);
        // After compaction: snapshot_index = 3, snapshot_term = 3 (term of entry 3).
        assert_eq!(node.term_at(3), 3, "snapshot boundary must return snapshot_term");
        // Entries 4 and 5 remain in the log.
        assert_eq!(node.term_at(4), 4);
        assert_eq!(node.term_at(5), 5);
    }

    // ── H-2: persist / load round-trip ───────────────────────────────────────

    /// Round-trip: persist then load must restore current_term, voted_for, and log.
    #[test]
    fn persist_and_load_raft_state_round_trip() {
        let mut node = RaftNode::new("leader-rt");
        node.current_term = 7;
        node.voted_for = Some("peer-2".to_string());
        node.log.push(RaftLogEntry { index: 1, term: 7, command: "INSERT INTO t VALUES (1)".into() });
        node.log.push(RaftLogEntry { index: 2, term: 7, command: "DELETE FROM t WHERE id='x'".into() });
        node.snapshot_index = 0;
        node.snapshot_term = 0;

        let dir = std::env::temp_dir().join("vng_raft_rt_test");
        std::fs::create_dir_all(&dir).unwrap();
        let data_dir = dir.to_str().unwrap();

        persist_raft_state(data_dir, &node);

        let loaded = load_raft_state(data_dir)
            .expect("persisted state must be loadable");
        assert_eq!(loaded.current_term, 7);
        assert_eq!(loaded.voted_for.as_deref(), Some("peer-2"));
        assert_eq!(loaded.log.len(), 2);
        assert_eq!(loaded.log[0].command, "INSERT INTO t VALUES (1)");
        assert_eq!(loaded.log[1].command, "DELETE FROM t WHERE id='x'");

        // Cleanup.
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `persist_raft_state` with an empty data_dir must be a no-op (no panic).
    #[test]
    fn persist_no_op_on_empty_data_dir() {
        let node = RaftNode::new("node-noop");
        persist_raft_state("", &node); // must not panic
    }

    /// `load_raft_state` must return `None` when no file exists.
    #[test]
    fn load_returns_none_when_no_file() {
        let dir = std::env::temp_dir().join("vng_raft_nofile_test_xyz123");
        // Ensure the dir doesn't exist.
        let _ = std::fs::remove_dir_all(&dir);
        let result = load_raft_state(dir.to_str().unwrap());
        assert!(result.is_none());
    }

    /// `restore_durable` must correctly set current_term, voted_for, and log.
    #[test]
    fn restore_durable_sets_all_fields() {
        let mut node = RaftNode::new("follower-restore");
        let durable = crate::RaftDurableState {
            current_term: 5,
            voted_for: Some("leader-1".to_string()),
            snapshot_index: 3,
            snapshot_term: 2,
            log: vec![
                RaftLogEntry { index: 4, term: 5, command: "INSERT INTO t VALUES (4)".into() },
            ],
        };
        node.restore_durable(durable);
        assert_eq!(node.current_term, 5);
        assert_eq!(node.voted_for.as_deref(), Some("leader-1"));
        assert_eq!(node.snapshot_index, 3);
        assert_eq!(node.snapshot_term, 2);
        assert_eq!(node.log.len(), 1);
        // commit_index and last_applied fast-forwarded to snapshot_index.
        assert_eq!(node.commit_index, 3);
        assert_eq!(node.last_applied, 3);
        // Role must remain Follower (transient — not restored).
        assert_eq!(node.role, RaftRole::Follower);
    }

    // M-1: verify db-scope prefix parsing used in apply_dml_command.
    #[test]
    fn vng_db_prefix_parse_with_db() {
        let cmd = "__vng_db:mydb\nINSERT INTO t VALUES (1)";
        let (db, sql) = if let Some(rest) = cmd.strip_prefix("__vng_db:") {
            if let Some(nl) = rest.find('\n') {
                (&rest[..nl], &rest[nl + 1..])
            } else { ("", cmd) }
        } else { ("", cmd) };
        assert_eq!(db, "mydb");
        assert_eq!(sql, "INSERT INTO t VALUES (1)");
    }

    #[test]
    fn vng_db_prefix_parse_empty_db() {
        let cmd = "__vng_db:\nDELETE FROM t WHERE id='x'";
        let (db, sql) = if let Some(rest) = cmd.strip_prefix("__vng_db:") {
            if let Some(nl) = rest.find('\n') {
                (&rest[..nl], &rest[nl + 1..])
            } else { ("", cmd) }
        } else { ("", cmd) };
        assert_eq!(db, "");
        assert_eq!(sql, "DELETE FROM t WHERE id='x'");
    }

    #[test]
    fn vng_db_prefix_parse_no_prefix() {
        let cmd = "INSERT INTO t VALUES (1)";
        let (db, sql) = if let Some(rest) = cmd.strip_prefix("__vng_db:") {
            if let Some(nl) = rest.find('\n') {
                (&rest[..nl], &rest[nl + 1..])
            } else { ("", cmd) }
        } else { ("", cmd) };
        assert_eq!(db, "");
        assert_eq!(sql, "INSERT INTO t VALUES (1)");
    }
}
