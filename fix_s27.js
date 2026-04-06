#!/usr/bin/env node
// Session 27 — 4 sprint items:
//   1. SQL:  has_null_literal field + detection + 3 tests  (sql: 108→111)
//   2. Exec: Offset LogicalPlan node + wiring + 2 tests    (exec: 28→30)
//   3. Svc:  GET /api/v1/store/wal/tail + 2 tests          (service: 337→339)
//   4. Svc:  POST /api/v1/driver/ping  + 2 tests           (service: 339→341)

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
    // A: new struct field
    [
        'A: add has_null_literal field',
        `    /// LIMIT value, if present.
    pub limit: Option<u64>,
    /// OFFSET value for pagination (S3-WS1-04).
    pub offset: Option<u64>,
}`,
        `    /// LIMIT value, if present.
    pub limit: Option<u64>,
    /// OFFSET value for pagination (S3-WS1-04).
    pub offset: Option<u64>,
    /// True when the WHERE clause contains IS NULL or IS NOT NULL (S3-WS1-06).
    pub has_null_literal: bool,
}`
    ],
    // B: parse detection before Ok(Statement::Select(stmt))
    [
        'B: detect IS NULL / IS NOT NULL',
        `                // Detect SELECT DISTINCT keyword (S3-WS1-04).
                if up_trim.starts_with("SELECTDISTINCT") {
                    stmt.is_distinct = true;
                }
                Ok(Statement::Select(stmt))`,
        `                // Detect SELECT DISTINCT keyword (S3-WS1-04).
                if up_trim.starts_with("SELECTDISTINCT") {
                    stmt.is_distinct = true;
                }
                // Detect IS NULL / IS NOT NULL predicates (S3-WS1-06).
                if up.contains("IS NULL") || up.contains("IS NOT NULL") {
                    stmt.has_null_literal = true;
                }
                Ok(Statement::Select(stmt))`
    ],
    // C: new test module after the last closing } of offset_tests
    [
        'C: append null_literal_tests module',
        `    #[test]
    fn select_without_offset_is_none() {
        let stmt = parse_one("SELECT name FROM employees").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert_eq!(s.offset, None, "plain SELECT must have offset = None");
    }
}`,
        `    #[test]
    fn select_without_offset_is_none() {
        let stmt = parse_one("SELECT name FROM employees").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert_eq!(s.offset, None, "plain SELECT must have offset = None");
    }
}

#[cfg(test)]
mod null_literal_tests {
    use super::*;

    fn parse_one(sql: &str) -> Result<Statement, ParseError> {
        parse(sql)
    }

    #[test]
    fn where_is_null_sets_has_null_literal_true() {
        let stmt = parse_one("SELECT * FROM t WHERE col IS NULL").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_null_literal, "IS NULL must set has_null_literal");
    }

    #[test]
    fn where_is_not_null_sets_has_null_literal_true() {
        let stmt = parse_one("SELECT id FROM orders WHERE deleted_at IS NOT NULL").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_null_literal, "IS NOT NULL must set has_null_literal");
    }

    #[test]
    fn plain_select_has_null_literal_false() {
        let stmt = parse_one("SELECT name FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_null_literal, "plain SELECT must have has_null_literal = false");
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
    // A: new Offset enum variant
    [
        'A: add Offset variant to LogicalPlan enum',
        `    /// Deduplication of result rows via SELECT DISTINCT (S3-WS1-04 is_distinct support).
    Distinct {
        input: Box<LogicalPlan>,
    },
    /// Unrecognised or unparseable statement.
    Unknown(String),`,
        `    /// Deduplication of result rows via SELECT DISTINCT (S3-WS1-04 is_distinct support).
    Distinct {
        input: Box<LogicalPlan>,
    },
    /// Pagination skip-N rows (S3-WS1-06 offset support).
    Offset {
        input: Box<LogicalPlan>,
        offset: u64,
    },
    /// Unrecognised or unparseable statement.
    Unknown(String),`
    ],
    // B: primary_table() arm
    [
        'B: primary_table Offset arm',
        `            LogicalPlan::WindowFn { input, .. } => input.primary_table(),
            LogicalPlan::Distinct { input } => input.primary_table(),
            _ => None,`,
        `            LogicalPlan::WindowFn { input, .. } => input.primary_table(),
            LogicalPlan::Distinct { input } => input.primary_table(),
            LogicalPlan::Offset { input, .. } => input.primary_table(),
            _ => None,`
    ],
    // C: has_aggregation() arm
    [
        'C: has_aggregation Offset arm',
        `            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),
            LogicalPlan::Distinct { input } => input.has_aggregation(),
            _ => false,`,
        `            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),
            LogicalPlan::Distinct { input } => input.has_aggregation(),
            LogicalPlan::Offset { input, .. } => input.has_aggregation(),
            _ => false,`
    ],
    // D: estimate_cost() arm
    [
        'D: estimate_cost Offset arm',
        `            LogicalPlan::Distinct { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows / 2,
                    relative_cost: inner.relative_cost + 0.3,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Unknown(_) => CostEstimate {`,
        `            LogicalPlan::Distinct { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows / 2,
                    relative_cost: inner.relative_cost + 0.3,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Offset { input, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.1,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Unknown(_) => CostEstimate {`
    ],
    // E: plan_select() — wrap in Offset after limit, before project
    [
        'E: plan_select Offset wrap between limit and project',
        `        // Project (only when not SELECT *)
        let after_project = if sel.columns != vec!["*".to_string()] && !sel.columns.is_empty() {
            LogicalPlan::Project {
                input: Box::new(after_limit),
                columns: sel.columns.clone(),
            }
        } else {
            after_limit
        };`,
        `        // Offset (S3-WS1-06 OFFSET support)
        let after_offset = if let Some(off) = sel.offset {
            if off > 0 {
                LogicalPlan::Offset {
                    input: Box::new(after_limit),
                    offset: off,
                }
            } else {
                after_limit
            }
        } else {
            after_limit
        };

        // Project (only when not SELECT *)
        let after_project = if sel.columns != vec!["*".to_string()] && !sel.columns.is_empty() {
            LogicalPlan::Project {
                input: Box::new(after_offset),
                columns: sel.columns.clone(),
            }
        } else {
            after_offset
        };`
    ],
    // F: two new tests at the end of the tests module
    [
        'F: add Offset planner tests',
        `    #[test]
    fn cost_distinct_query_routes_to_oltp() {
        let c = cost("SELECT DISTINCT name FROM employees");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "DISTINCT should route to OLTP");
    }
}`,
        `    #[test]
    fn cost_distinct_query_routes_to_oltp() {
        let c = cost("SELECT DISTINCT name FROM employees");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "DISTINCT should route to OLTP");
    }

    #[test]
    fn planner_select_with_offset_produces_offset_node() {
        let plan = plan("SELECT * FROM t LIMIT 10 OFFSET 5");
        // The outermost plan node should be Offset wrapping a Limit
        assert!(
            matches!(&plan, LogicalPlan::Offset { offset, .. } if *offset == 5),
            "LIMIT 10 OFFSET 5 should produce an Offset node with offset=5, got: {:?}", plan
        );
    }

    #[test]
    fn cost_offset_query_routes_to_oltp() {
        let c = cost("SELECT * FROM t LIMIT 10 OFFSET 5");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "OFFSET query should route to OLTP");
    }
}`
    ],
];

const r2 = applyReplacements(PLANNER, plannerReplacements);
console.log(`  => ${r2.changed} changed, ${r2.missed} missed`);

// ─────────────────────────────────────────────────────────────────────────────
// FILE 3 — main.rs  (two service endpoints)
// ─────────────────────────────────────────────────────────────────────────────
const MAIN = 'd:\\by\\polap-db\\services\\voltnuerongridd\\src\\main.rs';
console.log('\n=== main.rs ===');

const mainReplacements = [
    // ── ITEM 3: GET /api/v1/store/wal/tail ──────────────────────────────────

    // 3-A: structs after WalReplayResponse
    [
        '3-A: WalTailQuery + WalTailResponse structs',
        `#[derive(Debug, Serialize)]
struct WalReplayResponse {
    status: &'static str,
    total_records: usize,
    matched_records: usize,
    entries: Vec<WalReplayEntry>,
}

// ─── S10-WS15-02: CDC change-data-capture structs ─────────────────────────────`,
        `#[derive(Debug, Serialize)]
struct WalReplayResponse {
    status: &'static str,
    total_records: usize,
    matched_records: usize,
    entries: Vec<WalReplayEntry>,
}

// ─── S2-WS2-02: WAL tail structs ─────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct WalTailQuery {
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct WalTailResponse {
    status: &'static str,
    record_count: usize,
    limit_applied: usize,
    entries: Vec<WalReplayEntry>,
}

// ─── S10-WS15-02: CDC change-data-capture structs ─────────────────────────────`
    ],

    // 3-B: route registration
    [
        '3-B: wal/tail route',
        `        // S2-WS2-02: WAL replay (filtered read-back)
        .route("/api/v1/store/wal/replay", get(wal_replay))
        // S7-WS6-04: Chaos/game-day fault injection`,
        `        // S2-WS2-02: WAL replay (filtered read-back)
        .route("/api/v1/store/wal/replay", get(wal_replay))
        // S2-WS2-02: WAL tail (last N records)
        .route("/api/v1/store/wal/tail", get(wal_tail))
        // S7-WS6-04: Chaos/game-day fault injection`
    ],

    // 3-C: handler after wal_replay closing }
    [
        '3-C: wal_tail handler',
        `/// S2-WS2-02: replay WAL records into the row store (or dry-run).
async fn wal_recover(`,
        `/// S2-WS2-02: Return the last N WAL records (tail view).
async fn wal_tail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<WalTailQuery>,
) -> (StatusCode, Json<WalTailResponse>) {
    let _ = require_operator_auth(&headers, &state);
    let limit_applied = params.limit.unwrap_or(10).max(1).min(1_000);
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let all_records = wal.wal_records().to_vec();
    drop(wal);
    let total = all_records.len();
    let skip = if total > limit_applied { total - limit_applied } else { 0 };
    let entries: Vec<WalReplayEntry> = all_records
        .into_iter()
        .skip(skip)
        .map(|r| WalReplayEntry { sequence: r.sequence, key: r.key, value: r.value })
        .collect();
    let record_count = entries.len();
    (StatusCode::OK, Json(WalTailResponse {
        status: "ok",
        record_count,
        limit_applied,
        entries,
    }))
}

/// S2-WS2-02: replay WAL records into the row store (or dry-run).
async fn wal_recover(`
    ],

    // 3-D: tests after wal_bounds test
    [
        '3-D: wal_tail tests',
        `        assert!(body.newest_sequence.is_some(), "newest sequence must be Some after mutations");
    }

    // ── S7-WS6-04: Chaos health check ────────────────────────────────────────`,
        `        assert!(body.newest_sequence.is_some(), "newest sequence must be Some after mutations");
    }

    // ── S2-WS2-02: WAL tail ───────────────────────────────────────────────────
    #[tokio::test]
    async fn s2_ws2_02_wal_tail_empty_returns_zero_entries() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_tail(
            State(state),
            headers,
            axum::extract::Query(WalTailQuery::default()),
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.record_count, 0);
        assert!(body.entries.is_empty());
        assert_eq!(body.limit_applied, 10);
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_tail_respects_limit() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("k1", "v1");
            wal.append_mutation("k2", "v2");
            wal.append_mutation("k3", "v3");
            wal.append_mutation("k4", "v4");
            wal.append_mutation("k5", "v5");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_tail(
            State(state),
            headers,
            axum::extract::Query(WalTailQuery { limit: Some(3) }),
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.record_count, 3, "limit=3 means only 3 newest entries");
        assert_eq!(body.limit_applied, 3);
    }

    // ── S7-WS6-04: Chaos health check ────────────────────────────────────────`
    ],

    // ── ITEM 4: POST /api/v1/driver/ping ────────────────────────────────────

    // 4-A: structs after DriverQueryResponse
    [
        '4-A: DriverPingRequest + DriverPingResponse structs',
        `#[derive(Debug, Serialize)]
struct DriverQueryResponse {
    status: &'static str,
    session_token: String,
    sql: String,
    rows_returned: usize,
    executed_at_ms: u64,
}

#[derive(Serialize)]
struct IngestOutboxStatusResponse {`,
        `#[derive(Debug, Serialize)]
struct DriverQueryResponse {
    status: &'static str,
    session_token: String,
    sql: String,
    rows_returned: usize,
    executed_at_ms: u64,
}

// ─── S8-WS10-02: Driver session ping structs ─────────────────────────────────

#[derive(Debug, Deserialize)]
struct DriverPingRequest {
    session_token: String,
}

#[derive(Debug, Serialize)]
struct DriverPingResponse {
    status: &'static str,
    session_token: String,
    pinged_at_ms: u64,
}

#[derive(Serialize)]
struct IngestOutboxStatusResponse {`
    ],

    // 4-B: route registration
    [
        '4-B: driver/ping route',
        `        // S8-WS10-02: driver query pass-through
        .route("/api/v1/driver/query", post(driver_query))
        // S10-WS15-02: CDC stream from WAL`,
        `        // S8-WS10-02: driver query pass-through
        .route("/api/v1/driver/query", post(driver_query))
        // S8-WS10-02: driver session ping/keepalive
        .route("/api/v1/driver/ping", post(driver_ping))
        // S10-WS15-02: CDC stream from WAL`
    ],

    // 4-C: handler after driver_query closing }
    [
        '4-C: driver_ping handler',
        `// ─── S8-WS10-02: Driver health handler ──────────────────────────────────────

async fn driver_health(`,
        `// ─── S8-WS10-02: Driver session ping/keepalive ──────────────────────────────

/// S8-WS10-02: Ping/keepalive for an existing driver session.
async fn driver_ping(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DriverPingRequest>,
) -> Result<(StatusCode, Json<DriverPingResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let sessions = state.driver_sessions.lock().expect("driver_sessions lock");
    let session_exists = sessions.contains_key(&req.session_token);
    drop(sessions);
    if !session_exists {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                status: "error",
                reason: "invalid_session_token".to_string(),
                locale: "en".to_string(),
                localized_message: "Invalid or expired session token".to_string(),
            }),
        ));
    }
    let pinged_at_ms = now_unix_ms_u64();
    Ok((StatusCode::OK, Json(DriverPingResponse {
        status: "pong",
        session_token: req.session_token,
        pinged_at_ms,
    })))
}

// ─── S8-WS10-02: Driver health handler ──────────────────────────────────────

async fn driver_health(`
    ],

    // 4-D: tests after driver_query test
    [
        '4-D: driver_ping tests',
        `        assert_eq!(body.rows_returned, 0);
    }

    // ── S10-WS15-02: CDC stream filter ────────────────────────────────────────`,
        `        assert_eq!(body.rows_returned, 0);
    }

    // ── S8-WS10-02: Driver ping ───────────────────────────────────────────────
    #[tokio::test]
    async fn s8_ws10_02_driver_ping_invalid_session_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = DriverPingRequest { session_token: "ghost-token".to_string() };
        let result = driver_ping(State(state), headers, Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s8_ws10_02_driver_ping_valid_session_returns_pong() {
        let state = state_with_key(Some("test-key"));
        {
            let mut sessions = state.driver_sessions.lock().unwrap();
            sessions.insert("drv-sess-42".to_string(), DriverSession {
                driver_name: "test-drv".to_string(),
                driver_version: "1.0.0".to_string(),
                connected_at_ms: 0,
            });
        }
        let headers = operator_headers("test-key", "admin");
        let req = DriverPingRequest { session_token: "drv-sess-42".to_string() };
        let (status, Json(body)) = driver_ping(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "pong");
        assert_eq!(body.session_token, "drv-sess-42");
        assert!(body.pinged_at_ms > 0, "pinged_at_ms should be non-zero");
    }

    // ── S10-WS15-02: CDC stream filter ────────────────────────────────────────`
    ],
];

const r3 = applyReplacements(MAIN, mainReplacements);
console.log(`  => ${r3.changed} changed, ${r3.missed} missed`);

const total = r1.missed + r2.missed + r3.missed;
console.log(`\nDONE — total missed: ${total}`);
if (total > 0) process.exit(1);
