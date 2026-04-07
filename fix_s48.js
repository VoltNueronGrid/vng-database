#!/usr/bin/env node
// Session 48: has_in_subquery SQL field, InSubquery plan node,
//             GET /store/rows/count/distinct + GET /store/rows/key/exists endpoints
'use strict';
const fs = require('fs');

function applyAll() {
  // ════════════════════════════════════════════════════════════════
  // 1. ast.rs
  // ════════════════════════════════════════════════════════════════
  {
    let src = fs.readFileSync('crates/voltnuerongrid-sql/src/ast.rs', 'utf8');

    // 1a. Add field after has_interval
    src = src.replace(
      '    /// True when the query uses an INTERVAL expression (date arithmetic) (S3-WS1-23).\n    pub has_interval: bool,\n}',
      '    /// True when the query uses an INTERVAL expression (date arithmetic) (S3-WS1-23).\n    pub has_interval: bool,\n    /// True when the query uses an IN (SELECT ...) subquery predicate (S3-WS1-24).\n    pub has_in_subquery: bool,\n}'
    );

    // 1b. Add detection after INTERVAL block (Windows CRLF variant)
    src = src.replace(
      '                // Detect INTERVAL date arithmetic expressions (S3-WS1-23).\r\n                if up.contains("INTERVAL") {\r\n                    stmt.has_interval = true;\r\n                }\r\n                Ok(Statement::Select(stmt))',
      '                // Detect INTERVAL date arithmetic expressions (S3-WS1-23).\r\n                if up.contains("INTERVAL") {\r\n                    stmt.has_interval = true;\r\n                }\r\n                // Detect IN (SELECT ...) subquery predicate (S3-WS1-24).\r\n                if up.contains("IN (SELECT") || up.contains("IN(SELECT") {\r\n                    stmt.has_in_subquery = true;\r\n                }\r\n                Ok(Statement::Select(stmt))'
    );
    // LF-only variant fallback
    if (!src.includes('has_in_subquery = true')) {
      src = src.replace(
        '                // Detect INTERVAL date arithmetic expressions (S3-WS1-23).\n                if up.contains("INTERVAL") {\n                    stmt.has_interval = true;\n                }\n                Ok(Statement::Select(stmt))',
        '                // Detect INTERVAL date arithmetic expressions (S3-WS1-23).\n                if up.contains("INTERVAL") {\n                    stmt.has_interval = true;\n                }\n                // Detect IN (SELECT ...) subquery predicate (S3-WS1-24).\n                if up.contains("IN (SELECT") || up.contains("IN(SELECT") {\n                    stmt.has_in_subquery = true;\n                }\n                Ok(Statement::Select(stmt))'
      );
    }

    // 1c. Append test module after last test closing }
    const in_subquery_tests = `
// ─── S3-WS1-24: has_in_subquery tests ────────────────────────────────────────

#[cfg(test)]
mod in_subquery_tests {
    use super::*;

    #[test]
    fn select_with_in_subquery_sets_has_in_subquery() {
        let stmt = parse_one("SELECT id FROM orders WHERE user_id IN (SELECT id FROM users)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_in_subquery, "IN (SELECT ...) must set has_in_subquery = true");
    }

    #[test]
    fn in_subquery_detection_compact_form() {
        let stmt = parse_one("SELECT name FROM products WHERE cat_id IN(SELECT id FROM cats)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_in_subquery, "IN(SELECT...) compact form must set has_in_subquery = true");
    }

    #[test]
    fn plain_select_has_in_subquery_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_in_subquery, "plain SELECT without IN subquery must have has_in_subquery = false");
    }
}`;
    // Append before the very last `}` of the file
    const lastBrace = src.lastIndexOf('\n}');
    src = src.slice(0, lastBrace) + '\n' + in_subquery_tests + '\n}';

    fs.writeFileSync('crates/voltnuerongrid-sql/src/ast.rs', src, 'utf8');
    console.log('ast.rs updated');
  }

  // ════════════════════════════════════════════════════════════════
  // 2. planner.rs
  // ════════════════════════════════════════════════════════════════
  {
    let src = fs.readFileSync('crates/voltnuerongrid-exec/src/planner.rs', 'utf8');

    // 2a. Add InSubquery variant after Interval
    src = src.replace(
      '    /// INTERVAL date arithmetic expression (S3-WS1-23 has_interval support).\n    Interval {\n        input: Box<LogicalPlan>,\n    },\n    /// Window function',
      '    /// INTERVAL date arithmetic expression (S3-WS1-23 has_interval support).\n    Interval {\n        input: Box<LogicalPlan>,\n    },\n    /// IN (SELECT ...) subquery predicate (anti-join / semi-join pattern) (S3-WS1-24 has_in_subquery support).\n    InSubquery {\n        input: Box<LogicalPlan>,\n    },\n    /// Window function'
    );

    // 2b. primary_table arm after Interval
    src = src.replace(
      '            LogicalPlan::Interval { input } => input.primary_table(),\n            LogicalPlan::WindowFn',
      '            LogicalPlan::Interval { input } => input.primary_table(),\n            LogicalPlan::InSubquery { input } => input.primary_table(),\n            LogicalPlan::WindowFn'
    );

    // 2c. has_aggregation arm after Interval
    src = src.replace(
      '            LogicalPlan::Interval { input } => input.has_aggregation(),\n            LogicalPlan::WindowFn',
      '            LogicalPlan::Interval { input } => input.has_aggregation(),\n            LogicalPlan::InSubquery { input } => input.has_aggregation(),\n            LogicalPlan::WindowFn'
    );

    // 2d. estimate_cost arm after Interval arm
    src = src.replace(
      '            LogicalPlan::Interval { input } => {\n                let inner = Self::estimate_cost(input);\n                CostEstimate {\n                    estimated_rows: (inner.estimated_rows as f64 * 0.9) as u64,\n                    relative_cost: inner.relative_cost + 0.3,\n                    recommended_path: QueryPath::Olap,\n                }\n            }\n            LogicalPlan::WindowFn',
      '            LogicalPlan::Interval { input } => {\n                let inner = Self::estimate_cost(input);\n                CostEstimate {\n                    estimated_rows: (inner.estimated_rows as f64 * 0.9) as u64,\n                    relative_cost: inner.relative_cost + 0.3,\n                    recommended_path: QueryPath::Olap,\n                }\n            }\n            LogicalPlan::InSubquery { input } => {\n                let inner = Self::estimate_cost(input);\n                CostEstimate {\n                    estimated_rows: (inner.estimated_rows as f64 * 0.6) as u64,\n                    relative_cost: inner.relative_cost + 0.8,\n                    recommended_path: QueryPath::Olap,\n                }\n            }\n            LogicalPlan::WindowFn'
    );

    // 2e. plan_select: convert Interval to let after_interval + add InSubquery outermost
    src = src.replace(
      '        // Interval wrapper (S3-WS1-23 has_interval detection): outermost node.\n        if sel.has_interval {\n            LogicalPlan::Interval {\n                input: Box::new(after_trim),\n            }\n        } else {\n            after_trim\n        }\n    }',
      '        // Interval wrapper (S3-WS1-23 has_interval detection).\n        let after_interval = if sel.has_interval {\n            LogicalPlan::Interval {\n                input: Box::new(after_trim),\n            }\n        } else {\n            after_trim\n        };\n\n        // InSubquery wrapper (S3-WS1-24 has_in_subquery detection): outermost node.\n        if sel.has_in_subquery {\n            LogicalPlan::InSubquery {\n                input: Box::new(after_interval),\n            }\n        } else {\n            after_interval\n        }\n    }'
    );

    // 2f. Append tests before final closing }
    const new_tests = `
    // ── S3-WS1-24: InSubquery node tests ─────────────────────────────────────

    #[test]
    fn planner_in_subquery_select_produces_in_subquery_node() {
        let p = plan("SELECT id FROM orders WHERE user_id IN (SELECT id FROM users)");
        assert!(
            matches!(&p, LogicalPlan::InSubquery { .. }),
            "IN (SELECT ...) predicate must produce outermost InSubquery node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("orders"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_in_subquery_query_routes_to_olap() {
        let c = cost("SELECT name FROM products WHERE cat_id IN (SELECT id FROM cats)");
        assert_eq!(c.recommended_path, QueryPath::Olap, "IN subquery should route to OLAP");
        assert!(c.relative_cost >= 0.8, "InSubquery must carry at least 0.8 cost overhead");
    }`;
    src = src.replace(/\n}$/, new_tests + '\n}');

    fs.writeFileSync('crates/voltnuerongrid-exec/src/planner.rs', src, 'utf8');
    console.log('planner.rs updated');
  }

  // ════════════════════════════════════════════════════════════════
  // 3. main.rs — use splice-based approach for reliability
  // ════════════════════════════════════════════════════════════════
  {
    const lines = fs.readFileSync('services/voltnuerongridd/src/main.rs', 'utf8').split('\n');

    function findFirst(str, from = 0) {
      for (let i = from; i < lines.length; i++) if (lines[i].includes(str)) return i;
      return -1;
    }
    function findLast(str) {
      for (let i = lines.length - 1; i >= 0; i--) if (lines[i].includes(str)) return i;
      return -1;
    }

    // 3a. Structs: after RowsLastKeyResponse closing }
    const rowsLastKeyClose = findFirst("    last_key: String,") + 1; // next line is `}`
    console.log('RowsLastKey close at:', rowsLastKeyClose + 1, lines[rowsLastKeyClose]);
    lines.splice(rowsLastKeyClose + 1, 0,
      '',
      '// \u2500\u2500\u2500 S11-WS1-24: Rows count distinct + rows key exists structs \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
      '',
      '#[derive(Debug, Serialize)]',
      'struct RowsCountDistinctResponse {',
      '    status: &\'static str,',
      '    distinct_value_count: usize,',
      '}',
      '',
      '#[derive(Debug, Deserialize)]',
      'struct RowsKeyExistsQuery {',
      '    key: String,',
      '}',
      '',
      '#[derive(Debug, Serialize)]',
      'struct RowsKeyExistsResponse {',
      '    status: &\'static str,',
      '    key: String,',
      '    exists: bool,',
      '}'
    );
    console.log('Inserted structs');

    // 3b. Routes: after rows/last/key
    const lastKeyRoute = findFirst('store/rows/last/key');
    console.log('rows/last/key route at:', lastKeyRoute + 1);
    lines.splice(lastKeyRoute + 1, 0,
      '        // S11-WS1-24: Count of distinct row values in the store',
      '        .route("/api/v1/store/rows/count/distinct", get(rows_count_distinct))',
      '        // S11-WS1-24: Check if a given key exists in the row store',
      '        .route("/api/v1/store/rows/key/exists", get(rows_key_exists))'
    );
    console.log('Inserted routes');

    // 3c. Handlers: before S7-WS6-01 Raft vote handler comment
    const raftVote = findFirst('S7-WS6-01: Raft vote statistics endpoint');
    console.log('raft vote at:', raftVote + 1);
    // Find the blank line just before the raft vote comment (or the orphan // --- S comment before it)
    // Insert before the raft vote section (keep the orphan comment in place)
    const orphanLine = raftVote - 1; // usually blank or orphan comment
    lines.splice(orphanLine, 0,
      '// \u2500\u2500\u2500 S11-WS1-24: Rows count distinct endpoint \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
      '',
      '/// S11-WS1-24: Return the count of distinct row values in the MVCC row store.',
      'async fn rows_count_distinct(',
      '    State(state): State<AppState>,',
      '    headers: HeaderMap,',
      ') -> Result<(StatusCode, Json<RowsCountDistinctResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
      '    require_operator_auth(&headers, &state)?;',
      '    let rs = state.row_store.lock().expect("row_store lock rows_count_distinct");',
      '    let snapshot = rs.export_rows_snapshot();',
      '    drop(rs);',
      '    let mut distinct_values: Vec<String> = snapshot.into_iter().map(|(_, v)| {',
      '        v.into_iter().map(|(_, val)| val).next().unwrap_or_default()',
      '    }).collect();',
      '    distinct_values.sort();',
      '    distinct_values.dedup();',
      '    let distinct_value_count = distinct_values.len();',
      '    Ok((StatusCode::OK, Json(RowsCountDistinctResponse {',
      '        status: "ok",',
      '        distinct_value_count,',
      '    })))',
      '}',
      '',
      '// \u2500\u2500\u2500 S11-WS1-24: Rows key exists endpoint \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
      '',
      '/// S11-WS1-24: Return whether a given key exists in the MVCC row store.',
      'async fn rows_key_exists(',
      '    State(state): State<AppState>,',
      '    headers: HeaderMap,',
      '    Query(params): Query<RowsKeyExistsQuery>,',
      ') -> Result<(StatusCode, Json<RowsKeyExistsResponse>), (StatusCode, Json<AuthErrorResponse>)> {',
      '    require_operator_auth(&headers, &state)?;',
      '    let rs = state.row_store.lock().expect("row_store lock rows_key_exists");',
      '    let snapshot = rs.export_rows_snapshot();',
      '    drop(rs);',
      '    let exists = snapshot.contains_key(&params.key);',
      '    Ok((StatusCode::OK, Json(RowsKeyExistsResponse {',
      '        status: "ok",',
      '        key: params.key,',
      '        exists,',
      '    })))',
      '}',
      ''
    );
    console.log('Inserted handlers');

    // 3d. Tests: after s11_ws1_23 fn close, before mod close
    const s23MissingAuth = findLast('s11_ws1_23_rows_last_key_missing_auth');
    let s23FnClose = -1;
    for (let i = s23MissingAuth + 1; i < lines.length; i++) {
      if (lines[i].replace(/\r$/, '') === '    }') { s23FnClose = i; break; }
    }
    console.log('s23 fn close at:', s23FnClose + 1);

    lines.splice(s23FnClose + 1, 0,
      '',
      '    // \u2500\u2500 S11-WS1-24: Rows count distinct + rows key exists tests \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500',
      '',
      '    #[tokio::test]',
      '    async fn s11_ws1_24_rows_count_distinct_returns_ok() {',
      '        let state = state_with_key(Some("test-key"));',
      '        let headers = operator_headers("test-key", "admin");',
      '        let (status, Json(body)) = rows_count_distinct(State(state), headers).await.unwrap();',
      '        assert_eq!(status, StatusCode::OK);',
      '        assert_eq!(body.status, "ok");',
      '        assert_eq!(body.distinct_value_count, 0, "fresh store has no distinct values");',
      '    }',
      '',
      '    #[tokio::test]',
      '    async fn s11_ws1_24_rows_count_distinct_missing_auth_returns_401() {',
      '        let state = state_with_key(Some("test-key"));',
      '        let headers = HeaderMap::new();',
      '        let result = rows_count_distinct(State(state), headers).await;',
      '        assert!(result.is_err());',
      '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
      '    }',
      '',
      '    #[tokio::test]',
      '    async fn s11_ws1_24_rows_key_exists_returns_false_for_missing_key() {',
      '        let state = state_with_key(Some("test-key"));',
      '        let headers = operator_headers("test-key", "admin");',
      '        let params = Query(RowsKeyExistsQuery { key: "nonexistent".to_string() });',
      '        let (status, Json(body)) = rows_key_exists(State(state), headers, params).await.unwrap();',
      '        assert_eq!(status, StatusCode::OK);',
      '        assert_eq!(body.status, "ok");',
      '        assert!(!body.exists, "non-existent key must return exists = false");',
      '    }',
      '',
      '    #[tokio::test]',
      '    async fn s11_ws1_24_rows_key_exists_missing_auth_returns_401() {',
      '        let state = state_with_key(Some("test-key"));',
      '        let headers = HeaderMap::new();',
      '        let params = Query(RowsKeyExistsQuery { key: "k".to_string() });',
      '        let result = rows_key_exists(State(state), headers, params).await;',
      '        assert!(result.is_err());',
      '        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);',
      '    }'
    );
    console.log('Inserted tests');

    fs.writeFileSync('services/voltnuerongridd/src/main.rs', lines.join('\n'), 'utf8');
    console.log('main.rs updated. Total lines:', lines.length);
  }
}

applyAll();
