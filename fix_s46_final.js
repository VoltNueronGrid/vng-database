#!/usr/bin/env node
// Session 46 FINAL — all main.rs changes in one clean script
// Run on the RESET S45 file (22217 elements when split by \n)
'use strict';
const fs = require('fs');
const path = require('path');

const filePath = path.join(__dirname, 'services', 'voltnuerongridd', 'src', 'main.rs');
let lines = fs.readFileSync(filePath, 'utf8').split('\n');
console.log('Start lines:', lines.length, '(expect 22217)');

// Helper: find first line index containing a string
function findFirst(str, fromIdx = 0) {
  for (let i = fromIdx; i < lines.length; i++) {
    if (lines[i].includes(str)) return i;
  }
  return -1;
}
function findLast(str) {
  for (let i = lines.length - 1; i >= 0; i--) {
    if (lines[i].includes(str)) return i;
  }
  return -1;
}

// ── 1. Structs: insert after `total_transactions: u64,` + `}` (the RowsXidHistoryResponse closing)
// total_transactions line is at index 2624, next line (2625) is the `}` closing the struct
const totalTransIdx = findFirst('total_transactions: u64');
const structCloseIdx = totalTransIdx + 1; // The `}` after the struct fields
// verify
console.log('Struct close at index:', structCloseIdx, '(line', structCloseIdx + 1, ') =', JSON.stringify(lines[structCloseIdx]));

const structInsert = [
  '',
  '// \u2500\u2500\u2500 S11-WS1-22: WAL age + rows first key structs \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
  '',
  '#[derive(Debug, Serialize)]',
  'struct WalAgeResponse {',
  '    status: &\'static str,',
  '    oldest_sequence: u64,',
  '    newest_sequence: u64,',
  '    sequence_span: u64,',
  '}',
  '',
  '#[derive(Debug, Serialize)]',
  'struct RowsFirstKeyResponse {',
  '    status: &\'static str,',
  '    has_key: bool,',
  '    first_key: String,',
  '}',
];
lines.splice(structCloseIdx + 1, 0, ...structInsert);
console.log('Inserted', structInsert.length, 'struct lines after index', structCloseIdx);

// ── 2. WAL age route: after /store/wal/unique/keys
const walUniqueIdx = findFirst('store/wal/unique/keys');
console.log('wal/unique/keys at index:', walUniqueIdx);
lines.splice(walUniqueIdx + 1, 0,
  '        // S11-WS1-22: WAL age (oldest/newest sequence span)',
  '        .route("/api/v1/store/wal/age", get(wal_age))'
);
console.log('Inserted wal/age route');

// ── 3. Rows first key route: after /store/rows/xid/history
const xidHistoryIdx = findFirst('store/rows/xid/history');
console.log('xid/history at index:', xidHistoryIdx);
lines.splice(xidHistoryIdx + 1, 0,
  '        // S11-WS1-22: First key in the row store (alphabetically)',
  '        .route("/api/v1/store/rows/first/key", get(rows_first_key))'
);
console.log('Inserted rows/first/key route');

// ── 4. Handlers: insert before the orphan "// ─── S" comment (which is right before
//    // ─── S7-WS6-01: Raft vote statistics endpoint)
// The orphan comment at "// \u2500\u2500\u2500 S" is immediately before the handler section header
// Find the S7-WS6-01 comment
const raftVoteIdx = findFirst('S7-WS6-01: Raft vote statistics endpoint');
console.log('raft vote at index:', raftVoteIdx);
// The orphan `// ─── S` is at raftVoteIdx - 2 (there's a blank line before the handler comment)
// Check what's there:
for (let i = raftVoteIdx - 3; i <= raftVoteIdx; i++) {
  console.log('  ' + (i+1) + ': ' + JSON.stringify(lines[i]));
}
// Insert handlers BEFORE the orphan comment (at raftVoteIdx - 2)
const insertHandlerAt = raftVoteIdx - 1; // before the orphan comment line

const walAgeHandler = [
  '// \u2500\u2500\u2500 S11-WS1-22: WAL age endpoint \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
  '',
  '/// S11-WS1-22: Return the oldest and newest WAL sequence numbers and their span.',
  'async fn wal_age(',
  '    State(state): State<AppState>,',
  '    headers: HeaderMap,',
  ') -> Result<(StatusCode, Json<WalAgeResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
  '    require_operator_auth(&headers, &state)?;',
  '    let wal = state.wal_engine.lock().expect("wal_engine lock wal_age");',
  '    let records = wal.wal_records();',
  '    let oldest_sequence = records.first().map(|r| r.sequence).unwrap_or(0);',
  '    let newest_sequence = records.last().map(|r| r.sequence).unwrap_or(0);',
  '    let sequence_span = newest_sequence.saturating_sub(oldest_sequence);',
  '    drop(wal);',
  '    Ok((StatusCode::OK, Json(WalAgeResponse {',
  '        status: "ok",',
  '        oldest_sequence,',
  '        newest_sequence,',
  '        sequence_span,',
  '    })))',
  '}',
  '',
  '// \u2500\u2500\u2500 S11-WS1-22: Rows first key endpoint \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
  '',
  '/// S11-WS1-22: Return the first (alphabetically smallest) key currently in the row store.',
  'async fn rows_first_key(',
  '    State(state): State<AppState>,',
  '    headers: HeaderMap,',
  ') -> Result<(StatusCode, Json<RowsFirstKeyResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
  '    require_operator_auth(&headers, &state)?;',
  '    let rs = state.row_store.lock().expect("row_store lock rows_first_key");',
  '    let snapshot = rs.export_rows_snapshot();',
  '    drop(rs);',
  '    let mut keys: Vec<String> = snapshot.into_iter().map(|(k, _)| k).collect();',
  '    keys.sort();',
  '    let first_key = keys.into_iter().next().unwrap_or_default();',
  '    let has_key = !first_key.is_empty();',
  '    Ok((StatusCode::OK, Json(RowsFirstKeyResponse {',
  '        status: "ok",',
  '        has_key,',
  '        first_key,',
  '    })))',
  '}',
  '',
];
lines.splice(insertHandlerAt, 0, ...walAgeHandler);
console.log('Inserted', walAgeHandler.length, 'handler lines before index', insertHandlerAt);

// ── 5. Tests: find the closing `    }` of s11_ws1_21_rows_xid_history_missing_auth_returns_401
//   and insert after it (before the blank line and mod close)
const s21MissingAuthIdx = findLast('xid_history_missing_auth_returns_401');
console.log('s21 missing auth fn declaration at index:', s21MissingAuthIdx);
// The fn close is a line with `    }` after the assertion lines.
// Search forward from s21MissingAuthIdx
let s21FnCloseIdx = -1;
for (let i = s21MissingAuthIdx + 1; i < lines.length; i++) {
  const stripped = lines[i].replace(/\r$/, '');
  if (stripped === '    }') { s21FnCloseIdx = i; break; }
}
console.log('s21 fn close at index:', s21FnCloseIdx, '(line', s21FnCloseIdx + 1, ') =', JSON.stringify(lines[s21FnCloseIdx]));

const s22Tests = [
  '',
  '    // \u2500\u2500 S11-WS1-22: WAL age + rows first key tests \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_22_wal_age_returns_ok_with_span() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = operator_headers("test-key", "admin");',
  '        let (status, Json(body)) = wal_age(State(state), headers).await.unwrap();',
  '        assert_eq!(status, StatusCode::OK);',
  '        assert_eq!(body.status, "ok");',
  '        assert_eq!(body.sequence_span, body.newest_sequence.saturating_sub(body.oldest_sequence));',
  '    }',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_22_wal_age_missing_auth_returns_401() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = HeaderMap::new();',
  '        let result = wal_age(State(state), headers).await;',
  '        assert!(result.is_err());',
  '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
  '    }',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_22_rows_first_key_returns_ok_empty_store() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = operator_headers("test-key", "admin");',
  '        let (status, Json(body)) = rows_first_key(State(state), headers).await.unwrap();',
  '        assert_eq!(status, StatusCode::OK);',
  '        assert_eq!(body.status, "ok");',
  '        assert!(!body.has_key, "fresh empty store must have has_key = false");',
  '        assert_eq!(body.first_key, "", "empty store must have empty first_key");',
  '    }',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_22_rows_first_key_missing_auth_returns_401() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = HeaderMap::new();',
  '        let result = rows_first_key(State(state), headers).await;',
  '        assert!(result.is_err());',
  '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
  '    }',
];
lines.splice(s21FnCloseIdx + 1, 0, ...s22Tests);
console.log('Inserted', s22Tests.length, 'test lines after s21 fn close');

// Verify final structure
console.log('\nFinal last 55 lines:');
for (let i = Math.max(0, lines.length - 55); i < lines.length; i++) {
  if (lines[i] !== undefined) console.log(i + 1, JSON.stringify(lines[i]));
}

fs.writeFileSync(filePath, lines.join('\n'), 'utf8');
console.log('\nDone! Total lines:', lines.length);
