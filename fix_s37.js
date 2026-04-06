// fix_s37.js — Session 37: has_cast (SQL), Cast plan node (exec), ingest/schema/fields + wal/seq (service)
// Run with: node fix_s37.js
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

  // 1A: Add has_cast field after has_coalesce
  [
`    /// True when the query contains a COALESCE() expression (S3-WS1-12).
    pub has_coalesce: bool,
}`,
`    /// True when the query contains a COALESCE() expression (S3-WS1-12).
    pub has_coalesce: bool,
    /// True when the query contains a CAST() or :: type-cast expression (S3-WS1-13).
    pub has_cast: bool,
}`,
    'ast.rs 1A: Add has_cast field'
  ],

  // 1B: Add has_cast detection before Ok(Statement::Select(stmt))
  [
`                // Detect COALESCE() expression anywhere in the query (S3-WS1-12).
                if up_trim.contains("COALESCE(") {
                    stmt.has_coalesce = true;
                }
                Ok(Statement::Select(stmt))`,
`                // Detect COALESCE() expression anywhere in the query (S3-WS1-12).
                if up_trim.contains("COALESCE(") {
                    stmt.has_coalesce = true;
                }
                // Detect CAST() or :: type-cast expression anywhere in the query (S3-WS1-13).
                if up_trim.contains("CAST(") || up.contains("::") {
                    stmt.has_cast = true;
                }
                Ok(Statement::Select(stmt))`,
    'ast.rs 1B: Add has_cast detection'
  ],

  // 1C: Append cast_tests module after coalesce_tests closing brace
  [
`    fn plain_select_has_coalesce_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_coalesce, "plain SELECT without COALESCE must have has_coalesce = false");
    }
}`,
`    fn plain_select_has_coalesce_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_coalesce, "plain SELECT without COALESCE must have has_coalesce = false");
    }
}

#[cfg(test)]
mod cast_tests {
    use super::*;

    #[test]
    fn select_with_cast_sets_has_cast_true() {
        let stmt = parse_one("SELECT CAST(amount AS TEXT) FROM orders").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_cast, "CAST() expression must set has_cast = true");
    }

    #[test]
    fn select_with_pg_cast_operator_sets_has_cast_true() {
        let stmt = parse_one("SELECT amount::TEXT FROM orders").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_cast, ":: cast operator must set has_cast = true");
    }

    #[test]
    fn plain_select_has_cast_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_cast, "plain SELECT without CAST must have has_cast = false");
    }
}`,
    'ast.rs 1C: Append cast_tests module'
  ],
]);

// ─── 2. planner.rs ────────────────────────────────────────────────────────────
editFile('crates/voltnuerongrid-exec/src/planner.rs', [

  // 2A: Add Cast variant after Coalesce in enum
  [
`    /// COALESCE() null-coalescing expression (from S3-WS1-12 has_coalesce support).
    Coalesce {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`,
`    /// COALESCE() null-coalescing expression (from S3-WS1-12 has_coalesce support).
    Coalesce {
        input: Box<LogicalPlan>,
    },
    /// CAST() / :: type-cast expression (from S3-WS1-13 has_cast support).
    Cast {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`,
    'planner.rs 2A: Add Cast variant to enum'
  ],

  // 2B: Add Cast arm to primary_table()
  [
`            LogicalPlan::Case { input } => input.primary_table(),
            LogicalPlan::Coalesce { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`,
`            LogicalPlan::Case { input } => input.primary_table(),
            LogicalPlan::Coalesce { input } => input.primary_table(),
            LogicalPlan::Cast { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`,
    'planner.rs 2B: Add Cast arm to primary_table()'
  ],

  // 2C: Add Cast arm to has_aggregation()
  [
`            LogicalPlan::Case { input } => input.has_aggregation(),
            LogicalPlan::Coalesce { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`,
`            LogicalPlan::Case { input } => input.has_aggregation(),
            LogicalPlan::Coalesce { input } => input.has_aggregation(),
            LogicalPlan::Cast { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`,
    'planner.rs 2C: Add Cast arm to has_aggregation()'
  ],

  // 2D: Add Cast arm to estimate_cost() after Coalesce arm
  [
`            LogicalPlan::Coalesce { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.3,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`,
`            LogicalPlan::Coalesce { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.3,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Cast { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.2,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`,
    'planner.rs 2D: Add Cast arm to estimate_cost()'
  ],

  // 2E: Convert bare Coalesce if/else to let after_coalesce, add Cast block
  [
`        // Coalesce wrapper (S3-WS1-12 has_coalesce detection): outermost node.
        if sel.has_coalesce {
            LogicalPlan::Coalesce {
                input: Box::new(after_case),
            }
        } else {
            after_case
        }
    }`,
`        // Coalesce wrapper (S3-WS1-12 has_coalesce detection): outermost node.
        let after_coalesce = if sel.has_coalesce {
            LogicalPlan::Coalesce {
                input: Box::new(after_case),
            }
        } else {
            after_case
        };

        // Cast wrapper (S3-WS1-13 has_cast detection): outermost node.
        if sel.has_cast {
            LogicalPlan::Cast {
                input: Box::new(after_coalesce),
            }
        } else {
            after_coalesce
        }
    }`,
    'planner.rs 2E: Add Cast wrapper in plan_select()'
  ],

  // 2F: Add 2 new tests at end of test module
  [
`    #[test]
    fn cost_coalesce_query_routes_to_oltp() {
        let c = cost("SELECT COALESCE(name, 'unknown') FROM users");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "COALESCE should route to OLTP");
        assert!(c.relative_cost >= 0.3, "Coalesce must carry at least 0.3 cost overhead");
    }
}`,
`    #[test]
    fn cost_coalesce_query_routes_to_oltp() {
        let c = cost("SELECT COALESCE(name, 'unknown') FROM users");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "COALESCE should route to OLTP");
        assert!(c.relative_cost >= 0.3, "Coalesce must carry at least 0.3 cost overhead");
    }

    #[test]
    fn planner_cast_select_produces_cast_node() {
        let p = plan("SELECT CAST(amount AS TEXT) FROM orders");
        assert!(
            matches!(&p, LogicalPlan::Cast { .. }),
            "CAST() query should produce outermost Cast node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("orders"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_cast_query_routes_to_oltp() {
        let c = cost("SELECT CAST(amount AS TEXT) FROM orders");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "CAST should route to OLTP");
        assert!(c.relative_cost >= 0.2, "Cast must carry at least 0.2 cost overhead");
    }
}`,
    'planner.rs 2F: Add Cast planner tests'
  ],
]);

// ─── 3. main.rs ───────────────────────────────────────────────────────────────
editFile('services/voltnuerongridd/src/main.rs', [

  // 3A: Add IngestSchemaFields + WalSeq structs after RowsPageStatsResponse
  [
`// ─── S7-WS6-04: Chaos fire-drill structs ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChaosFireDrillRequest {
    drill_type: String,`,
`// ─── S11-WS1-13: Ingest schema fields structs ───────────────────────────────

#[derive(Debug, Deserialize)]
struct IngestSchemaFieldsQuery {
    schema_id: String,
}

#[derive(Debug, Serialize)]
struct SchemaFieldEntry {
    field_name: String,
    field_type: String,
}

#[derive(Debug, Serialize)]
struct IngestSchemaFieldsResponse {
    status: &'static str,
    schema_id: String,
    field_count: usize,
    fields: Vec<SchemaFieldEntry>,
}

// ─── S11-WS1-13: WAL sequence info structs ───────────────────────────────────

#[derive(Debug, Serialize)]
struct WalSeqResponse {
    status: &'static str,
    latest_sequence: u64,
    wal_len: usize,
    checkpoint_count: usize,
}

// ─── S7-WS6-04: Chaos fire-drill structs ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChaosFireDrillRequest {
    drill_type: String,`,
    'main.rs 3A: Add IngestSchemaFields + WalSeq structs'
  ],

  // 3B: Add /ingest/schema/fields route after /ingest/schema/list
  [
`        // S5-WS4-03: ingest schema list (format-filtered)
        .route("/api/v1/ingest/schema/list", get(ingest_schema_list))`,
`        // S5-WS4-03: ingest schema list (format-filtered)
        .route("/api/v1/ingest/schema/list", get(ingest_schema_list))
        // S11-WS1-13: Ingest schema field details
        .route("/api/v1/ingest/schema/fields", get(ingest_schema_fields))`,
    'main.rs 3B: Add /ingest/schema/fields route'
  ],

  // 4B: Add /store/wal/seq route after /store/wal/mutations
  [
`        // S2-WS2-03: WAL mutations (recent key-value changes)
        .route("/api/v1/store/wal/mutations", get(wal_mutations))`,
`        // S2-WS2-03: WAL mutations (recent key-value changes)
        .route("/api/v1/store/wal/mutations", get(wal_mutations))
        // S11-WS1-13: WAL latest sequence info
        .route("/api/v1/store/wal/seq", get(wal_seq))`,
    'main.rs 4B: Add /store/wal/seq route'
  ],

  // 3C+4C: Add ingest_schema_fields and wal_seq handlers after rows_page_stats handler
  [
`// ─── S7-WS6-01: Raft vote statistics endpoint ───────────────────────────────

/// S7-WS6-01: Return accumulated vote grant/reject counts for the current Raft node.`,
`// ─── S11-WS1-13: Ingest schema fields endpoint ──────────────────────────────

/// S11-WS1-13: Return field definitions for a specific ingest schema entry.
async fn ingest_schema_fields(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<IngestSchemaFieldsQuery>,
) -> Result<(StatusCode, Json<IngestSchemaFieldsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let registry = state.ingest_schema_registry.lock().expect("ingest_schema_registry lock fields");
    // Scaffold: look up schema by id; if found return its column list as field entries.
    let (field_count, fields) = if let Some(schema) = registry.iter().find(|s| s.schema_id == params.schema_id) {
        let entries: Vec<SchemaFieldEntry> = schema.columns.iter().map(|c| SchemaFieldEntry {
            field_name: c.name.clone(),
            field_type: c.data_type.clone(),
        }).collect();
        let n = entries.len();
        (n, entries)
    } else {
        (0, vec![])
    };
    drop(registry);
    Ok((StatusCode::OK, Json(IngestSchemaFieldsResponse {
        status: "ok",
        schema_id: params.schema_id,
        field_count,
        fields,
    })))
}

// ─── S11-WS1-13: WAL sequence info endpoint ──────────────────────────────────

/// S11-WS1-13: Return the latest WAL sequence number and record count.
async fn wal_seq(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalSeqResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock wal_seq");
    let latest_sequence = wal.latest_sequence();
    let wal_len = wal.wal_records().len();
    let checkpoint_count = wal.checkpoint_count();
    drop(wal);
    Ok((StatusCode::OK, Json(WalSeqResponse {
        status: "ok",
        latest_sequence,
        wal_len,
        checkpoint_count,
    })))
}

// ─── S7-WS6-01: Raft vote statistics endpoint ───────────────────────────────

/// S7-WS6-01: Return accumulated vote grant/reject counts for the current Raft node.`,
    'main.rs 3C+4C: Add ingest_schema_fields and wal_seq handlers'
  ],

  // 3D+4D: Add 4 new tests before end of test module
  [
`    #[tokio::test]
    async fn s11_ws1_12_rows_page_stats_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_page_stats(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

}`,
`    #[tokio::test]
    async fn s11_ws1_12_rows_page_stats_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_page_stats(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-13: Ingest schema fields endpoint tests ──────────────────────

    #[tokio::test]
    async fn s11_ws1_13_ingest_schema_fields_unknown_schema_returns_empty() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = ingest_schema_fields(
            State(state),
            headers,
            Query(IngestSchemaFieldsQuery { schema_id: "no-such-schema".to_string() }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.field_count, 0, "unknown schema must return zero fields");
        assert!(body.fields.is_empty());
    }

    #[tokio::test]
    async fn s11_ws1_13_ingest_schema_fields_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = ingest_schema_fields(
            State(state),
            headers,
            Query(IngestSchemaFieldsQuery { schema_id: "s1".to_string() }),
        ).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-13: WAL seq endpoint tests ───────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_13_wal_seq_fresh_state_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_seq(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.latest_sequence, 0, "fresh WAL must have sequence 0");
        assert_eq!(body.wal_len, 0);
    }

    #[tokio::test]
    async fn s11_ws1_13_wal_seq_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_seq(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

}`,
    'main.rs 3D+4D: Add ingest_schema_fields and wal_seq tests'
  ],
]);

console.log('fix_s37.js complete.');
