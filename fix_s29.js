#!/usr/bin/env node
// Session 29 — 4 sprint items:
//   1. SQL:  has_order_by field + detection + 3 tests           (sql: 114→117)
//   2. Exec: TopN combined Sort+Limit node + 2 tests            (exec: 32→34)
//   3. Svc:  GET /api/v1/store/wal/replay/count + 2 tests       (service: 345→347)
//   4. Svc:  GET /api/v1/store/cdc/cursor/list + 2 tests        (service: 347→349)

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
    // A: new struct field after has_group_by
    [
        'A: add has_order_by field',
        `    /// True when the query contains a GROUP BY clause (S3-WS1-06).
    pub has_group_by: bool,
}`,
        `    /// True when the query contains a GROUP BY clause (S3-WS1-06).
    pub has_group_by: bool,
    /// True when the query contains an ORDER BY clause (S3-WS1-06).
    pub has_order_by: bool,
}`
    ],
    // B: parse detection before Ok(Statement::Select(stmt))
    [
        'B: detect ORDER BY keyword',
        `                // Detect GROUP BY clause (S3-WS1-06).
                if up.contains("GROUP BY") {
                    stmt.has_group_by = true;
                }
                Ok(Statement::Select(stmt))`,
        `                // Detect GROUP BY clause (S3-WS1-06).
                if up.contains("GROUP BY") {
                    stmt.has_group_by = true;
                }
                // Detect ORDER BY clause (S3-WS1-06).
                if up.contains("ORDER BY") {
                    stmt.has_order_by = true;
                }
                Ok(Statement::Select(stmt))`
    ],
    // C: new test module after group_by_detection_tests end
    [
        'C: append order_by_detection_tests module',
        `    #[test]
    fn select_group_by_with_having_sets_has_group_by_true() {
        let stmt = parse_one("SELECT region, SUM(sales) FROM orders GROUP BY region HAVING SUM(sales) > 100").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_group_by, "GROUP BY ... HAVING must set has_group_by = true");
    }
}`,
        `    #[test]
    fn select_group_by_with_having_sets_has_group_by_true() {
        let stmt = parse_one("SELECT region, SUM(sales) FROM orders GROUP BY region HAVING SUM(sales) > 100").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_group_by, "GROUP BY ... HAVING must set has_group_by = true");
    }
}

#[cfg(test)]
mod order_by_detection_tests {
    use super::*;

    #[test]
    fn select_with_order_by_sets_has_order_by_true() {
        let stmt = parse_one("SELECT id, name FROM users ORDER BY name").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_order_by, "ORDER BY must set has_order_by = true");
    }

    #[test]
    fn select_without_order_by_has_order_by_false() {
        let stmt = parse_one("SELECT name FROM users WHERE active = 1").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_order_by, "query without ORDER BY must have has_order_by = false");
    }

    #[test]
    fn select_order_by_with_limit_sets_has_order_by_true() {
        let stmt = parse_one("SELECT * FROM orders ORDER BY created_at DESC LIMIT 10").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_order_by, "ORDER BY ... LIMIT must set has_order_by = true");
        assert_eq!(s.limit, Some(10), "LIMIT 10 must be parsed");
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
    // A: add TopN enum variant after Having
    [
        'A: add TopN variant to LogicalPlan enum',
        `    /// Post-aggregate HAVING filter (S3-WS1-06 has_group_by support).
    Having {
        input: Box<LogicalPlan>,
        condition: String,
    },
    /// Unrecognised or unparseable statement.
    Unknown(String),`,
        `    /// Post-aggregate HAVING filter (S3-WS1-06 has_group_by support).
    Having {
        input: Box<LogicalPlan>,
        condition: String,
    },
    /// Combined Sort+Limit optimisation for ORDER BY … LIMIT queries (S3-WS1-05).
    TopN {
        input: Box<LogicalPlan>,
        count: u64,
        order_by: String,
    },
    /// Unrecognised or unparseable statement.
    Unknown(String),`
    ],
    // B: primary_table() — add TopN to or-pattern with Sort/Limit
    [
        'B: primary_table TopN arm',
        `            | LogicalPlan::Sort { input, .. }
            | LogicalPlan::Limit { input, .. } => input.primary_table(),`,
        `            | LogicalPlan::Sort { input, .. }
            | LogicalPlan::Limit { input, .. }
            | LogicalPlan::TopN { input, .. } => input.primary_table(),`
    ],
    // C: has_aggregation() — add TopN to or-pattern with Sort/Limit
    [
        'C: has_aggregation TopN arm',
        `            | LogicalPlan::Sort { input, .. }
            | LogicalPlan::Limit { input, .. } => input.has_aggregation(),`,
        `            | LogicalPlan::Sort { input, .. }
            | LogicalPlan::Limit { input, .. }
            | LogicalPlan::TopN { input, .. } => input.has_aggregation(),`
    ],
    // D: estimate_cost() TopN arm after Limit
    [
        'D: estimate_cost TopN arm',
        `            LogicalPlan::Limit { input, count } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows.min(*count),
                    relative_cost: inner.relative_cost * 0.1,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Project { input, .. } => {`,
        `            LogicalPlan::Limit { input, count } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows.min(*count),
                    relative_cost: inner.relative_cost * 0.1,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::TopN { input, count, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows.min(*count),
                    relative_cost: inner.relative_cost * 1.3,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Project { input, .. } => {`
    ],
    // E: plan_select() — replace Sort+Limit with TopN when both present
    [
        'E: plan_select Sort+Limit → TopN optimisation',
        `        // Sort (ORDER BY)
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
        };

        // Limit
        let after_limit = if let Some(n) = sel.limit {
            LogicalPlan::Limit {
                input: Box::new(after_sort),
                count: n,
            }
        } else {
            after_sort
        };`,
        `        // Sort+Limit → TopN optimisation (S3-WS1-05): combine when both present.
        let after_limit = if !sel.order_by.is_empty() && sel.limit.is_some() {
            LogicalPlan::TopN {
                input: Box::new(after_having),
                count: sel.limit.unwrap(),
                order_by: sel.order_by.first().map(|o| o.column.clone()).unwrap_or_default(),
            }
        } else {
            // Sort (ORDER BY only)
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
            };
            // Limit (no ORDER BY)
            if let Some(n) = sel.limit {
                LogicalPlan::Limit {
                    input: Box::new(after_sort),
                    count: n,
                }
            } else {
                after_sort
            }
        };`
    ],
    // F: two new tests at end of tests module
    [
        'F: add TopN planner tests',
        `        assert!(c.relative_cost >= 1.0, "HAVING should carry cost >= 1.0");
    }
}`,
        `        assert!(c.relative_cost >= 1.0, "HAVING should carry cost >= 1.0");
    }

    #[test]
    fn planner_topn_produced_when_order_by_and_limit() {
        let p = plan("SELECT * FROM employees ORDER BY salary DESC LIMIT 5");
        assert!(
            matches!(&p, LogicalPlan::TopN { count, .. } if *count == 5),
            "ORDER BY … LIMIT should produce TopN node; got {p:?}"
        );
        assert_eq!(p.primary_table(), Some("employees"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_topn_query_routes_to_oltp() {
        let c = cost("SELECT * FROM orders ORDER BY created_at LIMIT 20");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "TopN query should route to OLTP");
        assert_eq!(c.estimated_rows, 20, "estimated rows capped at TopN count");
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
    // ── ITEM 3: GET /api/v1/store/wal/replay/count ─────────────────────────

    // 3-A: structs after WalSegmentListResponse
    [
        '3-A: WalReplayCountQuery + WalReplayCountResponse structs',
        `#[derive(Debug, Serialize)]
struct WalSegmentListResponse {
    status: &'static str,
    segment_count: usize,
    completed_segments: usize,
    active_record_count: usize,
    segments: Vec<WalSegment>,
}

// ─── S10-WS15-02: CDC change-data-capture structs ─────────────────────────────`,
        `#[derive(Debug, Serialize)]
struct WalSegmentListResponse {
    status: &'static str,
    segment_count: usize,
    completed_segments: usize,
    active_record_count: usize,
    segments: Vec<WalSegment>,
}

// ─── S2-WS2-02: WAL replay count structs ──────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct WalReplayCountQuery {
    table_filter: Option<String>,
    op_filter: Option<String>,
}

#[derive(Debug, Serialize)]
struct WalReplayCountResponse {
    status: &'static str,
    total_records: usize,
    matched_count: usize,
    table_filter: Option<String>,
    op_filter: Option<String>,
}

// ─── S10-WS15-02: CDC change-data-capture structs ─────────────────────────────`
    ],

    // 3-B: route after wal/segment/list
    [
        '3-B: wal/replay/count route',
        `        // S2-WS2-02: WAL segment list (checkpoint groups)
        .route("/api/v1/store/wal/segment/list", get(wal_segment_list))
        // S7-WS6-04: Chaos/game-day fault injection`,
        `        // S2-WS2-02: WAL segment list (checkpoint groups)
        .route("/api/v1/store/wal/segment/list", get(wal_segment_list))
        // S2-WS2-02: WAL replay count (filtered record count, no body)
        .route("/api/v1/store/wal/replay/count", get(wal_replay_count))
        // S7-WS6-04: Chaos/game-day fault injection`
    ],

    // 3-C: handler after wal_segment_list, before wal_recover
    [
        '3-C: wal_replay_count handler',
        `/// S2-WS2-02: replay WAL records into the row store (or dry-run).
async fn wal_recover(`,
        `// ─── S2-WS2-02: WAL replay count ──────────────────────────────────────────────

/// S2-WS2-02: Return the count of WAL records matching optional table/op filters.
async fn wal_replay_count(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<WalReplayCountQuery>,
) -> Result<(StatusCode, Json<WalReplayCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let records = wal.wal_records().to_vec();
    drop(wal);
    let total_records = records.len();
    let matched_count = records.iter().filter(|r| {
        let table_ok = query.table_filter.as_deref()
            .map(|t| r.key.starts_with(t))
            .unwrap_or(true);
        let op_ok = query.op_filter.as_deref()
            .map(|op| if op == "delete" { r.value == "__deleted__" } else { r.value != "__deleted__" })
            .unwrap_or(true);
        table_ok && op_ok
    }).count();
    Ok((StatusCode::OK, Json(WalReplayCountResponse {
        status: "ok",
        total_records,
        matched_count,
        table_filter: query.table_filter,
        op_filter: query.op_filter,
    })))
}

/// S2-WS2-02: replay WAL records into the row store (or dry-run).
async fn wal_recover(`
    ],

    // 3-D: tests after wal_segment_list tests
    [
        '3-D: wal_replay_count tests',
        `        assert!(active.start_sequence.is_some());
        assert!(active.end_sequence.is_some());
    }

    // ── S7-WS6-04: Chaos health check ────────────────────────────────────────`,
        `        assert!(active.start_sequence.is_some());
        assert!(active.end_sequence.is_some());
    }

    // ── S2-WS2-02: WAL replay count endpoint tests ───────────────────────────
    #[tokio::test]
    async fn s2_ws2_02_wal_replay_count_empty_state_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_replay_count(
            State(state),
            headers,
            axum::extract::Query(WalReplayCountQuery::default()),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_records, 0);
        assert_eq!(body.matched_count, 0);
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_replay_count_filters_by_op() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("k1", "v1");
            wal.append_mutation("k2", "__deleted__");
            wal.append_mutation("k3", "v3");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_replay_count(
            State(state),
            headers,
            axum::extract::Query(WalReplayCountQuery { table_filter: None, op_filter: Some("delete".to_string()) }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_records, 3);
        assert_eq!(body.matched_count, 1, "only 1 delete record");
    }

    // ── S7-WS6-04: Chaos health check ────────────────────────────────────────`
    ],

    // ── ITEM 4: GET /api/v1/store/cdc/cursor/list ──────────────────────────

    // 4-A: structs after CdcCursorResponse / before cdc rewind struct
    [
        '4-A: CdcCursorEntry + CdcCursorListResponse structs',
        `// ─── S10-WS15-02: CDC cursor rewind struct ────────────────────────────────────`,
        `// ─── S10-WS15-02: CDC cursor list structs ────────────────────────────────────

#[derive(Debug, Serialize)]
struct CdcCursorEntry {
    table_name: String,
    cursor_position: u64,
}

#[derive(Debug, Serialize)]
struct CdcCursorListResponse {
    status: &'static str,
    cursor_count: usize,
    cursors: Vec<CdcCursorEntry>,
}

// ─── S10-WS15-02: CDC cursor rewind struct ────────────────────────────────────`
    ],

    // 4-B: route after cdc/cursor/rewind
    [
        '4-B: cdc/cursor/list route',
        `        // S10-WS15-02: CDC cursor rewind
        .route("/api/v1/store/cdc/cursor/rewind", post(cdc_cursor_rewind))
        // S10-WS15-02: CDC aggregate metrics`,
        `        // S10-WS15-02: CDC cursor rewind
        .route("/api/v1/store/cdc/cursor/rewind", post(cdc_cursor_rewind))
        // S10-WS15-02: CDC cursor list (all tracked table positions)
        .route("/api/v1/store/cdc/cursor/list", get(cdc_cursor_list))
        // S10-WS15-02: CDC aggregate metrics`
    ],

    // 4-C: handler after cdc_cursor_rewind, before cdc_metrics
    [
        '4-C: cdc_cursor_list handler',
        `// ─── S10-WS15-02: CDC aggregate metrics ──────────────────────────────────────`,
        `// ─── S10-WS15-02: CDC cursor list ─────────────────────────────────────────────

/// S10-WS15-02: List all tracked CDC cursor positions across tables.
async fn cdc_cursor_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<CdcCursorListResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let cursors = state.cdc_cursors.lock().expect("cdc_cursors lock");
    let mut entries: Vec<CdcCursorEntry> = cursors
        .iter()
        .map(|(table, pos)| CdcCursorEntry {
            table_name: table.clone(),
            cursor_position: *pos,
        })
        .collect();
    entries.sort_by(|a, b| a.table_name.cmp(&b.table_name));
    let cursor_count = entries.len();
    Ok((StatusCode::OK, Json(CdcCursorListResponse {
        status: "ok",
        cursor_count,
        cursors: entries,
    })))
}

// ─── S10-WS15-02: CDC aggregate metrics ──────────────────────────────────────`
    ],

    // 4-D: tests after driver_pool_stats tests, before CDC stream filter
    [
        '4-D: cdc_cursor_list tests',
        `        let Err((status, _)) = result else { panic!("expected error") };
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // ── S10-WS15-02: CDC stream filter ────────────────────────────────────────`,
        `        let Err((status, _)) = result else { panic!("expected error") };
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // ── S10-WS15-02: CDC cursor list ──────────────────────────────────────────
    #[tokio::test]
    async fn s10_ws15_02_cdc_cursor_list_empty_on_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = cdc_cursor_list(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.cursor_count, 0);
        assert!(body.cursors.is_empty());
    }

    #[tokio::test]
    async fn s10_ws15_02_cdc_cursor_list_reflects_advanced_cursors() {
        let state = state_with_key(Some("test-key"));
        {
            let mut cursors = state.cdc_cursors.lock().unwrap();
            cursors.insert("orders".to_string(), 42);
            cursors.insert("users".to_string(), 7);
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = cdc_cursor_list(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.cursor_count, 2);
        let orders = body.cursors.iter().find(|c| c.table_name == "orders").unwrap();
        assert_eq!(orders.cursor_position, 42);
    }

    // ── S10-WS15-02: CDC stream filter ────────────────────────────────────────`
    ],
];

const r3 = applyReplacements(MAIN, mainReplacements);
console.log(`  => ${r3.changed} changed, ${r3.missed} missed`);

const total = r1.missed + r2.missed + r3.missed;
console.log(`\nDONE — total missed: ${total}`);
if (total > 0) process.exit(1);
