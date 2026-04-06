// fix_s34.js — Session 34: has_not (SQL), Not plan node (exec), store/rows/keys + wal/truncate (service)
// Run with: node fix_s34.js
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

  // 1A: Add has_not field after has_like
  [
`    /// True when the WHERE clause contains a LIKE or ILIKE predicate (S3-WS1-09).
    pub has_like: bool,
}`,
`    /// True when the WHERE clause contains a LIKE or ILIKE predicate (S3-WS1-09).
    pub has_like: bool,
    /// True when the WHERE clause contains a NOT keyword predicate (S3-WS1-10).
    pub has_not: bool,
}`,
    'ast.rs 1A: Add has_not field'
  ],

  // 1B: Add has_not detection before Ok(Statement::Select(stmt))
  [
`                // Detect LIKE / ILIKE predicate in WHERE (S3-WS1-09).
                if up.contains(" LIKE ") || up.contains(" ILIKE ") {
                    stmt.has_like = true;
                }
                Ok(Statement::Select(stmt))`,
`                // Detect LIKE / ILIKE predicate in WHERE (S3-WS1-09).
                if up.contains(" LIKE ") || up.contains(" ILIKE ") {
                    stmt.has_like = true;
                }
                // Detect NOT keyword predicate in WHERE (S3-WS1-10); exclude IS NOT NULL patterns.
                if up.contains(" NOT ") && !up.contains("IS NOT") {
                    stmt.has_not = true;
                }
                Ok(Statement::Select(stmt))`,
    'ast.rs 1B: Add has_not detection'
  ],

  // 1C: Append not_tests module after like_tests closing brace
  [
`    fn plain_select_has_like_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_like, "plain SELECT without LIKE must have has_like = false");
    }
}`,
`    fn plain_select_has_like_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_like, "plain SELECT without LIKE must have has_like = false");
    }
}

#[cfg(test)]
mod not_tests {
    use super::*;

    #[test]
    fn select_with_not_in_sets_has_not_true() {
        let stmt = parse_one("SELECT id FROM users WHERE id NOT IN (1, 2, 3)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_not, "NOT IN predicate must set has_not = true");
    }

    #[test]
    fn select_with_not_like_sets_has_not_true() {
        let stmt = parse_one("SELECT name FROM users WHERE name NOT LIKE '%admin%'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_not, "NOT LIKE predicate must set has_not = true");
    }

    #[test]
    fn plain_select_has_not_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_not, "plain SELECT without NOT must have has_not = false");
    }
}`,
    'ast.rs 1C: Append not_tests module'
  ],
]);

// ─── 2. planner.rs ────────────────────────────────────────────────────────────
editFile('crates/voltnuerongrid-exec/src/planner.rs', [

  // 2A: Add Not variant after Like in enum
  [
`    /// LIKE / ILIKE string pattern filter (from S3-WS1-09 has_like support).
    Like {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`,
`    /// LIKE / ILIKE string pattern filter (from S3-WS1-09 has_like support).
    Like {
        input: Box<LogicalPlan>,
    },
    /// NOT keyword predicate filter (from S3-WS1-10 has_not support).
    Not {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`,
    'planner.rs 2A: Add Not variant to enum'
  ],

  // 2B: Add Not arm to primary_table()
  [
`            LogicalPlan::InList { input } => input.primary_table(),
            LogicalPlan::Between { input } => input.primary_table(),
            LogicalPlan::Like { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`,
`            LogicalPlan::InList { input } => input.primary_table(),
            LogicalPlan::Between { input } => input.primary_table(),
            LogicalPlan::Like { input } => input.primary_table(),
            LogicalPlan::Not { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`,
    'planner.rs 2B: Add Not arm to primary_table()'
  ],

  // 2C: Add Not arm to has_aggregation()
  [
`            LogicalPlan::InList { input } => input.has_aggregation(),
            LogicalPlan::Between { input } => input.has_aggregation(),
            LogicalPlan::Like { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`,
`            LogicalPlan::InList { input } => input.has_aggregation(),
            LogicalPlan::Between { input } => input.has_aggregation(),
            LogicalPlan::Like { input } => input.has_aggregation(),
            LogicalPlan::Not { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`,
    'planner.rs 2C: Add Not arm to has_aggregation()'
  ],

  // 2D: Add Not arm to estimate_cost()
  [
`            LogicalPlan::Like { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.7) as u64,
                    relative_cost: inner.relative_cost + 1.2,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`,
`            LogicalPlan::Like { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.7) as u64,
                    relative_cost: inner.relative_cost + 1.2,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Not { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.85) as u64,
                    relative_cost: inner.relative_cost + 0.6,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`,
    'planner.rs 2D: Add Not arm to estimate_cost()'
  ],

  // 2E: Convert bare if/else for Like into let after_like, add Not block
  [
`        // Like wrapper (S3-WS1-09 has_like detection): outermost node.
        if sel.has_like {
            LogicalPlan::Like {
                input: Box::new(after_between),
            }
        } else {
            after_between
        }
    }`,
`        // Like wrapper (S3-WS1-09 has_like detection): outermost node.
        let after_like = if sel.has_like {
            LogicalPlan::Like {
                input: Box::new(after_between),
            }
        } else {
            after_between
        };

        // Not wrapper (S3-WS1-10 has_not detection): outermost node.
        if sel.has_not {
            LogicalPlan::Not {
                input: Box::new(after_like),
            }
        } else {
            after_like
        }
    }`,
    'planner.rs 2E: Add Not wrapper in plan_select()'
  ],

  // 2F: Add 2 new tests at end of test module
  [
`    #[test]
    fn cost_like_query_routes_to_olap() {
        let c = cost("SELECT name FROM users WHERE name LIKE '%Alice%'");
        assert_eq!(c.recommended_path, QueryPath::Olap, "LIKE should route to OLAP (full scan)");
        assert!(c.relative_cost >= 1.2, "Like must carry at least 1.2 cost overhead");
    }
}`,
`    #[test]
    fn cost_like_query_routes_to_olap() {
        let c = cost("SELECT name FROM users WHERE name LIKE '%Alice%'");
        assert_eq!(c.recommended_path, QueryPath::Olap, "LIKE should route to OLAP (full scan)");
        assert!(c.relative_cost >= 1.2, "Like must carry at least 1.2 cost overhead");
    }

    #[test]
    fn planner_not_select_produces_not_node() {
        let p = plan("SELECT id FROM users WHERE id NOT IN (1, 2, 3)");
        assert!(
            matches!(&p, LogicalPlan::Not { .. }),
            "NOT predicate query should produce outermost Not node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_not_query_routes_to_oltp() {
        let c = cost("SELECT id FROM users WHERE id NOT IN (1, 2, 3)");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "NOT predicate should route to OLTP");
        assert!(c.relative_cost >= 0.6, "Not must carry at least 0.6 cost overhead");
    }
}`,
    'planner.rs 2F: Add Not planner tests'
  ],
]);

// ─── 3. main.rs ───────────────────────────────────────────────────────────────
editFile('services/voltnuerongridd/src/main.rs', [

  // 3A: Add StoreRowsKeys + WalTruncate structs after ConnectorUpdateResponse
  [
`#[derive(Serialize)]
struct ConnectorUpdateResponse {
    status: &'static str,
    connector_id: String,
    updated: bool,
}

// ─── S7-WS6-04: Chaos fire-drill structs ────────────────────────────────────`,
`#[derive(Serialize)]
struct ConnectorUpdateResponse {
    status: &'static str,
    connector_id: String,
    updated: bool,
}

// ─── S11-WS1-10: Row store keys structs ─────────────────────────────────────

#[derive(Debug, Deserialize)]
struct StoreRowsKeysQuery {
    prefix: Option<String>,
}

#[derive(Serialize)]
struct StoreRowsKeysResponse {
    status: &'static str,
    total_keys: usize,
    keys: Vec<String>,
}

// ─── S11-WS1-10: WAL truncate structs ───────────────────────────────────────

#[derive(Debug, Deserialize)]
struct WalTruncateRequest {
    up_to_sequence: u64,
}

#[derive(Serialize)]
struct WalTruncateResponse {
    status: &'static str,
    records_removed: usize,
    new_record_count: usize,
    truncated: bool,
}

// ─── S7-WS6-04: Chaos fire-drill structs ────────────────────────────────────`,
    'main.rs 3A: Add StoreRowsKeys + WalTruncate structs'
  ],

  // 3B: Add store/rows/keys route after row_store_delete
  [
`        // S2-WS2-04: Row store delete by key
        .route("/api/v1/store/rows/delete", post(row_store_delete))
        // S5-WS4A-02: Broker adapter status + flush`,
`        // S2-WS2-04: Row store delete by key
        .route("/api/v1/store/rows/delete", post(row_store_delete))
        // S11-WS1-10: Row store key list
        .route("/api/v1/store/rows/keys", get(store_rows_keys))
        // S5-WS4A-02: Broker adapter status + flush`,
    'main.rs 3B: Add /store/rows/keys route'
  ],

  // 4B: Add WAL truncate route after wal_checkpoint_history
  [
`        // S2-WS2-02: WAL checkpoint history
        .route("/api/v1/store/wal/checkpoint/history", get(wal_checkpoint_history))
        // S2-WS2-02: WAL segment list (checkpoint groups)`,
`        // S2-WS2-02: WAL checkpoint history
        .route("/api/v1/store/wal/checkpoint/history", get(wal_checkpoint_history))
        // S11-WS1-10: WAL truncate up to sequence
        .route("/api/v1/store/wal/truncate", post(wal_truncate))
        // S2-WS2-02: WAL segment list (checkpoint groups)`,
    'main.rs 4B: Add /store/wal/truncate route'
  ],

  // 3C+4C: Add store_rows_keys and wal_truncate handlers after connector_update
  [
`    Ok((StatusCode::OK, Json(ConnectorUpdateResponse {
        status: "ok",
        connector_id: req.connector_id,
        updated,
    })))
}

// ─── S7-WS6-01: Raft vote statistics endpoint ───────────────────────────────`,
`    Ok((StatusCode::OK, Json(ConnectorUpdateResponse {
        status: "ok",
        connector_id: req.connector_id,
        updated,
    })))
}

// ─── S11-WS1-10: Row store key list endpoint ─────────────────────────────────

/// S11-WS1-10: Return primary keys from the row store, optionally filtered by prefix.
async fn store_rows_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<StoreRowsKeysQuery>,
) -> Result<(StatusCode, Json<StoreRowsKeysResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock store_rows_keys");
    let all_rows = rs.export_rows_snapshot();
    drop(rs);
    let keys: Vec<String> = all_rows
        .into_iter()
        .map(|(k, _)| k)
        .filter(|k| {
            params.prefix.as_ref().map(|p| k.starts_with(p.as_str())).unwrap_or(true)
        })
        .collect();
    let total_keys = keys.len();
    Ok((StatusCode::OK, Json(StoreRowsKeysResponse {
        status: "ok",
        total_keys,
        keys,
    })))
}

// ─── S11-WS1-10: WAL truncate endpoint ───────────────────────────────────────

/// S11-WS1-10: Truncate WAL records up to a given sequence by forcing a checkpoint.
async fn wal_truncate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<WalTruncateRequest>,
) -> Result<(StatusCode, Json<WalTruncateResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let records_before = {
        let wal = state.wal_engine.lock().expect("wal_engine lock wal_truncate_before");
        wal.wal_records().len()
    };
    let latest_seq = {
        let wal = state.wal_engine.lock().expect("wal_engine lock wal_truncate_seq");
        wal.latest_sequence()
    };
    let truncated = if latest_seq >= req.up_to_sequence && records_before > 0 {
        let mut wal = state.wal_engine.lock().expect("wal_engine lock wal_truncate_cp");
        wal.force_checkpoint();
        true
    } else {
        false
    };
    let new_record_count = {
        let wal = state.wal_engine.lock().expect("wal_engine lock wal_truncate_after");
        wal.wal_records().len()
    };
    let records_removed = records_before.saturating_sub(new_record_count);
    Ok((StatusCode::OK, Json(WalTruncateResponse {
        status: "ok",
        records_removed,
        new_record_count,
        truncated,
    })))
}

// ─── S7-WS6-01: Raft vote statistics endpoint ───────────────────────────────`,
    'main.rs 3C+4C: Add store_rows_keys and wal_truncate handlers'
  ],

  // 3D+4D: Add 4 new tests before end of test module
  [
`    #[tokio::test]
    async fn s5_e4a_01_connector_update_missing_returns_updated_false() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = ConnectorUpdateRequest {
            connector_id: "no-such-connector".to_string(),
            version: Some("9.9.9".to_string()),
            signed: None,
        };
        let (status, Json(body)) = connector_update(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.updated, "missing connector must return updated = false");
    }

}`,
`    #[tokio::test]
    async fn s5_e4a_01_connector_update_missing_returns_updated_false() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = ConnectorUpdateRequest {
            connector_id: "no-such-connector".to_string(),
            version: Some("9.9.9".to_string()),
            signed: None,
        };
        let (status, Json(body)) = connector_update(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.updated, "missing connector must return updated = false");
    }

    // ─── S11-WS1-10: Row store keys endpoint tests ────────────────────────────

    #[tokio::test]
    async fn s11_ws1_10_store_rows_keys_empty_on_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = store_rows_keys(
            State(state),
            headers,
            Query(StoreRowsKeysQuery { prefix: None }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_keys, 0, "fresh row store must have no keys");
        assert!(body.keys.is_empty());
    }

    #[tokio::test]
    async fn s11_ws1_10_store_rows_keys_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = store_rows_keys(
            State(state),
            headers,
            Query(StoreRowsKeysQuery { prefix: None }),
        ).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-10: WAL truncate endpoint tests ──────────────────────────────

    #[tokio::test]
    async fn s11_ws1_10_wal_truncate_empty_wal_returns_not_truncated() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = WalTruncateRequest { up_to_sequence: 1 };
        let (status, Json(body)) = wal_truncate(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.truncated, "empty WAL must return truncated = false");
        assert_eq!(body.records_removed, 0);
    }

    #[tokio::test]
    async fn s11_ws1_10_wal_truncate_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let req = WalTruncateRequest { up_to_sequence: 100 };
        let result = wal_truncate(State(state), headers, Json(req)).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

}`,
    'main.rs 3D+4D: Add store_rows_keys and wal_truncate tests'
  ],
]);

console.log('fix_s34.js complete.');
