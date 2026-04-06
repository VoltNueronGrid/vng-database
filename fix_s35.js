// fix_s35.js — Session 35: has_case (SQL), Case plan node (exec), store/rows/version + store/htap/stats (service)
// Run with: node fix_s35.js
const fs = require('fs');

function editFile(path, replacements) {
  let raw = fs.readFileSync(path, 'utf8');
  let content = raw.replace(/\r\n/g, '\n');
  let changed = 0;
  for (const [from, to, label] of replacements) {
    if (content.includes(from)) {
      content = content.replace(from, to);
      changed++;
      console.log(`[OK]   ${label}`);
    } else {
      console.log(`[MISS] ${label}`);
    }
  }
  fs.writeFileSync(path, content.replace(/\n/g, '\r\n'), 'utf8');
  console.log(`  => Saved ${path}  (${changed}/${replacements.length} applied)\n`);
}

// ─── 1. ast.rs ────────────────────────────────────────────────────────────────
editFile('crates/voltnuerongrid-sql/src/ast.rs', [

  // 1A: Add has_case field after has_not
  [
`    /// True when the WHERE clause contains a NOT keyword predicate (S3-WS1-10).
    pub has_not: bool,
}`,
`    /// True when the WHERE clause contains a NOT keyword predicate (S3-WS1-10).
    pub has_not: bool,
    /// True when the query contains a CASE WHEN expression (S3-WS1-11).
    pub has_case: bool,
}`,
    'ast.rs 1A: Add has_case field'
  ],

  // 1B: Add has_case detection before Ok(Statement::Select(stmt))
  [
`                // Detect NOT keyword predicate in WHERE (S3-WS1-10); exclude IS NOT NULL patterns.
                if up.contains(" NOT ") && !up.contains("IS NOT") {
                    stmt.has_not = true;
                }
                Ok(Statement::Select(stmt))`,
`                // Detect NOT keyword predicate in WHERE (S3-WS1-10); exclude IS NOT NULL patterns.
                if up.contains(" NOT ") && !up.contains("IS NOT") {
                    stmt.has_not = true;
                }
                // Detect CASE WHEN expression anywhere in the query (S3-WS1-11).
                if up.contains("CASE WHEN") {
                    stmt.has_case = true;
                }
                Ok(Statement::Select(stmt))`,
    'ast.rs 1B: Add has_case detection'
  ],

  // 1C: Append case_tests module after not_tests closing brace
  [
`    fn plain_select_has_not_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_not, "plain SELECT without NOT must have has_not = false");
    }
}`,
`    fn plain_select_has_not_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_not, "plain SELECT without NOT must have has_not = false");
    }
}

#[cfg(test)]
mod case_tests {
    use super::*;

    #[test]
    fn select_with_case_when_sets_has_case_true() {
        let stmt = parse_one("SELECT id, CASE WHEN age > 18 THEN 'adult' ELSE 'minor' END AS category FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_case, "CASE WHEN expression must set has_case = true");
    }

    #[test]
    fn select_with_case_when_in_where_sets_has_case_true() {
        let stmt = parse_one("SELECT id FROM orders WHERE CASE WHEN status = 'active' THEN 1 ELSE 0 END = 1").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_case, "CASE WHEN in WHERE clause must set has_case = true");
    }

    #[test]
    fn plain_select_has_case_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_case, "plain SELECT without CASE WHEN must have has_case = false");
    }
}`,
    'ast.rs 1C: Append case_tests module'
  ],
]);

// ─── 2. planner.rs ────────────────────────────────────────────────────────────
editFile('crates/voltnuerongrid-exec/src/planner.rs', [

  // 2A: Add Case variant after Not in enum
  [
`    /// NOT keyword predicate filter (from S3-WS1-10 has_not support).
    Not {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`,
`    /// NOT keyword predicate filter (from S3-WS1-10 has_not support).
    Not {
        input: Box<LogicalPlan>,
    },
    /// CASE WHEN analytical expression (from S3-WS1-11 has_case support).
    Case {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`,
    'planner.rs 2A: Add Case variant to enum'
  ],

  // 2B: Add Case arm to primary_table()
  [
`            LogicalPlan::Like { input } => input.primary_table(),
            LogicalPlan::Not { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`,
`            LogicalPlan::Like { input } => input.primary_table(),
            LogicalPlan::Not { input } => input.primary_table(),
            LogicalPlan::Case { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`,
    'planner.rs 2B: Add Case arm to primary_table()'
  ],

  // 2C: Add Case arm to has_aggregation()
  [
`            LogicalPlan::Like { input } => input.has_aggregation(),
            LogicalPlan::Not { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`,
`            LogicalPlan::Like { input } => input.has_aggregation(),
            LogicalPlan::Not { input } => input.has_aggregation(),
            LogicalPlan::Case { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`,
    'planner.rs 2C: Add Case arm to has_aggregation()'
  ],

  // 2D: Add Case arm to estimate_cost() after Not arm
  [
`            LogicalPlan::Not { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.85) as u64,
                    relative_cost: inner.relative_cost + 0.6,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`,
`            LogicalPlan::Not { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.85) as u64,
                    relative_cost: inner.relative_cost + 0.6,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Case { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.9) as u64,
                    relative_cost: inner.relative_cost + 1.5,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`,
    'planner.rs 2D: Add Case arm to estimate_cost()'
  ],

  // 2E: Convert bare if/else for Not into let after_not, add Case block
  [
`        // Not wrapper (S3-WS1-10 has_not detection): outermost node.
        if sel.has_not {
            LogicalPlan::Not {
                input: Box::new(after_like),
            }
        } else {
            after_like
        }
    }`,
`        // Not wrapper (S3-WS1-10 has_not detection): outermost node.
        let after_not = if sel.has_not {
            LogicalPlan::Not {
                input: Box::new(after_like),
            }
        } else {
            after_like
        };

        // Case wrapper (S3-WS1-11 has_case detection): outermost node.
        if sel.has_case {
            LogicalPlan::Case {
                input: Box::new(after_not),
            }
        } else {
            after_not
        }
    }`,
    'planner.rs 2E: Add Case wrapper in plan_select()'
  ],

  // 2F: Add 2 new tests at end of test module
  [
`    #[test]
    fn cost_not_query_routes_to_oltp() {
        let c = cost("SELECT id FROM users WHERE id NOT IN (1, 2, 3)");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "NOT predicate should route to OLTP");
        assert!(c.relative_cost >= 0.6, "Not must carry at least 0.6 cost overhead");
    }
}`,
`    #[test]
    fn cost_not_query_routes_to_oltp() {
        let c = cost("SELECT id FROM users WHERE id NOT IN (1, 2, 3)");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "NOT predicate should route to OLTP");
        assert!(c.relative_cost >= 0.6, "Not must carry at least 0.6 cost overhead");
    }

    #[test]
    fn planner_case_select_produces_case_node() {
        let p = plan("SELECT id, CASE WHEN age > 18 THEN 'adult' ELSE 'minor' END AS cat FROM users");
        assert!(
            matches!(&p, LogicalPlan::Case { .. }),
            "CASE WHEN query should produce outermost Case node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_case_query_routes_to_olap() {
        let c = cost("SELECT id, CASE WHEN age > 18 THEN 'adult' ELSE 'minor' END FROM users");
        assert_eq!(c.recommended_path, QueryPath::Olap, "CASE WHEN should route to OLAP");
        assert!(c.relative_cost >= 1.5, "Case must carry at least 1.5 cost overhead");
    }
}`,
    'planner.rs 2F: Add Case planner tests'
  ],
]);

// ─── 3. main.rs ───────────────────────────────────────────────────────────────
editFile('services/voltnuerongridd/src/main.rs', [

  // 3A: Add RowStoreVersion + HtapStats structs after WalTruncateResponse
  [
`// ─── S7-WS6-04: Chaos fire-drill structs ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChaosFireDrillRequest {`,
`// ─── S11-WS1-11: Row store version structs ──────────────────────────────────

#[derive(Debug, Serialize)]
struct RowStoreVersionResponse {
    status: &'static str,
    current_xid: u64,
    page_count: usize,
    total_rows: usize,
}

// ─── S11-WS1-11: HTAP stats structs ─────────────────────────────────────────

#[derive(Debug, Serialize)]
struct HtapStatsResponse {
    status: &'static str,
    table_count: usize,
    total_entries: usize,
}

// ─── S7-WS6-04: Chaos fire-drill structs ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChaosFireDrillRequest {`,
    'main.rs 3A: Add RowStoreVersion + HtapStats structs'
  ],

  // 3B: Add store/rows/version route after store/rows/keys
  [
`        // S11-WS1-10: Row store key list
        .route("/api/v1/store/rows/keys", get(store_rows_keys))
        // S5-WS4A-02: Broker adapter status + flush`,
`        // S11-WS1-10: Row store key list
        .route("/api/v1/store/rows/keys", get(store_rows_keys))
        // S11-WS1-11: Row store version / current transaction ID
        .route("/api/v1/store/rows/version", get(row_store_version))
        // S5-WS4A-02: Broker adapter status + flush`,
    'main.rs 3B: Add /store/rows/version route'
  ],

  // 4B: Add store/htap/stats route after htap_status
  [
`        .route("/api/v1/store/htap/status", get(htap_status))`,
`        .route("/api/v1/store/htap/status", get(htap_status))
        // S11-WS1-11: HTAP OLAP store statistics
        .route("/api/v1/store/htap/stats", get(htap_stats))`,
    'main.rs 4B: Add /store/htap/stats route'
  ],

  // 3C+4C: Add row_store_version and htap_stats handlers after wal_truncate handler
  [
`// ─── S7-WS6-01: Raft vote statistics endpoint ───────────────────────────────

/// S7-WS6-01: Return accumulated vote grant/reject counts for the current Raft node.`,
`// ─── S11-WS1-11: Row store version endpoint ─────────────────────────────────

/// S11-WS1-11: Return the current transaction ID and basic stats for the MVCC row store.
async fn row_store_version(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowStoreVersionResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock row_store_version");
    let current_xid = rs.current_xid();
    let page_count = rs.page_count();
    let total_rows = rs.total_row_count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowStoreVersionResponse {
        status: "ok",
        current_xid,
        page_count,
        total_rows,
    })))
}

// ─── S11-WS1-11: HTAP stats endpoint ─────────────────────────────────────────

/// S11-WS1-11: Return entry counts from the in-memory OLAP replica store.
async fn htap_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<HtapStatsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let olap = state.olap_store.lock().expect("olap_store lock htap_stats");
    let table_count = olap.len();
    let total_entries: usize = olap.values().map(|rows| rows.len()).sum();
    drop(olap);
    Ok((StatusCode::OK, Json(HtapStatsResponse {
        status: "ok",
        table_count,
        total_entries,
    })))
}

// ─── S7-WS6-01: Raft vote statistics endpoint ───────────────────────────────

/// S7-WS6-01: Return accumulated vote grant/reject counts for the current Raft node.`,
    'main.rs 3C+4C: Add row_store_version and htap_stats handlers'
  ],

  // 3D+4D: Add 4 new tests before end of test module
  [
`    #[tokio::test]
    async fn s11_ws1_10_wal_truncate_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let req = WalTruncateRequest { up_to_sequence: 100 };
        let result = wal_truncate(State(state), headers, Json(req)).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

}`,
`    #[tokio::test]
    async fn s11_ws1_10_wal_truncate_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let req = WalTruncateRequest { up_to_sequence: 100 };
        let result = wal_truncate(State(state), headers, Json(req)).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-11: Row store version endpoint tests ─────────────────────────

    #[tokio::test]
    async fn s11_ws1_11_row_store_version_fresh_state_returns_zero_xid() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = row_store_version(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.current_xid, 0, "fresh row store must have xid 0");
        assert_eq!(body.total_rows, 0);
    }

    #[tokio::test]
    async fn s11_ws1_11_row_store_version_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = row_store_version(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-11: HTAP stats endpoint tests ────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_11_htap_stats_empty_olap_store() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = htap_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.table_count, 0, "fresh OLAP store must have no tables");
        assert_eq!(body.total_entries, 0);
    }

    #[tokio::test]
    async fn s11_ws1_11_htap_stats_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = htap_stats(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

}`,
    'main.rs 3D+4D: Add row_store_version and htap_stats tests'
  ],
]);

console.log('fix_s35.js complete.');
