#!/usr/bin/env node
// Session 47 — ALL changes in one script:
//   SQL: has_interval + detection + tests
//   Exec: Interval variant + arms + plan_select letbinding + tests
//   Service: WalKeysList + RowsLastKey structs, routes, handlers, tests
'use strict';
const fs = require('fs');
const path = require('path');
const BASE = path.join(__dirname);

// ─────────────────────────────── ast.rs ─────────────────────────────────────
{
  const file = path.join(BASE, 'crates', 'voltnuerongrid-sql', 'src', 'ast.rs');
  let c = fs.readFileSync(file, 'utf8');

  // 1a. Add has_interval field after has_trim
  c = c.replace(
    '    /// True when the query contains a TRIM / LTRIM / RTRIM function call (S3-WS1-22).\n    pub has_trim: bool,',
    '    /// True when the query contains a TRIM / LTRIM / RTRIM function call (S3-WS1-22).\n    pub has_trim: bool,\n    /// True when the query uses an INTERVAL expression (date arithmetic) (S3-WS1-23).\n    pub has_interval: bool,'
  );
  console.log('ast.rs: added has_interval field');

  // 1b. Add INTERVAL detection after TRIM detection
  c = c.replace(
    '                // Detect TRIM / LTRIM / RTRIM function calls (S3-WS1-22).\n                if up_trim.contains("TRIM(") || up_trim.contains("LTRIM(") || up_trim.contains("RTRIM(") {\n                    stmt.has_trim = true;\n                }',
    '                // Detect TRIM / LTRIM / RTRIM function calls (S3-WS1-22).\n                if up_trim.contains("TRIM(") || up_trim.contains("LTRIM(") || up_trim.contains("RTRIM(") {\n                    stmt.has_trim = true;\n                }\n                // Detect INTERVAL date arithmetic (S3-WS1-23).\n                if up.contains("INTERVAL ") || up.contains("INTERVAL\'") {\n                    stmt.has_interval = true;\n                }'
  );
  console.log('ast.rs: added INTERVAL detection');

  // 1c. Add interval_tests module at end of file (before last `}`)
  const intervalTests = `
// ─── S3-WS1-23: has_interval tests ───────────────────────────────────────────

#[cfg(test)]
mod interval_tests {
    use super::*;

    #[test]
    fn select_with_interval_sets_has_interval() {
        let stmt = parse_one("SELECT created_at + INTERVAL '7 days' FROM events").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_interval, "INTERVAL expression must set has_interval = true");
    }

    #[test]
    fn interval_detection_alternate_form() {
        let stmt = parse_one("SELECT * FROM logs WHERE ts > NOW() - INTERVAL '1 hour'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_interval, "INTERVAL in WHERE clause must set has_interval = true");
    }

    #[test]
    fn plain_select_has_interval_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_interval, "plain SELECT without INTERVAL must have has_interval = false");
    }
}`;
  // Find end of trim_tests module (last `}` in file)
  const lastBrace = c.lastIndexOf('\n}');
  c = c.slice(0, lastBrace) + '\n}' + intervalTests;
  console.log('ast.rs: added interval_tests module');

  fs.writeFileSync(file, c, 'utf8');
}

// ─────────────────────────────── planner.rs ─────────────────────────────────
{
  const file = path.join(BASE, 'crates', 'voltnuerongrid-exec', 'src', 'planner.rs');
  let c = fs.readFileSync(file, 'utf8');

  // 2a. Add Interval variant after Trim
  c = c.replace(
    '    /// TRIM / LTRIM / RTRIM string function applied to result set (S3-WS1-22 has_trim support).\n    Trim {\n        input: Box<LogicalPlan>,\n    },',
    '    /// TRIM / LTRIM / RTRIM string function applied to result set (S3-WS1-22 has_trim support).\n    Trim {\n        input: Box<LogicalPlan>,\n    },\n    /// INTERVAL date arithmetic expression (S3-WS1-23 has_interval support).\n    Interval {\n        input: Box<LogicalPlan>,\n    },'
  );
  console.log('planner.rs: added Interval variant');

  // 2b. Add Interval arm in primary_table()
  c = c.replace(
    '            LogicalPlan::Trim { input } => input.primary_table(),\n            LogicalPlan::WindowFn { input, .. } => input.primary_table(),',
    '            LogicalPlan::Trim { input } => input.primary_table(),\n            LogicalPlan::Interval { input } => input.primary_table(),\n            LogicalPlan::WindowFn { input, .. } => input.primary_table(),'
  );
  console.log('planner.rs: added Interval primary_table arm');

  // 2c. Add Interval arm in has_aggregation()
  c = c.replace(
    '            LogicalPlan::Trim { input } => input.has_aggregation(),\n            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),',
    '            LogicalPlan::Trim { input } => input.has_aggregation(),\n            LogicalPlan::Interval { input } => input.has_aggregation(),\n            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),'
  );
  console.log('planner.rs: added Interval has_aggregation arm');

  // 2d. Add Interval arm in estimate_cost()
  c = c.replace(
    '            LogicalPlan::Trim { input } => {\n                let inner = Self::estimate_cost(input);\n                CostEstimate {\n                    estimated_rows: inner.estimated_rows,\n                    relative_cost: inner.relative_cost + 0.05,\n                    recommended_path: QueryPath::Oltp,\n                }\n            }\n            LogicalPlan::WindowFn { input, .. } => {',
    '            LogicalPlan::Trim { input } => {\n                let inner = Self::estimate_cost(input);\n                CostEstimate {\n                    estimated_rows: inner.estimated_rows,\n                    relative_cost: inner.relative_cost + 0.05,\n                    recommended_path: QueryPath::Oltp,\n                }\n            }\n            LogicalPlan::Interval { input } => {\n                let inner = Self::estimate_cost(input);\n                CostEstimate {\n                    estimated_rows: (inner.estimated_rows as f64 * 0.9) as u64,\n                    relative_cost: inner.relative_cost + 0.3,\n                    recommended_path: QueryPath::Olap,\n                }\n            }\n            LogicalPlan::WindowFn { input, .. } => {'
  );
  console.log('planner.rs: added Interval estimate_cost arm');

  // 2e. Convert Trim bare if/else to let + add Interval outermost
  c = c.replace(
    '        // Trim wrapper (S3-WS1-22 has_trim detection): outermost node.\n        if sel.has_trim {\n            LogicalPlan::Trim {\n                input: Box::new(after_not_in),\n            }\n        } else {\n            after_not_in\n        }\n    }',
    '        // Trim wrapper (S3-WS1-22 has_trim detection).\n        let after_trim = if sel.has_trim {\n            LogicalPlan::Trim {\n                input: Box::new(after_not_in),\n            }\n        } else {\n            after_not_in\n        };\n\n        // Interval wrapper (S3-WS1-23 has_interval detection): outermost node.\n        if sel.has_interval {\n            LogicalPlan::Interval {\n                input: Box::new(after_trim),\n            }\n        } else {\n            after_trim\n        }\n    }'
  );
  console.log('planner.rs: updated plan_select with Interval outermost');

  // 2f. Add 2 new tests at end
  const trimTestsEnd = c.lastIndexOf('\n}');
  const intervalPlannerTests = `

    // ── S3-WS1-23: Interval node tests ───────────────────────────────────────

    #[test]
    fn planner_interval_select_produces_interval_node() {
        let p = plan("SELECT created_at + INTERVAL '7 days' FROM events");
        assert!(
            matches!(&p, LogicalPlan::Interval { .. }),
            "INTERVAL expression must produce outermost Interval node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("events"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_interval_query_routes_to_olap() {
        let c = cost("SELECT * FROM logs WHERE ts > NOW() - INTERVAL '1 hour'");
        assert_eq!(c.recommended_path, QueryPath::Olap, "INTERVAL expressions should route to OLAP");
        assert!(c.relative_cost >= 0.3, "Interval must carry at least 0.3 cost overhead");
    }
}`;
  c = c.slice(0, trimTestsEnd) + intervalPlannerTests;
  console.log('planner.rs: added Interval planner tests');

  fs.writeFileSync(file, c, 'utf8');
}

// ─────────────────────────────── main.rs ────────────────────────────────────
{
  const file = path.join(BASE, 'services', 'voltnuerongridd', 'src', 'main.rs');
  let lines = fs.readFileSync(file, 'utf8').split('\n');
  console.log('main.rs start lines:', lines.length);

  function findFirst(str, from = 0) {
    for (let i = from; i < lines.length; i++) if (lines[i].includes(str)) return i;
    return -1;
  }
  function findLast(str) {
    for (let i = lines.length - 1; i >= 0; i--) if (lines[i].includes(str)) return i;
    return -1;
  }

  // 3a. Structs: insert after RowsFirstKeyResponse closing }
  // Find "first_key: String," then closing }
  const firstKeyFieldIdx = findFirst('first_key: String,');
  const firstKeyStructClose = firstKeyFieldIdx + 1; // the `}` line
  console.log('RowsFirstKey struct close at line:', firstKeyStructClose + 1, JSON.stringify(lines[firstKeyStructClose]));

  const structInsert = [
    '',
    '// \u2500\u2500\u2500 S11-WS1-23: WAL keys list + rows last key structs \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
    '',
    '#[derive(Debug, Serialize)]',
    'struct WalKeysListResponse {',
    '    status: &\'static str,',
    '    key_count: usize,',
    '    keys: Vec<String>,',
    '}',
    '',
    '#[derive(Debug, Serialize)]',
    'struct RowsLastKeyResponse {',
    '    status: &\'static str,',
    '    has_key: bool,',
    '    last_key: String,',
    '}',
  ];
  lines.splice(firstKeyStructClose + 1, 0, ...structInsert);
  console.log('main.rs: inserted structs');

  // 3b. Route: after /store/wal/age
  const walAgeRouteIdx = findFirst('store/wal/age", get(wal_age)');
  console.log('wal/age route at line:', walAgeRouteIdx + 1);
  lines.splice(walAgeRouteIdx + 1, 0,
    '        // S11-WS1-23: List all unique keys in the WAL',
    '        .route("/api/v1/store/wal/keys/list", get(wal_keys_list))'
  );
  console.log('main.rs: inserted wal/keys/list route');

  // 3c. Route: after /store/rows/first/key
  const firstKeyRouteIdx = findFirst('store/rows/first/key", get(rows_first_key)');
  console.log('rows/first/key route at line:', firstKeyRouteIdx + 1);
  lines.splice(firstKeyRouteIdx + 1, 0,
    '        // S11-WS1-23: Last alphabetically-sorted key in the row store',
    '        .route("/api/v1/store/rows/last/key", get(rows_last_key))'
  );
  console.log('main.rs: inserted rows/last/key route');

  // 3d. Handlers: insert before the orphaned "// ─── S" comment line
  // Find "S7-WS6-01: Raft vote statistics endpoint"
  const raftVoteIdx = findFirst('S7-WS6-01: Raft vote statistics endpoint');
  console.log('raft vote handler comment at line:', raftVoteIdx + 1);
  // Orphan comment is at raftVoteIdx - 1 (the `// ─── S` line), blank line at raftVoteIdx - 2
  // We insert before the orphan comment
  const insertHandlerAt = raftVoteIdx - 1;

  const handlers = [
    '// \u2500\u2500\u2500 S11-WS1-23: WAL keys list endpoint \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
    '',
    '/// S11-WS1-23: Return a deduplicated, sorted list of all keys present in the WAL.',
    'async fn wal_keys_list(',
    '    State(state): State<AppState>,',
    '    headers: HeaderMap,',
    ') -> Result<(StatusCode, Json<WalKeysListResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
    '    require_operator_auth(&headers, &state)?;',
    '    let wal = state.wal_engine.lock().expect("wal_engine lock wal_keys_list");',
    '    let records = wal.wal_records().to_vec();',
    '    drop(wal);',
    '    let mut keys: Vec<String> = records.into_iter().map(|r| r.key).collect();',
    '    keys.sort();',
    '    keys.dedup();',
    '    let key_count = keys.len();',
    '    Ok((StatusCode::OK, Json(WalKeysListResponse {',
    '        status: "ok",',
    '        key_count,',
    '        keys,',
    '    })))',
    '}',
    '',
    '// \u2500\u2500\u2500 S11-WS1-23: Rows last key endpoint \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
    '',
    '/// S11-WS1-23: Return the last (alphabetically largest) key currently in the row store.',
    'async fn rows_last_key(',
    '    State(state): State<AppState>,',
    '    headers: HeaderMap,',
    ') -> Result<(StatusCode, Json<RowsLastKeyResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
    '    require_operator_auth(&headers, &state)?;',
    '    let rs = state.row_store.lock().expect("row_store lock rows_last_key");',
    '    let snapshot = rs.export_rows_snapshot();',
    '    drop(rs);',
    '    let mut keys: Vec<String> = snapshot.into_iter().map(|(k, _)| k).collect();',
    '    keys.sort();',
    '    let last_key = keys.into_iter().last().unwrap_or_default();',
    '    let has_key = !last_key.is_empty();',
    '    Ok((StatusCode::OK, Json(RowsLastKeyResponse {',
    '        status: "ok",',
    '        has_key,',
    '        last_key,',
    '    })))',
    '}',
    '',
  ];
  lines.splice(insertHandlerAt, 0, ...handlers);
  console.log('main.rs: inserted handlers');

  // 3e. Tests: insert after the last s11_ws1_22 test closing }
  const lastS22TestFnDecl = findLast('async fn s11_ws1_22_rows_first_key_missing_auth_returns_401()');
  console.log('Last s22 test fn at line:', lastS22TestFnDecl + 1);
  // The fn close is a `    }` after the UNAUTHORIZED assert
  let s22FnCloseIdx = -1;
  for (let i = lastS22TestFnDecl + 1; i < lines.length; i++) {
    if (lines[i].replace(/\r$/, '') === '    }') { s22FnCloseIdx = i; break; }
  }
  console.log('s22 last test fn close at line:', s22FnCloseIdx + 1, JSON.stringify(lines[s22FnCloseIdx]));

  const s23Tests = [
    '',
    '    // \u2500\u2500 S11-WS1-23: WAL keys list + rows last key tests \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
    '',
    '    #[tokio::test]',
    '    async fn s11_ws1_23_wal_keys_list_returns_ok_empty_wal() {',
    '        let state = state_with_key(Some("test-key"));',
    '        let headers = operator_headers("test-key", "admin");',
    '        let (status, Json(body)) = wal_keys_list(State(state), headers).await.unwrap();',
    '        assert_eq!(status, StatusCode::OK);',
    '        assert_eq!(body.status, "ok");',
    '        assert_eq!(body.key_count, 0, "fresh WAL must have zero keys");',
    '        assert!(body.keys.is_empty(), "keys list must be empty for fresh WAL");',
    '    }',
    '',
    '    #[tokio::test]',
    '    async fn s11_ws1_23_wal_keys_list_missing_auth_returns_401() {',
    '        let state = state_with_key(Some("test-key"));',
    '        let headers = HeaderMap::new();',
    '        let result = wal_keys_list(State(state), headers).await;',
    '        assert!(result.is_err());',
    '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
    '    }',
    '',
    '    #[tokio::test]',
    '    async fn s11_ws1_23_rows_last_key_returns_ok_empty_store() {',
    '        let state = state_with_key(Some("test-key"));',
    '        let headers = operator_headers("test-key", "admin");',
    '        let (status, Json(body)) = rows_last_key(State(state), headers).await.unwrap();',
    '        assert_eq!(status, StatusCode::OK);',
    '        assert_eq!(body.status, "ok");',
    '        assert!(!body.has_key, "fresh empty store must have has_key = false");',
    '        assert_eq!(body.last_key, "", "empty store must have empty last_key");',
    '    }',
    '',
    '    #[tokio::test]',
    '    async fn s11_ws1_23_rows_last_key_missing_auth_returns_401() {',
    '        let state = state_with_key(Some("test-key"));',
    '        let headers = HeaderMap::new();',
    '        let result = rows_last_key(State(state), headers).await;',
    '        assert!(result.is_err());',
    '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
    '    }',
  ];
  lines.splice(s22FnCloseIdx + 1, 0, ...s23Tests);
  console.log('main.rs: inserted S23 tests');

  fs.writeFileSync(file, lines.join('\n'), 'utf8');
  console.log('main.rs done! Total lines:', lines.length);
}

console.log('\nAll Session 47 changes applied.');
