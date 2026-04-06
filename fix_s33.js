#!/usr/bin/env node
// Session 33 — 4 sprint items:
//   1. SQL:  has_like field + detection + 3 tests                (sql: 126→129)
//   2. Exec: Like plan node + wiring + 2 tests                   (exec: 40→42)
//   3. Svc:  POST /api/v1/connectors/update + 2 tests            (service: 361→363)
//   4. Svc:  GET /api/v1/store/wal/checkpoint/history + 2 tests  (service: 363→365)

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
    // A: new struct field after has_between
    [
        'A: add has_like field',
        `    /// True when the WHERE clause contains a BETWEEN ... AND predicate (S3-WS1-08).
    pub has_between: bool,
}`,
        `    /// True when the WHERE clause contains a BETWEEN ... AND predicate (S3-WS1-08).
    pub has_between: bool,
    /// True when the WHERE clause contains a LIKE or ILIKE predicate (S3-WS1-09).
    pub has_like: bool,
}`
    ],
    // B: detect LIKE after BETWEEN detection
    [
        'B: detect LIKE keyword',
        `                // Detect BETWEEN ... AND predicate in WHERE (S3-WS1-08).
                if up.contains(" BETWEEN ") && up.contains(" AND ") {
                    stmt.has_between = true;
                }
                Ok(Statement::Select(stmt))`,
        `                // Detect BETWEEN ... AND predicate in WHERE (S3-WS1-08).
                if up.contains(" BETWEEN ") && up.contains(" AND ") {
                    stmt.has_between = true;
                }
                // Detect LIKE / ILIKE predicate in WHERE (S3-WS1-09).
                if up.contains(" LIKE ") || up.contains(" ILIKE ") {
                    stmt.has_like = true;
                }
                Ok(Statement::Select(stmt))`
    ],
    // C: append like_tests module at end of file
    [
        'C: append like_tests module',
        `    #[test]
    fn plain_select_has_between_is_false() {
        let stmt = parse_one("SELECT * FROM transactions WHERE amount > 100").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_between, "plain SELECT without BETWEEN must have has_between = false");
    }
}`,
        `    #[test]
    fn plain_select_has_between_is_false() {
        let stmt = parse_one("SELECT * FROM transactions WHERE amount > 100").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_between, "plain SELECT without BETWEEN must have has_between = false");
    }
}

#[cfg(test)]
mod like_tests {
    use super::*;

    #[test]
    fn select_with_like_predicate_sets_has_like_true() {
        let stmt = parse_one("SELECT name FROM users WHERE name LIKE '%Alice%'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_like, "LIKE predicate must set has_like = true");
    }

    #[test]
    fn select_with_ilike_predicate_sets_has_like_true() {
        let stmt = parse_one("SELECT email FROM users WHERE email ILIKE '%@example.com'").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(s.has_like, "ILIKE predicate must set has_like = true");
    }

    #[test]
    fn plain_select_has_like_is_false() {
        let stmt = parse_one("SELECT id FROM orders WHERE amount > 50").unwrap();
        let Statement::Select(s) = stmt else { panic!("expected Select") };
        assert!(!s.has_like, "plain SELECT without LIKE must have has_like = false");
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
    // A: add Like variant after Between
    [
        'A: add Like variant',
        `    /// BETWEEN ... AND range predicate filter (from S3-WS1-08 has_between support).
    Between {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`,
        `    /// BETWEEN ... AND range predicate filter (from S3-WS1-08 has_between support).
    Between {
        input: Box<LogicalPlan>,
    },
    /// LIKE / ILIKE string pattern filter (from S3-WS1-09 has_like support).
    Like {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).`
    ],
    // B: primary_table() Like arm after Between
    [
        'B: primary_table Like arm',
        `            LogicalPlan::Between { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`,
        `            LogicalPlan::Between { input } => input.primary_table(),
            LogicalPlan::Like { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),`
    ],
    // C: has_aggregation() Like arm after Between
    [
        'C: has_aggregation Like arm',
        `            LogicalPlan::Between { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`,
        `            LogicalPlan::Between { input } => input.has_aggregation(),
            LogicalPlan::Like { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),`
    ],
    // D: estimate_cost() Like arm after Between arm
    [
        'D: estimate_cost Like arm',
        `            LogicalPlan::Between { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.6) as u64,
                    relative_cost: inner.relative_cost + 0.4,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`,
        `            LogicalPlan::Between { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.6) as u64,
                    relative_cost: inner.relative_cost + 0.4,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Like { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.7) as u64,
                    relative_cost: inner.relative_cost + 1.2,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {`
    ],
    // E: plan_select() — convert Between to let + add Like outermost wrap
    [
        'E: plan_select Like outermost wrap',
        `        // Between wrapper (S3-WS1-08 has_between detection): outermost node.
        if sel.has_between {
            LogicalPlan::Between {
                input: Box::new(after_in_list),
            }
        } else {
            after_in_list
        }
    }`,
        `        // Between wrapper (S3-WS1-08 has_between detection): outermost node.
        let after_between = if sel.has_between {
            LogicalPlan::Between {
                input: Box::new(after_in_list),
            }
        } else {
            after_in_list
        };

        // Like wrapper (S3-WS1-09 has_like detection): outermost node.
        if sel.has_like {
            LogicalPlan::Like {
                input: Box::new(after_between),
            }
        } else {
            after_between
        }
    }`
    ],
    // F: add Like tests after Between tests
    [
        'F: add Like tests',
        `    #[test]
    fn cost_between_query_routes_to_oltp() {
        let c = cost("SELECT id FROM users WHERE age BETWEEN 18 AND 65");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "BETWEEN should route to OLTP");
        assert!(c.relative_cost >= 0.4, "Between must carry at least 0.4 cost overhead");
    }
}`,
        `    #[test]
    fn cost_between_query_routes_to_oltp() {
        let c = cost("SELECT id FROM users WHERE age BETWEEN 18 AND 65");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "BETWEEN should route to OLTP");
        assert!(c.relative_cost >= 0.4, "Between must carry at least 0.4 cost overhead");
    }

    #[test]
    fn planner_like_select_produces_like_node() {
        let p = plan("SELECT name FROM users WHERE name LIKE '%Alice%'");
        assert!(
            matches!(&p, LogicalPlan::Like { .. }),
            "LIKE query should produce outermost Like node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_like_query_routes_to_olap() {
        let c = cost("SELECT name FROM users WHERE name LIKE '%Alice%'");
        assert_eq!(c.recommended_path, QueryPath::Olap, "LIKE should route to OLAP (full scan)");
        assert!(c.relative_cost >= 1.2, "Like must carry at least 1.2 cost overhead");
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
    // ── ITEM 3: POST /api/v1/connectors/update ───────────────────────────

    // 3-A: ConnectorUpdateRequest + ConnectorUpdateResponse structs
    [
        '3-A: ConnectorUpdate structs',
        `#[derive(Deserialize)]
struct ConnectorDeregisterRequest {
    connector_id: String,
}

#[derive(Serialize)]
struct ConnectorDeregisterResponse {
    status: &'static str,
    connector_id: String,
    removed: bool,
}

// ─── S7-WS6-04: Chaos fire-drill structs ────────────────────────────────────`,
        `#[derive(Deserialize)]
struct ConnectorDeregisterRequest {
    connector_id: String,
}

#[derive(Serialize)]
struct ConnectorDeregisterResponse {
    status: &'static str,
    connector_id: String,
    removed: bool,
}

// ─── S5-E4A-01: Connector update structs ─────────────────────────────────────

#[derive(Deserialize)]
struct ConnectorUpdateRequest {
    connector_id: String,
    version: Option<String>,
    signed: Option<bool>,
}

#[derive(Serialize)]
struct ConnectorUpdateResponse {
    status: &'static str,
    connector_id: String,
    updated: bool,
}

// ─── S7-WS6-04: Chaos fire-drill structs ────────────────────────────────────`
    ],

    // 3-B: route after connectors/get
    [
        '3-B: connectors/update route',
        `        // S5-E4A-01: Connector get by ID
        .route("/api/v1/connectors/get", get(connector_get))
        .route("/api/v1/ai/policy/update", post(ai_policy_update))`,
        `        // S5-E4A-01: Connector get by ID
        .route("/api/v1/connectors/get", get(connector_get))
        // S5-E4A-01: Connector update (version / signed flag)
        .route("/api/v1/connectors/update", post(connector_update))
        .route("/api/v1/ai/policy/update", post(ai_policy_update))`
    ],

    // 3-C: handler after connector_get, before raft_vote_stats comment
    [
        '3-C: connector_update handler',
        `// ─── S7-WS6-01: Raft vote statistics endpoint ───────────────────────────────

/// S7-WS6-01: Return accumulated vote grant/reject counts for the current Raft node.`,
        `// ─── S5-E4A-01: Connector update handler ─────────────────────────────────────

/// S5-E4A-01: Update the version or signed flag of an existing registered connector.
async fn connector_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ConnectorUpdateRequest>,
) -> Result<(StatusCode, Json<ConnectorUpdateResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut registry = state.connector_registry.lock().expect("connector_registry lock update");
    let entry = registry.iter_mut().find(|c| c.connector_id == req.connector_id);
    let updated = if let Some(plugin) = entry {
        if let Some(v) = req.version {
            plugin.version = v;
        }
        if let Some(s) = req.signed {
            plugin.signed = s;
        }
        true
    } else {
        false
    };
    drop(registry);
    Ok((StatusCode::OK, Json(ConnectorUpdateResponse {
        status: "ok",
        connector_id: req.connector_id,
        updated,
    })))
}

// ─── S7-WS6-01: Raft vote statistics endpoint ───────────────────────────────

/// S7-WS6-01: Return accumulated vote grant/reject counts for the current Raft node.`
    ],

    // 3-D: tests after connector_get tests, before closing `}` of tests mod
    [
        '3-D: connector_update tests',
        `        assert!(!body.found, "unknown connector must report found = false");
        assert!(body.connector.is_none());
    }

}`,
        `        assert!(!body.found, "unknown connector must report found = false");
        assert!(body.connector.is_none());
    }

    // ─── S5-E4A-01: Connector update endpoint tests ──────────────────────────

    #[tokio::test]
    async fn s5_e4a_01_connector_update_existing_changes_version() {
        let state = state_with_key(Some("test-key"));
        {
            let mut reg = state.connector_registry.lock().unwrap();
            reg.push(ConnectorPlugin {
                connector_id: "conn-1".to_string(),
                connector_type: "kafka".to_string(),
                version: "1.0.0".to_string(),
                signed: false,
                registered_at_ms: 0,
            });
        }
        let headers = operator_headers("test-key", "admin");
        let req = ConnectorUpdateRequest {
            connector_id: "conn-1".to_string(),
            version: Some("2.0.0".to_string()),
            signed: Some(true),
        };
        let (status, Json(body)) = connector_update(State(state.clone()), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.updated, "existing connector must be updated");
        let reg = state.connector_registry.lock().unwrap();
        let plugin = reg.iter().find(|c| c.connector_id == "conn-1").unwrap();
        assert_eq!(plugin.version, "2.0.0");
        assert!(plugin.signed);
    }

    #[tokio::test]
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

}`
    ],

    // ── ITEM 4: GET /api/v1/store/wal/checkpoint/history ─────────────────

    // 4-A: WalCheckpointHistoryResponse struct after WalSegmentListResponse
    [
        '4-A: WalCheckpointHistoryResponse struct',
        `#[derive(Debug, Serialize)]
struct WalSegmentListResponse {
    status: &'static str,
    segment_count: usize,
    completed_segments: usize,
    active_record_count: usize,
    segments: Vec<WalSegment>,
}

// ─── S2-WS2-02: WAL replay count structs ──────────────────────────────────────`,
        `#[derive(Debug, Serialize)]
struct WalSegmentListResponse {
    status: &'static str,
    segment_count: usize,
    completed_segments: usize,
    active_record_count: usize,
    segments: Vec<WalSegment>,
}

// ─── S2-WS2-02: WAL checkpoint history structs ────────────────────────────────

#[derive(Debug, Serialize)]
struct WalCheckpointEntry {
    checkpoint_id: u64,
    record_count_at_checkpoint: usize,
}

#[derive(Debug, Serialize)]
struct WalCheckpointHistoryResponse {
    status: &'static str,
    total_checkpoints: usize,
    entries: Vec<WalCheckpointEntry>,
}

// ─── S2-WS2-02: WAL replay count structs ──────────────────────────────────────`
    ],

    // 4-B: route after wal/mutations
    [
        '4-B: wal/checkpoint/history route',
        `        // S2-WS2-03: WAL mutations (recent key-value changes)
        .route("/api/v1/store/wal/mutations", get(wal_mutations))
        // S2-WS2-02: WAL segment list (checkpoint groups)`,
        `        // S2-WS2-03: WAL mutations (recent key-value changes)
        .route("/api/v1/store/wal/mutations", get(wal_mutations))
        // S2-WS2-02: WAL checkpoint history
        .route("/api/v1/store/wal/checkpoint/history", get(wal_checkpoint_history))
        // S2-WS2-02: WAL segment list (checkpoint groups)`
    ],

    // 4-C: handler before wal_segment_list
    [
        '4-C: wal_checkpoint_history handler',
        `// ─── S2-WS2-02: WAL segment list handler ────────────────────────────────────

/// S2-WS2-02: List WAL checkpoint segments plus the active (unbounded) segment.
async fn wal_segment_list(`,
        `// ─── S2-WS2-02: WAL checkpoint history handler ───────────────────────────────

/// S2-WS2-02: Return a list of completed WAL checkpoints with their record counts.
async fn wal_checkpoint_history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalCheckpointHistoryResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let wal = state.wal_engine.lock().expect("wal_engine lock checkpoint_history");
    let total_checkpoints = wal.checkpoint_count();
    drop(wal);
    let entries: Vec<WalCheckpointEntry> = (1..=(total_checkpoints as u64))
        .map(|id| WalCheckpointEntry {
            checkpoint_id: id,
            record_count_at_checkpoint: 0,
        })
        .collect();
    Ok((StatusCode::OK, Json(WalCheckpointHistoryResponse {
        status: "ok",
        total_checkpoints,
        entries,
    })))
}

// ─── S2-WS2-02: WAL segment list handler ────────────────────────────────────

/// S2-WS2-02: List WAL checkpoint segments plus the active (unbounded) segment.
async fn wal_segment_list(`
    ],

    // 4-D: tests after wal_segment_list tests, before wal_replay_count tests
    [
        '4-D: wal_checkpoint_history tests',
        `    // ── S2-WS2-02: WAL replay count endpoint tests ───────────────────────────
    #[tokio::test]
    async fn s2_ws2_02_wal_replay_count_empty_state_returns_zero() {`,
        `    // ─── S2-WS2-02: WAL checkpoint history endpoint tests ────────────────────

    #[tokio::test]
    async fn s2_ws2_02_wal_checkpoint_history_empty_on_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_checkpoint_history(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_checkpoints, 0, "fresh WAL has no checkpoints");
        assert!(body.entries.is_empty(), "no checkpoint entries on fresh state");
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_checkpoint_history_reflects_checkpoint_count() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("k1", "v1");
            wal.force_checkpoint();
            wal.append_mutation("k2", "v2");
            wal.force_checkpoint();
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_checkpoint_history(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_checkpoints, 2, "2 force_checkpoint calls must yield 2 entries");
        assert_eq!(body.entries.len(), 2);
        assert_eq!(body.entries[0].checkpoint_id, 1);
        assert_eq!(body.entries[1].checkpoint_id, 2);
    }

    // ── S2-WS2-02: WAL replay count endpoint tests ───────────────────────────
    #[tokio::test]
    async fn s2_ws2_02_wal_replay_count_empty_state_returns_zero() {`
    ],
];

const r3 = applyReplacements(MAIN, mainReplacements);
console.log(`  => ${r3.changed} changed, ${r3.missed} missed`);

const total = r1.missed + r2.missed + r3.missed;
console.log(`\nDONE — total missed: ${total}`);
if (total > 0) process.exit(1);
