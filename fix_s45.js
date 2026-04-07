const fs = require('fs');
const content = fs.readFileSync('services/voltnuerongridd/src/main.rs', 'utf8');
const lines = content.split('\n');

console.log('Total lines before:', lines.length);
console.log('Line 22125:', lines[22125]);
console.log('Line 22126:', lines[22126]);
console.log('Line 22173:', lines[22173]);

// Lines 22125-22172 (0-indexed) contain:
//   22125: "    async fn s11_ws1_20_rows_tombstone_count_missing_auth_returns_401() {"
//   22126: "        let state = state_with_key(Some("test-key"));"
//   22127-22167: misplaced s21 tests
//   22168-22172: orphaned s20 body continuation + closing }
// We replace lines 22125-22172 with the correct s20 test body

const before = lines.slice(0, 22125); // up to line 22124 (the #[tokio::test] line)
const after = lines.slice(22173);     // from line 22174 onward (blank + s19 tests + closing })

const fixed = [
    '    async fn s11_ws1_20_rows_tombstone_count_missing_auth_returns_401() {',
    '        let state = state_with_key(Some("test-key"));',
    '        let headers = HeaderMap::new();',
    '        let result = rows_tombstone_count(State(state), headers).await;',
    '        assert!(result.is_err());',
    '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
    '    }',
];

const newLines = [...before, ...fixed, ...after];
fs.writeFileSync('services/voltnuerongridd/src/main.rs', newLines.join('\n'), 'utf8');
console.log('Done. New line count:', newLines.length);
// Verify
const check = fs.readFileSync('services/voltnuerongridd/src/main.rs', 'utf8').split('\n');
for (let i = 22120; i < 22135; i++) {
    console.log((i+1) + ': ' + check[i]);
}
