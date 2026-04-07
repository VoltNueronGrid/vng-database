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

// ── 1. Structs: insert after RowsKeyExistsResponse closing } ─────────────────
let kexStructStart = findFirst(l => l.includes('struct RowsKeyExistsResponse'));
if (kexStructStart === -1) { console.error('ERROR: RowsKeyExistsResponse struct not found'); process.exit(1); }
let kexStructClose = -1;
for (let i = kexStructStart + 1; i < kexStructStart + 8; i++) {
  if (lines[i] && lines[i].replace(/\r/, '').trim() === '}') {
    kexStructClose = i;
    break;
  }
}
if (kexStructClose === -1) { console.error('ERROR: RowsKeyExistsResponse close not found'); process.exit(1); }
console.log('Struct insertion after line:', kexStructClose + 1);

const structBlock = [
  '',
  '// \u2500\u2500\u2500 S11-WS1-25: Rows value search + WAL record count structs \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
  '#[derive(Debug, Deserialize)]',
  'struct RowsValueSearchQuery {',
  '    value: String,',
  '}',
  '',
  '#[derive(Debug, Serialize)]',
  'struct RowsValueSearchResponse {',
  '    status: &\'static str,',
  '    match_count: usize,',
  '    matches: Vec<String>,',
  '}',
  '',
  '#[derive(Debug, Serialize)]',
  'struct WalRecordCountResponse {',
  '    status: &\'static str,',
  '    record_count: usize,',
  '}',
];
lines.splice(kexStructClose + 1, 0, ...structBlock);
console.log('Structs inserted.');

// ── 2. Routes: insert after rows/key/exists route ────────────────────────────
let routeLine = findLast(l => l.includes('/api/v1/store/rows/key/exists') && l.includes('get(rows_key_exists)'));
if (routeLine === -1) { console.error('ERROR: rows/key/exists route not found'); process.exit(1); }
console.log('Route insertion after line:', routeLine + 1);

const routeBlock = [
  '        // S11-WS1-25: Search rows by value',
  '        .route("/api/v1/store/rows/value/search", get(rows_value_search))',
  '        // S11-WS1-25: Count total WAL records',
  '        .route("/api/v1/store/wal/record/count", get(wal_record_count))',
];
lines.splice(routeLine + 1, 0, ...routeBlock);
console.log('Routes inserted.');

// ── 3. Handlers: insert before // ─── S7-WS6-01: Raft vote statistics endpoint ─
let raftAnchor = findFirst(l => l.includes('S7-WS6-01: Raft vote statistics endpoint'));
if (raftAnchor === -1) { console.error('ERROR: Raft vote anchor not found'); process.exit(1); }
console.log('Handler insertion before line:', raftAnchor + 1);

const handlerBlock = [
  '/// S11-WS1-25: Search rows whose payload contains the given value.',
  'async fn rows_value_search(',
  '    State(state): State<AppState>,',
  '    headers: HeaderMap,',
  '    Query(params): Query<RowsValueSearchQuery>,',
  ') -> Result<(StatusCode, Json<RowsValueSearchResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
  '    require_operator_auth(&headers, &state)?;',
  '    let rs = state.row_store.lock().expect("row_store lock rows_value_search");',
  '    let snapshot = rs.export_rows_snapshot();',
  '    drop(rs);',
  '    let needle = params.value.to_lowercase();',
  '    let matches: Vec<String> = snapshot',
  '        .into_iter()',
  '        .filter(|(_, payload)| payload.values().any(|v| v.to_lowercase().contains(needle.as_str())))',
  '        .map(|(k, _)| k)',
  '        .collect();',
  '    let match_count = matches.len();',
  '    Ok((StatusCode::OK, Json(RowsValueSearchResponse {',
  '        status: "ok",',
  '        match_count,',
  '        matches,',
  '    })))',
  '}',
  '',
  '/// S11-WS1-25: Return total count of WAL records.',
  'async fn wal_record_count(',
  '    State(state): State<AppState>,',
  '    headers: HeaderMap,',
  ') -> Result<(StatusCode, Json<WalRecordCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
  '    require_operator_auth(&headers, &state)?;',
  '    let wal = state.wal_engine.lock().expect("wal_engine lock wal_record_count");',
  '    let record_count = wal.wal_records().len();',
  '    drop(wal);',
  '    Ok((StatusCode::OK, Json(WalRecordCountResponse {',
  '        status: "ok",',
  '        record_count,',
  '    })))',
  '}',
  '',
];
lines.splice(raftAnchor, 0, ...handlerBlock);
console.log('Handlers inserted.');

// ── 4. Tests: insert after s11_ws1_24_rows_key_exists fn close, before mod close ─
let s24Idx = findLast(l => l.includes('s11_ws1_24_rows_key_exists_missing_auth'));
if (s24Idx === -1) { console.error('ERROR: s24 test fn not found'); process.exit(1); }
let s24Close = -1;
for (let i = s24Idx + 1; i < lines.length; i++) {
  if (lines[i].replace(/\r/, '').trim() === '}') { s24Close = i; break; }
}
if (s24Close === -1) { console.error('ERROR: s24 fn close not found'); process.exit(1); }
console.log('Test insertion after line:', s24Close + 1);

const testBlock = [
  '',
  '    // \u2500\u2500 S11-WS1-25: rows value search + wal record count tests \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
  '    #[tokio::test]',
  '    async fn s11_ws1_25_rows_value_search_returns_ok() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = operator_headers("test-key", "admin");',
  '        let params = Query(RowsValueSearchQuery { value: "test".to_string() });',
  '        let (status, Json(body)) = rows_value_search(State(state), headers, params).await.unwrap();',
  '        assert_eq!(status, StatusCode::OK);',
  '        assert_eq!(body.status, "ok");',
  '    }',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_25_rows_value_search_missing_auth_returns_401() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = HeaderMap::new();',
  '        let params = Query(RowsValueSearchQuery { value: "test".to_string() });',
  '        let result = rows_value_search(State(state), headers, params).await;',
  '        assert!(result.is_err());',
  '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
  '    }',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_25_wal_record_count_returns_ok() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = operator_headers("test-key", "admin");',
  '        let (status, Json(body)) = wal_record_count(State(state), headers).await.unwrap();',
  '        assert_eq!(status, StatusCode::OK);',
  '        assert_eq!(body.status, "ok");',
  '    }',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_25_wal_record_count_missing_auth_returns_401() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = HeaderMap::new();',
  '        let result = wal_record_count(State(state), headers).await;',
  '        assert!(result.is_err());',
  '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
  '    }',
];
lines.splice(s24Close + 1, 0, ...testBlock);
console.log('Tests inserted.');

fs.writeFileSync(filePath, lines.join('\n'), 'utf8');
console.log('Done. Total lines:', lines.length);
