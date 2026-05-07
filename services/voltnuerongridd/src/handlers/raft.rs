use axum::extract::{State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use crate::{AppState, AuthErrorResponse, now_unix_ms};
use crate::auth::{require_operator_auth, require_operator_privilege};

// ─── Raft DTOs ──────────────────────────────────────────────────────────



// ─── S7-WS6-02: Raft log entries response ─────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RaftLogResponse {
    status: &'static str,
    log_length: usize,
    commit_index: u64,
    entries: Vec<RaftLogEntry>,
}


// ─── S7-WS6-02: Raft heartbeat response ──────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RaftHeartbeatResponse {
    status: &'static str,
    role: String,
    term: u64,
    ticks_reset_to: u64,
    heartbeat_accepted: bool,
}


// ─── S7-WS6-03: Raft cluster member list structs ──────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RaftMemberEntry {
    node_id: String,
    role: String,
    term: u64,
    fencing_token: u64,
}


#[derive(Debug, Serialize)]
pub(crate) struct RaftMemberListResponse {
    status: &'static str,
    member_count: usize,
    members: Vec<RaftMemberEntry>,
}


// ─── S7-WS6-01: Raft vote statistics ─────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RaftVoteStatsResponse {
    status: &'static str,
    current_term: u64,
    total_votes_granted: u64,
    total_votes_rejected: u64,
}


// ─── S7-WS6-03: Raft current leader response ────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RaftLeaderResponse {
    status: &'static str,
    node_id: String,
    role: String,
    current_term: u64,
    is_leader: bool,
    fencing_token: u64,
}


// ─── S7-WS6-02: Raft snapshot response ───────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct RaftSnapshotResponse {
    status: &'static str,
    node_id: String,
    term: u64,
    commit_index: u64,
    last_applied: u64,
    log_length: usize,
    fencing_token: u64,
}


// ─── S7-WS6-02: Raft commit progress struct ──────────────────────────────────

#[derive(Serialize)]
pub(crate) struct RaftCommitProgressResponse {
    status: &'static str,
    commit_index: u64,
    last_applied: u64,
    log_length: usize,
    uncommitted: usize,
}


// ─── S7-WS6-02: Raft election status response ─────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RaftElectionStatusResponse {
    status: &'static str,
    role: RaftRole,
    ticks_since_heartbeat: u64,
    election_timeout_ticks: u64,
    remaining_ticks: u64,
    is_election_pending: bool,
}


// ─── S7-WS6-03: Raft fencing token struct ──────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct RaftFenceResponse {
    status: &'static str,
    fencing_token: u64,
    role: RaftRole,
    current_term: u64,
}



// ─── S7-WS6-02: Raft endpoint structs ────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct RaftStatusResponse {
    status: &'static str,
    raft: RaftStatusSnapshot,
}


/// S7-WS6-03: Advance the election timer by one logical tick.
///
/// In a real deployment a background task would call this; the HTTP endpoint
/// enables deterministic testing without real timers.
#[derive(Serialize)]
pub(crate) struct RaftTickResponse {
    status: &'static str,
    ticks_since_heartbeat: u64,
    role: raft::RaftRole,
    current_term: u64,
    election_triggered: bool,
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
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Execute)?;
    let resp = state.raft_state.lock().expect("raft_state lock").handle_vote_request(&req);
    Ok(Json(resp))
}


/// Handle an incoming AppendEntries RPC (heartbeat or log replication).
pub(crate) async fn raft_append(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RaftAppendRequest>,
) -> Result<Json<RaftAppendResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Execute)?;
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

