// fix_s36.js — Session 36: has_coalesce (SQL), Coalesce plan node (exec), connectors/health + rows/page/stats (service)
// Run with: node fix_s36.js
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

  // 1A: Add has_coalesce field after has_case
  [
`    /// True when the query contains a CASE WHEN expression (S3-WS1-11).
    pub has_case: bool,
}`,
`    /// True when the query contains a CASE WHEN expression (S3-WS1-11).
    pub has_case: bool,
    /// True when the query contains a COALESCE() expression (S3-WS1-12).
    pub has_coalesce: bool,
}`,
    'ast.rs 1A: Add has_coalesce field'
  ],

  // 1B: Add has_coalesce detection before Ok(Statement::Select(stmt))
  [
`                // Detect CASE WHEN expression anywhere in the query (S3-WS1-11).
                if up.contains("CASE WHEN") {
                    stmt.has_case = true;
                }
                Ok(Statement::Select(stmt))`,
`                // Detect CASE WHEN expression anywhere in the query (S3-WS1-11).
                if up.contains("CASE WHEN") {
                    stmt.has_case = true;
                }
                // Detect COALESCE() expression anywhere in the query (S3-WS1-12).
                if up_trim.contains("COALESCE(") {
                    stmt.has_coalesce = true;
                }
                Ok(Statement::Select(stmt))`,
    'ast.rs 1B: Add has_coalesce detection'
  ],

  // 1C: Append coalesce_tests module after case_tests closing brace
  [
`    fn plain_select_has_case_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_case, "plain SELECT without CASE WHEN must have has_case = false");
    }
}`,
`    fn plain_select_has_case_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_case, "plain SELECT without CASE WHEN must have has_case = false");
    }
}

#[cfg(test)]
mod coalesce_tests {
    use super::*;

    #[test]
    fn select_with_coalesce_sets_has_coalesce_true() {
        let stmt = parse_one("SELECT COALESCE(name, 'unknown') FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_coalesce, "COALESCE() expression must set has_coalesce = true");
    }

    #[test]
    fn select_with_coalesce_in_where_sets_has_coalesce_true() {
        let stmt = parse_one("SELECT id FROM orders WHERE COALESCE(status, 'pending') = 'active'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_coalesce, "COALESCE() in WHERE clause must set has_coalesce = true");
    }

    #[test]
    fn plain_select_has_coalesce_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_coalesce, "plain SELECT without COALESCE must have has_coalesce = false");
    }
}`,
    'ast.rs 1C: Append coalesce_tests module'
  ],
]);

// ─── 2. planner.rs ────────────────────────────────────────────────────────────
editFile('crates/voltnuerongrid-exec/src/planner.rs', [

  // 2A: Add Coalesce variant after Case in enum
  [
`    /// CASE WHEN analytical expression (from S3-WS1-11 has_case support).
    Case {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`,
`    /// CASE WHEN analytical expression (from S3-WS1-11 has_case support).
    Case {
        input: Box<LogicalPlan>,
    },
    /// COALESCE() null-coalescing expression (from S3-WS1-12 has_coalesce support).
    Coalesce {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`,
    'planner.rs 2A: Add Coalesce variant to enum'
  ],

  // 2B: Add Coalesce arm to primary_table()
  [
`            LogicalPlan::Not { input } => input.primary_table(),
            LogicalPlan::Case { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`,
`            LogicalPlan::Not { input } => input.primary_table(),
            LogicalPlan::Case { input } => input.primary_table(),
            LogicalPlan::Coalesce { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`,
    'planner.rs 2B: Add Coalesce arm to primary_table()'
  ],

  // 2C: Add Coalesce arm to has_aggregation()
  [
`            LogicalPlan::Not { input } => input.has_aggregation(),
            LogicalPlan::Case { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`,
`            LogicalPlan::Not { input } => input.has_aggregation(),
            LogicalPlan::Case { input } => input.has_aggregation(),
            LogicalPlan::Coalesce { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`,
    'planner.rs 2C: Add Coalesce arm to has_aggregation()'
  ],

  // 2D: Add Coalesce arm to estimate_cost() after Case arm
  [
`            LogicalPlan::Case { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.9) as u64,
                    relative_cost: inner.relative_cost + 1.5,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`,
`            LogicalPlan::Case { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.9) as u64,
                    relative_cost: inner.relative_cost + 1.5,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Coalesce { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.3,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`,
    'planner.rs 2D: Add Coalesce arm to estimate_cost()'
  ],

  // 2E: Convert bare if/else for Case to let after_case, add Coalesce block
  [
`        // Case wrapper (S3-WS1-11 has_case detection): outermost node.
        if sel.has_case {
            LogicalPlan::Case {
                input: Box::new(after_not),
            }
        } else {
            after_not
        }
    }`,
`        // Case wrapper (S3-WS1-11 has_case detection): outermost node.
        let after_case = if sel.has_case {
            LogicalPlan::Case {
                input: Box::new(after_not),
            }
        } else {
            after_not
        };

        // Coalesce wrapper (S3-WS1-12 has_coalesce detection): outermost node.
        if sel.has_coalesce {
            LogicalPlan::Coalesce {
                input: Box::new(after_case),
            }
        } else {
            after_case
        }
    }`,
    'planner.rs 2E: Add Coalesce wrapper in plan_select()'
  ],

  // 2F: Add 2 new tests at end of test module
  [
`    #[test]
    fn cost_case_query_routes_to_olap() {
        let c = cost("SELECT id, CASE WHEN age > 18 THEN 'adult' ELSE 'minor' END FROM users");
        assert_eq!(c.recommended_path, QueryPath::Olap, "CASE WHEN should route to OLAP");
        assert!(c.relative_cost >= 1.5, "Case must carry at least 1.5 cost overhead");
    }
}`,
`    #[test]
    fn cost_case_query_routes_to_olap() {
        let c = cost("SELECT id, CASE WHEN age > 18 THEN 'adult' ELSE 'minor' END FROM users");
        assert_eq!(c.recommended_path, QueryPath::Olap, "CASE WHEN should route to OLAP");
        assert!(c.relative_cost >= 1.5, "Case must carry at least 1.5 cost overhead");
    }

    #[test]
    fn planner_coalesce_select_produces_coalesce_node() {
        let p = plan("SELECT COALESCE(name, 'unknown') FROM users");
        assert!(
            matches!(&p, LogicalPlan::Coalesce { .. }),
            "COALESCE() query should produce outermost Coalesce node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_coalesce_query_routes_to_oltp() {
        let c = cost("SELECT COALESCE(name, 'unknown') FROM users");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "COALESCE should route to OLTP");
        assert!(c.relative_cost >= 0.3, "Coalesce must carry at least 0.3 cost overhead");
    }
}`,
    'planner.rs 2F: Add Coalesce planner tests'
  ],
]);

// ─── 3. main.rs ───────────────────────────────────────────────────────────────
editFile('services/voltnuerongridd/src/main.rs', [

  // 3A: Add ConnectorHealth + RowsPageStats structs after HtapStatsResponse
  [
`// ─── S7-WS6-04: Chaos fire-drill structs ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChaosFireDrillRequest {
    drill_type: String,`,
`// ─── S11-WS1-12: Connector health structs ───────────────────────────────────

#[derive(Debug, Serialize)]
struct ConnectorHealthEntry {
    connector_id: String,
    connector_type: String,
    version: String,
    signed: bool,
    healthy: bool,
}

#[derive(Debug, Serialize)]
struct ConnectorHealthResponse {
    status: &'static str,
    total: usize,
    healthy: usize,
    entries: Vec<ConnectorHealthEntry>,
}

// ─── S11-WS1-12: Row store page stats structs ────────────────────────────────

#[derive(Debug, Serialize)]
struct RowsPageStatsResponse {
    status: &'static str,
    page_count: usize,
    total_rows: usize,
    visible_rows: usize,
    current_xid: u64,
}

// ─── S7-WS6-04: Chaos fire-drill structs ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChaosFireDrillRequest {
    drill_type: String,`,
    'main.rs 3A: Add ConnectorHealth + RowsPageStats structs'
  ],

  // 3B: Add connectors/health route after connectors/update
  [
`        // S5-E4A-01: Connector update (version / signed flag)
        .route("/api/v1/connectors/update", post(connector_update))
        .route("/api/v1/ai/policy/update", post(ai_policy_update))`,
`        // S5-E4A-01: Connector update (version / signed flag)
        .route("/api/v1/connectors/update", post(connector_update))
        // S11-WS1-12: Connector health check
        .route("/api/v1/connectors/health", get(connectors_health))
        .route("/api/v1/ai/policy/update", post(ai_policy_update))`,
    'main.rs 3B: Add /connectors/health route'
  ],

  // 4B: Add rows/page/stats route after rows/version
  [
`        // S11-WS1-11: Row store version / current transaction ID
        .route("/api/v1/store/rows/version", get(row_store_version))
        // S5-WS4A-02: Broker adapter status + flush`,
`        // S11-WS1-11: Row store version / current transaction ID
        .route("/api/v1/store/rows/version", get(row_store_version))
        // S11-WS1-12: Row store page-level stats
        .route("/api/v1/store/rows/page/stats", get(rows_page_stats))
        // S5-WS4A-02: Broker adapter status + flush`,
    'main.rs 4B: Add /store/rows/page/stats route'
  ],

  // 3C+4C: Add connectors_health and rows_page_stats handlers after htap_stats handler
  [
`// ─── S7-WS6-01: Raft vote statistics endpoint ───────────────────────────────

/// S7-WS6-01: Return accumulated vote grant/reject counts for the current Raft node.`,
`// ─── S11-WS1-12: Connector health check endpoint ────────────────────────────

/// S11-WS1-12: Return health status for all registered connectors.
async fn connectors_health(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ConnectorHealthResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let registry = state.connector_registry.lock().expect("connector_registry lock connectors_health");
    let entries: Vec<ConnectorHealthEntry> = registry.iter().map(|c| ConnectorHealthEntry {
        connector_id: c.connector_id.clone(),
        connector_type: c.connector_type.clone(),
        version: c.version.clone(),
        signed: c.signed,
        // Scaffold: signed connectors are considered healthy; unsigned ones are degraded.
        healthy: c.signed,
    }).collect();
    let total = entries.len();
    let healthy = entries.iter().filter(|e| e.healthy).count();
    drop(registry);
    Ok((StatusCode::OK, Json(ConnectorHealthResponse {
        status: "ok",
        total,
        healthy,
        entries,
    })))
}

// ─── S11-WS1-12: Row store page stats endpoint ───────────────────────────────

/// S11-WS1-12: Return page-level statistics from the MVCC row store.
async fn rows_page_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowsPageStatsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock rows_page_stats");
    let page_count = rs.page_count();
    let total_rows = rs.total_row_count();
    let current_xid = rs.current_xid();
    let visible_rows = rs.visible_row_count(current_xid);
    drop(rs);
    Ok((StatusCode::OK, Json(RowsPageStatsResponse {
        status: "ok",
        page_count,
        total_rows,
        visible_rows,
        current_xid,
    })))
}

// ─── S7-WS6-01: Raft vote statistics endpoint ───────────────────────────────

/// S7-WS6-01: Return accumulated vote grant/reject counts for the current Raft node.`,
    'main.rs 3C+4C: Add connectors_health and rows_page_stats handlers'
  ],

  // 3D+4D: Add 4 new tests before end of test module
  [
`    #[tokio::test]
    async fn s11_ws1_11_htap_stats_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = htap_stats(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

}`,
`    #[tokio::test]
    async fn s11_ws1_11_htap_stats_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = htap_stats(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-12: Connector health endpoint tests ──────────────────────────

    #[tokio::test]
    async fn s11_ws1_12_connectors_health_empty_registry() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = connectors_health(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total, 0, "fresh registry must have no connectors");
        assert_eq!(body.healthy, 0);
    }

    #[tokio::test]
    async fn s11_ws1_12_connectors_health_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = connectors_health(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-12: Row store page stats endpoint tests ──────────────────────

    #[tokio::test]
    async fn s11_ws1_12_rows_page_stats_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_page_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.visible_rows, 0, "fresh row store must have no visible rows");
        assert_eq!(body.current_xid, 0);
    }

    #[tokio::test]
    async fn s11_ws1_12_rows_page_stats_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_page_stats(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

}`,
    'main.rs 3D+4D: Add connectors_health and rows_page_stats tests'
  ],
]);

console.log('fix_s36.js complete.');
