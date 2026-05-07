use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use crate::{AppState, AuthErrorResponse};
use crate::auth::require_operator_auth;

// ─── S10-WS15-02: CDC change-data-capture structs ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CdcEvent {
    pub(crate) sequence: u64,
    pub(crate) op: String,
    pub(crate) table_name: String,
    pub(crate) key: String,
    pub(crate) payload: String,
    pub(crate) captured_at_ms: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct CdcStreamResponse {
    pub(crate) status: &'static str,
    pub(crate) event_count: usize,
    pub(crate) events: Vec<CdcEvent>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct CdcStreamFilterQuery {
    pub(crate) table: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CdcStreamFilterResponse {
    pub(crate) status: &'static str,
    pub(crate) table_filter: Option<String>,
    pub(crate) event_count: usize,
    pub(crate) events: Vec<CdcEvent>,
}

/// Query params for `GET /api/v1/store/cdc/stream/latest`.
#[derive(Debug, Deserialize, Default)]
pub(crate) struct CdcLatestQuery {
    pub(crate) limit: Option<usize>,
}

/// Response for `GET /api/v1/store/cdc/stream/latest`.
#[derive(Serialize)]
pub(crate) struct CdcLatestResponse {
    pub(crate) status: &'static str,
    pub(crate) event_count: usize,
    pub(crate) limit_applied: usize,
    pub(crate) events: Vec<CdcEvent>,
}

// ─── S10-WS15-02: CDC cursor tracking structs ─────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct CdcCursorQuery {
    pub(crate) table: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CdcCursorAdvanceRequest {
    pub(crate) table_name: String,
    pub(crate) position: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct CdcCursorResponse {
    pub(crate) status: &'static str,
    pub(crate) table_name: String,
    pub(crate) cursor_position: u64,
}

// ─── S10-WS15-02: CDC cursor list structs ────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct CdcCursorEntry {
    pub(crate) table_name: String,
    pub(crate) cursor_position: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct CdcCursorListResponse {
    pub(crate) status: &'static str,
    pub(crate) cursor_count: usize,
    pub(crate) cursors: Vec<CdcCursorEntry>,
}

// ─── S10-WS15-02: CDC cursor rewind struct ────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct CdcCursorRewindRequest {
    pub(crate) table_name: String,
}

// ─── S10-WS15-02: CDC aggregate metrics response ─────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct CdcMetricsResponse {
    pub(crate) status: &'static str,
    pub(crate) total_events: usize,
    pub(crate) insert_count: usize,
    pub(crate) delete_count: usize,
    pub(crate) tables_seen: usize,
}

// ─── S10-WS15-02: CDC handler functions ──────────────────────────────────────

/// S10-WS15-02: Stream committed mutations as CDC events derived from the WAL.
pub(crate) async fn cdc_stream(
    State(state): State<AppState>,
) -> (StatusCode, Json<CdcStreamResponse>) {
    let wal = state.wal_engine.lock().expect("wal_engine lock cdc");
    let events: Vec<CdcEvent> = wal.wal_records()
        .iter()
        .map(|r| CdcEvent {
            sequence: r.sequence,
            op: if r.value == "__deleted__" {
                "delete".to_string()
            } else {
                "insert".to_string()
            },
            table_name: "row_store".to_string(),
            key: r.key.clone(),
            payload: r.value.clone(),
            captured_at_ms: 0,
        })
        .collect();
    let event_count = events.len();
    drop(wal);
    (StatusCode::OK, Json(CdcStreamResponse {
        status: "ok",
        event_count,
        events,
    }))
}

// ─── S10-WS15-02: CDC cursor tracking ────────────────────────────────────────

/// S10-WS15-02: Filter CDC stream by table name.
pub(crate) async fn cdc_stream_filter(
    State(state): State<AppState>,
    Query(query): Query<CdcStreamFilterQuery>,
) -> (StatusCode, Json<CdcStreamFilterResponse>) {
    let wal = state.wal_engine.lock().expect("wal_engine lock cdc_filter");
    let all_events: Vec<CdcEvent> = wal.wal_records()
        .iter()
        .map(|r| CdcEvent {
            sequence: r.sequence,
            op: if r.value == "__deleted__" { "delete".to_string() } else { "insert".to_string() },
            table_name: "row_store".to_string(),
            key: r.key.clone(),
            payload: r.value.clone(),
            captured_at_ms: 0,
        })
        .collect();
    drop(wal);
    let events: Vec<CdcEvent> = match &query.table {
        Some(t) => all_events.into_iter().filter(|e| e.table_name == *t).collect(),
        None => all_events,
    };
    let event_count = events.len();
    (StatusCode::OK, Json(CdcStreamFilterResponse {
        status: "ok",
        table_filter: query.table,
        event_count,
        events,
    }))
}

/// S10-WS15-02: Return the latest N CDC events from the WAL stream.
pub(crate) async fn cdc_stream_latest(
    State(state): State<AppState>,
    Query(query): Query<CdcLatestQuery>,
) -> (StatusCode, Json<CdcLatestResponse>) {
    let limit = query.limit.unwrap_or(10).min(1000);
    let wal = state.wal_engine.lock().expect("wal_engine lock cdc_latest");
    let all_events: Vec<CdcEvent> = wal.wal_records()
        .iter()
        .map(|r| CdcEvent {
            sequence: r.sequence,
            op: if r.value == "__deleted__" { "delete".to_string() } else { "insert".to_string() },
            table_name: "row_store".to_string(),
            key: r.key.clone(),
            payload: r.value.clone(),
            captured_at_ms: 0,
        })
        .collect();
    drop(wal);
    let total = all_events.len();
    let skip = if total > limit { total - limit } else { 0 };
    let events: Vec<CdcEvent> = all_events.into_iter().skip(skip).collect();
    let event_count = events.len();
    (StatusCode::OK, Json(CdcLatestResponse {
        status: "ok",
        event_count,
        limit_applied: limit,
        events,
    }))
}

/// S10-WS15-02: Read the current CDC cursor position for a given table.
pub(crate) async fn cdc_cursor_status(
    State(state): State<AppState>,
    Query(q): Query<CdcCursorQuery>,
) -> (StatusCode, Json<CdcCursorResponse>) {
    let cursors = state.cdc_cursors.lock().expect("cdc_cursors lock");
    let pos = *cursors.get(&q.table).unwrap_or(&0);
    (StatusCode::OK, Json(CdcCursorResponse {
        status: "ok",
        table_name: q.table,
        cursor_position: pos,
    }))
}

/// S10-WS15-02: Advance (or initialise) the CDC cursor for a table to a given position.
pub(crate) async fn cdc_cursor_advance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CdcCursorAdvanceRequest>,
) -> Result<(StatusCode, Json<CdcCursorResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut cursors = state.cdc_cursors.lock().expect("cdc_cursors lock");
    cursors.insert(req.table_name.clone(), req.position);
    Ok((StatusCode::OK, Json(CdcCursorResponse {
        status: "ok",
        table_name: req.table_name,
        cursor_position: req.position,
    })))
}

// ─── S10-WS15-02: CDC cursor rewind — reset a table cursor to 0 ───────────────

/// S10-WS15-02: Rewind (reset to 0) the CDC cursor for the specified table.
pub(crate) async fn cdc_cursor_rewind(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CdcCursorRewindRequest>,
) -> Result<(StatusCode, Json<CdcCursorResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut cursors = state.cdc_cursors.lock().expect("cdc_cursors lock");
    cursors.insert(req.table_name.clone(), 0);
    Ok((StatusCode::OK, Json(CdcCursorResponse {
        status: "ok",
        table_name: req.table_name,
        cursor_position: 0,
    })))
}

// ─── S10-WS15-02: CDC cursor list ─────────────────────────────────────────────

/// S10-WS15-02: List all tracked CDC cursor positions across tables.
pub(crate) async fn cdc_cursor_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<CdcCursorListResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let cursors = state.cdc_cursors.lock().expect("cdc_cursors lock");
    let mut entries: Vec<CdcCursorEntry> = cursors
        .iter()
        .map(|(table, pos)| CdcCursorEntry {
            table_name: table.clone(),
            cursor_position: *pos,
        })
        .collect();
    entries.sort_by(|a, b| a.table_name.cmp(&b.table_name));
    let cursor_count = entries.len();
    Ok((StatusCode::OK, Json(CdcCursorListResponse {
        status: "ok",
        cursor_count,
        cursors: entries,
    })))
}

// ─── S10-WS15-02: CDC aggregate metrics ──────────────────────────────────────

/// S10-WS15-02: Return aggregate CDC event metrics (total/insert/delete counts, tables seen).
pub(crate) async fn cdc_metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<CdcMetricsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine cdc_metrics lock");
    let records = wal.wal_records().to_vec();
    drop(wal);
    let total_events = records.len();
    let insert_count = records.iter().filter(|r| r.value != "__deleted__").count();
    let delete_count = records.iter().filter(|r| r.value == "__deleted__").count();
    let tables_seen = records.iter()
        .filter_map(|r| r.key.split(':').next().map(|t| t.to_string()))
        .collect::<std::collections::HashSet<_>>()
        .len();
    Ok((StatusCode::OK, Json(CdcMetricsResponse {
        status: "ok",
        total_events,
        insert_count,
        delete_count,
        tables_seen,
    })))
}
