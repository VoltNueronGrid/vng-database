'use strict';
const fs = require('fs');
const path = require('path');

const filePath = path.join(__dirname, 'services', 'voltnuerongridd', 'src', 'main.rs');
const raw = fs.readFileSync(filePath, 'utf8');
const lines = raw.split('\n');

function findFirst(pred) {
  for (let i = 0; i < lines.length; i++) {
    if (pred(lines[i], i)) return i;
  }
  return -1;
}

function findLast(pred) {
  let last = -1;
  for (let i = 0; i < lines.length; i++) {
    if (pred(lines[i], i)) last = i;
  }
  return last;
}

// ── 1. Structs: insert after WalCheckpointAgeResponse closing } ──────────────
let wcaStart = findFirst(l => l.includes('struct WalCheckpointAgeResponse'));
if (wcaStart === -1) { console.error('ERROR: WalCheckpointAgeResponse struct not found'); process.exit(1); }
let wcaClose = -1;
for (let i = wcaStart + 1; i < wcaStart + 10; i++) {
  if (lines[i] && lines[i].replace(/\r/, '').trim() === '}') { wcaClose = i; break; }
}
if (wcaClose === -1) { console.error('ERROR: WalCheckpointAgeResponse close not found'); process.exit(1); }
console.log('Struct insertion after line:', wcaClose + 1);

const structBlock = [
  '',
  '// \u2500\u2500\u2500 S11-WS1-27: Rows payload size + WAL flush count structs \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
  '#[derive(Debug, Serialize)]',
  'struct RowsPayloadSizeResponse {',
  '    status: &\'static str,',
  '    total_fields: usize,',
  '    row_count: usize,',
  '}',
  '',
  '#[derive(Debug, Serialize)]',
  'struct WalFlushCountResponse {',
  '    status: &\'static str,',
  '    flush_count: usize,',
  '}',
];
lines.splice(wcaClose + 1, 0, ...structBlock);
console.log('Structs inserted.');

// ── 2. Routes: insert after wal/checkpoint/age route ─────────────────────────
let routeLine = findLast(l => l.includes('/api/v1/store/wal/checkpoint/age') && l.includes('get(wal_checkpoint_age)'));
if (routeLine === -1) { console.error('ERROR: wal/checkpoint/age route not found'); process.exit(1); }
console.log('Route insertion after line:', routeLine + 1);

const routeBlock = [
  '        // S11-WS1-27: Total payload field count across all rows',
  '        .route("/api/v1/store/rows/payload/size", get(rows_payload_size))',
  '        // S11-WS1-27: WAL flush count (total writes)',
  '        .route("/api/v1/store/wal/flush/count", get(wal_flush_count))',
];
lines.splice(routeLine + 1, 0, ...routeBlock);
console.log('Routes inserted.');

// ── 3. Handlers: insert before // ─── S7-WS6-01: Raft vote statistics endpoint ─
let raftAnchor = findFirst(l => l.includes('S7-WS6-01: Raft vote statistics endpoint'));
if (raftAnchor === -1) { console.error('ERROR: Raft vote anchor not found'); process.exit(1); }
console.log('Handler insertion before line:', raftAnchor + 1);

const handlerBlock = [
  '/// S11-WS1-27: Return total payload field count and row count across all MVCC rows.',
  'async fn rows_payload_size(',
  '    State(state): State<AppState>,',
  '    headers: HeaderMap,',
  ') -> Result<(StatusCode, Json<RowsPayloadSizeResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
  '    require_operator_auth(&headers, &state)?;',
  '    let rs = state.row_store.lock().expect("row_store lock rows_payload_size");',
  '    let snapshot = rs.export_rows_snapshot();',
  '    drop(rs);',
  '    let row_count = snapshot.len();',
  '    let total_fields: usize = snapshot.iter().map(|(_, p)| p.len()).sum();',
  '    Ok((StatusCode::OK, Json(RowsPayloadSizeResponse {',
  '        status: "ok",',
  '        total_fields,',
  '        row_count,',
  '    })))',
  '}',
  '',
  '/// S11-WS1-27: Return total WAL flush (write) count.',
  'async fn wal_flush_count(',
  '    State(state): State<AppState>,',
  '    headers: HeaderMap,',
  ') -> Result<(StatusCode, Json<WalFlushCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
  '    require_operator_auth(&headers, &state)?;',
  '    let wal = state.wal_engine.lock().expect("wal_engine lock wal_flush_count");',
  '    let flush_count = wal.wal_records().len();',
  '    drop(wal);',
  '    Ok((StatusCode::OK, Json(WalFlushCountResponse {',
  '        status: "ok",',
  '        flush_count,',
  '    })))',
  '}',
  '',
];
lines.splice(raftAnchor, 0, ...handlerBlock);
console.log('Handlers inserted.');

// ── 4. Tests: insert after s11_ws1_26_wal_checkpoint_age_missing_auth fn close ─
let s26Idx = findLast(l => l.includes('s11_ws1_26_wal_checkpoint_age_missing_auth'));
if (s26Idx === -1) { console.error('ERROR: s26 test fn not found'); process.exit(1); }
let s26Close = -1;
for (let i = s26Idx + 1; i < lines.length; i++) {
  if (lines[i].replace(/\r/, '').trim() === '}') { s26Close = i; break; }
}
if (s26Close === -1) { console.error('ERROR: s26 fn close not found'); process.exit(1); }
console.log('Test insertion after line:', s26Close + 1);

const testBlock = [
  '',
  '    // \u2500\u2500 S11-WS1-27: rows payload size + wal flush count tests \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
  '    #[tokio::test]',
  '    async fn s11_ws1_27_rows_payload_size_returns_ok() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = operator_headers("test-key", "admin");',
  '        let (status, Json(body)) = rows_payload_size(State(state), headers).await.unwrap();',
  '        assert_eq!(status, StatusCode::OK);',
  '        assert_eq!(body.status, "ok");',
  '    }',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_27_rows_payload_size_missing_auth_returns_401() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = HeaderMap::new();',
  '        let result = rows_payload_size(State(state), headers).await;',
  '        assert!(result.is_err());',
  '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
  '    }',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_27_wal_flush_count_returns_ok() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = operator_headers("test-key", "admin");',
  '        let (status, Json(body)) = wal_flush_count(State(state), headers).await.unwrap();',
  '        assert_eq!(status, StatusCode::OK);',
  '        assert_eq!(body.status, "ok");',
  '    }',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_27_wal_flush_count_missing_auth_returns_401() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = HeaderMap::new();',
  '        let result = wal_flush_count(State(state), headers).await;',
  '        assert!(result.is_err());',
  '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
  '    }',
];
lines.splice(s26Close + 1, 0, ...testBlock);
console.log('Tests inserted.');

fs.writeFileSync(filePath, lines.join('\n'), 'utf8');
console.log('Done. Total lines:', lines.length);
