use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::Ordering;
use voltnuerongrid_audit::AuditEventKind;
use voltnuerongrid_auth::PrivilegeAction;
use voltnuerongrid_exec::QueryPath;
use voltnuerongrid_sql::{eval_legacy_numeric_aggregation, I18nCatalog, SqlAnalyzer, SqlStatementKind};
use voltnuerongrid_sql::legacy_aggregations::SUPPORTED_LEGACY_AGGREGATIONS;
use voltnuerongrid_store::ddl_catalog::{parse_ddl_info, CatalogResult};
use crate::{AppState, AuthErrorResponse, RuntimeAccessPrincipal, AcidTxEntry};
use crate::{SqlTransactionResponse, PessimisticLockRecord};
use crate::{CommandDispatcher, CanonicalCommandName, CanonicalError};
use crate::{now_unix_ms, build_http_envelope};
use crate::{execute_transaction_statements, acquire_sql_data_plane_connection, release_sql_data_plane_connection};
use crate::{acquire_pessimistic_lock, release_pessimistic_lock};
use crate::{execute_oltp_select, df_select_owned, run_async_in_executor};
use crate::{execute_udf_runtime_scaffold, udf_function_catalog_contract, udf_guard_policy_contract, build_udf_execution_plan};
use crate::{route_path_name, try_handle_call_insert_rows_demo};
use crate::{extract_delete_key_from_sql, extract_update_row_from_sql, extract_column_names_from_ddl, extract_insert_row_from_sql, extract_all_insert_rows};
use crate::{persist_sql_statement};
use crate::auth::{require_sql_runtime_principal, locale_from_headers};
use crate::audit_helpers::append_runtime_audit_event;

// ─── SQL DTOs ─────────────────────────────────────────────────────────────────

#[derive(Clone, Deserialize)]
pub(crate) struct SqlTransactionRequest {
    pub(crate) statements: Vec<String>,
    /// Requested isolation level: "read_committed" (default), "repeatable_read", "serializable"
    pub(crate) isolation_level: Option<String>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct SqlAnalyzeRequest {
    pub(crate) sql_batch: String,
}

#[derive(Serialize)]
pub(crate) struct AnalyzedStatement {
    pub(crate) statement: String,
    pub(crate) kind: String,
    pub(crate) requires_transaction: bool,
    pub(crate) touches_catalog: bool,
    pub(crate) accepted: bool,
}

#[derive(Serialize)]
pub(crate) struct SqlAnalyzeResponse {
    pub(crate) status: &'static str,
    pub(crate) total_statements: usize,
    pub(crate) rejected_statements: usize,
    pub(crate) statements: Vec<AnalyzedStatement>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct SqlRouteRequest {
    pub(crate) sql_batch: String,
}

#[derive(Serialize)]
pub(crate) struct RoutedStatementResponse {
    pub(crate) statement: String,
    /// Routing path from `HtapQueryRouter` (heuristic).
    pub(crate) path: String,
    /// Cost-model recommended path from `QueryPlanner` (S3-WS1-05).
    pub(crate) planner_path: String,
    pub(crate) estimated_rows: u64,
    pub(crate) relative_cost: f64,
}

#[derive(Serialize)]
pub(crate) struct SqlRouteResponse {
    pub(crate) status: &'static str,
    pub(crate) route_path: String,
    pub(crate) reason: String,
    pub(crate) statements: Vec<RoutedStatementResponse>,
    /// Aggregate planner cost across all statements in the batch.
    pub(crate) batch_estimated_rows: u64,
    pub(crate) batch_relative_cost: f64,
}

#[derive(Clone, Deserialize)]
pub(crate) struct SqlExecuteRequest {
    pub(crate) sql_batch: String,
    pub(crate) max_rows: Option<usize>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct LegacyAggResult {
    /// Aggregate function name (e.g. `"SUM"`, `"COUNT"`).
    pub(crate) aggregation: String,
    /// Computed result; `None` when evaluation errored.
    pub(crate) result: Option<f64>,
    /// Error message when evaluation failed.
    pub(crate) error: Option<String>,
    /// Indicates this result came through the legacy aggregation routing path.
    pub(crate) source: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SqlExecuteResponse {
    pub(crate) status: String,
    pub(crate) route_path: String,
    pub(crate) reason: String,
    pub(crate) transaction: Option<SqlTransactionResponse>,
    pub(crate) olap: Option<OlapQueryResponse>,
    pub(crate) rejected_statement_count: usize,
    #[serde(skip_deserializing, default)]
    pub(crate) udf_results: Option<Vec<UdfExecutionResult>>,
    pub(crate) udf_guardrail_status: Option<String>,
    #[serde(skip_deserializing, default)]
    pub(crate) udf_function_catalog: Vec<UdfFunctionCatalogEntry>,
    #[serde(skip_deserializing, default)]
    pub(crate) udf_guard_policies: Vec<UdfLanguageGuardPolicy>,
    #[serde(skip_deserializing, default)]
    pub(crate) udf_execution_plan: Vec<UdfExecutionPlanStep>,
    pub(crate) legacy_agg_results: Option<Vec<LegacyAggResult>>,
    /// Dominant cost-model recommended path for the batch (S3-WS1-05).
    pub(crate) planner_path: Option<String>,
    /// Physical OLTP executor results: actual rows from PagedRowStore for point-read SELECT (S4-WS3-02).
    pub(crate) oltp_rows: Option<Vec<OltpRowResult>>,
    /// Vectorized OLAP aggregation results from columnar executor (S4-WS3-02).
    pub(crate) olap_agg_results: Option<Vec<OlapVecAggResult>>,
    /// Column metadata for SELECT results — readable by the UI client.
    pub(crate) columns: Option<Vec<serde_json::Value>>,
    /// Row data for SELECT results — readable by the UI client.
    pub(crate) rows: Option<Vec<serde_json::Value>>,
}

/// S4-WS3-02: a single result row returned by the physical OLTP executor.
#[derive(Serialize, Deserialize)]
pub(crate) struct OltpRowResult {
    pub(crate) key: String,
    pub(crate) data: std::collections::HashMap<String, String>,
}

/// S4-WS3-02: a single vectorized aggregation result from the OLAP columnar executor.
#[derive(Serialize, Deserialize)]
pub(crate) struct OlapVecAggResult {
    pub(crate) column: String,
    pub(crate) op: String,
    pub(crate) value: String,
    pub(crate) row_count: usize,
}

// ─── UDF DTOs ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct UdfExecutionResult {
    pub(crate) language: &'static str,
    pub(crate) function: &'static str,
    pub(crate) input: String,
    pub(crate) output: String,
}

#[derive(Serialize)]
pub(crate) struct UdfFunctionCatalogEntry {
    pub(crate) name: &'static str,
    pub(crate) language: &'static str,
    pub(crate) deterministic: bool,
    pub(crate) status: &'static str,
}

#[derive(Serialize)]
pub(crate) struct UdfLanguageGuardPolicy {
    pub(crate) language: &'static str,
    pub(crate) blocked_tokens: Vec<&'static str>,
    pub(crate) max_input_bytes: usize,
}

#[derive(Serialize)]
pub(crate) struct UdfExecutionPlanStep {
    pub(crate) statement: String,
    pub(crate) route_path: String,
    pub(crate) udf_invocations: Vec<UdfInvocationPlan>,
}

#[derive(Serialize)]
pub(crate) struct UdfInvocationPlan {
    pub(crate) function: &'static str,
    pub(crate) language: &'static str,
    pub(crate) guard_policy: &'static str,
}

// ─── PessimisticLock DTOs ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct PessimisticLockAcquireRequest {
    pub(crate) transaction_id: String,
    pub(crate) resource: String,
    pub(crate) owner: Option<String>,
    pub(crate) ttl_ms: Option<u64>,
    pub(crate) wait_timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
pub(crate) struct PessimisticLockReleaseRequest {
    pub(crate) transaction_id: String,
    pub(crate) resource: String,
}

#[derive(Serialize)]
pub(crate) struct PessimisticLockResponse {
    pub(crate) status: &'static str,
    pub(crate) lock_state: &'static str,
    pub(crate) reason: String,
    pub(crate) lock: Option<PessimisticLockRecord>,
}

#[derive(Serialize)]
pub(crate) struct PessimisticLockContentionMetricsResponse {
    pub(crate) status: &'static str,
    pub(crate) deadlock_detections: u64,
    pub(crate) scan_cap_timeouts: u64,
    pub(crate) wait_timeouts: u64,
    pub(crate) lock_grants: u64,
    pub(crate) lock_conflicts: u64,
    pub(crate) lock_releases: u64,
    pub(crate) contention_ratio: f64,
}

// ─── S2-WS2-05: Transaction isolation stats structs ──────────────────────────

#[derive(Serialize)]
pub(crate) struct TxIsolationEntry {
    pub(crate) transaction_id: String,
    pub(crate) isolation_level: String,
    pub(crate) snapshot_xid: Option<u64>,
    pub(crate) statement_count: usize,
}

#[derive(Serialize)]
pub(crate) struct TxIsolationStatsResponse {
    pub(crate) status: &'static str,
    pub(crate) active_count: usize,
    pub(crate) transactions: Vec<TxIsolationEntry>,
}

// ─── OLAP DTOs ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct OlapQueryRequest {
    pub(crate) query: String,
    pub(crate) max_rows: Option<usize>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct OlapQueryResponse {
    pub(crate) status: String,
    pub(crate) query_signature: String,
    pub(crate) elapsed_ms: u128,
    pub(crate) rows: usize,
}

// ─── AcidTransactions DTO ─────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct AcidTransactionsResponse {
    pub(crate) status: &'static str,
    pub(crate) active_count: usize,
    pub(crate) total_count: usize,
    pub(crate) transactions: Vec<AcidTxEntry>,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

pub(crate) async fn sql_transaction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SqlTransactionRequest>,
) -> Result<(StatusCode, Json<SqlTransactionResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_sql_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Execute,
        "sql/transaction",
    )?;
    let dispatcher = CommandDispatcher::new();
    let envelope = build_http_envelope(
        &headers,
        CanonicalCommandName::SqlTransaction,
        req.clone(),
        "http-sql-transaction",
    );
    let tx_context = dispatcher.dispatch_sql_transaction_context(&envelope);
    let statements = tx_context.payload.statements;
    let requested_isolation_level = tx_context.payload.isolation_level;
    let connection_id = acquire_sql_data_plane_connection(&state, &headers, &principal, "sql/transaction")?;
    // REQ-23: ACID transaction state machine tracking
    {
        let now_ms = now_unix_ms();
        let tx_id = {
            let identity = match &principal {
                RuntimeAccessPrincipal::Operator(op) => op.operator_id.clone(),
                RuntimeAccessPrincipal::TenantUser(tu) => tu.user_id.clone(),
            };
            format!("tx-{}-{}", identity, now_ms)
        };
        let has_begin = statements.iter().any(|s| {
            matches!(SqlAnalyzer::analyze_statement(s).kind, SqlStatementKind::Begin)
        });
        let has_commit = statements.iter().any(|s| {
            matches!(SqlAnalyzer::analyze_statement(s).kind, SqlStatementKind::Commit)
        });
        let has_rollback = statements.iter().any(|s| {
            matches!(SqlAnalyzer::analyze_statement(s).kind, SqlStatementKind::Rollback)
        });
        let iso_level = requested_isolation_level
            .as_deref()
            .unwrap_or("read_committed")
            .to_string();
        let mut acid = state.acid_transactions.lock().expect("acid_tx lock");
        if has_begin {
            acid.begin(&tx_id, &state.node_id, &iso_level, now_ms);
        }
        for stmt in &statements {
            let upper = stmt.to_ascii_uppercase();
            let kind = SqlAnalyzer::classify_statement(stmt);
            // REQ-23: wire SAVEPOINT / RELEASE SAVEPOINT / ROLLBACK TO SAVEPOINT
            match kind {
                SqlStatementKind::Savepoint => {
                    // Extract savepoint name: SAVEPOINT <name>
                    if let Some(sp_name) = stmt.split_ascii_whitespace().nth(1) {
                        acid.add_savepoint(&tx_id, sp_name);
                    }
                }
                SqlStatementKind::ReleaseSavepoint => {
                    // Extract savepoint name: RELEASE SAVEPOINT <name>
                    if let Some(sp_name) = stmt.split_ascii_whitespace().nth(2) {
                        acid.release_savepoint(&tx_id, sp_name);
                    }
                }
                SqlStatementKind::RollbackToSavepoint => {
                    // Extract savepoint name: ROLLBACK TO [SAVEPOINT] <name>
                    // Tokens: ROLLBACK(0) TO(1) [SAVEPOINT(2)] name(2 or 3)
                    let tokens: Vec<&str> = stmt.split_ascii_whitespace().collect();
                    let sp_name = if tokens.get(2).map(|t| t.to_ascii_uppercase()) == Some("SAVEPOINT".to_string()) {
                        tokens.get(3).copied()
                    } else {
                        tokens.get(2).copied()
                    };
                    if let Some(sp) = sp_name {
                        acid.rollback_to_savepoint(&tx_id, sp);
                    }
                }
                _ => {}
            }
            // REQ-23: extract modified table for conflict detection
            // UPDATE <table> SET ... → token index 1; INSERT INTO <table> / DELETE FROM <table> → index 2
            let affected = if upper.starts_with("UPDATE ") {
                stmt.split_ascii_whitespace()
                    .nth(1)
                    .map(|t| t.trim_end_matches(|c: char| c == '(' || c == ' ').to_string())
            } else if upper.starts_with("INSERT INTO ") || upper.starts_with("DELETE FROM ") {
                stmt.split_ascii_whitespace()
                    .nth(2)
                    .map(|t| t.trim_end_matches(|c: char| c == '(' || c == ' ').to_string())
            } else {
                None
            };
            acid.record_statement(&tx_id, affected);
        }
        if has_commit {
            // REQ-23: abort with 409 if a serializable write conflict is detected
            if let Some(conflict_table) = acid.check_serializable_conflict(&tx_id) {
                acid.rollback(&tx_id, now_ms);
                drop(acid);
                let locale = locale_from_headers(&headers);
                let localized = I18nCatalog::message(locale, "unauthorized");
                return Err((
                    StatusCode::CONFLICT,
                    Json(AuthErrorResponse {
                        status: "error",
                        reason: format!("serializable_write_conflict:{conflict_table}"),
                        locale: locale.as_str().to_string(),
                        localized_message: localized.message.to_string(),
                    }),
                ));
            }
            // S2-WS2-05: write-write conflict detection using row-store snapshot xid.
            // Collect keys about to be written and check for concurrent modifications.
            {
                let mut write_keys: Vec<String> = Vec::new();
                for stmt in &statements {
                    let upper = stmt.trim_start().to_ascii_uppercase();
                    if upper.starts_with("INSERT") {
                        if let Some((k, _)) = extract_insert_row_from_sql(stmt) {
                            write_keys.push(k);
                        }
                    } else if upper.starts_with("UPDATE") {
                        if let Some((k, _)) = extract_update_row_from_sql(stmt) {
                            write_keys.push(k);
                        }
                    } else if upper.starts_with("DELETE") {
                        if let Some(k) = extract_delete_key_from_sql(stmt) {
                            write_keys.push(k);
                        }
                    }
                }
                if !write_keys.is_empty() {
                    let rs = state.row_store.lock().expect("row_store lock conflict check");
                    let snapshot_xid = acid.row_store_snapshot_xid(&tx_id)
                        .unwrap_or(0);
                    for key in &write_keys {
                        if rs.was_modified_after(key, snapshot_xid) {
                            drop(rs);
                            acid.rollback(&tx_id, now_ms);
                            drop(acid);
                            let locale = locale_from_headers(&headers);
                            let localized = I18nCatalog::message(locale, "unauthorized");
                            let canonical_error = CanonicalError {
                                request_id: tx_context.request_id.clone(),
                                transport: tx_context.transport,
                                kind: "conflict",
                                message: format!("write_write_conflict:{key}"),
                            };
                            return Err((
                                StatusCode::CONFLICT,
                                Json(AuthErrorResponse {
                                    status: "error",
                                    reason: canonical_error.message,
                                    locale: locale.as_str().to_string(),
                                    localized_message: localized.message.to_string(),
                                }),
                            ));
                        }
                    }
                }
            }
            acid.commit(&tx_id, now_ms);
            // S2-WS2-05: flush committed DML (INSERT/UPDATE/DELETE) into PagedRowStore.
            // Write intents are registered before each write and released after the flush
            // so that concurrent transactions see the in-progress lock via begin_write_intent.
            {
                let mut rs = state.row_store.lock().expect("row_store lock");
                // Record snapshot xid before allocating the write xid
                let snapshot_xid = rs.current_xid();
                acid.set_row_store_snapshot(&tx_id, snapshot_xid);
                let xid = rs.begin_xid();
                for stmt in &statements {
                    let upper = stmt.trim_start().to_ascii_uppercase();
                    if upper.starts_with("INSERT") {
                        // Use extract_all_insert_rows to handle multi-row INSERT correctly.
                        // Each row is individually inserted and individually WAL-persisted.
                        for (k, d, single_sql) in extract_all_insert_rows(stmt) {
                            let _ = rs.begin_write_intent(xid, &k);
                            rs.insert(xid, &k, d);
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, &single_sql);
                        }
                    } else if upper.starts_with("DELETE") {
                        if let Some(k) = extract_delete_key_from_sql(stmt) {
                            let _ = rs.begin_write_intent(xid, &k);
                            rs.delete(xid, &k);
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                        }
                    } else if upper.starts_with("UPDATE") {
                        if let Some((k, d)) = extract_update_row_from_sql(stmt) {
                            let _ = rs.begin_write_intent(xid, &k);
                            rs.insert(xid, &k, d);
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                        }
                    }
                }
                // S2-WS2-02: record committed DML mutations in the WAL engine for
                // durability and recovery replay.
                {
                    let mut wal = state.wal_engine.lock().expect("wal_engine lock");
                    for stmt in &req.statements {
                        let upper = stmt.trim_start().to_ascii_uppercase();
                        if upper.starts_with("INSERT") {
                            if let Some((k, d)) = extract_insert_row_from_sql(stmt) {
                                let val = serde_json::to_string(&d).unwrap_or_default();
                                wal.append_mutation(&k, &val);
                            }
                        } else if upper.starts_with("DELETE") {
                            if let Some(k) = extract_delete_key_from_sql(stmt) {
                                wal.append_mutation(&k, "__deleted__");
                            }
                        } else if upper.starts_with("UPDATE") {
                            if let Some((k, d)) = extract_update_row_from_sql(stmt) {
                                let val = serde_json::to_string(&d).unwrap_or_default();
                                wal.append_mutation(&k, &val);
                            }
                        }
                    }
                    let _ = wal.maybe_checkpoint();
                }
                // Release all intents for this xid — writes are now committed and visible.
                rs.release_write_intents(xid);
            }
            // S4-WS3-04: publish each committed DML mutation to RowStoreSyncOrigin for HTAP consumers.
            {
                use voltnuerongrid_store::htap_sync::MutationOp;
                let mut origin = state.sync_origin.lock().expect("sync_origin lock");
                for stmt in &req.statements {
                    let upper = stmt.trim_start().to_ascii_uppercase();
                    if upper.starts_with("INSERT") {
                        if let Some((k, _d)) = extract_insert_row_from_sql(stmt) {
                            origin.append("row_store", &k, stmt, MutationOp::Insert);
                        }
                    } else if upper.starts_with("DELETE") {
                        if let Some(k) = extract_delete_key_from_sql(stmt) {
                            origin.append("row_store", &k, stmt, MutationOp::Delete);
                        }
                    } else if upper.starts_with("UPDATE") {
                        if let Some((k, _d)) = extract_update_row_from_sql(stmt) {
                            origin.append("row_store", &k, stmt, MutationOp::Update);
                        }
                    }
                }
            }
        } else if has_rollback {
            acid.rollback(&tx_id, now_ms);
        }
    }
    let (status, response) = execute_transaction_statements(req.statements);
    append_runtime_audit_event(
        &state,
        AuditEventKind::Sql,
        &principal,
        "sql_transaction",
        if status == StatusCode::OK { "ok" } else { "error" },
        json!({
            "route_scope": "sql/transaction",
            "statements_executed": response.statements_executed,
            "requires_transaction": response.requires_transaction,
            "touches_catalog": response.touches_catalog,
            "rejected_statement_count": response.rejected_statement_count,
        }),
    );
    release_sql_data_plane_connection(&state, &connection_id);
    Ok((status, Json(response)))
}

pub(crate) async fn sql_pessimistic_lock_acquire(
    State(state): State<AppState>,
    Json(req): Json<PessimisticLockAcquireRequest>,
) -> (StatusCode, Json<PessimisticLockResponse>) {
    let now_ms = now_unix_ms();
    let ttl_ms = req.ttl_ms.unwrap_or(30_000).clamp(1_000, 300_000);
    let owner = req
        .owner
        .unwrap_or_else(|| "runtime-transaction-manager".to_string());
    let mut lock_table = match state.pessimistic_locks.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(PessimisticLockResponse {
                    status: "error",
                    lock_state: "failed",
                    reason: "lock_state_poisoned".to_string(),
                    lock: None,
                }),
            )
        }
    };
    let mut wait_graph = match state.pessimistic_lock_waits.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(PessimisticLockResponse {
                    status: "error",
                    lock_state: "failed",
                    reason: "wait_graph_state_poisoned".to_string(),
                    lock: None,
                }),
            )
        }
    };

    let (status, response) =
        acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            &req.transaction_id,
            &req.resource,
            &owner,
            ttl_ms,
            req.wait_timeout_ms.unwrap_or(0),
            now_ms,
        );
    match response.lock_state {
        "deadlock_risk" => { state.pessimistic_lock_metrics.deadlock_detections.fetch_add(1, Ordering::Relaxed); }
        "wait_timeout" if response.reason.contains("scan_cap") => { state.pessimistic_lock_metrics.scan_cap_timeouts.fetch_add(1, Ordering::Relaxed); }
        "wait_timeout" => { state.pessimistic_lock_metrics.wait_timeouts.fetch_add(1, Ordering::Relaxed); }
        "acquired" | "renewed" => { state.pessimistic_lock_metrics.lock_grants.fetch_add(1, Ordering::Relaxed); }
        "held_by_other_transaction" => { state.pessimistic_lock_metrics.lock_conflicts.fetch_add(1, Ordering::Relaxed); }
        _ => {}
    }
    (status, Json(response))
}

pub(crate) async fn sql_pessimistic_lock_release(
    State(state): State<AppState>,
    Json(req): Json<PessimisticLockReleaseRequest>,
) -> (StatusCode, Json<PessimisticLockResponse>) {
    let mut lock_table = match state.pessimistic_locks.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(PessimisticLockResponse {
                    status: "error",
                    lock_state: "failed",
                    reason: "lock_state_poisoned".to_string(),
                    lock: None,
                }),
            )
        }
    };
    let mut wait_graph = match state.pessimistic_lock_waits.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(PessimisticLockResponse {
                    status: "error",
                    lock_state: "failed",
                    reason: "wait_graph_state_poisoned".to_string(),
                    lock: None,
                }),
            )
        }
    };
    let (status, response) =
        release_pessimistic_lock(&mut lock_table, &mut wait_graph, &req.transaction_id, &req.resource);
    if response.lock_state == "released" {
        state.pessimistic_lock_metrics.lock_releases.fetch_add(1, Ordering::Relaxed);
    }
    (status, Json(response))
}

pub(crate) async fn sql_pessimistic_lock_metrics(
    State(state): State<AppState>,
) -> Json<PessimisticLockContentionMetricsResponse> {
    let deadlock_detections = state.pessimistic_lock_metrics.deadlock_detections.load(Ordering::Relaxed);
    let scan_cap_timeouts = state.pessimistic_lock_metrics.scan_cap_timeouts.load(Ordering::Relaxed);
    let wait_timeouts = state.pessimistic_lock_metrics.wait_timeouts.load(Ordering::Relaxed);
    let lock_grants = state.pessimistic_lock_metrics.lock_grants.load(Ordering::Relaxed);
    let lock_conflicts = state.pessimistic_lock_metrics.lock_conflicts.load(Ordering::Relaxed);
    let lock_releases = state.pessimistic_lock_metrics.lock_releases.load(Ordering::Relaxed);
    let total_attempts = deadlock_detections + scan_cap_timeouts + wait_timeouts + lock_grants + lock_conflicts;
    let contention_ratio = if total_attempts > 0 {
        (deadlock_detections + scan_cap_timeouts + wait_timeouts + lock_conflicts) as f64 / total_attempts as f64
    } else {
        0.0
    };
    Json(PessimisticLockContentionMetricsResponse {
        status: "ok",
        deadlock_detections,
        scan_cap_timeouts,
        wait_timeouts,
        lock_grants,
        lock_conflicts,
        lock_releases,
        contention_ratio,
    })
}

pub(crate) async fn sql_analyze(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SqlAnalyzeRequest>,
) -> Result<Json<SqlAnalyzeResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_sql_runtime_principal(&headers, &state, PrivilegeAction::Read, "sql/analyze")?;
    let dispatcher = CommandDispatcher::new();
    let envelope = build_http_envelope(
        &headers,
        CanonicalCommandName::SqlAnalyze,
        req.clone(),
        "http-sql-analyze",
    );
    let response = dispatcher.dispatch_sql_analyze(&envelope);
    append_runtime_audit_event(
        &state,
        AuditEventKind::Sql,
        &principal,
        "sql_analyze",
        "ok",
        json!({
            "route_scope": "sql/analyze",
            "total_statements": response.payload.total_statements,
            "rejected_statements": response.payload.rejected_statements,
            "request_id": response.request_id,
        }),
    );
    Ok(Json(response.payload))
}

pub(crate) async fn sql_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SqlRouteRequest>,
) -> Result<Json<SqlRouteResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_sql_runtime_principal(&headers, &state, PrivilegeAction::Read, "sql/route")?;
    let connection_id = acquire_sql_data_plane_connection(&state, &headers, &principal, "sql/route")?;
    let dispatcher = CommandDispatcher::new();
    let envelope = build_http_envelope(
        &headers,
        CanonicalCommandName::SqlRoute,
        req.clone(),
        "http-sql-route",
    );
    let response = dispatcher.dispatch_sql_route(&envelope);
    append_runtime_audit_event(
        &state,
        AuditEventKind::Sql,
        &principal,
        "sql_route",
        "ok",
        json!({
            "route_scope": "sql/route",
            "route_path": response.payload.route_path,
            "statement_count": response.payload.statements.len(),
            "reason": response.payload.reason,
            "request_id": response.request_id,
        }),
    );
    release_sql_data_plane_connection(&state, &connection_id);
    Ok(Json(response.payload))
}

pub(crate) async fn sql_execute(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SqlExecuteRequest>,
) -> Result<(StatusCode, Json<SqlExecuteResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_sql_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Execute,
        "sql/execute",
    )?;
    let connection_id = acquire_sql_data_plane_connection(&state, &headers, &principal, "sql/execute")?;
    let dispatcher = CommandDispatcher::new();
    let envelope = build_http_envelope(
        &headers,
        CanonicalCommandName::SqlExecute,
        req.clone(),
        "http-sql-execute",
    );
    let decision = dispatcher.dispatch_sql_execute_route_decision(&envelope);
    let parsed = SqlAnalyzer::parse_batch(&req.sql_batch);

    // ── Demo CALL intercept (TODO: replace with real stored-procedure execution) ─
    // Handle CALL insert_rows('<table>', <count>) for the studio demo button.
    // This is NOT a real stored-procedure runtime — it is a fixed-name shortcut
    // that synthesises rows with heuristic values. Once CREATE PROCEDURE / UDF
    // execution lands, remove this and route through the real catalog.
    // Tracked as gap §4.3 in gaps-may26-1.md.
    if let Some(early) = try_handle_call_insert_rows_demo(&state, &headers, &principal, &connection_id, &req) {
        return early;
    }


    let udf_function_catalog = udf_function_catalog_contract();
    let udf_guard_policies = udf_guard_policy_contract();
    let udf_execution_plan = build_udf_execution_plan(&req.sql_batch);
    let udf_execution = execute_udf_runtime_scaffold(&req.sql_batch);

    let udf_results = match udf_execution {
        Ok(results) => results,
        Err(reason) => {
            let canonical_error = CanonicalError {
                request_id: envelope.request_id.clone(),
                transport: envelope.transport,
                kind: "validation",
                message: reason.clone(),
            };
            append_runtime_audit_event(
                &state,
                AuditEventKind::Sql,
                &principal,
                "sql_execute",
                "blocked",
                json!({
                    "route_scope": "sql/execute",
                    "route_path": route_path_name(decision.payload.path),
                    "reason": canonical_error.message,
                    "error_kind": canonical_error.kind,
                    "request_id": canonical_error.request_id,
                    "rejected_statement_count": parsed.len(),
                    "udf_guardrail_status": "blocked",
                }),
            );
            let response = Ok((
                StatusCode::BAD_REQUEST,
                Json(SqlExecuteResponse {
                    status: "error".to_string(),
                    route_path: route_path_name(decision.payload.path).to_string(),
                    reason: canonical_error.message,
                    transaction: None,
                    olap: None,
                    rejected_statement_count: parsed.len(),
                    udf_results: None,
                    udf_guardrail_status: Some("blocked".to_string()),
                    udf_function_catalog,
                    udf_guard_policies,
                    udf_execution_plan,
                    legacy_agg_results: None,
                    planner_path: None,
                    oltp_rows: None,
                    olap_agg_results: None,
                    columns: None,
                    rows: None,
                }),
            ));
            release_sql_data_plane_connection(&state, &connection_id);
            return response;
        }
    };

    if matches!(decision.payload.path, QueryPath::Unknown) {
        append_runtime_audit_event(
            &state,
            AuditEventKind::Sql,
            &principal,
            "sql_execute",
            "error",
            json!({
                "route_scope": "sql/execute",
                "route_path": "unknown",
                "reason": decision.payload.reason,
                "rejected_statement_count": parsed.len(),
            }),
        );
        let response = Ok((
            StatusCode::BAD_REQUEST,
            Json(SqlExecuteResponse {
                status: "error".to_string(),
                route_path: "unknown".to_string(),
                reason: decision.payload.reason,
                transaction: None,
                olap: None,
                rejected_statement_count: parsed.len(),
                udf_results: None,
                udf_guardrail_status: None,
                udf_function_catalog,
                udf_guard_policies,
                udf_execution_plan,
                legacy_agg_results: None,
                planner_path: None,
                oltp_rows: None,
                olap_agg_results: None,
                columns: None,
                rows: None,
            }),
        ));
        release_sql_data_plane_connection(&state, &connection_id);
        return response;
    }

    // ── Statement dispatch ───────────────────────────────────────────────────
    let mut transaction_statements = Vec::new();
    let mut olap_statements = Vec::new();
    for statement in parsed {
        let analysis = SqlAnalyzer::analyze_statement(&statement.raw);
        if analysis.kind == SqlStatementKind::Select {
            olap_statements.push(statement.raw);
        } else {
            transaction_statements.push(statement.raw);
        }
    }

    let mut transaction = None;
    let mut olap = None;
    let mut rejected_statement_count = 0usize;
    // Hoisted so the DML WAL/row_store commit block below can access it
    // regardless of whether touches_catalog is true or false.
    let mut ddl_snapshot: Vec<String> = Vec::new();

    if !transaction_statements.is_empty() {
        // REQ-02: snapshot statements for DDL catalog update after ownership transfer
        ddl_snapshot = transaction_statements.clone();
        let (status, response) = execute_transaction_statements(transaction_statements);
        rejected_statement_count += response.rejected_statement_count;
        if status != StatusCode::OK {
            append_runtime_audit_event(
                &state,
                AuditEventKind::Sql,
                &principal,
                "sql_execute",
                "error",
                json!({
                    "route_scope": "sql/execute",
                    "route_path": route_path_name(decision.payload.path),
                    "reason": decision.payload.reason,
                    "rejected_statement_count": rejected_statement_count,
                    "transaction_status": response.status,
                }),
            );
            let response = Ok((
                status,
                Json(SqlExecuteResponse {
                    status: "error".to_string(),
                    route_path: route_path_name(decision.payload.path).to_string(),
                    reason: decision.payload.reason,
                    transaction: Some(response),
                    olap: None,
                    rejected_statement_count,
                    udf_results: None,
                    udf_guardrail_status: None,
                    udf_function_catalog,
                    udf_guard_policies,
                    udf_execution_plan,
                    legacy_agg_results: None,
                    planner_path: None,
                    oltp_rows: None,
                    olap_agg_results: None,
                    columns: None,
                    rows: None,
                }),
            ));
            release_sql_data_plane_connection(&state, &connection_id);
            return response;
        }
        transaction = Some(response);
        // REQ-02: update DDL catalog when DDL statements touched the catalog
        if transaction.as_ref().map(|r| r.touches_catalog).unwrap_or(false) {
            let now_ms = now_unix_ms();
            let mut catalog = state.ddl_catalog.lock().expect("ddl_catalog lock");
            let mut ddl_warning: Option<String> = None;
            for stmt in &ddl_snapshot {
                if let Some(info) = parse_ddl_info(stmt) {
                    match info.operation {
                        "create" => {
                            let result = catalog.record_create(
                                &info.object_kind,
                                &info.database_name,
                                &info.schema_name,
                                &info.object_name,
                                stmt,
                                now_ms,
                                info.replace_ok,
                            );
                            if result == CatalogResult::AlreadyExists {
                                // Record warning but continue — DML statements in the same
                                // batch should still execute (e.g. INSERT after CREATE TABLE).
                                ddl_warning = Some(format!(
                                    "{} '{}' already exists",
                                    info.object_kind, info.object_name
                                ));
                                append_runtime_audit_event(
                                    &state,
                                    AuditEventKind::Sql,
                                    &principal,
                                    "sql_execute",
                                    "warning",
                                    json!({
                                        "route_scope": "sql/execute",
                                        "warning": ddl_warning,
                                    }),
                                );
                            } else {
                                // Persist to WAL so this DDL survives a restart.
                                persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Ddl, stmt);
                            }
                        }
                        "drop" => {
                            catalog.record_drop(&info.database_name, &info.schema_name, &info.object_name);
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Ddl, stmt);
                        }
                        "alter" => {
                            catalog.record_alter(&info.database_name, &info.schema_name, &info.object_name, stmt, now_ms);
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Ddl, stmt);
                        }
                        _ => {}
                    }
                }
            }
            // If the entire batch is DDL-only and we got an AlreadyExists, return 409 now.
            // If there are DML statements too, we fall through so they still execute.
            let has_dml = ddl_snapshot.iter().any(|s| {
                let u = s.trim_start().to_ascii_uppercase();
                u.starts_with("INSERT")
                    || u.starts_with("UPDATE")
                    || u.starts_with("DELETE")
                    || u.starts_with("SELECT")
            });
            if let Some(ref warn_msg) = ddl_warning {
                if !has_dml {
                    drop(catalog);
                    let err_response = Ok((
                        StatusCode::CONFLICT,
                        Json(SqlExecuteResponse {
                            status: "error".to_string(),
                            route_path: route_path_name(decision.payload.path).to_string(),
                            reason: warn_msg.clone(),
                            transaction: None,
                            olap: None,
                            rejected_statement_count: 0,
                            udf_results: None,
                            udf_guardrail_status: None,
                            udf_function_catalog: vec![],
                            udf_guard_policies: vec![],
                            udf_execution_plan: vec![],
                            legacy_agg_results: None,
                            planner_path: None,
                            oltp_rows: None,
                            olap_agg_results: None,
                            columns: None,
                            rows: None,
                        }),
                    ));
                    release_sql_data_plane_connection(&state, &connection_id);
                    return err_response;
                }
                // Has DML: store the warning to attach to the final response below
                drop(catalog);
            }
        }
    }
    // Execute DML (INSERT/UPDATE/DELETE) against the row store for ALL committed
    // non-SELECT transactions — pure DML, mixed DDL+DML, etc. Each row is individually
    // written and appended to the DML WAL so data survives a server restart.
    if transaction.as_ref().map(|r| r.statements_executed > 0).unwrap_or(false) {
        let has_dml = ddl_snapshot.iter().any(|s| {
            let u = s.trim_start().to_ascii_uppercase();
            u.starts_with("INSERT") || u.starts_with("UPDATE") || u.starts_with("DELETE")
        });
        if has_dml {
            // Pre-apply leadership check: in a multi-node cluster, followers must
            // not write DML locally — they have no way to replicate.  Try to proxy
            // the request transparently to the current leader; fall back to 503.
            let peer_count = state.raft_peers.len();
            if peer_count > 0 {
                let is_leader = {
                    let node = state.raft_state.lock().expect("raft_state lock leadership_precheck");
                    node.role == crate::RaftRole::Leader
                };
                if !is_leader {
                    let leader_url = state.current_leader_url.lock().expect("leader_url lock").clone();
                    release_sql_data_plane_connection(&state, &connection_id);
                    if let Some(ref url) = leader_url {
                        let forward_body = serde_json::json!({
                            "sql_batch": req.sql_batch,
                            "max_rows": req.max_rows,
                        });
                        let mut builder = reqwest::Client::new()
                            .post(format!("{url}/api/v1/sql/execute"))
                            .json(&forward_body);
                        for hdr in &["x-vng-admin-key", "x-vng-operator-id", "authorization",
                                     "x-vng-session-id", "x-request-id"] {
                            if let Some(val) = headers.get(*hdr).and_then(|v| v.to_str().ok()) {
                                builder = builder.header(*hdr, val);
                            }
                        }
                        if let Some(token) = state.cluster_token.as_ref().as_deref() {
                            builder = builder.header("x-vng-cluster-token", token);
                        }
                        if let Ok(leader_resp) = builder.send().await {
                            let leader_status = StatusCode::from_u16(leader_resp.status().as_u16())
                                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                            if let Ok(body) = leader_resp.json::<SqlExecuteResponse>().await {
                                return Ok((leader_status, Json(body)));
                            }
                        }
                    }
                    // Leader URL unknown or forward failed — return 503 with hint.
                    let reason = match &leader_url {
                        Some(url) => format!("not_leader: forward to {url} failed; retry directly"),
                        None => "not_leader: no known leader yet; retry later".to_string(),
                    };
                    return Ok((StatusCode::SERVICE_UNAVAILABLE, Json(SqlExecuteResponse {
                        status: "error".to_string(),
                        route_path: route_path_name(decision.payload.path).to_string(),
                        reason,
                        transaction: None,
                        olap: None,
                        rejected_statement_count: 0,
                        udf_results: None,
                        udf_guardrail_status: None,
                        udf_function_catalog: vec![],
                        udf_guard_policies: vec![],
                        udf_execution_plan: vec![],
                        legacy_agg_results: None,
                        planner_path: None,
                        oltp_rows: None,
                        olap_agg_results: None,
                        columns: None,
                        rows: None,
                    })));
                }
            }

            // Apply DML to row store (we're leader, or single-node with no peers).
            let mut rs = state.row_store.lock().expect("row_store lock dml_execute");
            let xid = rs.begin_xid();
            for stmt in &ddl_snapshot {
                let upper = stmt.trim_start().to_ascii_uppercase();
                if upper.starts_with("INSERT") {
                    for (k, d, single_sql) in extract_all_insert_rows(stmt) {
                        let _ = rs.begin_write_intent(xid, &k);
                        rs.insert(xid, &k, d);
                        persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, &single_sql);
                    }
                } else if upper.starts_with("UPDATE") {
                    if let Some((k, d)) = extract_update_row_from_sql(stmt) {
                        let _ = rs.begin_write_intent(xid, &k);
                        rs.insert(xid, &k, d);
                        persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                    }
                } else if upper.starts_with("DELETE") {
                    if let Some(k) = extract_delete_key_from_sql(stmt) {
                        let _ = rs.begin_write_intent(xid, &k);
                        rs.delete(xid, &k);
                        persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                    }
                }
            }
            rs.release_write_intents(xid);
            drop(rs); // release row_store lock before any async work
        }

        // Linearisable leader write: append DML batch to Raft log.
        // - Single-node leader: commit is immediate, no waiting needed.
        // - Multi-node leader: append with pending oneshot receivers and wait
        //   for `raft_last_applied_tx` to confirm quorum before returning 200.
        // - Follower: skip (DML already applied locally above).
        if has_dml {
            let mut node = state.raft_state.lock().expect("raft_state lock sql_execute dml");
            let is_leader = node.role == crate::RaftRole::Leader;
            let total_peers = node.next_index.len();
            if is_leader {
                let dml_stmts: Vec<String> = ddl_snapshot.iter()
                    .filter(|s| {
                        let u = s.trim_start().to_ascii_uppercase();
                        u.starts_with("INSERT") || u.starts_with("UPDATE") || u.starts_with("DELETE")
                    })
                    .cloned()
                    .collect();
                if total_peers == 0 {
                    // Single-node: append without waiting (commit is immediate).
                    for cmd in dml_stmts {
                        node.append_command(cmd, 0);
                    }
                    drop(node);
                } else {
                    // Multi-node: append pending and wait for quorum via block_in_place.
                    let indexed: Vec<(u64, tokio::sync::oneshot::Receiver<u64>)> =
                        dml_stmts.into_iter()
                            .map(|cmd| node.append_command_pending(cmd, total_peers))
                            .collect();
                    drop(node);
                    // Returns false if any receiver timed out (quorum not reached).
                    let quorum_ok = tokio::task::block_in_place(|| {
                        let handle = tokio::runtime::Handle::current();
                        for (_, rx) in indexed {
                            use tokio::time::{timeout, Duration};
                            match handle.block_on(timeout(Duration::from_secs(2), rx)) {
                                Ok(_) => {},
                                Err(_) => return false,
                            }
                        }
                        true
                    });
                    if !quorum_ok {
                        release_sql_data_plane_connection(&state, &connection_id);
                        return Ok((
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(SqlExecuteResponse {
                                status: "error".to_string(),
                                route_path: route_path_name(decision.payload.path).to_string(),
                                reason: "raft_quorum_timeout: DML could not be committed within 2s".to_string(),
                                transaction: None,
                                olap: None,
                                rejected_statement_count: 0,
                                udf_results: None,
                                udf_guardrail_status: None,
                                udf_function_catalog: vec![],
                                udf_guard_policies: vec![],
                                udf_execution_plan: vec![],
                                legacy_agg_results: None,
                                planner_path: None,
                                oltp_rows: None,
                                olap_agg_results: None,
                                columns: None,
                                rows: None,
                            }),
                        ));
                    }
                }
            } else {
                drop(node); // Single-node follower: DML applied locally, no replication needed.
            }
        }
    }

    if !olap_statements.is_empty() {
        // DataFusion path: mirrors the df_select_owned pattern used in the
        // olap_agg_results block below. execute_olap_query is no longer called
        // here so all OLAP SELECT dispatch goes through a single code path.
        use voltnuerongrid_exec_datafusion::{collect_query_table_names, SelectOutput};
        let started = std::time::Instant::now();
        let query = olap_statements.join("; ");
        let limit = req.max_rows.unwrap_or(1_000).min(100_000);
        let rs = state.row_store.lock().expect("row_store lock olap_execute");
        let table_names = collect_query_table_names(&query);
        let all_rows = rs.export_rows_snapshot();
        drop(rs);
        let mut table_rows: std::collections::HashMap<String, Vec<(String, voltnuerongrid_store::mvcc::RowData)>> =
            std::collections::HashMap::new();
        for name in &table_names {
            let prefix = format!("{name}:");
            let filtered: Vec<_> = all_rows
                .iter()
                .filter(|(k, _)| *k == name.as_str() || k.starts_with(&prefix))
                .cloned()
                .collect();
            table_rows.insert(name.clone(), filtered);
        }
        if table_rows.is_empty() {
            table_rows.insert("rows".to_string(), all_rows);
        }
        let row_count = match run_async_in_executor(df_select_owned(query.clone(), table_rows, limit)) {
            Ok(SelectOutput::Rows(rows)) => rows.len(),
            Ok(SelectOutput::Aggregate(_)) => 1,
            Err(_) => 0,
        };
        olap = Some(OlapQueryResponse {
            status: "ok".to_string(),
            query_signature: query.chars().take(64).collect(),
            elapsed_ms: started.elapsed().as_millis(),
            rows: row_count,
        });
    }

    // REQ-12: Detect legacy aggregate functions in OLAP SELECT statements and
    // route them through eval_legacy_numeric_aggregation.
    let legacy_agg_results: Option<Vec<LegacyAggResult>> = {
        let mut agg_results: Vec<LegacyAggResult> = Vec::new();
        // REQ-12: collect real numeric values from all ingest stores; fall back to synthetic sample.
        let mut real_values: Vec<f64> = Vec::new();
        for store in [
            &state.ingest_csv_records,
            &state.ingest_json_records,
            &state.ingest_parquet_records,
            &state.ingest_excel_records,
        ] {
            if let Ok(guard) = store.lock() {
                for records in guard.values() {
                    for rec in records {
                        if let Ok(jv) = serde_json::from_str::<serde_json::Value>(&rec.payload) {
                            if let Some(obj) = jv.as_object() {
                                for v in obj.values() {
                                    if let Some(n) = v.as_f64() { real_values.push(n); }
                                }
                            } else if let Some(n) = jv.as_f64() {
                                real_values.push(n);
                            }
                        } else {
                            for field in rec.payload.split(',') {
                                if let Ok(f) = field.trim().parse::<f64>() { real_values.push(f); }
                            }
                        }
                    }
                }
            }
        }
        let sample_storage: Vec<f64>;
        let sample: &[f64] = if real_values.is_empty() {
            &[1.0, 2.0, 3.0, 4.0, 5.0]
        } else {
            sample_storage = real_values;
            &sample_storage
        };
        for stmt in &olap_statements {
            let upper = stmt.to_ascii_uppercase();
            for &agg in SUPPORTED_LEGACY_AGGREGATIONS {
                if upper.contains(&format!("{agg}(")) || upper.contains(&format!("{agg} (")) {
                    let eval = eval_legacy_numeric_aggregation(agg, sample, None);
                    agg_results.push(LegacyAggResult {
                        aggregation: agg.to_string(),
                        result: eval.as_ref().ok().copied(),
                        error: eval.err(),
                        source: "legacy_agg_olap_path".to_string(),
                    });
                }
            }
        }
        if agg_results.is_empty() { None } else { Some(agg_results) }
    };

    // S3-WS1-05: derive dominant planner path for the execute batch
    let planner_path: Option<String> = {
        use voltnuerongrid_exec::{QueryPlanner, QueryPath};
        use voltnuerongrid_sql::parse_one;
        let mut max_cost: f64 = f64::NEG_INFINITY;
        let mut dominant: Option<String> = None;
        for stmt in &olap_statements {
            if let Ok(parsed) = parse_one(stmt) {
                let plan = QueryPlanner::plan(&parsed);
                let estimate = QueryPlanner::estimate_cost(&plan);
                let path_str = match estimate.recommended_path {
                    QueryPath::Olap => "olap",
                    QueryPath::Hybrid => "hybrid",
                    QueryPath::Oltp => "oltp",
                    QueryPath::Unknown => continue,
                };
                if estimate.relative_cost > max_cost {
                    max_cost = estimate.relative_cost;
                    dominant = Some(path_str.to_string());
                }
            }
        }
        dominant
    };

    // S4-WS3-02: OLTP physical executor dispatch
    let oltp_rows: Option<Vec<OltpRowResult>> =
        if planner_path.as_deref() == Some("oltp") && !olap_statements.is_empty() {
            let rs = state.row_store.lock().expect("row_store lock oltp select");
            let limit = req.max_rows.unwrap_or(10_000).min(100_000);
            let rows = execute_oltp_select(&olap_statements, &rs, limit);
            if rows.is_empty() { None } else { Some(rows) }
        } else {
            None
        };

    // S3-WS1-05 (DataFusion): OLAP / hybrid aggregate dispatch.
    // Runs the first OLAP SELECT through DataFusion and maps the output to
    // `OlapVecAggResult` rows.  Aggregate queries produce one result per
    // output column; plain SELECTs produce a single "row_count" summary.
    let olap_agg_results: Option<Vec<OlapVecAggResult>> =
        if matches!(planner_path.as_deref(), Some("olap") | Some("hybrid")) {
            use voltnuerongrid_exec_datafusion::{collect_query_table_names, SelectOutput, AggregateCell};
            let limit = req.max_rows.unwrap_or(10_000).min(100_000);
            // Use the first OLAP statement (dominant one).
            let first_sql = olap_statements.first().map(|s| s.clone());
            if let Some(sql) = first_sql {
                // Snapshot rows for all referenced tables.
                let rs = state.row_store.lock().expect("row_store lock olap agg df");
                let table_names = collect_query_table_names(&sql);
                let all_rows = rs.export_rows_snapshot();
                drop(rs);
                let mut table_rows: std::collections::HashMap<String, Vec<(String, voltnuerongrid_store::mvcc::RowData)>> =
                    std::collections::HashMap::new();
                for name in &table_names {
                    let prefix = format!("{name}:");
                    let filtered: Vec<_> = all_rows
                        .iter()
                        .filter(|(k, _)| *k == name.as_str() || k.starts_with(&prefix))
                        .cloned()
                        .collect();
                    table_rows.insert(name.clone(), filtered);
                }
                if table_rows.is_empty() {
                    table_rows.insert("rows".to_string(), all_rows);
                }
                match run_async_in_executor(df_select_owned(sql, table_rows, limit)) {
                    Ok(SelectOutput::Aggregate(agg)) => {
                        let mut out: Vec<OlapVecAggResult> = agg.columns.iter()
                            .zip(agg.values.iter())
                            .map(|(col, val)| {
                                let value_str = match val {
                                    AggregateCell::Int(i) => i.to_string(),
                                    AggregateCell::Float(f) => f.to_string(),
                                    AggregateCell::Text(t) => t.clone(),
                                    AggregateCell::Null => String::new(),
                                };
                                OlapVecAggResult {
                                    column: col.clone(),
                                    op: "aggregate".to_string(),
                                    value: value_str,
                                    row_count: 1,
                                }
                            })
                            .collect();
                        out.sort_by(|a, b| a.column.cmp(&b.column));
                        if out.is_empty() { None } else { Some(out) }
                    }
                    Ok(SelectOutput::Rows(rows)) => {
                        if rows.is_empty() {
                            None
                        } else {
                            // Non-aggregate OLAP: emit a single row-count summary entry.
                            Some(vec![OlapVecAggResult {
                                column: "*".to_string(),
                                op: "count".to_string(),
                                value: rows.len().to_string(),
                                row_count: rows.len(),
                            }])
                        }
                    }
                    Err(_) => None,
                }
            } else {
                None
            }
        } else {
            None
        };

    // Build client-visible columns + rows from the row store for any SELECT query.
    // This is the primary path the UI uses to display query results.
    let (result_columns, result_rows): (Option<Vec<serde_json::Value>>, Option<Vec<serde_json::Value>>) =
        if !olap_statements.is_empty() {
            use voltnuerongrid_sql::{parse_one, Statement};
            let rs = state.row_store.lock().expect("row_store lock select_result_builder");
            let snapshot_xid = rs.current_xid();
            let all_rows: Vec<(String, std::collections::HashMap<String, String>)> = rs
                .scan_at_snapshot(snapshot_xid)
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect();
            drop(rs);
            let limit = req.max_rows.unwrap_or(10_000).min(100_000);
            // final ordered column list and rows for this response
            let mut ordered_cols: Vec<String> = Vec::new();
            let mut out_rows: Vec<serde_json::Value> = Vec::new();

            for stmt_str in &olap_statements {
                if let Ok(Statement::Select(sel)) = parse_one(stmt_str) {
                    // Determine which table to filter on (FROM clause table name)
                    let filter_table: Option<String> = sel.table.as_deref().map(|f| {
                        f.split_ascii_whitespace()
                            .next()
                            .unwrap_or(f)
                            .rsplit('.')
                            .next()
                            .unwrap_or(f)
                            .to_ascii_lowercase()
                    });

                    // Fetch real DDL column names for this table (CREATE TABLE definition order).
                    // These are used to (a) build the column header list in the right order,
                    // and (b) remap positional `col_N` storage keys back to readable names.
                    let ddl_cols: Vec<String> = filter_table.as_deref().map(|tbl| {
                        let catalog = state.ddl_catalog.lock().expect("ddl_catalog lock result_builder");
                        catalog.get(tbl)
                            .map(|e| extract_column_names_from_ddl(&e.original_statement))
                            .unwrap_or_default()
                    }).unwrap_or_default();

                    // Build WHERE key filter (RHS of `col = 'val'`)
                    let key_filter: Option<String> = sel.where_clause.as_deref().and_then(|w| {
                        let eq = w.find('=')?;
                        let rhs = w[eq + 1..].trim();
                        let val = rhs.trim_matches('\'').trim_matches('"').trim().to_string();
                        if val.is_empty() { None } else { Some(val) }
                    });

                    // Determine the projected column list from SELECT clause.
                    // `sel.columns` should contain ["*"] or explicit names.
                    let select_star = sel.columns.is_empty()
                        || sel.columns.iter().any(|c| c == "*");

                    for (key, data) in &all_rows {
                        if out_rows.len() >= limit { break; }
                        // Filter by table name
                        let row_table = data.get("__table").map(|s| s.to_ascii_lowercase());
                        if let Some(ft) = &filter_table {
                            if row_table.as_deref() != Some(ft.as_str()) { continue; }
                        }
                        // Filter by WHERE key
                        if let Some(kf) = &key_filter {
                            if !key.contains(kf.as_str()) { continue; }
                        }

                        // Build row object using DDL column order when available.
                        // Remap "col_N" storage keys to real DDL column names.
                        let mut row_obj = serde_json::Map::new();

                        if !ddl_cols.is_empty() && select_star {
                            // Emit columns in CREATE TABLE order; remap col_N if needed.
                            for (idx, col_name) in ddl_cols.iter().enumerate() {
                                // Try real column name first, then positional fallback key.
                                let val = data.get(col_name)
                                    .or_else(|| data.get(&format!("col_{idx}")))
                                    .cloned()
                                    .unwrap_or_default();
                                row_obj.insert(col_name.clone(), serde_json::Value::String(val));
                                if !ordered_cols.contains(col_name) {
                                    ordered_cols.push(col_name.clone());
                                }
                            }
                        } else {
                            // No DDL info: fall back to sorted storage keys, still readable.
                            let mut sorted_keys: Vec<&str> = data.keys()
                                .filter(|k| !k.starts_with("__"))
                                .map(|k| k.as_str())
                                .collect();
                            sorted_keys.sort();
                            for col in &sorted_keys {
                                row_obj.insert((*col).to_string(), serde_json::Value::String(data[*col].clone()));
                                if !ordered_cols.contains(&(*col).to_string()) {
                                    ordered_cols.push((*col).to_string());
                                }
                            }
                        }

                        if !row_obj.is_empty() {
                            out_rows.push(serde_json::Value::Object(row_obj));
                        }
                    }
                }
            }
            if out_rows.is_empty() {
                (None, None)
            } else {
                let cols: Vec<serde_json::Value> = ordered_cols
                    .iter()
                    .map(|c| serde_json::json!({"name": c, "data_type": "text"}))
                    .collect();
                (Some(cols), Some(out_rows))
            }
        } else {
            (None, None)
        };

    let response = SqlExecuteResponse {
        status: "ok".to_string(),
        route_path: route_path_name(decision.payload.path).to_string(),
        reason: decision.payload.reason,
        transaction,
        olap,
        rejected_statement_count,
        udf_results: if udf_results.is_empty() {
            None
        } else {
            Some(udf_results)
        },
        udf_guardrail_status: Some("passed".to_string()),
        udf_function_catalog,
        udf_guard_policies,
        udf_execution_plan,
        legacy_agg_results,
        planner_path,
        oltp_rows,
        olap_agg_results,
        columns: result_columns,
        rows: result_rows,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Sql,
        &principal,
        "sql_execute",
        "ok",
        json!({
            "route_scope": "sql/execute",
            "route_path": response.route_path,
            "reason": response.reason,
            "rejected_statement_count": response.rejected_statement_count,
            "udf_guardrail_status": response.udf_guardrail_status,
        }),
    );
    release_sql_data_plane_connection(&state, &connection_id);
    Ok((
        StatusCode::OK,
        Json(response),
    ))
}

// ─── S2-WS2-05: Transaction isolation stats handler ─────────────────────────

pub(crate) async fn sql_transactions_isolation(
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

pub(crate) async fn sql_transactions_active(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<AcidTransactionsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_sql_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "sql/transactions/active",
    )?;
    let acid = state.acid_transactions.lock().expect("acid_tx lock");
    let all = acid.all_transactions();
    let active = acid.active_transactions();
    let resp = AcidTransactionsResponse {
        status: "ok",
        active_count: active.len(),
        total_count: all.len(),
        transactions: active.iter().map(|t| (*t).clone()).collect(),
    };
    Ok((StatusCode::OK, Json(resp)))
}
