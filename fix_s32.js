#!/usr/bin/env node
// Session 32 — 4 sprint items:
//   1. SQL:  has_between field + detection + 3 tests            (sql: 123→126)
//   2. Exec: Between plan node + wiring + 2 tests               (exec: 38→40)
//   3. Svc:  POST /api/v1/ingest/connector/validate + 2 tests   (service: 357→359)
//   4. Svc:  GET /api/v1/store/wal/mutations + 2 tests          (service: 359→361)

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
    // A: new struct field after has_in_list
    [
        'A: add has_between field',
        `    /// True when the WHERE clause contains an IN (list) predicate (S3-WS1-07).
    pub has_in_list: bool,
}`,
        `    /// True when the WHERE clause contains an IN (list) predicate (S3-WS1-07).
    pub has_in_list: bool,
    /// True when the WHERE clause contains a BETWEEN ... AND predicate (S3-WS1-08).
    pub has_between: bool,
}`
    ],
    // B: detect BETWEEN after IN list detection
    [
        'B: detect BETWEEN keyword',
        `                // Detect IN list predicate in WHERE (S3-WS1-07).
                // Exclude subquery form "IN (SELECT ..." so has_subquery stays exclusive.
                if up.contains(" IN (") && !up.contains("(SELECT") {
                    stmt.has_in_list = true;
                }
                Ok(Statement::Select(stmt))`,
        `                // Detect IN list predicate in WHERE (S3-WS1-07).
                // Exclude subquery form "IN (SELECT ..." so has_subquery stays exclusive.
                if up.contains(" IN (") && !up.contains("(SELECT") {
                    stmt.has_in_list = true;
                }
                // Detect BETWEEN ... AND predicate in WHERE (S3-WS1-08).
                if up.contains(" BETWEEN ") && up.contains(" AND ") {
                    stmt.has_between = true;
                }
                Ok(Statement::Select(stmt))`
    ],
    // C: append between_tests module at end of file
    [
        'C: append between_tests module',
        `    #[test]
    fn plain_select_has_in_list_is_false() {
        let stmt = parse_one("SELECT * FROM orders WHERE total > 100").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_in_list, "plain SELECT without IN must have has_in_list = false");
    }
}`,
        `    #[test]
    fn plain_select_has_in_list_is_false() {
        let stmt = parse_one("SELECT * FROM orders WHERE total > 100").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_in_list, "plain SELECT without IN must have has_in_list = false");
    }
}

#[cfg(test)]
mod between_tests {
    use super::*;

    #[test]
    fn select_with_between_sets_has_between_true() {
        let stmt = parse_one("SELECT id FROM users WHERE age BETWEEN 18 AND 65").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_between, "BETWEEN ... AND predicate must set has_between = true");
    }

    #[test]
    fn select_with_between_string_range() {
        let stmt = parse_one("SELECT id FROM orders WHERE order_date BETWEEN '2024-01-01' AND '2024-12-31'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_between, "BETWEEN with date strings must set has_between = true");
    }

    #[test]
    fn plain_select_has_between_is_false() {
        let stmt = parse_one("SELECT * FROM transactions WHERE amount > 100").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_between, "plain SELECT without BETWEEN must have has_between = false");
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
    // A: add Between variant after InList
    [
        'A: add Between variant',
        `    /// IN-list predicate filter (from S3-WS1-07 has_in_list support).
    InList {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`,
        `    /// IN-list predicate filter (from S3-WS1-07 has_in_list support).
    InList {
        input: Box<LogicalPlan>,
    },
    /// BETWEEN ... AND range predicate filter (from S3-WS1-08 has_between support).
    Between {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`
    ],
    // B: primary_table() Between arm
    [
        'B: primary_table Between arm',
        `            LogicalPlan::InList { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`,
        `            LogicalPlan::InList { input } => input.primary_table(),
            LogicalPlan::Between { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`
    ],
    // C: has_aggregation() Between arm
    [
        'C: has_aggregation Between arm',
        `            LogicalPlan::InList { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`,
        `            LogicalPlan::InList { input } => input.has_aggregation(),
            LogicalPlan::Between { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`
    ],
    // D: estimate_cost() Between arm after InList block
    [
        'D: estimate_cost Between arm',
        `            LogicalPlan::InList { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.8) as u64,
                    relative_cost: inner.relative_cost + 0.5,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`,
        `            LogicalPlan::InList { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.8) as u64,
                    relative_cost: inner.relative_cost + 0.5,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Between { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.75) as u64,
                    relative_cost: inner.relative_cost + 0.4,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`
    ],
    // E: plan_select() — convert InList to let + add Between outermost wrap
    [
        'E: plan_select Between outermost wrap',
        `        // InList wrapper (S3-WS1-07 has_in_list detection): outermost node.
        if sel.has_in_list {
            LogicalPlan::InList {
                input: Box::new(after_subquery),
            }
        } else {
            after_subquery
        }
    }`,
        `        // InList wrapper (S3-WS1-07 has_in_list detection): outermost node.
        let after_in_list = if sel.has_in_list {
            LogicalPlan::InList {
                input: Box::new(after_subquery),
            }
        } else {
            after_subquery
        };

        // Between wrapper (S3-WS1-08 has_between detection): outermost node.
        if sel.has_between {
            LogicalPlan::Between {
                input: Box::new(after_in_list),
            }
        } else {
            after_in_list
        }
    }`
    ],
    // F: two new tests at end of tests module
    [
        'F: add Between tests',
        `    #[test]
    fn cost_in_list_query_routes_to_oltp() {
        let c = cost("SELECT id FROM users WHERE id IN (1, 2, 3)");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "IN list should route to OLTP");
        assert!(c.relative_cost >= 0.5, "InList must carry at least 0.5 cost overhead");
    }
}`,
        `    #[test]
    fn cost_in_list_query_routes_to_oltp() {
        let c = cost("SELECT id FROM users WHERE id IN (1, 2, 3)");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "IN list should route to OLTP");
        assert!(c.relative_cost >= 0.5, "InList must carry at least 0.5 cost overhead");
    }

    #[test]
    fn planner_between_select_produces_between_node() {
        let p = plan("SELECT id FROM users WHERE age BETWEEN 18 AND 65");
        assert!(
            matches!(&p, LogicalPlan::Between { .. }),
            "BETWEEN query should produce outermost Between node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_between_query_routes_to_oltp() {
        let c = cost("SELECT id FROM users WHERE age BETWEEN 18 AND 65");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "BETWEEN should route to OLTP");
        assert!(c.relative_cost >= 0.4, "Between must carry at least 0.4 cost overhead");
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
    // ── ITEM 3: POST /api/v1/ingest/connector/validate ──────────────────────

    // 3-A: IngestConnectorValidateRequest + Response structs
    [
        '3-A: IngestConnectorValidate structs',
        `#[derive(Serialize)]
struct IngestFormatDetectResponse {
    status: &'static str,
    detected_format: String,
    confidence: f64,
    field_count: usize,
}

// ─── S2-WS2-05: Transaction isolation stats structs ──────────────────────────`,
        `#[derive(Serialize)]
struct IngestFormatDetectResponse {
    status: &'static str,
    detected_format: String,
    confidence: f64,
    field_count: usize,
}

// ─── S5-WS4-04: Connector validation structs ──────────────────────────────────

#[derive(Debug, Deserialize)]
struct IngestConnectorValidateRequest {
    connector_id: String,
    format: String,
    config_json: String,
}

#[derive(Serialize)]
struct IngestConnectorValidateResponse {
    status: &'static str,
    valid: bool,
    issues: Vec<String>,
}

// ─── S2-WS2-05: Transaction isolation stats structs ──────────────────────────`
    ],

    // 3-B: route after ingest/format/detect
    [
        '3-B: ingest/connector/validate route',
        `        // S5-WS4-03: ingest format auto-detection
        .route("/api/v1/ingest/format/detect", post(ingest_format_detect))
        .route("/api/v1/ingest/outbox/status", get(ingest_outbox_status))`,
        `        // S5-WS4-03: ingest format auto-detection
        .route("/api/v1/ingest/format/detect", post(ingest_format_detect))
        // S5-WS4-04: ingest connector configuration validation
        .route("/api/v1/ingest/connector/validate", post(ingest_connector_validate))
        .route("/api/v1/ingest/outbox/status", get(ingest_outbox_status))`
    ],

    // 3-C: handler after ingest_format_detect, before S8-WS10-02
    [
        '3-C: ingest_connector_validate handler',
        `// ─── S8-WS10-02: driver query pass-through ──────────────────────────────────

/// S8-WS10-02: Execute a simple query through a driver session (scaffold).`,
        `// ─── S5-WS4-04: Connector configuration validation handler ──────────────────

/// S5-WS4-04: Validate that a connector config is well-formed before registration.
async fn ingest_connector_validate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestConnectorValidateRequest>,
) -> Result<(StatusCode, Json<IngestConnectorValidateResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut issues: Vec<String> = Vec::new();
    if req.connector_id.trim().is_empty() {
        issues.push("connector_id cannot be empty".to_string());
    }
    match req.format.as_str() {
        "json" | "csv" | "parquet" | "excel" => {}
        _ => issues.push(format!("unsupported format '{}'; expected: json, csv, parquet, excel", req.format)),
    }
    if let Err(e) = serde_json::from_str::<serde_json::Value>(&req.config_json) {
        issues.push(format!("config_json is not valid JSON: {e}"));
    }
    let valid = issues.is_empty();
    Ok((StatusCode::OK, Json(IngestConnectorValidateResponse {
        status: "ok",
        valid,
        issues,
    })))
}

// ─── S8-WS10-02: driver query pass-through ──────────────────────────────────

/// S8-WS10-02: Execute a simple query through a driver session (scaffold).`
    ],

    // 3-D: tests after ingest_format_detect_json_sample, before broker section
    [
        '3-D: ingest_connector_validate tests',
        `        assert!(body.confidence >= 0.9, "json confidence must be >= 0.9");
    }

    // ─── S5-WS4A-02: Broker adapter integration tests ────────────────────────`,
        `        assert!(body.confidence >= 0.9, "json confidence must be >= 0.9");
    }

    // ─── S5-WS4-04: Connector validation tests ──────────────────────────────

    #[tokio::test]
    async fn s5_ws4_04_ingest_connector_validate_json_format() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = IngestConnectorValidateRequest {
            connector_id: "conn-1".to_string(),
            format: "json".to_string(),
            config_json: r#"{"batch_size": 100}"#.to_string(),
        };
        let (status, Json(body)) = ingest_connector_validate(
            State(state), headers, Json(req),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.valid, "valid JSON config with known format must pass");
        assert!(body.issues.is_empty(), "no issues for a valid request");
    }

    #[tokio::test]
    async fn s5_ws4_04_ingest_connector_validate_unknown_format_is_invalid() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = IngestConnectorValidateRequest {
            connector_id: "conn-2".to_string(),
            format: "xml".to_string(),
            config_json: r#"{"tag": "row"}"#.to_string(),
        };
        let (status, Json(body)) = ingest_connector_validate(
            State(state), headers, Json(req),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.valid, "unknown format must fail validation");
        assert!(!body.issues.is_empty(), "issues must describe the format error");
    }

    // ─── S5-WS4A-02: Broker adapter integration tests ────────────────────────`
    ],

    // ── ITEM 4: GET /api/v1/store/wal/mutations ────────────────────────────

    // 4-A: WalMutationsQuery + WalMutationRecord + WalMutationsResponse structs
    [
        '4-A: WalMutations structs',
        `#[derive(Debug, Serialize)]
struct WalTailResponse {
    status: &'static str,
    record_count: usize,
    limit_applied: usize,
    entries: Vec<WalReplayEntry>,
}

// ─── S2-WS2-02: WAL segment list structs ─────────────────────────────────────`,
        `#[derive(Debug, Serialize)]
struct WalTailResponse {
    status: &'static str,
    record_count: usize,
    limit_applied: usize,
    entries: Vec<WalReplayEntry>,
}

// ─── S2-WS2-03: WAL mutations query structs ───────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct WalMutationsQuery {
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct WalMutationRecord {
    sequence: u64,
    key: String,
    value: String,
}

#[derive(Debug, Serialize)]
struct WalMutationsResponse {
    status: &'static str,
    mutation_count: usize,
    limit_applied: usize,
    mutations: Vec<WalMutationRecord>,
}

// ─── S2-WS2-02: WAL segment list structs ─────────────────────────────────────`
    ],

    // 4-B: route after wal/tail
    [
        '4-B: wal/mutations route',
        `        // S2-WS2-02: WAL tail (last N records)
        .route("/api/v1/store/wal/tail", get(wal_tail))
        // S2-WS2-02: WAL segment list (checkpoint groups)`,
        `        // S2-WS2-02: WAL tail (last N records)
        .route("/api/v1/store/wal/tail", get(wal_tail))
        // S2-WS2-03: WAL mutations (recent key-value changes)
        .route("/api/v1/store/wal/mutations", get(wal_mutations))
        // S2-WS2-02: WAL segment list (checkpoint groups)`
    ],

    // 4-C: handler after wal_tail, before S2-WS2-02 WAL segment list handler
    [
        '4-C: wal_mutations handler',
        `// ─── S2-WS2-02: WAL segment list handler ────────────────────────────────────

/// S2-WS2-02: List WAL checkpoint segments plus the active (unbounded) segment.`,
        `// ─── S2-WS2-03: WAL mutations handler ──────────────────────────────────────

/// S2-WS2-03: Return recent mutation records from WAL with key+value pairs.
async fn wal_mutations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<WalMutationsQuery>,
) -> (StatusCode, Json<WalMutationsResponse>) {
    let _ = require_operator_auth(&headers, &state);
    let limit_applied = params.limit.unwrap_or(50).max(1).min(10_000);
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let all_records = wal.wal_records().to_vec();
    drop(wal);
    let total = all_records.len();
    let skip = if total > limit_applied { total - limit_applied } else { 0 };
    let mutations: Vec<WalMutationRecord> = all_records
        .into_iter()
        .skip(skip)
        .map(|r| WalMutationRecord { sequence: r.sequence, key: r.key, value: r.value })
        .collect();
    let mutation_count = mutations.len();
    (StatusCode::OK, Json(WalMutationsResponse {
        status: "ok",
        mutation_count,
        limit_applied,
        mutations,
    }))
}

// ─── S2-WS2-02: WAL segment list handler ────────────────────────────────────

/// S2-WS2-02: List WAL checkpoint segments plus the active (unbounded) segment.`
    ],

    // 4-D: tests after s2_ws2_02_wal_tail_respects_limit, before WAL segment list section
    [
        '4-D: wal_mutations tests',
        `        assert_eq!(body.record_count, 3, "limit=3 means only 3 newest entries");
        assert_eq!(body.limit_applied, 3);
    }

    // ── S2-WS2-02: WAL segment list ───────────────────────────────────────────`,
        `        assert_eq!(body.record_count, 3, "limit=3 means only 3 newest entries");
        assert_eq!(body.limit_applied, 3);
    }

    // ── S2-WS2-03: WAL mutations tests ───────────────────────────────────────

    #[tokio::test]
    async fn s2_ws2_03_wal_mutations_returns_keys_and_values() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("user:101", "alice@example.com");
            wal.append_mutation("user:102", "bob@example.com");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_mutations(
            State(state),
            headers,
            axum::extract::Query(WalMutationsQuery { limit: Some(10) }),
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.mutation_count, 2);
        assert_eq!(body.mutations[0].key, "user:101");
        assert_eq!(body.mutations[0].value, "alice@example.com");
        assert_eq!(body.mutations[1].key, "user:102");
        assert_eq!(body.mutations[1].value, "bob@example.com");
    }

    #[tokio::test]
    async fn s2_ws2_03_wal_mutations_respects_limit() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            for i in 0..100u64 {
                wal.append_mutation(&format!("k{}", i), &format!("v{}", i));
            }
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_mutations(
            State(state),
            headers,
            axum::extract::Query(WalMutationsQuery { limit: Some(25) }),
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.mutation_count, 25, "limit=25 means only 25 newest mutations");
        assert_eq!(body.limit_applied, 25);
    }

    // ── S2-WS2-02: WAL segment list ───────────────────────────────────────────`
    ],
];

const r3 = applyReplacements(MAIN, mainReplacements);
console.log(`  => ${r3.changed} changed, ${r3.missed} missed`);

const total = r1.missed + r2.missed + r3.missed;
console.log(`\nDONE — total missed: ${total}`);
if (total > 0) process.exit(1);
