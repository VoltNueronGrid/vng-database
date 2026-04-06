#!/usr/bin/env node
// Session 31 — 4 sprint items:
//   1. SQL:  has_in_list field + detection + 3 tests          (sql: 120→123)
//   2. Exec: InList plan node + wiring + 2 tests              (exec: 36→38)
//   3. Svc:  POST /api/v1/ingest/format/detect + 2 tests      (service: 353→355)
//   4. Svc:  GET /api/v1/cluster/raft/vote/stats + 2 tests    (service: 355→357)

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
    // A: new struct field after has_having
    [
        'A: add has_in_list field',
        `    /// True when the query contains a HAVING clause (S3-WS1-06).
    pub has_having: bool,
}`,
        `    /// True when the query contains a HAVING clause (S3-WS1-06).
    pub has_having: bool,
    /// True when the WHERE clause contains an IN (list) predicate (S3-WS1-07).
    pub has_in_list: bool,
}`
    ],
    // B: detect IN list after HAVING detection
    [
        'B: detect IN list keyword',
        `                // Detect HAVING clause (S3-WS1-06).
                if up.contains("HAVING") {
                    stmt.has_having = true;
                }
                Ok(Statement::Select(stmt))`,
        `                // Detect HAVING clause (S3-WS1-06).
                if up.contains("HAVING") {
                    stmt.has_having = true;
                }
                // Detect IN list predicate in WHERE (S3-WS1-07).
                // Exclude subquery form "IN (SELECT ..." so has_subquery stays exclusive.
                if up.contains(" IN (") && !up.contains("(SELECT") {
                    stmt.has_in_list = true;
                }
                Ok(Statement::Select(stmt))`
    ],
    // C: append in_list_tests module at end of file
    [
        'C: append in_list_tests module',
        `    #[test]
    fn select_having_also_sets_has_group_by_true() {
        let stmt = parse_one("SELECT region, SUM(sales) FROM orders GROUP BY region HAVING SUM(sales) > 1000").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_having, "HAVING must set has_having = true");
        assert!(s.has_group_by, "GROUP BY ... HAVING must also set has_group_by = true");
    }
}`,
        `    #[test]
    fn select_having_also_sets_has_group_by_true() {
        let stmt = parse_one("SELECT region, SUM(sales) FROM orders GROUP BY region HAVING SUM(sales) > 1000").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_having, "HAVING must set has_having = true");
        assert!(s.has_group_by, "GROUP BY ... HAVING must also set has_group_by = true");
    }
}

#[cfg(test)]
mod in_list_tests {
    use super::*;

    #[test]
    fn select_with_in_predicate_sets_has_in_list_true() {
        let stmt = parse_one("SELECT id FROM users WHERE id IN (1, 2, 3)").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_in_list, "IN (list) predicate must set has_in_list = true");
    }

    #[test]
    fn select_with_in_list_string_values() {
        let stmt = parse_one("SELECT name FROM products WHERE category IN ('A', 'B', 'C')").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_in_list, "IN with string literals must set has_in_list = true");
    }

    #[test]
    fn plain_select_has_in_list_is_false() {
        let stmt = parse_one("SELECT * FROM orders WHERE total > 100").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_in_list, "plain SELECT without IN must have has_in_list = false");
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
    // A: add InList variant after Union (before WindowFn comment)
    [
        'A: add InList variant',
        `    /// UNION / set-operation combining two result sets (from S3-WS1-04 has_union support).
    Union {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`,
        `    /// UNION / set-operation combining two result sets (from S3-WS1-04 has_union support).
    Union {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
    },
    /// IN-list predicate filter (from S3-WS1-07 has_in_list support).
    InList {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`
    ],
    // B: primary_table() InList arm
    [
        'B: primary_table InList arm',
        `            LogicalPlan::Union { left, .. } => left.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`,
        `            LogicalPlan::Union { left, .. } => left.primary_table(),
            LogicalPlan::InList { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`
    ],
    // C: has_aggregation() InList arm
    [
        'C: has_aggregation InList arm',
        `            LogicalPlan::Union { left, right } => {
                left.has_aggregation() || right.has_aggregation()
            }
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`,
        `            LogicalPlan::Union { left, right } => {
                left.has_aggregation() || right.has_aggregation()
            }
            LogicalPlan::InList { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`
    ],
    // D: estimate_cost() InList arm after Union cost block
    [
        'D: estimate_cost InList arm',
        `            LogicalPlan::Union { left, right } => {
                let lc = Self::estimate_cost(left);
                let rc = Self::estimate_cost(right);
                CostEstimate {
                    estimated_rows: lc.estimated_rows.saturating_add(rc.estimated_rows),
                    relative_cost: lc.relative_cost + rc.relative_cost + 2.0,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`,
        `            LogicalPlan::Union { left, right } => {
                let lc = Self::estimate_cost(left);
                let rc = Self::estimate_cost(right);
                CostEstimate {
                    estimated_rows: lc.estimated_rows.saturating_add(rc.estimated_rows),
                    relative_cost: lc.relative_cost + rc.relative_cost + 2.0,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::InList { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.8) as u64,
                    relative_cost: inner.relative_cost + 0.5,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`
    ],
    // E: plan_select() — convert Subquery block to let + add InList outermost wrap
    [
        'E: plan_select InList outermost wrap',
        `        // Subquery wrapper (S3-WS1-04 has_subquery detection): outermost node.
        if sel.has_subquery {
            LogicalPlan::Subquery {
                input: Box::new(after_distinct),
            }
        } else {
            after_distinct
        }
    }`,
        `        // Subquery wrapper (S3-WS1-04 has_subquery detection): outermost node.
        let after_subquery = if sel.has_subquery {
            LogicalPlan::Subquery {
                input: Box::new(after_distinct),
            }
        } else {
            after_distinct
        };

        // InList wrapper (S3-WS1-07 has_in_list detection): outermost node.
        if sel.has_in_list {
            LogicalPlan::InList {
                input: Box::new(after_subquery),
            }
        } else {
            after_subquery
        }
    }`
    ],
    // F: add InList tests after Subquery tests (before closing brace of tests mod)
    [
        'F: add InList tests',
        `    #[test]
    fn cost_subquery_routes_to_hybrid() {
        let c = cost("SELECT id FROM orders WHERE id IN (SELECT id FROM recent_orders)");
        assert_eq!(c.recommended_path, QueryPath::Hybrid, "subquery should route to Hybrid");
        assert!(c.relative_cost >= 2.0, "subquery carries cost >= 2.0 overhead");
    }
}`,
        `    #[test]
    fn cost_subquery_routes_to_hybrid() {
        let c = cost("SELECT id FROM orders WHERE id IN (SELECT id FROM recent_orders)");
        assert_eq!(c.recommended_path, QueryPath::Hybrid, "subquery should route to Hybrid");
        assert!(c.relative_cost >= 2.0, "subquery carries cost >= 2.0 overhead");
    }

    #[test]
    fn planner_in_list_select_produces_in_list_node() {
        let p = plan("SELECT id FROM users WHERE id IN (1, 2, 3)");
        assert!(
            matches!(&p, LogicalPlan::InList { .. }),
            "IN list query should produce outermost InList node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_in_list_query_routes_to_oltp() {
        let c = cost("SELECT id FROM users WHERE id IN (1, 2, 3)");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "IN list should route to OLTP");
        assert!(c.relative_cost >= 0.5, "InList must carry at least 0.5 cost overhead");
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
    // ── ITEM 3: POST /api/v1/ingest/format/detect ─────────────────────────

    // 3-A: IngestFormatDetectRequest + IngestFormatDetectResponse structs
    [
        '3-A: IngestFormatDetect structs',
        `#[derive(Serialize)]
struct IngestSchemaListResponse {
    status: &'static str,
    format_filter: Option<String>,
    connector_count: usize,
    entries: Vec<IngestSchemaEntry>,
}

// ─── S2-WS2-05: Transaction isolation stats structs ──────────────────────────`,
        `#[derive(Serialize)]
struct IngestSchemaListResponse {
    status: &'static str,
    format_filter: Option<String>,
    connector_count: usize,
    entries: Vec<IngestSchemaEntry>,
}

// ─── S5-WS4-03: Ingest format detection structs ──────────────────────────────

#[derive(Debug, Deserialize)]
struct IngestFormatDetectRequest {
    sample_data: String,
}

#[derive(Serialize)]
struct IngestFormatDetectResponse {
    status: &'static str,
    detected_format: String,
    confidence: f64,
    field_count: usize,
}

// ─── S2-WS2-05: Transaction isolation stats structs ──────────────────────────`
    ],

    // 3-B: route after ingest/schema/list
    [
        '3-B: ingest/format/detect route',
        `        // S5-WS4-03: ingest schema list (format-filtered)
        .route("/api/v1/ingest/schema/list", get(ingest_schema_list))
        .route("/api/v1/ingest/outbox/status", get(ingest_outbox_status))`,
        `        // S5-WS4-03: ingest schema list (format-filtered)
        .route("/api/v1/ingest/schema/list", get(ingest_schema_list))
        // S5-WS4-03: ingest format auto-detection
        .route("/api/v1/ingest/format/detect", post(ingest_format_detect))
        .route("/api/v1/ingest/outbox/status", get(ingest_outbox_status))`
    ],

    // 3-C: handler after ingest_schema_list closing }, before S8-WS10-02
    [
        '3-C: ingest_format_detect handler',
        `// ─── S8-WS10-02: driver query pass-through ──────────────────────────────────

/// S8-WS10-02: Execute a simple query through a driver session (scaffold).`,
        `// ─── S5-WS4-03: Ingest format auto-detection handler ─────────────────────────

/// S5-WS4-03: Analyse a raw data sample and return the detected format + field count.
async fn ingest_format_detect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestFormatDetectRequest>,
) -> Result<(StatusCode, Json<IngestFormatDetectResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let sample = req.sample_data.trim();
    let (detected_format, confidence, field_count) =
        if sample.starts_with('[') || sample.starts_with('{') {
            let fc = if let Ok(v) = serde_json::from_str::<serde_json::Value>(sample) {
                if let Some(obj) = v.as_object() {
                    obj.len()
                } else if let Some(arr) = v.as_array() {
                    arr.first().and_then(|x| x.as_object()).map(|o| o.len()).unwrap_or(0)
                } else { 0 }
            } else { 0 };
            ("json".to_string(), 0.95f64, fc)
        } else if sample.lines().next().map(|l| l.contains(',')).unwrap_or(false) {
            let fc = sample.lines().next().map(|l| l.split(',').count()).unwrap_or(0);
            ("csv".to_string(), 0.85f64, fc)
        } else {
            ("unknown".to_string(), 0.0f64, 0usize)
        };
    Ok((StatusCode::OK, Json(IngestFormatDetectResponse {
        status: "ok",
        detected_format,
        confidence,
        field_count,
    })))
}

// ─── S8-WS10-02: driver query pass-through ──────────────────────────────────

/// S8-WS10-02: Execute a simple query through a driver session (scaffold).`
    ],

    // 3-D: tests after ingest_schema_list_csv_filter test, before broker tests
    [
        '3-D: ingest_format_detect tests',
        `        assert_eq!(body.entries[0].format, "csv");
        assert_eq!(body.format_filter.as_deref(), Some("csv"));
    }

    // ─── S5-WS4A-02: Broker adapter integration tests ────────────────────────`,
        `        assert_eq!(body.entries[0].format, "csv");
        assert_eq!(body.format_filter.as_deref(), Some("csv"));
    }

    // ─── S5-WS4-03: Ingest format detect endpoint tests ──────────────────────

    #[tokio::test]
    async fn s5_ws4_03_ingest_format_detect_csv_sample() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = IngestFormatDetectRequest {
            sample_data: "id,name,email\n1,Alice,a@x.com\n".to_string(),
        };
        let (status, Json(body)) = ingest_format_detect(
            State(state), headers, Json(req),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.detected_format, "csv");
        assert_eq!(body.field_count, 3);
        assert!(body.confidence >= 0.8, "csv confidence must be >= 0.8");
    }

    #[tokio::test]
    async fn s5_ws4_03_ingest_format_detect_json_sample() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = IngestFormatDetectRequest {
            sample_data: r#"{"id": 1, "name": "Bob", "score": 42}"#.to_string(),
        };
        let (status, Json(body)) = ingest_format_detect(
            State(state), headers, Json(req),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.detected_format, "json");
        assert_eq!(body.field_count, 3);
        assert!(body.confidence >= 0.9, "json confidence must be >= 0.9");
    }

    // ─── S5-WS4A-02: Broker adapter integration tests ────────────────────────`
    ],

    // ── ITEM 4: GET /api/v1/cluster/raft/vote/stats ───────────────────────

    // 4-A: RaftVoteStatsResponse struct after RaftMemberListResponse
    [
        '4-A: RaftVoteStatsResponse struct',
        `#[derive(Debug, Serialize)]
struct RaftMemberListResponse {
    status: &'static str,
    member_count: usize,
    members: Vec<RaftMemberEntry>,
}

// ─── S7-WS6-03: Raft current leader response ────────────────────────────────`,
        `#[derive(Debug, Serialize)]
struct RaftMemberListResponse {
    status: &'static str,
    member_count: usize,
    members: Vec<RaftMemberEntry>,
}

// ─── S7-WS6-01: Raft vote statistics ─────────────────────────────────────────

#[derive(Debug, Serialize)]
struct RaftVoteStatsResponse {
    status: &'static str,
    current_term: u64,
    total_votes_granted: u64,
    total_votes_rejected: u64,
}

// ─── S7-WS6-03: Raft current leader response ────────────────────────────────`
    ],

    // 4-B: route after raft/fence
    [
        '4-B: raft/vote/stats route',
        `        // S7-WS6-03: Raft fencing token
        .route("/api/v1/cluster/raft/fence", get(raft_fence))
        .route("/api/v1/store/rows/scan", post(store_rows_scan))`,
        `        // S7-WS6-03: Raft fencing token
        .route("/api/v1/cluster/raft/fence", get(raft_fence))
        // S7-WS6-01: Raft vote statistics
        .route("/api/v1/cluster/raft/vote/stats", get(raft_vote_stats))
        .route("/api/v1/store/rows/scan", post(store_rows_scan))`
    ],

    // 4-C: handler before raft_fence
    [
        '4-C: raft_vote_stats handler',
        `// ─── S7-WS6-03: Raft fencing token endpoint ──────────────────────────────────────────

/// Return the current fencing token for the Raft node.
async fn raft_fence(`,
        `// ─── S7-WS6-01: Raft vote statistics endpoint ───────────────────────────────

/// S7-WS6-01: Return accumulated vote grant/reject counts for the current Raft node.
async fn raft_vote_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RaftVoteStatsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let snap = state.raft_state.lock().expect("raft_state lock").status();
    // Scaffold: vote accumulation not yet tracked in RaftNode;
    // expose current_term only and return zeroed counters.
    Ok((StatusCode::OK, Json(RaftVoteStatsResponse {
        status: "ok",
        current_term: snap.current_term,
        total_votes_granted: 0,
        total_votes_rejected: 0,
    })))
}

// ─── S7-WS6-03: Raft fencing token endpoint ──────────────────────────────────────────

/// Return the current fencing token for the Raft node.
async fn raft_fence(`
    ],

    // 4-D: tests after raft_leader tests, before fencing token tests
    [
        '4-D: raft_vote_stats tests',
        `    #[tokio::test]
    async fn s7_ws6_03_raft_leader_reflects_term_after_vote() {
        let state = state_with_key(Some("test-key"));
        {
            let mut node = state.raft_state.lock().unwrap();
            node.current_term = 5;
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_leader(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.current_term, 5, "leader response must reflect updated term");
    }

    // ─── S7-WS6-03: Raft fencing token tests ─────────────────────────────`,
        `    #[tokio::test]
    async fn s7_ws6_03_raft_leader_reflects_term_after_vote() {
        let state = state_with_key(Some("test-key"));
        {
            let mut node = state.raft_state.lock().unwrap();
            node.current_term = 5;
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_leader(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.current_term, 5, "leader response must reflect updated term");
    }

    // ─── S7-WS6-01: Raft vote statistics tests ───────────────────────────────

    #[tokio::test]
    async fn s7_ws6_01_raft_vote_stats_fresh_state_shows_zero_counts() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_vote_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.total_votes_granted, 0, "fresh node must have zero votes granted");
        assert_eq!(body.total_votes_rejected, 0, "fresh node must have zero votes rejected");
    }

    #[tokio::test]
    async fn s7_ws6_01_raft_vote_stats_reflects_current_term() {
        let state = state_with_key(Some("test-key"));
        {
            let mut node = state.raft_state.lock().unwrap();
            node.current_term = 7;
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_vote_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.current_term, 7, "vote stats must reflect current raft term");
    }

    // ─── S7-WS6-03: Raft fencing token tests ─────────────────────────────`
    ],
];

const r3 = applyReplacements(MAIN, mainReplacements);
console.log(`  => ${r3.changed} changed, ${r3.missed} missed`);

const total = r1.missed + r2.missed + r3.missed;
console.log(`\nDONE — total missed: ${total}`);
if (total > 0) process.exit(1);
