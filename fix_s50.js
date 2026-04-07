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

// ── 1. Structs: insert after WalRecordCountResponse closing } ────────────────
let wrcStructStart = findFirst(l => l.includes('struct WalRecordCountResponse'));
if (wrcStructStart === -1) { console.error('ERROR: WalRecordCountResponse struct not found'); process.exit(1); }
let wrcStructClose = -1;
for (let i = wrcStructStart + 1; i < wrcStructStart + 8; i++) {
  if (lines[i] && lines[i].replace(/\r/, '').trim() === '}') {
    wrcStructClose = i;
    break;
  }
}
if (wrcStructClose === -1) { console.error('ERROR: WalRecordCountResponse close not found'); process.exit(1); }
console.log('Struct insertion after line:', wrcStructClose + 1);

const structBlock = [
  '',
  '// \u2500\u2500\u2500 S11-WS1-26: Rows count range + WAL checkpoint age structs \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
  '#[derive(Debug, Deserialize)]',
  'struct RowsCountRangeQuery {',
  '    prefix: Option<String>,',
  '}',
  '',
  '#[derive(Debug, Serialize)]',
  'struct RowsCountRangeResponse {',
  '    status: &\'static str,',
  '    row_count: usize,',
  '    prefix: Option<String>,',
  '}',
  '',
  '#[derive(Debug, Serialize)]',
  'struct WalCheckpointAgeResponse {',
  '    status: &\'static str,',
  '    checkpoint_count: usize,',
  '    oldest_sequence: u64,',
  '    newest_sequence: u64,',
  '}',
];
lines.splice(wrcStructClose + 1, 0, ...structBlock);
console.log('Structs inserted.');

// ── 2. Routes: insert after wal/record/count route ────────────────────────────
let routeLine = findLast(l => l.includes('/api/v1/store/wal/record/count') && l.includes('get(wal_record_count)'));
if (routeLine === -1) { console.error('ERROR: wal/record/count route not found'); process.exit(1); }
console.log('Route insertion after line:', routeLine + 1);

const routeBlock = [
  '        // S11-WS1-26: Count rows optionally filtered by key prefix',
  '        .route("/api/v1/store/rows/count/range", get(rows_count_range))',
  '        // S11-WS1-26: WAL checkpoint age (oldest/newest seqno)',
  '        .route("/api/v1/store/wal/checkpoint/age", get(wal_checkpoint_age))',
];
lines.splice(routeLine + 1, 0, ...routeBlock);
console.log('Routes inserted.');

// ── 3. Handlers: insert before // ─── S7-WS6-01: Raft vote statistics endpoint ─
let raftAnchor = findFirst(l => l.includes('S7-WS6-01: Raft vote statistics endpoint'));
if (raftAnchor === -1) { console.error('ERROR: Raft vote anchor not found'); process.exit(1); }
console.log('Handler insertion before line:', raftAnchor + 1);

const handlerBlock = [
  '/// S11-WS1-26: Count rows optionally filtered by a key prefix.',
  'async fn rows_count_range(',
  '    State(state): State<AppState>,',
  '    headers: HeaderMap,',
  '    Query(params): Query<RowsCountRangeQuery>,',
  ') -> Result<(StatusCode, Json<RowsCountRangeResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
  '    require_operator_auth(&headers, &state)?;',
  '    let rs = state.row_store.lock().expect("row_store lock rows_count_range");',
  '    let snapshot = rs.export_rows_snapshot();',
  '    drop(rs);',
  '    let row_count = match &params.prefix {',
  '        Some(p) => snapshot.iter().filter(|(k, _)| k.starts_with(p.as_str())).count(),',
  '        None => snapshot.len(),',
  '    };',
  '    Ok((StatusCode::OK, Json(RowsCountRangeResponse {',
  '        status: "ok",',
  '        row_count,',
  '        prefix: params.prefix,',
  '    })))',
  '}',
  '',
  '/// S11-WS1-26: Return WAL checkpoint age (oldest/newest sequence numbers).',
  'async fn wal_checkpoint_age(',
  '    State(state): State<AppState>,',
  '    headers: HeaderMap,',
  ') -> Result<(StatusCode, Json<WalCheckpointAgeResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
  '    require_operator_auth(&headers, &state)?;',
  '    let wal = state.wal_engine.lock().expect("wal_engine lock wal_checkpoint_age");',
  '    let records = wal.wal_records();',
  '    let oldest_sequence = records.first().map(|r| r.sequence).unwrap_or(0);',
  '    let newest_sequence = records.last().map(|r| r.sequence).unwrap_or(0);',
  '    let checkpoint_count = wal.checkpoint_count();',
  '    drop(wal);',
  '    Ok((StatusCode::OK, Json(WalCheckpointAgeResponse {',
  '        status: "ok",',
  '        checkpoint_count,',
  '        oldest_sequence,',
  '        newest_sequence,',
  '    })))',
  '}',
  '',
];
lines.splice(raftAnchor, 0, ...handlerBlock);
console.log('Handlers inserted.');

// ── 4. Tests: insert after s11_ws1_25_wal_record_count_missing_auth fn close ─
let s25Idx = findLast(l => l.includes('s11_ws1_25_wal_record_count_missing_auth'));
if (s25Idx === -1) { console.error('ERROR: s25 test fn not found'); process.exit(1); }
let s25Close = -1;
for (let i = s25Idx + 1; i < lines.length; i++) {
  if (lines[i].replace(/\r/, '').trim() === '}') { s25Close = i; break; }
}
if (s25Close === -1) { console.error('ERROR: s25 fn close not found'); process.exit(1); }
console.log('Test insertion after line:', s25Close + 1);

const testBlock = [
  '',
  '    // \u2500\u2500 S11-WS1-26: rows count range + wal checkpoint age tests \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
  '    #[tokio::test]',
  '    async fn s11_ws1_26_rows_count_range_returns_ok() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = operator_headers("test-key", "admin");',
  '        let params = Query(RowsCountRangeQuery { prefix: None });',
  '        let (status, Json(body)) = rows_count_range(State(state), headers, params).await.unwrap();',
  '        assert_eq!(status, StatusCode::OK);',
  '        assert_eq!(body.status, "ok");',
  '    }',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_26_rows_count_range_missing_auth_returns_401() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = HeaderMap::new();',
  '        let params = Query(RowsCountRangeQuery { prefix: None });',
  '        let result = rows_count_range(State(state), headers, params).await;',
  '        assert!(result.is_err());',
  '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
  '    }',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_26_wal_checkpoint_age_returns_ok() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = operator_headers("test-key", "admin");',
  '        let (status, Json(body)) = wal_checkpoint_age(State(state), headers).await.unwrap();',
  '        assert_eq!(status, StatusCode::OK);',
  '        assert_eq!(body.status, "ok");',
  '    }',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_26_wal_checkpoint_age_missing_auth_returns_401() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = HeaderMap::new();',
  '        let result = wal_checkpoint_age(State(state), headers).await;',
  '        assert!(result.is_err());',
  '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
  '    }',
];
lines.splice(s25Close + 1, 0, ...testBlock);
console.log('Tests inserted.');

fs.writeFileSync(filePath, lines.join('\n'), 'utf8');
console.log('Done. Total lines:', lines.length);
