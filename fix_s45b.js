const fs = require('fs');
const content = fs.readFileSync('services/voltnuerongridd/src/main.rs', 'utf8');
const lines = content.split('\n');

console.log('Total before:', lines.length);

// Insert before the final "}" which is at index 22173 (line 22174)
const insertAt = 22173; // 0-indexed line of the closing "}"

const s21tests = [
    '',
    '    // ─── S11-WS1-21: WAL unique keys endpoint tests ───────────────────────────',
    '',
    '    #[tokio::test]',
    '    async fn s11_ws1_21_wal_unique_keys_fresh_wal_returns_zero() {',
    '        let state = state_with_key(Some("test-key"));',
    '        let headers = operator_headers("test-key", "admin");',
    '        let (status, Json(body)) = wal_unique_keys(State(state), headers).await.unwrap();',
    '        assert_eq!(status, StatusCode::OK);',
    '        assert_eq!(body.unique_key_count, 0, "fresh WAL must have zero unique keys");',
    '    }',
    '',
    '    #[tokio::test]',
    '    async fn s11_ws1_21_wal_unique_keys_missing_auth_returns_401() {',
    '        let state = state_with_key(Some("test-key"));',
    '        let headers = HeaderMap::new();',
    '        let result = wal_unique_keys(State(state), headers).await;',
    '        assert!(result.is_err());',
    '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
    '    }',
    '',
    '    // ─── S11-WS1-21: Rows XID history endpoint tests ──────────────────────────',
    '',
    '    #[tokio::test]',
    '    async fn s11_ws1_21_rows_xid_history_fresh_store_returns_zero_xid() {',
    '        let state = state_with_key(Some("test-key"));',
    '        let headers = operator_headers("test-key", "admin");',
    '        let (status, Json(body)) = rows_xid_history(State(state), headers).await.unwrap();',
    '        assert_eq!(status, StatusCode::OK);',
    '        assert_eq!(body.current_xid, 0, "fresh store must have current_xid = 0");',
    '        assert_eq!(body.next_xid, 1, "next_xid must be current_xid + 1");',
    '    }',
    '',
    '    #[tokio::test]',
    '    async fn s11_ws1_21_rows_xid_history_missing_auth_returns_401() {',
    '        let state = state_with_key(Some("test-key"));',
    '        let headers = HeaderMap::new();',
    '        let result = rows_xid_history(State(state), headers).await;',
    '        assert!(result.is_err());',
    '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
    '    }',
    '',
];

const before = lines.slice(0, insertAt);
const after = lines.slice(insertAt);
const newLines = [...before, ...s21tests, ...after];
fs.writeFileSync('services/voltnuerongridd/src/main.rs', newLines.join('\n'), 'utf8');
console.log('Done. New line count:', newLines.length);
// Verify end
const check = newLines;
for (let i = check.length - 10; i < check.length; i++) {
    console.log((i+1) + ': ' + check[i]);
}
