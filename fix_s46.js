#!/usr/bin/env node
// Session 46 fix script — adds WAL age + rows first key endpoints to main.rs
'use strict';
const fs = require('fs');
const path = require('path');

const filePath = path.join(__dirname, 'services', 'voltnuerongridd', 'src', 'main.rs');
let lines = fs.readFileSync(filePath, 'utf8').split('\n');

// ── 1. Structs: insert after line 2626 (the closing } of RowsXidHistoryResponse)
// Line 2625 = "    total_transactions: u64," (1-indexed), so line 2626 = "}"
// We want to insert 18 new lines AFTER line 2626 (0-indexed index 2625)
const structInsertAfter = 2626; // 1-indexed line number of the closing "}"
const structLines = [
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
lines.splice(structInsertAfter, 0, ...structLines);
console.log('Inserted structs after line', structInsertAfter);

// ── 2. WAL age route: after /store/wal/unique/keys (now shifted by structLines.length)
// Original line 3921 + structLines.length offset
const walUniqueKeysLine = 3921 + structLines.length; // 1-indexed
// Find exact line (safer)
let walUniqueIdx = -1;
for (let i = 0; i < lines.length; i++) {
  if (lines[i].includes('store/wal/unique/keys')) { walUniqueIdx = i; break; }
}
if (walUniqueIdx < 0) { console.error('ERROR: could not find wal/unique/keys route'); process.exit(1); }
const walAgeRoute = [
  '        // S11-WS1-22: WAL age (oldest/newest sequence span)',
  '        .route("/api/v1/store/wal/age", get(wal_age))',
];
lines.splice(walUniqueIdx + 1, 0, ...walAgeRoute);
console.log('Inserted wal/age route after line', walUniqueIdx + 1);

// ── 3. Rows first key route: after /store/rows/xid/history (now shifted again)
let xidHistoryIdx = -1;
for (let i = 0; i < lines.length; i++) {
  if (lines[i].includes('store/rows/xid/history')) { xidHistoryIdx = i; break; }
}
if (xidHistoryIdx < 0) { console.error('ERROR: could not find rows/xid/history route'); process.exit(1); }
const firstKeyRoute = [
  '        // S11-WS1-22: First key in the row store (alphabetically)',
  '        .route("/api/v1/store/rows/first/key", get(rows_first_key))',
];
lines.splice(xidHistoryIdx + 1, 0, ...firstKeyRoute);
console.log('Inserted rows/first/key route after line', xidHistoryIdx + 1);

// ── 4. Handlers: insert before the orphan "// \u2500\u2500\u2500 S" comment (line 8853 originally)
// Find the "S7-WS6-01: Raft vote statistics endpoint" handler comment
let raftVoteHandlerIdx = -1;
for (let i = 0; i < lines.length; i++) {
  if (lines[i].includes('S7-WS6-01: Raft vote statistics endpoint')) {
    // We want the comment at the start of the block, which has the box-drawing chars
    raftVoteHandlerIdx = i;
    break;
  }
}
if (raftVoteHandlerIdx < 0) { console.error('ERROR: could not find S7-WS6-01 Raft vote handler comment'); process.exit(1); }
// The orphan "// \u2500\u2500\u2500 S" comment is the line just before, so insert before raftVoteHandlerIdx - 1
// (the blank line before the orphan comment), actually let's insert before the "// \u2500\u2500\u2500 S\n\n// \u2500\u2500\u2500 S7" block
// which means we insert 2 lines before raftVoteHandlerIdx (before the orphan "// --- S" line)
const handlerInsertBefore = raftVoteHandlerIdx - 1; // The orphan comment line
const handlerLines = [
  '// \u2500\u2500\u2500 S11-WS1-22: WAL age endpoint \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
  '',
  '/// S11-WS1-22: Return the oldest and newest WAL sequence numbers and their span.',
  'async fn wal_age(',
  '    State(state): State<AppState>,',
  '    headers: HeaderMap,',
  ') -> Result<(StatusCode, Json<WalAgeResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
  '    require_admin_key(&headers, &state)?;',
  '    let wal = state.wal.lock().await;',
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
  '    require_admin_key(&headers, &state)?;',
  '    let rs = state.row_store.lock().await;',
  '    let mut keys: Vec<String> = rs.export_rows_snapshot().into_keys().collect();',
  '    keys.sort();',
  '    let first_key = keys.into_iter().next().unwrap_or_default();',
  '    let has_key = !first_key.is_empty();',
  '    drop(rs);',
  '    Ok((StatusCode::OK, Json(RowsFirstKeyResponse {',
  '        status: "ok",',
  '        has_key,',
  '        first_key,',
  '    })))',
  '}',
  '',
];
lines.splice(handlerInsertBefore, 0, ...handlerLines);
console.log('Inserted handler functions before line', handlerInsertBefore + 1);

// ── 5. Tests: insert before the final closing "}" of the tests module
// Find the last occurrence of "xid_history_missing_auth_returns_401" to locate the end-of-tests region
let lastTestIdx = -1;
for (let i = lines.length - 1; i >= 0; i--) {
  if (lines[i].includes('xid_history_missing_auth_returns_401')) { lastTestIdx = i; break; }
}
if (lastTestIdx < 0) { console.error('ERROR: could not find xid_history_missing_auth test'); process.exit(1); }
// The END of that test function is 4 lines after (opening fn, assert, assert_eq, })
// then we have: }  (closing the test mod)
// Find the closing } of the tests mod after the last test
let closingBraceIdx = -1;
for (let i = lastTestIdx + 1; i < lines.length; i++) {
  if (lines[i].trim() === '}') { closingBraceIdx = i; break; }
}
if (closingBraceIdx < 0) { console.error('ERROR: could not find closing } of tests mod'); process.exit(1); }

const testLines = [
  '',
  '    // \u2500\u2500 S11-WS1-22: WAL age + rows first key tests \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
  '',
  '    #[tokio::test]',
  '    async fn s11_ws1_22_wal_age_returns_ok_with_span() {',
  '        let state = state_with_key(Some("test-key"));',
  '        let headers = admin_headers("test-key");',
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
  '        let headers = admin_headers("test-key");',
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
  '',
];
lines.splice(closingBraceIdx, 0, ...testLines);
console.log('Inserted tests before closing } at line', closingBraceIdx + 1);

// Write the file back
fs.writeFileSync(filePath, lines.join('\n'), 'utf8');
console.log('Done! Total lines:', lines.length);
