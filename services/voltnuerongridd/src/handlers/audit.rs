use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use voltnuerongrid_audit::{AuditEventKind, AppendOnlyAuditSink};
use voltnuerongrid_auth::PrivilegeAction;
use crate::{AppState, AuthErrorResponse, RuntimeAccessPrincipal, AuditEvent};
use crate::auth::{require_operator_auth, require_operator_privilege, require_audit_runtime_principal};
use crate::audit_helpers::{append_runtime_audit_event, filter_audit_events_for_principal};

// ─── Audit DTOs ───────────────────────────────────────────────────────────────


// S9-WS8A-02: audit chain verify response
#[derive(Serialize)]
pub(crate) struct AuditChainVerifyResponse {
    pub(crate) status: &'static str,
    pub(crate) event_count: usize,
    pub(crate) chain_valid: bool,
    pub(crate) genesis_hash: &'static str,
}



// ─── S9-WS8A-01: Audit CLI summary response ──────────────────────────────────

/// Response for `GET /api/v1/audit/cli/summary`.
#[derive(Serialize)]
pub(crate) struct AuditCliSummaryResponse {
    pub(crate) status: &'static str,
    pub(crate) total_events: usize,
    pub(crate) chain_valid: bool,
    pub(crate) last_event_kind: String,
    pub(crate) export_hint: &'static str,
}


// ─── S9-WS8A-02: Audit export struct ─────────────────────────────────────────

#[derive(Deserialize, Default)]
pub(crate) struct AuditExportQuery {
    pub(crate) cursor: Option<usize>,
    pub(crate) limit: Option<usize>,
}


#[derive(Serialize)]
pub(crate) struct AuditExportResponse {
    pub(crate) status: &'static str,
    pub(crate) event_count: usize,
    pub(crate) total_event_count: usize,
    pub(crate) cursor: usize,
    pub(crate) limit: usize,
    pub(crate) file_backed: bool,
    pub(crate) audit_log_path: Option<String>,
    pub(crate) events: Vec<AuditEvent>,
}


// ─── S9-WS8A-02: Audit integrity snapshot response ───────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct AuditSnapshotResponse {
    pub(crate) status: &'static str,
    pub(crate) snapshot_at_ms: u64,
    pub(crate) event_count: usize,
    pub(crate) chain_valid: bool,
    pub(crate) genesis_hash: &'static str,
}


// ─── S9-WS8A-02: Audit purge structs ────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct AuditPurgeRequest {
    #[allow(dead_code)]
    pub(crate) confirm: bool,
}


#[derive(Serialize)]
pub(crate) struct AuditPurgeResponse {
    pub(crate) status: &'static str,
    pub(crate) events_purged: usize,
    pub(crate) chain_reset: bool,
}


#[derive(Deserialize)]
pub(crate) struct AuditEventsQuery {
    pub(crate) max_items: Option<usize>,
}


#[derive(Serialize)]
pub(crate) struct AuditEventsResponse {
    pub(crate) status: &'static str,
    pub(crate) total_events: usize,
    pub(crate) events: Vec<AuditEvent>,
}


// ─── Audit handlers ──────────────────────────────────────────────────────────



pub(crate) async fn audit_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditEventsQuery>,
) -> Result<Json<AuditEventsResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_audit_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "audit/events",
    )?;
    let max_items = query.max_items.unwrap_or(100).min(1_000);
    let events = state
        .audit_sink
        .lock()
        .map(|sink| sink.latest(max_items))
        .unwrap_or_default();
    let events = filter_audit_events_for_principal(events, &principal);
    Ok(Json(AuditEventsResponse {
        status: "ok",
        total_events: events.len(),
        events,
    }))
}


/// S9-WS8A-02: verify the tamper-evident hash chain across all audit events.
pub(crate) async fn audit_chain_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AuditChainVerifyResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_audit_runtime_principal(&headers, &state, PrivilegeAction::Read, "audit/chain/verify")?;
    let events = state
        .audit_sink
        .lock()
        .map(|sink| sink.all().to_vec())
        .unwrap_or_default();
    let event_count = events.len();
    let chain_valid = AppendOnlyAuditSink::verify_chain(&events);
    Ok(Json(AuditChainVerifyResponse {
        status: "ok",
        event_count,
        chain_valid,
        genesis_hash: "genesis-0000000000000000",
    }))
}


// ─── S9-WS8A-02: Audit integrity snapshot ────────────────────────────────────

/// S9-WS8A-02: Return a point-in-time integrity snapshot of the audit chain.
pub(crate) async fn audit_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<AuditSnapshotResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_audit_runtime_principal(&headers, &state, PrivilegeAction::Read, "audit/snapshot")?;
    let events = state
        .audit_sink
        .lock()
        .map(|sink| sink.all().to_vec())
        .unwrap_or_default();
    let event_count = events.len();
    let chain_valid = AppendOnlyAuditSink::verify_chain(&events);
    let snapshot_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    Ok((StatusCode::OK, Json(AuditSnapshotResponse {
        status: "ok",
        snapshot_at_ms,
        event_count,
        chain_valid,
        genesis_hash: "genesis-0000000000000000",
    })))
}


// ─── S9-WS8A-02: Audit purge — flush the in-memory audit sink ────────────────

/// S9-WS8A-02: Purge all buffered audit events (requires operator auth).
pub(crate) async fn audit_purge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(_req): Json<AuditPurgeRequest>,
) -> Result<(StatusCode, Json<AuditPurgeResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let events_purged = {
        let mut sink = state.audit_sink.lock().expect("audit_sink lock");
        let count = sink.all().len();
        *sink = AppendOnlyAuditSink::new();
        count
    };
    Ok((StatusCode::OK, Json(AuditPurgeResponse {
        status: "ok",
        events_purged,
        chain_reset: true,
    })))
}


// ─── S9-WS8A-01: Audit CLI summary ──────────────────────────────────────────

/// S9-WS8A-01: Return a CLI-friendly summary of the current audit chain state.
pub(crate) async fn audit_cli_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<AuditCliSummaryResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_audit_runtime_principal(&headers, &state, PrivilegeAction::Read, "audit/cli/summary")?;
    let events = state
        .audit_sink
        .lock()
        .map(|sink| sink.all().to_vec())
        .unwrap_or_default();
    let total_events = events.len();
    let chain_valid = AppendOnlyAuditSink::verify_chain(&events);
    let last_event_kind = events
        .last()
        .map(|e| format!("{:?}", e.kind))
        .unwrap_or_else(|| "none".to_string());
    let export_hint = "Use GET /api/v1/audit/export to download the full event log";
    Ok((StatusCode::OK, Json(AuditCliSummaryResponse {
        status: "ok",
        total_events,
        chain_valid,
        last_event_kind,
        export_hint,
    })))
}



// ─── S9-WS8A-02: Audit export endpoint ───────────────────────────────────────

/// Return all buffered audit events and indicate whether file-backed logging is active.
pub(crate) async fn audit_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<AuditExportQuery>,
) -> Result<(StatusCode, Json<AuditExportResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "audit.read",
        "audit/export",
        PrivilegeAction::Read,
    )?;
    let principal = RuntimeAccessPrincipal::Operator(operator);
    let all_events = state.audit_sink.lock().expect("audit_sink lock").all().to_vec();
    let total_event_count = all_events.len();
    let cursor = params.cursor.unwrap_or(0).min(total_event_count);
    let limit = params.limit.unwrap_or(1000).max(1).min(10000);
    let events: Vec<AuditEvent> = all_events.into_iter().skip(cursor).take(limit).collect();
    let event_count = events.len();
    let file_backed = state.audit_log_path.is_some();
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "audit_export",
        "ok",
        json!({ "route_scope": "audit/export", "event_count": event_count, "file_backed": file_backed }),
    );
    Ok((
        StatusCode::OK,
        Json(AuditExportResponse {
            status: "ok",
            event_count,
            total_event_count,
            cursor,
            limit,
            file_backed,
            audit_log_path: state.audit_log_path.clone(),
            events,
        }),
    ))
}

