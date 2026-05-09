use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Serialize;
use voltnuerongrid_auth::PrivilegeAction;
use crate::{AppState, AuthErrorResponse, SnapshotChunkSession};
use crate::{RaftAppendRequest, RaftAppendResponse, RaftInstallSnapshotRequest, RaftInstallSnapshotResponse, RaftLogEntry, RaftRole, RaftSnapshotChunkRequest, RaftSnapshotChunkResponse, RaftStatusSnapshot, RaftVoteRequest, RaftVoteResponse};
use crate::auth::require_cluster_failover_privilege;

/// Return `Ok(())` if the request carries a valid cluster token
/// (`Authorization: Bearer <VNG_CLUSTER_TOKEN>`).
///
/// Used by Raft intra-cluster endpoints so peers can authenticate without
/// needing an operator account — they just need the shared cluster secret.
fn check_cluster_token(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<(), ()> {
    let configured = match state.cluster_token.as_ref().as_deref() {
        Some(t) => t,
        None => return Err(()), // no token configured — cannot satisfy
    };
    let bearer = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if bearer == configured { Ok(()) } else { Err(()) }
}

// ─── Raft DTOs ──────────────────────────────────────────────────────────



// ─── S7-WS6-02: Raft log entries response ─────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RaftLogResponse {
    pub(crate) status: &'static str,
    pub(crate) log_length: usize,
    pub(crate) commit_index: u64,
    pub(crate) entries: Vec<RaftLogEntry>,
}


// ─── S7-WS6-02: Raft heartbeat response ──────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RaftHeartbeatResponse {
    pub(crate) status: &'static str,
    pub(crate) role: String,
    pub(crate) term: u64,
    pub(crate) ticks_reset_to: u64,
    pub(crate) heartbeat_accepted: bool,
}


// ─── S7-WS6-03: Raft cluster member list structs ──────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RaftMemberEntry {
    pub(crate) node_id: String,
    pub(crate) role: String,
    pub(crate) term: u64,
    pub(crate) fencing_token: u64,
}


#[derive(Debug, Serialize)]
pub(crate) struct RaftMemberListResponse {
    pub(crate) status: &'static str,
    pub(crate) member_count: usize,
    pub(crate) members: Vec<RaftMemberEntry>,
}


// ─── S7-WS6-01: Raft vote statistics ─────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RaftVoteStatsResponse {
    pub(crate) status: &'static str,
    pub(crate) current_term: u64,
    pub(crate) total_votes_granted: u64,
    pub(crate) total_votes_rejected: u64,
}


// ─── S7-WS6-03: Raft current leader response ────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RaftLeaderResponse {
    pub(crate) status: &'static str,
    pub(crate) node_id: String,
    pub(crate) role: String,
    pub(crate) current_term: u64,
    pub(crate) is_leader: bool,
    pub(crate) fencing_token: u64,
}


// ─── S7-WS6-02: Raft snapshot response ───────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct RaftSnapshotResponse {
    pub(crate) status: &'static str,
    pub(crate) node_id: String,
    pub(crate) term: u64,
    pub(crate) commit_index: u64,
    pub(crate) last_applied: u64,
    pub(crate) log_length: usize,
    pub(crate) fencing_token: u64,
}


// ─── S7-WS6-02: Raft commit progress struct ──────────────────────────────────

#[derive(Serialize)]
pub(crate) struct RaftCommitProgressResponse {
    pub(crate) status: &'static str,
    pub(crate) commit_index: u64,
    pub(crate) last_applied: u64,
    pub(crate) log_length: usize,
    pub(crate) uncommitted: usize,
}


// ─── S7-WS6-02: Raft election status response ─────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RaftElectionStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) role: RaftRole,
    pub(crate) ticks_since_heartbeat: u64,
    pub(crate) election_timeout_ticks: u64,
    pub(crate) remaining_ticks: u64,
    pub(crate) is_election_pending: bool,
}


// ─── S7-WS6-03: Raft fencing token struct ──────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct RaftFenceResponse {
    pub(crate) status: &'static str,
    pub(crate) fencing_token: u64,
    pub(crate) role: RaftRole,
    pub(crate) current_term: u64,
}



// ─── S7-WS6-02: Raft endpoint structs ────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct RaftStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) raft: RaftStatusSnapshot,
}


/// S7-WS6-03: Advance the election timer by one logical tick.
///
/// In a real deployment a background task would call this; the HTTP endpoint
/// enables deterministic testing without real timers.
#[derive(Serialize)]
pub(crate) struct RaftTickResponse {
    pub(crate) status: &'static str,
    pub(crate) ticks_since_heartbeat: u64,
    pub(crate) role: RaftRole,
    pub(crate) current_term: u64,
    pub(crate) election_triggered: bool,
}


// ─── Raft handlers ───────────────────────────────────────────────────────



// ─── S7-WS6-02: Raft log entries endpoint ────────────────────────────────────────────

pub(crate) async fn raft_log(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RaftLogResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Read)?;
    let node = state.raft_state.lock().expect("raft_state lock");
    let log_length = node.log.len();
    let commit_index = node.commit_index;
    let entries = node.log.clone();
    drop(node);
    Ok((StatusCode::OK, Json(RaftLogResponse {
        status: "ok",
        log_length,
        commit_index,
        entries,
    })))
}


// ─── S7-WS6-02: Raft heartbeat endpoint ──────────────────────────────────────

/// S7-WS6-02: Accept a heartbeat — resets the tick counter and confirms leader comms.
pub(crate) async fn raft_heartbeat(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RaftHeartbeatResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Execute)?;
    let mut node = state.raft_state.lock().expect("raft_state lock");
    node.ticks_since_heartbeat = 0;
    let role = format!("{:?}", node.role);
    let term = node.current_term;
    drop(node);
    Ok((StatusCode::OK, Json(RaftHeartbeatResponse {
        status: "ok",
        role,
        term,
        ticks_reset_to: 0,
        heartbeat_accepted: true,
    })))
}


// ─── S7-WS6-02: Raft election status handler ─────────────────────────────────

/// S7-WS6-02: Return the current election timer status for this Raft node.
pub(crate) async fn raft_election_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RaftElectionStatusResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Read)?;
    let node = state.raft_state.lock().expect("raft_state election_status lock");
    let role = node.role;
    let ticks = node.ticks_since_heartbeat;
    let timeout = node.election_timeout_ticks;
    let remaining = timeout.saturating_sub(ticks);
    let is_election_pending = matches!(node.role, RaftRole::Candidate);
    drop(node);
    Ok((StatusCode::OK, Json(RaftElectionStatusResponse {
        status: "ok",
        role,
        ticks_since_heartbeat: ticks,
        election_timeout_ticks: timeout,
        remaining_ticks: remaining,
        is_election_pending,
    })))
}


// ─── S7-WS6-02: Raft commit progress handler ────────────────────────────────

pub(crate) async fn raft_commit_progress(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RaftCommitProgressResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Read)?;
    let node = state.raft_state.lock().expect("raft_state lock");
    let commit_index = node.commit_index;
    let last_applied = node.last_applied;
    let log_length = node.log.len();
    let uncommitted = log_length.saturating_sub(commit_index as usize);
    drop(node);
    Ok((StatusCode::OK, Json(RaftCommitProgressResponse {
        status: "ok",
        commit_index,
        last_applied,
        log_length,
        uncommitted,
    })))
}


// ─── S7-WS6-02: Raft point-in-time snapshot handler ────────────────────────

/// S7-WS6-02: Return a point-in-time snapshot of the Raft node state.
pub(crate) async fn raft_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RaftSnapshotResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Read)?;
    let snap = state.raft_state.lock().expect("raft_state snapshot lock").status();
    Ok((StatusCode::OK, Json(RaftSnapshotResponse {
        status: "ok",
        node_id: snap.node_id,
        term: snap.current_term,
        commit_index: snap.commit_index,
        last_applied: snap.last_applied,
        log_length: snap.log_length,
        fencing_token: snap.fencing_token,
    })))
}


// ─── S7-WS6-03: Raft cluster member list endpoint ────────────────────────────

/// S7-WS6-03: Return the list of known Raft cluster members (scaffold: local node only).
pub(crate) async fn raft_member_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RaftMemberListResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Read)?;
    let node = state.raft_state.lock().expect("raft_state lock");
    let member = RaftMemberEntry {
        node_id: node.node_id.clone(),
        role: format!("{:?}", node.role),
        term: node.current_term,
        fencing_token: node.fencing_token,
    };
    drop(node);
    Ok((StatusCode::OK, Json(RaftMemberListResponse {
        status: "ok",
        member_count: 1,
        members: vec![member],
    })))
}


// ─── S7-WS6-03: Raft current leader endpoint ────────────────────────────────

/// S7-WS6-03: Return this node's view of the current Raft leader.
pub(crate) async fn raft_leader(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RaftLeaderResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Read)?;
    let node = state.raft_state.lock().expect("raft_state lock");
    let is_leader = matches!(node.role, RaftRole::Leader);
    let response = RaftLeaderResponse {
        status: "ok",
        node_id: node.node_id.clone(),
        role: format!("{:?}", node.role),
        current_term: node.current_term,
        is_leader,
        fencing_token: node.fencing_token,
    };
    drop(node);
    Ok((StatusCode::OK, Json(response)))
}


/// S7-WS6-01: Return accumulated vote grant/reject counts for the current Raft node.
pub(crate) async fn raft_vote_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RaftVoteStatsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Read)?;
    let snap = state.raft_state.lock().expect("raft_state lock").status();
    // Scaffold: vote accumulation not yet tracked in RaftNode;
    // expose current_term only and return zeroed counters.
    Ok((StatusCode::OK, Json(RaftVoteStatsResponse {
        status: "ok",
        current_term: snap.current_term,
        total_votes_granted: 0,
        total_votes_rejected: 0,
    })))
}


// ─── S7-WS6-03: Raft fencing token endpoint ──────────────────────────────────────────

/// Return the current fencing token for the Raft node.
pub(crate) async fn raft_fence(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RaftFenceResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Read)?;
    let snap = state.raft_state.lock().expect("raft_state lock").status();
    Ok((
        StatusCode::OK,
        Json(RaftFenceResponse {
            status: "ok",
            fencing_token: snap.fencing_token,
            role: snap.role,
            current_term: snap.current_term,
        }),
    ))
}


// ─── S7-WS6-02: Raft consensus endpoints ─────────────────────────────────────

/// Return the current Raft node status.
pub(crate) async fn raft_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RaftStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Read)?;
    let snap = state.raft_state.lock().expect("raft_state lock").status();
    Ok(Json(RaftStatusResponse { status: "ok", raft: snap }))
}


/// Handle an incoming RequestVote RPC.
pub(crate) async fn raft_vote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RaftVoteRequest>,
) -> Result<Json<RaftVoteResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    // Accept intra-cluster token OR operator credentials — whichever is present.
    if check_cluster_token(&headers, &state).is_err() {
        require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Execute)?;
    }
    let resp = state.raft_state.lock().expect("raft_state lock").handle_vote_request(&req);
    Ok(Json(resp))
}


/// Handle an incoming AppendEntries RPC (heartbeat or log replication).
pub(crate) async fn raft_append(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RaftAppendRequest>,
) -> Result<Json<RaftAppendResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    // Accept intra-cluster token OR operator credentials — whichever is present.
    if check_cluster_token(&headers, &state).is_err() {
        require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Execute)?;
    }
    // Store the leader's advertised URL so followers can forward DML writes.
    if let Some(leader_url) = headers.get("x-vng-leader-url").and_then(|v| v.to_str().ok()) {
        *state.current_leader_url.lock().expect("leader_url lock") = Some(leader_url.to_string());
    }
    let resp = state.raft_state.lock().expect("raft_state lock").handle_append_entries(&req);
    Ok(Json(resp))
}


pub(crate) async fn raft_tick(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RaftTickResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Execute)?;
    let mut node = state.raft_state.lock().expect("raft_state lock");
    let role_before = node.role;
    node.tick();
    let election_triggered = node.role != role_before;
    Ok(Json(RaftTickResponse {
        status: "ok",
        ticks_since_heartbeat: node.ticks_since_heartbeat,
        role: node.role,
        current_term: node.current_term,
        election_triggered,
    }))
}


// ─── Raft snapshot install endpoint ─────────────────────────────────────────

/// Convert a JSON value to its string representation for row-store storage.
/// Strings are returned as-is (no quotes); numbers and booleans use their
/// display form; null becomes empty string.  Avoids silent data loss from
/// `.as_str().unwrap_or("")` which drops non-string scalars.
fn json_value_to_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Install a snapshot from the leader (§7).
///
/// Accepts intra-cluster token OR operator credentials.
/// Calls `handle_install_snapshot` to update Raft state, then replaces the
/// row-store with the snapshot rows using `PagedRowStore::replace_all`.
pub(crate) async fn raft_install_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RaftInstallSnapshotRequest>,
) -> Result<Json<RaftInstallSnapshotResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    // Accept intra-cluster token OR operator credentials.
    if check_cluster_token(&headers, &state).is_err() {
        require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Execute)?;
    }
    let resp = {
        let mut node = state.raft_state.lock().expect("raft_state lock");
        node.handle_install_snapshot(&req)
    };
    if resp.success {
        // Replace row-store with snapshot rows.
        let rows = req.rows.into_iter().map(|(k, v)| {
            let data: std::collections::HashMap<String, String> = match v {
                serde_json::Value::Object(m) => m.into_iter()
                    .map(|(col_k, col_v)| (col_k, json_value_to_str(&col_v)))
                    .collect(),
                _ => std::collections::HashMap::new(),
            };
            (k, data)
        });
        let mut rs = state.row_store.lock().expect("row_store lock");
        rs.replace_all(rows);
    }
    Ok(Json(resp))
}


// ─── §7-chunked: incremental snapshot transfer endpoint ──────────────────────

/// Receive one chunk of an incremental snapshot transfer from the leader.
///
/// The leader splits the full row-store export into fixed-size chunks and sends
/// them sequentially.  Each chunk carries the same `session_id`; the follower
/// accumulates rows until `is_last == true`, then applies the snapshot atomically.
pub(crate) async fn raft_install_snapshot_chunk(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RaftSnapshotChunkRequest>,
) -> Result<Json<RaftSnapshotChunkResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    if check_cluster_token(&headers, &state).is_err() {
        require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Execute)?;
    }

    // Guard: reject stale-term chunks.
    {
        let node = state.raft_state.lock().expect("raft_state lock");
        if req.term < node.current_term {
            return Ok(Json(RaftSnapshotChunkResponse {
                term: node.current_term,
                success: false,
                next_expected_chunk: 0,
                complete: false,
            }));
        }
    }

    let mut sessions = state.snapshot_chunk_sessions.lock().expect("chunk sessions lock");
    let session = sessions.entry(req.session_id.clone()).or_insert_with(|| SnapshotChunkSession {
        term: req.term,
        leader_id: req.leader_id.clone(),
        snapshot_index: req.snapshot_index,
        snapshot_term: req.snapshot_term,
        rows: Vec::new(),
        next_expected_chunk: 0,
    });

    // Reject out-of-order chunks.
    if req.chunk_index != session.next_expected_chunk {
        let expected = session.next_expected_chunk;
        return Ok(Json(RaftSnapshotChunkResponse {
            term: req.term,
            success: false,
            next_expected_chunk: expected,
            complete: false,
        }));
    }

    session.rows.extend(req.rows.into_iter());
    session.next_expected_chunk += 1;

    if !req.is_last {
        let next = session.next_expected_chunk;
        return Ok(Json(RaftSnapshotChunkResponse {
            term: req.term,
            success: true,
            next_expected_chunk: next,
            complete: false,
        }));
    }

    // Final chunk — apply the full snapshot.
    let accumulated: Vec<(String, serde_json::Value)> = std::mem::take(&mut session.rows);
    let snap_index = session.snapshot_index;
    let snap_term = session.snapshot_term;
    sessions.remove(&req.session_id);
    drop(sessions);

    // Build a synthetic install-snapshot request to reuse existing Raft state logic.
    let install_req = RaftInstallSnapshotRequest {
        term: req.term,
        leader_id: req.leader_id.clone(),
        snapshot_index: snap_index,
        snapshot_term: snap_term,
        rows: accumulated.clone(),
    };
    let resp = {
        let mut node = state.raft_state.lock().expect("raft_state lock");
        node.handle_install_snapshot(&install_req)
    };
    if resp.success {
        let rows = accumulated.into_iter().map(|(k, v)| {
            let data: std::collections::HashMap<String, String> = match v {
                serde_json::Value::Object(m) => m.into_iter()
                    .map(|(col_k, col_v)| (col_k, json_value_to_str(&col_v)))
                    .collect(),
                _ => std::collections::HashMap::new(),
            };
            (k, data)
        });
        let mut rs = state.row_store.lock().expect("row_store lock");
        rs.replace_all(rows);
    }

    Ok(Json(RaftSnapshotChunkResponse {
        term: req.term,
        success: resp.success,
        next_expected_chunk: 0,
        complete: resp.success,
    }))
}

