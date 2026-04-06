#!/usr/bin/env node
// Session 30 — 4 sprint items:
//   1. SQL:  has_having field + detection + 3 tests                 (sql: 117→120)
//   2. Exec: Subquery plan node + wiring + 2 tests                  (exec: 34→36)
//   3. Svc:  POST /api/v1/store/rows/delete + 2 tests               (service: 349→351)
//   4. Svc:  GET /api/v1/ingest/schema/list + 2 tests               (service: 351→353)

const fs = require('fs');

function applyReplacements(filePath, replacements) {
    let raw = fs.readFileSync(filePath, 'utf8');
    let content = raw.replace(/\r\n/g, '\n');
    let changed = 0, missed = 0;
    for (const [label, oldStr, newStr] of replacements) {
        if (content.includes(oldStr)) {
            content = content.replace(oldStr, newStr);
            console.log(`  [OK]   ${label}`);
            changed++;
        } else {
            console.log(`  [MISS] ${label}`);
            missed++;
        }
    }
    fs.writeFileSync(filePath, content.replace(/\n/g, '\r\n'), 'utf8');
    return { changed, missed };
}

// ─────────────────────────────────────────────────────────────────────────────
// FILE 1 — ast.rs
// ─────────────────────────────────────────────────────────────────────────────
const AST = 'd:\\by\\polap-db\\crates\\voltnuerongrid-sql\\src\\ast.rs';
console.log('\n=== ast.rs ===');

const astReplacements = [
    // A: new struct field after has_order_by
    [
        'A: add has_having field',
        `    /// True when the query contains an ORDER BY clause (S3-WS1-06).
    pub has_order_by: bool,
}`,
        `    /// True when the query contains an ORDER BY clause (S3-WS1-06).
    pub has_order_by: bool,
    /// True when the query contains a HAVING clause (S3-WS1-06).
    pub has_having: bool,
}`
    ],
    // B: parse detection before Ok(Statement::Select(stmt))
    [
        'B: detect HAVING keyword',
        `                // Detect ORDER BY clause (S3-WS1-06).
                if up.contains("ORDER BY") {
                    stmt.has_order_by = true;
                }
                Ok(Statement::Select(stmt))`,
        `                // Detect ORDER BY clause (S3-WS1-06).
                if up.contains("ORDER BY") {
                    stmt.has_order_by = true;
                }
                // Detect HAVING clause (S3-WS1-06).
                if up.contains("HAVING") {
                    stmt.has_having = true;
                }
                Ok(Statement::Select(stmt))`
    ],
    // C: new test module at end of file
    [
        'C: append having_flag_tests module',
        `    #[test]
    fn select_order_by_with_limit_sets_has_order_by_true() {
        let stmt = parse_one("SELECT * FROM orders ORDER BY created_at DESC LIMIT 10").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_order_by, "ORDER BY ... LIMIT must set has_order_by = true");
        assert_eq!(s.limit, Some(10), "LIMIT 10 must be parsed");
    }
}`,
        `    #[test]
    fn select_order_by_with_limit_sets_has_order_by_true() {
        let stmt = parse_one("SELECT * FROM orders ORDER BY created_at DESC LIMIT 10").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_order_by, "ORDER BY ... LIMIT must set has_order_by = true");
        assert_eq!(s.limit, Some(10), "LIMIT 10 must be parsed");
    }
}

#[cfg(test)]
mod having_flag_tests {
    use super::*;

    #[test]
    fn select_with_having_sets_has_having_true() {
        let stmt = parse_one("SELECT dept, COUNT(*) FROM employees GROUP BY dept HAVING COUNT(*) > 5").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_having, "HAVING clause must set has_having = true");
    }

    #[test]
    fn select_without_having_has_having_false() {
        let stmt = parse_one("SELECT name FROM users WHERE active = 1").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_having, "query without HAVING must have has_having = false");
    }

    #[test]
    fn select_having_also_sets_has_group_by_true() {
        let stmt = parse_one("SELECT region, SUM(sales) FROM orders GROUP BY region HAVING SUM(sales) > 1000").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_having, "HAVING must set has_having = true");
        assert!(s.has_group_by, "GROUP BY ... HAVING must also set has_group_by = true");
    }
}`
    ],
];

const r1 = applyReplacements(AST, astReplacements);
console.log(`  => ${r1.changed} changed, ${r1.missed} missed`);

// ─────────────────────────────────────────────────────────────────────────────
// FILE 2 — planner.rs
// ─────────────────────────────────────────────────────────────────────────────
const PLANNER = 'd:\\by\\polap-db\\crates\\voltnuerongrid-exec\\src\\planner.rs';
console.log('\n=== planner.rs ===');

const plannerReplacements = [
    // A: add Subquery variant after TopN
    [
        'A: add Subquery variant',
        `    /// Combined Sort+Limit optimisation for ORDER BY … LIMIT queries (S3-WS1-05).
    TopN {
        input: Box<LogicalPlan>,
        count: u64,
        order_by: String,
    },
    /// Unrecognised or unparseable statement.
    Unknown(String),`,
        `    /// Combined Sort+Limit optimisation for ORDER BY … LIMIT queries (S3-WS1-05).
    TopN {
        input: Box<LogicalPlan>,
        count: u64,
        order_by: String,
    },
    /// Correlated or scalar subquery wrapper (S3-WS1-04 has_subquery support).
    Subquery {
        input: Box<LogicalPlan>,
    },
    /// Unrecognised or unparseable statement.
    Unknown(String),`
    ],
    // B: primary_table() arm for Subquery
    [
        'B: primary_table Subquery arm',
        `            LogicalPlan::Having { input, .. } => input.primary_table(),
            _ => None,`,
        `            LogicalPlan::Having { input, .. } => input.primary_table(),
            LogicalPlan::Subquery { input } => input.primary_table(),
            _ => None,`
    ],
    // C: has_aggregation() arm for Subquery
    [
        'C: has_aggregation Subquery arm',
        `            LogicalPlan::Having { input, .. } => input.has_aggregation(),
            _ => false,`,
        `            LogicalPlan::Having { input, .. } => input.has_aggregation(),
            LogicalPlan::Subquery { input } => input.has_aggregation(),
            _ => false,`
    ],
    // D: estimate_cost() Subquery arm before Unknown
    [
        'D: estimate_cost Subquery arm',
        `            LogicalPlan::Unknown(_) => CostEstimate {
                estimated_rows: 0,
                relative_cost: 0.0,
                recommended_path: QueryPath::Unknown,
            },`,
        `            LogicalPlan::Subquery { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 2.0,
                    recommended_path: QueryPath::Hybrid,
                }
            }
            LogicalPlan::Unknown(_) => CostEstimate {
                estimated_rows: 0,
                relative_cost: 0.0,
                recommended_path: QueryPath::Unknown,
            },`
    ],
    // E: plan_select() outermost Subquery wrap
    [
        'E: plan_select Subquery outermost wrap',
        `        // SELECT DISTINCT deduplication (S3-WS1-04 is_distinct detection): wrap outermost.
        if sel.is_distinct {
            LogicalPlan::Distinct {
                input: Box::new(after_window),
            }
        } else {
            after_window
        }
    }`,
        `        // SELECT DISTINCT deduplication (S3-WS1-04 is_distinct detection): wrap outermost.
        let after_distinct = if sel.is_distinct {
            LogicalPlan::Distinct {
                input: Box::new(after_window),
            }
        } else {
            after_window
        };

        // Subquery wrapper (S3-WS1-04 has_subquery detection): outermost node.
        if sel.has_subquery {
            LogicalPlan::Subquery {
                input: Box::new(after_distinct),
            }
        } else {
            after_distinct
        }
    }`
    ],
    // F: two new tests
    [
        'F: add Subquery tests',
        `    fn cost_topn_query_routes_to_oltp() {
        let c = cost("SELECT * FROM orders ORDER BY created_at LIMIT 20");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "TopN query should route to OLTP");
        assert_eq!(c.estimated_rows, 20, "estimated rows capped at TopN count");
    }
}`,
        `    fn cost_topn_query_routes_to_oltp() {
        let c = cost("SELECT * FROM orders ORDER BY created_at LIMIT 20");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "TopN query should route to OLTP");
        assert_eq!(c.estimated_rows, 20, "estimated rows capped at TopN count");
    }

    #[test]
    fn planner_subquery_produces_subquery_node() {
        let p = plan("SELECT id FROM orders WHERE id IN (SELECT id FROM recent_orders)");
        assert!(
            matches!(&p, LogicalPlan::Subquery { .. }),
            "query with subquery should produce outermost Subquery node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("orders"));
    }

    #[test]
    fn cost_subquery_routes_to_hybrid() {
        let c = cost("SELECT id FROM orders WHERE id IN (SELECT id FROM recent_orders)");
        assert_eq!(c.recommended_path, QueryPath::Hybrid, "subquery should route to Hybrid");
        assert!(c.relative_cost >= 2.0, "subquery carries cost >= 2.0 overhead");
    }
}`
    ],
];

const r2 = applyReplacements(PLANNER, plannerReplacements);
console.log(`  => ${r2.changed} changed, ${r2.missed} missed`);

// ─────────────────────────────────────────────────────────────────────────────
// FILE 3 — main.rs
// ─────────────────────────────────────────────────────────────────────────────
const MAIN = 'd:\\by\\polap-db\\services\\voltnuerongridd\\src\\main.rs';
console.log('\n=== main.rs ===');

const mainReplacements = [
    // ── ITEM 3: POST /api/v1/store/rows/delete ─────────────────────────────

    // 3-A: RowDeleteRequest + RowDeleteResponse structs after RowCountResponse
    [
        '3-A: RowDeleteRequest + RowDeleteResponse structs',
        `#[derive(Serialize)]
struct RowCountResponse {
    status: &'static str,
    snapshot_xid: u64,
    key_prefix: Option<String>,
    count: usize,
}

// ─── S9-WS8A-02: Audit export struct ─────────────────────────────────────────`,
        `#[derive(Serialize)]
struct RowCountResponse {
    status: &'static str,
    snapshot_xid: u64,
    key_prefix: Option<String>,
    count: usize,
}

// ─── S2-WS2-04: Row store delete-by-key structs ──────────────────────────────

#[derive(Debug, Deserialize)]
struct RowDeleteRequest {
    key: String,
}

#[derive(Serialize)]
struct RowDeleteResponse {
    status: &'static str,
    key: String,
    deleted: bool,
}

// ─── S9-WS8A-02: Audit export struct ─────────────────────────────────────────`
    ],

    // 3-B: route after rows/count, before broker routes
    [
        '3-B: rows/delete route',
        `        // S2-WS2-04: Row store key-prefix count
        .route("/api/v1/store/rows/count", get(row_store_count))
        // S5-WS4A-02: Broker adapter status + flush`,
        `        // S2-WS2-04: Row store key-prefix count
        .route("/api/v1/store/rows/count", get(row_store_count))
        // S2-WS2-04: Row store delete by key
        .route("/api/v1/store/rows/delete", post(row_store_delete))
        // S5-WS4A-02: Broker adapter status + flush`
    ],

    // 3-C: handler after row_store_count, before S5-WS4A-02
    [
        '3-C: row_store_delete handler',
        `// ─── S5-WS4A-02: Broker adapter status + flush ────────────────────────────────

/// S5-WS4A-02: Report the status of all registered broker adapters.`,
        `// ─── S2-WS2-04: Row store delete-by-key handler ──────────────────────────────

/// S2-WS2-04: Delete a specific row by key from the row store and WAL.
async fn row_store_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RowDeleteRequest>,
) -> Result<(StatusCode, Json<RowDeleteResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut rs = state.row_store.lock().expect("row_store lock delete");
    let snapshot_xid = rs.current_xid();
    let rows = rs.scan_at_snapshot(snapshot_xid);
    let exists = rows.contains_key(req.key.as_str());
    if exists {
        let xid = rs.begin_xid();
        rs.delete(xid, &req.key);
        drop(rs);
        let mut wal = state.wal_engine.lock().expect("wal_engine lock delete");
        wal.append_mutation(&req.key, "__deleted__");
    } else {
        drop(rs);
    }
    Ok((StatusCode::OK, Json(RowDeleteResponse {
        status: "ok",
        key: req.key,
        deleted: exists,
    })))
}

// ─── S5-WS4A-02: Broker adapter status + flush ────────────────────────────────

/// S5-WS4A-02: Report the status of all registered broker adapters.`
    ],

    // 3-D: tests after row_count prefix test, before row_snapshot test
    [
        '3-D: row_store_delete tests',
        `        assert_eq!(filtered.count, 2, "2 orders rows match the prefix");
        assert_eq!(filtered.key_prefix.as_deref(), Some("orders:"));
    }

    #[tokio::test]
    async fn s2_ws2_04_row_snapshot_shows_inserted_rows() {`,
        `        assert_eq!(filtered.count, 2, "2 orders rows match the prefix");
        assert_eq!(filtered.key_prefix.as_deref(), Some("orders:"));
    }

    // ── S2-WS2-04: Row store delete-by-key endpoint tests ─────────────────────

    #[tokio::test]
    async fn s2_ws2_04_row_delete_existing_key_returns_deleted_true() {
        let state = state_with_key(Some("test-key"));
        {
            let mut rs = state.row_store.lock().unwrap();
            let xid = rs.begin_xid();
            rs.insert(xid, "orders:99", std::collections::HashMap::from([("v".to_string(), "x".to_string())]));
        }
        let headers = operator_headers("test-key", "admin");
        let req = RowDeleteRequest { key: "orders:99".to_string() };
        let (status, Json(body)) = row_store_delete(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.deleted, "existing key must report deleted = true");
        assert_eq!(body.key, "orders:99");
    }

    #[tokio::test]
    async fn s2_ws2_04_row_delete_missing_key_returns_deleted_false() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = RowDeleteRequest { key: "no-such-key".to_string() };
        let (status, Json(body)) = row_store_delete(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.deleted, "missing key must report deleted = false");
    }

    #[tokio::test]
    async fn s2_ws2_04_row_snapshot_shows_inserted_rows() {`
    ],

    // ── ITEM 4: GET /api/v1/ingest/schema/list ────────────────────────────

    // 4-A: IngestSchemaListQuery + IngestSchemaListResponse structs
    [
        '4-A: IngestSchemaListQuery + IngestSchemaListResponse structs',
        `#[derive(Serialize)]
struct IngestSchemaRegistryResponse {
    status: &'static str,
    connector_count: usize,
    entries: Vec<IngestSchemaEntry>,
}

// ─── S2-WS2-05: Transaction isolation stats structs ──────────────────────────`,
        `#[derive(Serialize)]
struct IngestSchemaRegistryResponse {
    status: &'static str,
    connector_count: usize,
    entries: Vec<IngestSchemaEntry>,
}

// ─── S5-WS4-03: Ingest schema list (format-filtered) structs ─────────────────

#[derive(Debug, Deserialize, Default)]
struct IngestSchemaListQuery {
    format: Option<String>,
}

#[derive(Serialize)]
struct IngestSchemaListResponse {
    status: &'static str,
    format_filter: Option<String>,
    connector_count: usize,
    entries: Vec<IngestSchemaEntry>,
}

// ─── S2-WS2-05: Transaction isolation stats structs ──────────────────────────`
    ],

    // 4-B: route after ingest/schema, before outbox/status
    [
        '4-B: ingest/schema/list route',
        `        // S5-WS4-03: ingest schema registry
        .route("/api/v1/ingest/schema", get(ingest_schema_registry))
        .route("/api/v1/ingest/outbox/status", get(ingest_outbox_status))`,
        `        // S5-WS4-03: ingest schema registry
        .route("/api/v1/ingest/schema", get(ingest_schema_registry))
        // S5-WS4-03: ingest schema list (format-filtered)
        .route("/api/v1/ingest/schema/list", get(ingest_schema_list))
        .route("/api/v1/ingest/outbox/status", get(ingest_outbox_status))`
    ],

    // 4-C: handler after ingest_infer_columns, before S8-WS10-02 comment
    [
        '4-C: ingest_schema_list handler',
        `    vec![
        IngestSchemaColumn { name: "key".to_string(), inferred_type: "utf8" },
        IngestSchemaColumn { name: "payload".to_string(), inferred_type: "utf8" },
    ]
}

// ─── S8-WS10-02: driver query pass-through ──────────────────────────────────`,
        `    vec![
        IngestSchemaColumn { name: "key".to_string(), inferred_type: "utf8" },
        IngestSchemaColumn { name: "payload".to_string(), inferred_type: "utf8" },
    ]
}

// ─── S5-WS4-03: Ingest schema list (format-filtered) handler ─────────────────

/// S5-WS4-03: List ingest schema entries, optionally filtered by format (csv/json/parquet/excel).
async fn ingest_schema_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<IngestSchemaListQuery>,
) -> Result<(StatusCode, Json<IngestSchemaListResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_ingest_runtime_privilege(&headers, &state, PrivilegeAction::Read, "ingest/schema")?;
    let csv_map = state.ingest_csv_records.lock().expect("csv schema list lock");
    let json_map = state.ingest_json_records.lock().expect("json schema list lock");
    let mut entries: Vec<IngestSchemaEntry> = Vec::new();
    let fmt = params.format.as_deref();
    if fmt.is_none() || fmt == Some("csv") {
        for (connector_id, records) in csv_map.iter() {
            entries.push(IngestSchemaEntry {
                connector_id: connector_id.clone(),
                format: "csv".to_string(),
                row_count: records.len(),
                columns: ingest_infer_columns(records),
            });
        }
    }
    if fmt.is_none() || fmt == Some("json") {
        for (connector_id, records) in json_map.iter() {
            entries.push(IngestSchemaEntry {
                connector_id: connector_id.clone(),
                format: "json".to_string(),
                row_count: records.len(),
                columns: ingest_infer_columns(records),
            });
        }
    }
    let connector_count = entries.len();
    drop(csv_map);
    drop(json_map);
    Ok((StatusCode::OK, Json(IngestSchemaListResponse {
        status: "ok",
        format_filter: params.format,
        connector_count,
        entries,
    })))
}

// ─── S8-WS10-02: driver query pass-through ──────────────────────────────────`
    ],

    // 4-D: tests after ingest_schema_registry tests, before broker tests
    [
        '4-D: ingest_schema_list tests',
        `        assert_eq!(body.entries[0].format, "csv");
        assert_eq!(body.entries[0].row_count, 2);
        assert!(!body.entries[0].columns.is_empty());
    }

    // ─── S5-WS4A-02: Broker adapter integration tests ────────────────────────`,
        `        assert_eq!(body.entries[0].format, "csv");
        assert_eq!(body.entries[0].row_count, 2);
        assert!(!body.entries[0].columns.is_empty());
    }

    // ─── S5-WS4-03: Ingest schema list endpoint tests ────────────────────────

    #[tokio::test]
    async fn s5_ws4_03_ingest_schema_list_no_filter_returns_all_formats() {
        use voltnuerongrid_ingest::IngestRecord;
        let state = state_with_key(Some("test-key"));
        {
            let mut csv = state.ingest_csv_records.lock().unwrap();
            csv.insert("csv-orders".to_string(), vec![
                IngestRecord { key: "r1".to_string(), payload: "id=1".to_string() },
            ]);
            let mut json = state.ingest_json_records.lock().unwrap();
            json.insert("json-events".to_string(), vec![
                IngestRecord { key: "e1".to_string(), payload: r#"{"id":1}"#.to_string() },
            ]);
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = ingest_schema_list(
            State(state), headers, Query(IngestSchemaListQuery { format: None }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.connector_count, 2, "no filter returns both csv and json entries");
        assert!(body.format_filter.is_none());
    }

    #[tokio::test]
    async fn s5_ws4_03_ingest_schema_list_csv_filter_excludes_json() {
        use voltnuerongrid_ingest::IngestRecord;
        let state = state_with_key(Some("test-key"));
        {
            let mut csv = state.ingest_csv_records.lock().unwrap();
            csv.insert("csv-orders".to_string(), vec![
                IngestRecord { key: "r1".to_string(), payload: "id=1".to_string() },
            ]);
            let mut json = state.ingest_json_records.lock().unwrap();
            json.insert("json-events".to_string(), vec![
                IngestRecord { key: "e1".to_string(), payload: r#"{"id":1}"#.to_string() },
            ]);
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = ingest_schema_list(
            State(state), headers, Query(IngestSchemaListQuery { format: Some("csv".to_string()) }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.connector_count, 1, "csv filter must return only csv entries");
        assert_eq!(body.entries[0].format, "csv");
        assert_eq!(body.format_filter.as_deref(), Some("csv"));
    }

    // ─── S5-WS4A-02: Broker adapter integration tests ────────────────────────`
    ],
];

const r3 = applyReplacements(MAIN, mainReplacements);
console.log(`  => ${r3.changed} changed, ${r3.missed} missed`);

const total = r1.missed + r2.missed + r3.missed;
console.log(`\nDONE — total missed: ${total}`);
if (total > 0) process.exit(1);
