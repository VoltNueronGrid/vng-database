#!/usr/bin/env node
// Session 28 — 4 sprint items:
//   1. SQL:  has_group_by field + detection + 3 tests        (sql: 111→114)
//   2. Exec: Having LogicalPlan node + wiring + 2 tests      (exec: 30→32)
//   3. Svc:  GET /api/v1/store/wal/segment/list + 2 tests    (service: 341→343)
//   4. Svc:  GET /api/v1/driver/pool/stats  + 2 tests        (service: 343→345)

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
    // A: new struct field after has_null_literal
    [
        'A: add has_group_by field',
        `    /// True when the WHERE clause contains IS NULL or IS NOT NULL (S3-WS1-06).
    pub has_null_literal: bool,
}`,
        `    /// True when the WHERE clause contains IS NULL or IS NOT NULL (S3-WS1-06).
    pub has_null_literal: bool,
    /// True when the query contains a GROUP BY clause (S3-WS1-06).
    pub has_group_by: bool,
}`
    ],
    // B: parse detection before Ok(Statement::Select(stmt))
    [
        'B: detect GROUP BY keyword',
        `                // Detect IS NULL / IS NOT NULL predicates (S3-WS1-06).
                if up.contains("IS NULL") || up.contains("IS NOT NULL") {
                    stmt.has_null_literal = true;
                }
                Ok(Statement::Select(stmt))`,
        `                // Detect IS NULL / IS NOT NULL predicates (S3-WS1-06).
                if up.contains("IS NULL") || up.contains("IS NOT NULL") {
                    stmt.has_null_literal = true;
                }
                // Detect GROUP BY clause (S3-WS1-06).
                if up.contains("GROUP BY") {
                    stmt.has_group_by = true;
                }
                Ok(Statement::Select(stmt))`
    ],
    // C: new test module after null_literal_tests end
    [
        'C: append group_by_detection_tests module',
        `    #[test]
    fn plain_select_has_null_literal_false() {
        let stmt = parse_one("SELECT name FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_null_literal, "plain SELECT must have has_null_literal = false");
    }
}`,
        `    #[test]
    fn plain_select_has_null_literal_false() {
        let stmt = parse_one("SELECT name FROM users").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_null_literal, "plain SELECT must have has_null_literal = false");
    }
}

#[cfg(test)]
mod group_by_detection_tests {
    use super::*;

    #[test]
    fn select_with_group_by_sets_has_group_by_true() {
        let stmt = parse_one("SELECT dept, COUNT(*) FROM employees GROUP BY dept").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_group_by, "GROUP BY must set has_group_by = true");
    }

    #[test]
    fn select_without_group_by_has_group_by_false() {
        let stmt = parse_one("SELECT name FROM users WHERE active = 1").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_group_by, "query without GROUP BY must have has_group_by = false");
    }

    #[test]
    fn select_group_by_with_having_sets_has_group_by_true() {
        let stmt = parse_one("SELECT region, SUM(sales) FROM orders GROUP BY region HAVING SUM(sales) > 100").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_group_by, "GROUP BY ... HAVING must set has_group_by = true");
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
    // A: add Having enum variant after Offset
    [
        'A: add Having variant to LogicalPlan enum',
        `    /// Pagination skip-N rows (S3-WS1-06 offset support).
    Offset {
        input: Box<LogicalPlan>,
        offset: u64,
    },
    /// Unrecognised or unparseable statement.
    Unknown(String),`,
        `    /// Pagination skip-N rows (S3-WS1-06 offset support).
    Offset {
        input: Box<LogicalPlan>,
        offset: u64,
    },
    /// Post-aggregate HAVING filter (S3-WS1-06 has_group_by support).
    Having {
        input: Box<LogicalPlan>,
        condition: String,
    },
    /// Unrecognised or unparseable statement.
    Unknown(String),`
    ],
    // B: primary_table() arm for Having
    [
        'B: primary_table Having arm',
        `            LogicalPlan::Offset { input, .. } => input.primary_table(),
            _ => None,`,
        `            LogicalPlan::Offset { input, .. } => input.primary_table(),
            LogicalPlan::Having { input, .. } => input.primary_table(),
            _ => None,`
    ],
    // C: has_aggregation() arm for Having
    [
        'C: has_aggregation Having arm',
        `            LogicalPlan::Offset { input, .. } => input.has_aggregation(),
            _ => false,`,
        `            LogicalPlan::Offset { input, .. } => input.has_aggregation(),
            LogicalPlan::Having { input, .. } => input.has_aggregation(),
            _ => false,`
    ],
    // D: estimate_cost() arm for Having
    [
        'D: estimate_cost Having arm',
        `            LogicalPlan::Offset { input, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.1,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Unknown(_) => CostEstimate {`,
        `            LogicalPlan::Offset { input, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.1,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Having { input, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows / 2,
                    relative_cost: inner.relative_cost + 1.0,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Unknown(_) => CostEstimate {`
    ],
    // E: plan_select() — insert Having stage between after_agg and after_sort
    [
        'E: plan_select Having stage + fix after_sort ref',
        `        // Sort (ORDER BY)
        let after_sort = if !sel.order_by.is_empty() {
            LogicalPlan::Sort {
                input: Box::new(after_agg),
                order_by: sel
                    .order_by
                    .iter()
                    .map(|o| (o.column.clone(), o.descending))
                    .collect(),
            }
        } else {
            after_agg
        };`,
        `        // HAVING (post-aggregate filter, S3-WS1-06 has_group_by support)
        let after_having = if let Some(cond) = &sel.having {
            LogicalPlan::Having {
                input: Box::new(after_agg),
                condition: cond.clone(),
            }
        } else {
            after_agg
        };

        // Sort (ORDER BY)
        let after_sort = if !sel.order_by.is_empty() {
            LogicalPlan::Sort {
                input: Box::new(after_having),
                order_by: sel
                    .order_by
                    .iter()
                    .map(|o| (o.column.clone(), o.descending))
                    .collect(),
            }
        } else {
            after_having
        };`
    ],
    // F: two new tests
    [
        'F: add Having planner tests',
        `    #[test]
    fn cost_offset_query_routes_to_oltp() {
        let c = cost("SELECT * FROM t LIMIT 10 OFFSET 5");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "OFFSET query should route to OLTP");
    }
}`,
        `    #[test]
    fn cost_offset_query_routes_to_oltp() {
        let c = cost("SELECT * FROM t LIMIT 10 OFFSET 5");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "OFFSET query should route to OLTP");
    }

    #[test]
    fn planner_having_produces_having_node() {
        // SELECT * avoids Project wrapper so Having is outermost plan node.
        let p = plan("SELECT * FROM employees GROUP BY dept HAVING COUNT(*) > 5");
        assert!(
            matches!(&p, LogicalPlan::Having { condition, .. } if condition.to_uppercase().contains("COUNT")),
            "GROUP BY ... HAVING should produce a Having node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("employees"));
    }

    #[test]
    fn cost_having_query_routes_to_olap() {
        let c = cost("SELECT * FROM orders GROUP BY region HAVING SUM(sales) > 100");
        assert_eq!(c.recommended_path, QueryPath::Olap, "HAVING query should route to OLAP");
        assert!(c.relative_cost >= 1.0, "HAVING should carry cost >= 1.0");
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
    // ── ITEM 3: GET /api/v1/store/wal/segment/list ──────────────────────────

    // 3-A: structs after WalTailResponse
    [
        '3-A: WalSegment + WalSegmentListResponse structs',
        `#[derive(Debug, Serialize)]
struct WalTailResponse {
    status: &'static str,
    record_count: usize,
    limit_applied: usize,
    entries: Vec<WalReplayEntry>,
}

// ─── S10-WS15-02: CDC change-data-capture structs ─────────────────────────────`,
        `#[derive(Debug, Serialize)]
struct WalTailResponse {
    status: &'static str,
    record_count: usize,
    limit_applied: usize,
    entries: Vec<WalReplayEntry>,
}

// ─── S2-WS2-02: WAL segment list structs ─────────────────────────────────────

#[derive(Debug, Serialize)]
struct WalSegment {
    segment_id: u64,
    is_active: bool,
    record_count: usize,
    start_sequence: Option<u64>,
    end_sequence: Option<u64>,
}

#[derive(Debug, Serialize)]
struct WalSegmentListResponse {
    status: &'static str,
    segment_count: usize,
    completed_segments: usize,
    active_record_count: usize,
    segments: Vec<WalSegment>,
}

// ─── S10-WS15-02: CDC change-data-capture structs ─────────────────────────────`
    ],

    // 3-B: route registration after wal/tail
    [
        '3-B: wal/segment/list route',
        `        // S2-WS2-02: WAL tail (last N records)
        .route("/api/v1/store/wal/tail", get(wal_tail))
        // S7-WS6-04: Chaos/game-day fault injection`,
        `        // S2-WS2-02: WAL tail (last N records)
        .route("/api/v1/store/wal/tail", get(wal_tail))
        // S2-WS2-02: WAL segment list (checkpoint groups)
        .route("/api/v1/store/wal/segment/list", get(wal_segment_list))
        // S7-WS6-04: Chaos/game-day fault injection`
    ],

    // 3-C: handler after wal_tail closing }
    [
        '3-C: wal_segment_list handler',
        `    (StatusCode::OK, Json(WalTailResponse {
        status: "ok",
        record_count,
        limit_applied,
        entries,
    }))
}

/// S2-WS2-02: replay WAL records into the row store (or dry-run).
async fn wal_recover(`,
        `    (StatusCode::OK, Json(WalTailResponse {
        status: "ok",
        record_count,
        limit_applied,
        entries,
    }))
}

// ─── S2-WS2-02: WAL segment list handler ────────────────────────────────────

/// S2-WS2-02: List WAL checkpoint segments plus the active (unbounded) segment.
async fn wal_segment_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> (StatusCode, Json<WalSegmentListResponse>) {
    let _ = require_operator_auth(&headers, &state);
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let completed = wal.checkpoint_count();
    let active_records = wal.wal_records().to_vec();
    drop(wal);
    let active_record_count = active_records.len();
    let mut segments: Vec<WalSegment> = (1..=(completed as u64))
        .map(|id| WalSegment {
            segment_id: id,
            is_active: false,
            record_count: 0,
            start_sequence: None,
            end_sequence: None,
        })
        .collect();
    segments.push(WalSegment {
        segment_id: completed as u64 + 1,
        is_active: true,
        record_count: active_record_count,
        start_sequence: active_records.first().map(|r| r.sequence),
        end_sequence: active_records.last().map(|r| r.sequence),
    });
    let segment_count = segments.len();
    (StatusCode::OK, Json(WalSegmentListResponse {
        status: "ok",
        segment_count,
        completed_segments: completed,
        active_record_count,
        segments,
    }))
}

/// S2-WS2-02: replay WAL records into the row store (or dry-run).
async fn wal_recover(`
    ],

    // 3-D: tests after wal_tail_respects_limit
    [
        '3-D: wal_segment_list tests',
        `        assert_eq!(body.record_count, 3, "limit=3 means only 3 newest entries");
        assert_eq!(body.limit_applied, 3);
    }

    // ── S7-WS6-04: Chaos health check ────────────────────────────────────────`,
        `        assert_eq!(body.record_count, 3, "limit=3 means only 3 newest entries");
        assert_eq!(body.limit_applied, 3);
    }

    // ── S2-WS2-02: WAL segment list ───────────────────────────────────────────
    #[tokio::test]
    async fn s2_ws2_02_wal_segment_list_empty_returns_one_active_segment() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_segment_list(State(state), headers).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.segment_count, 1, "fresh state has exactly 1 active segment");
        assert_eq!(body.completed_segments, 0);
        assert_eq!(body.active_record_count, 0);
        assert!(body.segments.last().unwrap().is_active, "last segment must be active");
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_segment_list_shows_active_segment_record_count() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("k1", "v1");
            wal.append_mutation("k2", "v2");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_segment_list(State(state), headers).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.active_record_count, 2, "2 mutations in active segment");
        let active = body.segments.iter().find(|s| s.is_active).unwrap();
        assert_eq!(active.record_count, 2);
        assert!(active.start_sequence.is_some());
        assert!(active.end_sequence.is_some());
    }

    // ── S7-WS6-04: Chaos health check ────────────────────────────────────────`
    ],

    // ── ITEM 4: GET /api/v1/driver/pool/stats ───────────────────────────────

    // 4-A: route registration after driver/ping
    [
        '4-A: driver/pool/stats route',
        `        // S8-WS10-02: driver session ping/keepalive
        .route("/api/v1/driver/ping", post(driver_ping))
        // S10-WS15-02: CDC stream from WAL`,
        `        // S8-WS10-02: driver session ping/keepalive
        .route("/api/v1/driver/ping", post(driver_ping))
        // S8-WS10-02: driver pool stats (operator-facing)
        .route("/api/v1/driver/pool/stats", get(driver_pool_stats))
        // S10-WS15-02: CDC stream from WAL`
    ],

    // 4-B: handler before driver_health
    [
        '4-B: driver_pool_stats handler',
        `// ─── S8-WS10-02: Driver health handler ──────────────────────────────────────

async fn driver_health(
    State(state): State<AppState>,
    headers: HeaderMap,`,
        `// ─── S8-WS10-02: Driver pool stats (operator-facing) ────────────────────────

/// S8-WS10-02: Return driver connection pool statistics.
async fn driver_pool_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<PoolStatsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let now_ms = now_unix_ms_u64();
    let stats = state.driver_pool.lock().expect("driver_pool stats lock").pool_stats(now_ms);
    Ok((StatusCode::OK, Json(pool_stats_response(&stats))))
}

// ─── S8-WS10-02: Driver health handler ──────────────────────────────────────

async fn driver_health(
    State(state): State<AppState>,
    headers: HeaderMap,`
    ],

    // 4-C: tests after driver_ping tests
    [
        '4-C: driver_pool_stats tests',
        `        assert_eq!(body.session_token, "drv-sess-42");
        assert!(body.pinged_at_ms > 0, "pinged_at_ms should be non-zero");
    }

    // ── S10-WS15-02: CDC stream filter ────────────────────────────────────────`,
        `        assert_eq!(body.session_token, "drv-sess-42");
        assert!(body.pinged_at_ms > 0, "pinged_at_ms should be non-zero");
    }

    // ── S8-WS10-02: Driver pool stats ────────────────────────────────────────
    #[tokio::test]
    async fn s8_ws10_02_driver_pool_stats_fresh_state_shows_closed_circuit_breaker() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = driver_pool_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.circuit_breaker_state, "closed", "fresh pool circuit breaker must be closed");
        assert_eq!(body.active_connections, 0);
    }

    #[tokio::test]
    async fn s8_ws10_02_driver_pool_stats_requires_operator_auth() {
        let state = state_with_key(Some("test-key"));
        let bad_headers = operator_headers("wrong-key", "admin");
        let result = driver_pool_stats(State(state), bad_headers).await;
        assert!(result.is_err(), "wrong api key must return auth error");
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // ── S10-WS15-02: CDC stream filter ────────────────────────────────────────`
    ],
];

const r3 = applyReplacements(MAIN, mainReplacements);
console.log(`  => ${r3.changed} changed, ${r3.missed} missed`);

const total = r1.missed + r2.missed + r3.missed;
console.log(`\nDONE — total missed: ${total}`);
if (total > 0) process.exit(1);
