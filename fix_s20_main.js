#!/usr/bin/env node
// fix_s20_main.js — Session 20 endpoint additions
// S5-WS4-03  GET /api/v1/ingest/schema
// S2-WS2-05  GET /api/v1/sql/transactions/isolation
// S7-WS6-02  GET /api/v1/cluster/raft/commit
// S8-WS10-02 GET /api/v1/driver/health

const fs = require('fs');
const path = require('path');

const FILE = path.join('services', 'voltnuerongridd', 'src', 'main.rs');
let src = fs.readFileSync(FILE, 'utf8').replace(/\r\n/g, '\n');
let n = 0;

function once(anchor, replacement) {
  const count = src.split(anchor).length - 1;
  if (count === 0) { console.error('ANCHOR NOT FOUND:\n  ' + anchor.slice(0, 120)); process.exit(1); }
  if (count > 1)   { console.error('MULTIPLE MATCHES (' + count + '):\n  ' + anchor.slice(0, 120)); process.exit(1); }
  src = src.replace(anchor, replacement);
  n++;
  console.log(`  [${n}] applied`);
}

// ── A. New structs ──────────────────────────────────────────────────────────
// Insert all new response structs just before IngestOutboxStatusResponse.

once(
`#[derive(Serialize)]
struct IngestOutboxStatusResponse {`,
`// ─── S5-WS4-03: Ingest schema registry structs ──────────────────────────────

#[derive(Serialize)]
struct IngestSchemaColumn {
    name: String,
    inferred_type: &'static str,
}

#[derive(Serialize)]
struct IngestSchemaEntry {
    connector_id: String,
    format: String,
    row_count: usize,
    columns: Vec<IngestSchemaColumn>,
}

#[derive(Serialize)]
struct IngestSchemaRegistryResponse {
    status: &'static str,
    connector_count: usize,
    entries: Vec<IngestSchemaEntry>,
}

// ─── S2-WS2-05: Transaction isolation stats structs ──────────────────────────

#[derive(Serialize)]
struct TxIsolationEntry {
    transaction_id: String,
    isolation_level: String,
    snapshot_xid: Option<u64>,
    statement_count: usize,
}

#[derive(Serialize)]
struct TxIsolationStatsResponse {
    status: &'static str,
    active_count: usize,
    transactions: Vec<TxIsolationEntry>,
}

// ─── S7-WS6-02: Raft commit progress struct ──────────────────────────────────

#[derive(Serialize)]
struct RaftCommitProgressResponse {
    status: &'static str,
    commit_index: u64,
    last_applied: u64,
    log_length: usize,
    uncommitted: usize,
}

// ─── S8-WS10-02: Driver health struct ────────────────────────────────────────

#[derive(Serialize)]
struct DriverHealthResponse {
    status: &'static str,
    active_sessions: usize,
    pool_circuit_breaker: String,
    pool_active_connections: usize,
    pool_total_acquired: u64,
    healthy: bool,
}

#[derive(Serialize)]
struct IngestOutboxStatusResponse {`
);

// ── B. Routes ───────────────────────────────────────────────────────────────

once(
`        .route("/api/v1/ingest/status", get(ingest_status))`,
`        .route("/api/v1/ingest/status", get(ingest_status))
        // S5-WS4-03: ingest schema registry
        .route("/api/v1/ingest/schema", get(ingest_schema_registry))`
);

once(
`        .route("/api/v1/sql/transactions/active", get(sql_transactions_active))`,
`        .route("/api/v1/sql/transactions/active", get(sql_transactions_active))
        // S2-WS2-05: isolation stats per active transaction
        .route("/api/v1/sql/transactions/isolation", get(sql_transactions_isolation))`
);

once(
`        .route("/api/v1/cluster/raft/log", get(raft_log))`,
`        .route("/api/v1/cluster/raft/log", get(raft_log))
        // S7-WS6-02: raft commit progress
        .route("/api/v1/cluster/raft/commit", get(raft_commit_progress))`
);

once(
`        .route("/api/v1/driver/sessions", get(driver_session_list))`,
`        .route("/api/v1/driver/sessions", get(driver_session_list))
        // S8-WS10-02: driver pool health
        .route("/api/v1/driver/health", get(driver_health))`
);

// ── C. Handlers ─────────────────────────────────────────────────────────────

// S5-WS4-03: insert ingest_schema_registry before evaluate_deadlock_scan_outcome
once(
`fn evaluate_deadlock_scan_outcome(`,
`// ─── S5-WS4-03: Ingest schema registry handler ──────────────────────────────

async fn ingest_schema_registry(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<IngestSchemaRegistryResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_ingest_runtime_privilege(&headers, &state, PrivilegeAction::Read, "ingest/schema")?;
    let csv_map = state.ingest_csv_records.lock().expect("csv schema lock");
    let json_map = state.ingest_json_records.lock().expect("json schema lock");
    let mut entries: Vec<IngestSchemaEntry> = Vec::new();
    for (connector_id, records) in csv_map.iter() {
        let columns = ingest_infer_columns(records);
        entries.push(IngestSchemaEntry {
            connector_id: connector_id.clone(),
            format: "csv".to_string(),
            row_count: records.len(),
            columns,
        });
    }
    for (connector_id, records) in json_map.iter() {
        let columns = ingest_infer_columns(records);
        entries.push(IngestSchemaEntry {
            connector_id: connector_id.clone(),
            format: "json".to_string(),
            row_count: records.len(),
            columns,
        });
    }
    let connector_count = entries.len();
    drop(csv_map);
    drop(json_map);
    Ok((StatusCode::OK, Json(IngestSchemaRegistryResponse { status: "ok", connector_count, entries })))
}

fn ingest_infer_columns(
    records: &[voltnuerongrid_ingest::IngestRecord],
) -> Vec<IngestSchemaColumn> {
    if records.is_empty() {
        return vec![IngestSchemaColumn { name: "payload".to_string(), inferred_type: "utf8" }];
    }
    vec![
        IngestSchemaColumn { name: "key".to_string(), inferred_type: "utf8" },
        IngestSchemaColumn { name: "payload".to_string(), inferred_type: "utf8" },
    ]
}

fn evaluate_deadlock_scan_outcome(`
);

// S2-WS2-05: insert sql_transactions_isolation before store_htap_apply
once(
`/// Apply a batch of HTAP mutations to the in-memory OLAP replica.
async fn store_htap_apply(`,
`// ─── S2-WS2-05: Transaction isolation stats handler ─────────────────────────

async fn sql_transactions_isolation(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<TxIsolationStatsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_sql_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "sql/transactions/isolation",
    )?;
    let acid = state.acid_transactions.lock().expect("acid_tx isolation lock");
    let active = acid.active_transactions();
    let transactions: Vec<TxIsolationEntry> = active
        .iter()
        .map(|t| TxIsolationEntry {
            transaction_id: t.transaction_id.clone(),
            isolation_level: t.isolation_level.clone(),
            snapshot_xid: t.row_store_snapshot_xid,
            statement_count: t.statement_count,
        })
        .collect();
    let active_count = transactions.len();
    drop(acid);
    Ok((StatusCode::OK, Json(TxIsolationStatsResponse { status: "ok", active_count, transactions })))
}

/// Apply a batch of HTAP mutations to the in-memory OLAP replica.
async fn store_htap_apply(`
);

// S7-WS6-02: insert raft_commit_progress before raft_member_list section
once(
`// ─── S7-WS6-03: Raft cluster member list endpoint ────────────────────────────`,
`// ─── S7-WS6-02: Raft commit progress handler ────────────────────────────────

async fn raft_commit_progress(
    State(state): State<AppState>,
) -> (StatusCode, Json<RaftCommitProgressResponse>) {
    let node = state.raft_state.lock().expect("raft_state lock");
    let commit_index = node.commit_index;
    let last_applied = node.last_applied;
    let log_length = node.log.len();
    let uncommitted = log_length.saturating_sub(commit_index as usize);
    drop(node);
    (StatusCode::OK, Json(RaftCommitProgressResponse {
        status: "ok",
        commit_index,
        last_applied,
        log_length,
        uncommitted,
    }))
}

// ─── S7-WS6-03: Raft cluster member list endpoint ────────────────────────────`
);

// S8-WS10-02: insert driver_health after ingest_schema_registry
// (evaluate_deadlock_scan_outcome was already used as anchor above for ingest_schema_registry,
//  so use the unique next sentinel: function signature line)
once(
`fn evaluate_deadlock_scan_outcome(
    wait_graph: &HashMap<String, String>,`,
`// ─── S8-WS10-02: Driver health handler ──────────────────────────────────────

async fn driver_health(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<DriverHealthResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let active_sessions = state.driver_sessions.lock().expect("driver_sessions health lock").len();
    let now_ms = now_unix_ms_u64();
    let pool_stats = state.driver_pool.lock().expect("driver_pool health lock").pool_stats(now_ms);
    let healthy = pool_stats.circuit_breaker_state == "closed" && active_sessions < 1_000;
    Ok((StatusCode::OK, Json(DriverHealthResponse {
        status: "ok",
        active_sessions,
        pool_circuit_breaker: pool_stats.circuit_breaker_state.clone(),
        pool_active_connections: pool_stats.active_connections,
        pool_total_acquired: pool_stats.total_acquired,
        healthy,
    })))
}

fn evaluate_deadlock_scan_outcome(
    wait_graph: &HashMap<String, String>,`
);

// ── D. Tests ─────────────────────────────────────────────────────────────────

// S7-WS6-02: raft commit progress tests
once(
`    // ─── S7-WS6-03: Raft fencing token tests ─────────────────────────────`,
`    // ── S7-WS6-02: Raft commit progress endpoint tests ──────────────────────

    #[tokio::test]
    async fn s7_ws6_02_raft_commit_progress_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let (status, Json(body)) = raft_commit_progress(State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.commit_index, 0);
        assert_eq!(body.log_length, 0);
        assert_eq!(body.uncommitted, 0);
    }

    #[tokio::test]
    async fn s7_ws6_02_raft_commit_progress_after_log_append() {
        let state = state_with_key(Some("test-key"));
        {
            let mut node = state.raft_state.lock().unwrap();
            node.log.push(raft::RaftLogEntry { index: 1, term: 1, command: "SET x=1".to_string() });
        }
        let (status, Json(body)) = raft_commit_progress(State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.log_length, 1);
        assert_eq!(body.uncommitted, 1, "log has 1 entry, commit_index=0 => uncommitted=1");
    }

    // ─── S7-WS6-03: Raft fencing token tests ─────────────────────────────`
);

// S8-WS10-02: driver health tests
once(
`    // ── S10-WS15-02: CDC stream filter ────────────────────────────────────────`,
`    // ── S8-WS10-02: Driver health endpoint tests ─────────────────────────────

    #[tokio::test]
    async fn s8_ws10_02_driver_health_fresh_state_no_sessions() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = driver_health(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.active_sessions, 0);
        assert_eq!(body.pool_circuit_breaker, "closed");
        assert!(body.healthy);
    }

    #[tokio::test]
    async fn s8_ws10_02_driver_health_reflects_active_sessions() {
        let state = state_with_key(Some("test-key"));
        {
            let mut sessions = state.driver_sessions.lock().unwrap();
            sessions.insert("sess-1".to_string(), DriverSession {
                driver_name: "rust-driver".to_string(),
                driver_version: "1.0.0".to_string(),
                connected_at_ms: 0,
            });
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = driver_health(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.active_sessions, 1);
        assert!(body.healthy);
    }

    // ── S10-WS15-02: CDC stream filter ────────────────────────────────────────`
);

// S5-WS4-03: ingest schema tests
once(
`    // ─── S5-WS4A-02: Broker adapter integration tests ────────────────────────`,
`    // ── S5-WS4-03: Ingest schema registry endpoint tests ─────────────────────

    #[tokio::test]
    async fn s5_ws4_03_ingest_schema_empty_state_no_connectors() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = ingest_schema_registry(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.connector_count, 0);
        assert!(body.entries.is_empty());
    }

    #[tokio::test]
    async fn s5_ws4_03_ingest_schema_reflects_csv_connector() {
        use voltnuerongrid_ingest::IngestRecord;
        let state = state_with_key(Some("test-key"));
        {
            let mut csv = state.ingest_csv_records.lock().unwrap();
            csv.insert("csv-orders".to_string(), vec![
                IngestRecord { key: "r1".to_string(), payload: "id=1".to_string() },
                IngestRecord { key: "r2".to_string(), payload: "id=2".to_string() },
            ]);
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = ingest_schema_registry(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.connector_count, 1);
        assert_eq!(body.entries[0].connector_id, "csv-orders");
        assert_eq!(body.entries[0].format, "csv");
        assert_eq!(body.entries[0].row_count, 2);
        assert!(!body.entries[0].columns.is_empty());
    }

    // ─── S5-WS4A-02: Broker adapter integration tests ────────────────────────`
);

// S2-WS2-05: isolation stats tests
once(
`    // ─── S2-WS2-05: Write-write conflict detection ────────────────────────────`,
`    // ── S2-WS2-05: Transaction isolation stats endpoint tests ─────────────────

    #[tokio::test]
    async fn s2_ws2_05_isolation_stats_empty_on_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = sql_transactions_isolation(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.active_count, 0);
        assert!(body.transactions.is_empty());
    }

    #[tokio::test]
    async fn s2_ws2_05_isolation_stats_shows_active_transaction() {
        let state = state_with_key(Some("test-key"));
        {
            let mut acid = state.acid_transactions.lock().unwrap();
            acid.begin("tx-iso-1", "serializable", 0u128);
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = sql_transactions_isolation(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.active_count, 1);
        assert_eq!(body.transactions[0].transaction_id, "tx-iso-1");
        assert_eq!(body.transactions[0].isolation_level, "serializable");
    }

    // ─── S2-WS2-05: Write-write conflict detection ────────────────────────────`
);

fs.writeFileSync(FILE, src.replace(/\n/g, '\r\n'), 'utf8');
console.log(`\nDone: ${n} replacements applied.`);
