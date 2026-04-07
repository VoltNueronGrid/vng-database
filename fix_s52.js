// fix_s52.js — Session 52 string-replace approach
'use strict';
const fs = require('fs');
const MAIN = 'd:/by/polap-db/services/voltnuerongridd/src/main.rs';
let src = fs.readFileSync(MAIN, 'utf8');

function rep(search, replacement) {
  if (!src.includes(search)) throw new Error('NOT FOUND: ' + search.slice(0,80));
  src = src.replace(search, replacement);
}

// 1. Structs after WalFlushCountResponse
rep(
`struct WalFlushCountResponse {\r\n    status: &'static str,\r\n    flush_count: usize,\r\n}`,
`struct WalFlushCountResponse {\r\n    status: &'static str,\r\n    flush_count: usize,\r\n}\r\n\r\n// ─── S3-WS1-28: rows/field/count + wal/entry/latest structs ─────────────────\r\n\r\n#[derive(Debug, Serialize)]\r\nstruct RowsFieldCountResponse {\r\n    status: &'static str,\r\n    total_fields: usize,\r\n    row_count: usize,\r\n}\r\n\r\n#[derive(Debug, Serialize)]\r\nstruct WalEntryLatestResponse {\r\n    status: &'static str,\r\n    has_entry: bool,\r\n    entry_sequence: u64,\r\n}`
);

// 2. Routes after wal/flush/count
rep(
`        .route("/api/v1/store/wal/flush/count", get(wal_flush_count))\r\n        // S11-WS1-19: Scan all rows visible at current snapshot`,
`        .route("/api/v1/store/wal/flush/count", get(wal_flush_count))\r\n        // S3-WS1-28: rows/field/count + wal/entry/latest\r\n        .route("/api/v1/store/rows/field/count", get(rows_field_count))\r\n        .route("/api/v1/store/wal/entry/latest", get(wal_entry_latest))\r\n        // S11-WS1-19: Scan all rows visible at current snapshot`
);

// 3. Handlers before S7-WS6-01
rep(
`// ─── S7-WS6-01: Raft vote statistics endpoint`,
`// ─── S3-WS1-28: rows/field/count endpoint ──────────────────────────────────\r\nasync fn rows_field_count(\r\n    State(state): State<Arc<AppState>>,\r\n    headers: HeaderMap,\r\n) -> Result<Json<RowsFieldCountResponse>, (StatusCode, Json<AuthErrorResponse>)> {\r\n    require_operator_auth(&headers, &state)?;\r\n    let snapshot = state.row_store.lock().expect("row_store lock rows_field_count").export_rows_snapshot();\r\n    let row_count = snapshot.len();\r\n    let total_fields = snapshot.values().map(|r| r.len()).sum::<usize>();\r\n    Ok(Json(RowsFieldCountResponse { status: "ok", total_fields, row_count }))\r\n}\r\n\r\n// ─── S3-WS1-28: wal/entry/latest endpoint ────────────────────────────────────\r\nasync fn wal_entry_latest(\r\n    State(state): State<Arc<AppState>>,\r\n    headers: HeaderMap,\r\n) -> Result<Json<WalEntryLatestResponse>, (StatusCode, Json<AuthErrorResponse>)> {\r\n    require_operator_auth(&headers, &state)?;\r\n    let records = state.wal_engine.lock().expect("wal_engine lock wal_entry_latest").wal_records();\r\n    let entry_sequence = records.last().map(|r| r.sequence).unwrap_or(0);\r\n    let has_entry = !records.is_empty();\r\n    Ok(Json(WalEntryLatestResponse { status: "ok", has_entry, entry_sequence }))\r\n}\r\n\r\n// ─── S7-WS6-01: Raft vote statistics endpoint`
);

// 4. Tests: anchor on last test fn close
rep(
`        let res = wal_flush_count(State(state), hdrs).await;\r\n        assert!(res.is_err(), "missing auth should be rejected");\r\n        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);\r\n    }\r\n}`,
`        let res = wal_flush_count(State(state), hdrs).await;\r\n        assert!(res.is_err(), "missing auth should be rejected");\r\n        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);\r\n    }\r\n\r\n    // ── S3-WS1-28: rows_field_count tests ──────────────────────────────────────\r\n\r\n    #[tokio::test]\r\n    async fn s11_ws1_28_rows_field_count_ok() {\r\n        let state = state_with_key(Some("test-key"));\r\n        let hdrs = operator_headers("test-key", "admin");\r\n        let res = rows_field_count(State(state), hdrs).await;\r\n        assert!(res.is_ok(), "rows_field_count should return ok");\r\n        let body = res.unwrap().0;\r\n        assert_eq!(body.status, "ok");\r\n    }\r\n\r\n    #[tokio::test]\r\n    async fn s11_ws1_28_rows_field_count_missing_auth() {\r\n        let state = state_with_key(Some("test-key"));\r\n        let hdrs = HeaderMap::new();\r\n        let res = rows_field_count(State(state), hdrs).await;\r\n        assert!(res.is_err(), "missing auth should be rejected");\r\n        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);\r\n    }\r\n\r\n    // ── S3-WS1-28: wal_entry_latest tests ──────────────────────────────────────\r\n\r\n    #[tokio::test]\r\n    async fn s11_ws1_28_wal_entry_latest_ok() {\r\n        let state = state_with_key(Some("test-key"));\r\n        let hdrs = operator_headers("test-key", "admin");\r\n        let res = wal_entry_latest(State(state), hdrs).await;\r\n        assert!(res.is_ok(), "wal_entry_latest should return ok");\r\n        let body = res.unwrap().0;\r\n        assert_eq!(body.status, "ok");\r\n    }\r\n\r\n    #[tokio::test]\r\n    async fn s11_ws1_28_wal_entry_latest_missing_auth() {\r\n        let state = state_with_key(Some("test-key"));\r\n        let hdrs = HeaderMap::new();\r\n        let res = wal_entry_latest(State(state), hdrs).await;\r\n        assert!(res.is_err(), "missing auth should be rejected");\r\n        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);\r\n    }\r\n}`
);

fs.writeFileSync(MAIN, src, 'utf8');
console.log('done. Lines:', src.split('\n').length);
