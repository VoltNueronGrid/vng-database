use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
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
use crate::{execute_olap_query, execute_oltp_select, df_select_owned, run_async_in_executor};
use crate::{execute_udf_runtime_scaffold, udf_function_catalog_contract, udf_guard_policy_contract, build_udf_execution_plan};
use crate::{route_path_name, try_handle_call_insert_rows_demo};
use crate::{extract_delete_key_from_sql, extract_update_row_from_sql, extract_column_names_from_ddl, extract_insert_row_from_sql, extract_all_insert_rows};
use crate::helpers::sql_parse::{extract_bulk_update_target, extract_bulk_delete_target};
use crate::helpers::sql_parse::{db_prefix_key, make_table_scan_prefix};
use crate::{persist_sql_statement};
use crate::auth::{require_sql_runtime_principal, locale_from_headers};
use crate::audit_helpers::append_runtime_audit_event;

// ─── M-3: graceful 503 helper ───────────────────────────────────────────────
/// Build a 503 AuthErrorResponse for mutex-poisoned internal state.
/// Handler hot-paths use this instead of `.expect()` so a poisoned mutex
/// returns 503 to the caller rather than crashing the process.
#[inline]
fn lock_poisoned_err(what: &str) -> (StatusCode, Json<AuthErrorResponse>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(AuthErrorResponse {
            status: "error",
            reason: format!("{what} mutex poisoned"),
            locale: "en".to_string(),
            localized_message: "Service temporarily unavailable".to_string(),
        }),
    )
}

// ─── M-8 Rule 1: typed value inference ───────────────────────────────────────

/// Coerce a string storage value to the most specific JSON scalar type.
///
/// Priority: null → boolean → integer → float → string (Codd Rule 1 partial).
/// This prevents every column from appearing as "text" in client result sets
/// when the underlying value is a number or boolean.
#[inline]
fn infer_json_value(s: &str) -> serde_json::Value {
    if s.is_empty() || s.eq_ignore_ascii_case("null") {
        return serde_json::Value::Null;
    }
    if s.eq_ignore_ascii_case("true") { return serde_json::Value::Bool(true); }
    if s.eq_ignore_ascii_case("false") { return serde_json::Value::Bool(false); }
    if let Ok(i) = s.parse::<i64>() { return serde_json::Value::Number(i.into()); }
    if let Ok(f) = s.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(f) {
            return serde_json::Value::Number(n);
        }
    }
    serde_json::Value::String(s.to_string())
}

/// Return the PostgreSQL-style type name for a JSON value produced by `infer_json_value`.
#[inline]
fn json_value_pg_type(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null    => "text",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(n) => {
            if n.is_f64() { "double precision" } else { "integer" }
        }
        serde_json::Value::String(_) => "text",
        _ => "text",
    }
}

// ─── Gap #3: undo log helper ─────────────────────────────────────────────────

/// Record a before-image into the undo log for the given connection.
/// `before` = current visible data at the key (None if the row did not exist).
fn record_undo(
    undo_log: &std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<(String, Option<voltnuerongrid_store::mvcc::RowData>)>>>>,
    conn_id: &str,
    key: &str,
    before: Option<voltnuerongrid_store::mvcc::RowData>,
) {
    let mut log = undo_log.lock().expect("tx_undo_log lock");
    log.entry(conn_id.to_string())
        .or_default()
        .push((key.to_string(), before));
}

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

#[derive(Clone, Default, Deserialize)]
pub(crate) struct SqlExecuteRequest {
    pub(crate) sql_batch: String,
    pub(crate) max_rows: Option<usize>,
    /// M-6: Optional default isolation level for inline transactions started within
    /// this execute batch.  Honoured when BEGIN is part of the sql_batch.
    /// Values: "read_committed" (default), "repeatable_read", "serializable".
    pub(crate) isolation_level: Option<String>,
    /// M-6: Optional client-side statement timeout hint (milliseconds).  The server
    /// records this in the audit log; enforcement is left to a future watchdog task.
    pub(crate) statement_timeout_ms: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct LegacyAggResult {
    /// Aggregate function name (e.g. `"SUM"`, `"COUNT"`).
    pub(crate) aggregation: String,
    /// Computed result; `None` when evaluation errored.
    pub(crate) result: Option<f64>,
    /// Error message when evaluation failed.
    pub(crate) error: Option<String>,
    /// Indicates this result came through the legacy aggregation routing path.
    pub(crate) source: &'static str,
}

#[derive(Serialize)]
pub(crate) struct SqlExecuteResponse {
    pub(crate) status: &'static str,
    pub(crate) route_path: String,
    pub(crate) reason: String,
    pub(crate) transaction: Option<SqlTransactionResponse>,
    pub(crate) olap: Option<OlapQueryResponse>,
    pub(crate) rejected_statement_count: usize,
    pub(crate) udf_results: Option<Vec<UdfExecutionResult>>,
    pub(crate) udf_guardrail_status: Option<String>,
    pub(crate) udf_function_catalog: Vec<UdfFunctionCatalogEntry>,
    pub(crate) udf_guard_policies: Vec<UdfLanguageGuardPolicy>,
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
#[derive(Serialize)]
pub(crate) struct OltpRowResult {
    pub(crate) key: String,
    pub(crate) data: std::collections::HashMap<String, String>,
}

/// S4-WS3-02: a single vectorized aggregation result from the OLAP columnar executor.
#[derive(Serialize)]
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

#[derive(Serialize)]
pub(crate) struct OlapQueryResponse {
    pub(crate) status: &'static str,
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

#[tracing::instrument(skip_all, name = "sql.transaction")]
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
    // Gap #2: database scope for this transaction (prefix all row keys).
    let db: String = headers
        .get("x-vng-database")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
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
        // C-3: For repeatable_read, capture the row-store snapshot Xid at BEGIN time
        // so all reads in this transaction see a consistent view.  Must be captured
        // *before* acquiring acid_transactions lock to avoid deadlock.
        let begin_snapshot_xid: Option<u64> = if has_begin && iso_level == "repeatable_read" {
            let rs = match state.row_store.lock() {
                Ok(g) => g,
                Err(_) => return Err(lock_poisoned_err("row_store")),
            };
            Some(rs.current_xid())
        } else {
            None
        };

        let mut acid = match state.acid_transactions.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned_err("acid_transactions")),
        };
        if has_begin {
            acid.begin(&tx_id, &state.node_id, &iso_level, now_ms, begin_snapshot_xid);
            // Register connection → tx mapping so sql_execute can look up the
            // repeatable-read snapshot for standalone SELECT calls.
            if iso_level == "repeatable_read" {
                if let Ok(mut conn_map) = state.connection_tx_active.lock() {
                    conn_map.insert(connection_id.clone(), tx_id.clone());
                }
            }
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
            // M-7: Collect the set of row keys this transaction intends to write.
            // Used for both (a) write-write conflict detection (S2-WS2-05) and
            // (b) row-level serializable OCC conflict detection.
            let commit_write_keys: std::collections::HashSet<String> = {
                let mut keys = std::collections::HashSet::new();
                for stmt in &statements {
                    let upper = stmt.trim_start().to_ascii_uppercase();
                    if upper.starts_with("INSERT") {
                        for (raw_k, _, _) in extract_all_insert_rows(stmt) {
                            keys.insert(db_prefix_key(&db, &raw_k));
                        }
                    } else if upper.starts_with("UPDATE") {
                        if let Some((raw_k, _)) = extract_update_row_from_sql(stmt) {
                            keys.insert(db_prefix_key(&db, &raw_k));
                        }
                    } else if upper.starts_with("DELETE") {
                        if let Some(raw_k) = extract_delete_key_from_sql(stmt) {
                            keys.insert(db_prefix_key(&db, &raw_k));
                        }
                    }
                }
                keys
            };
            // M-7: Row-level serializable conflict detection.
            // Conflict only when a committed serializable peer wrote the exact
            // same row key(s) — no false positives from non-overlapping writes.
            if let Some(conflict_key) = acid.check_serializable_conflict_row_level(&tx_id, &commit_write_keys) {
                acid.rollback(&tx_id, now_ms);
                drop(acid);
                let locale = locale_from_headers(&headers);
                let localized = I18nCatalog::message(locale, "unauthorized");
                return Err((
                    StatusCode::CONFLICT,
                    Json(AuthErrorResponse {
                        status: "error",
                        reason: format!("serializable_write_conflict:{conflict_key}"),
                        locale: locale.as_str().to_string(),
                        localized_message: localized.message.to_string(),
                    }),
                ));
            }
            // S2-WS2-05: write-write conflict detection using row-store snapshot xid.
            {
                if !commit_write_keys.is_empty() {
                    let rs = match state.row_store.lock() {
                        Ok(g) => g,
                        Err(_) => return Err(lock_poisoned_err("row_store")),
                    };
                    let snapshot_xid = acid.row_store_snapshot_xid(&tx_id)
                        .unwrap_or(0);
                    for key in &commit_write_keys {
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
            // M-7: Record the committed write keys into the acid entry so that
            // future serializable transactions can detect row-level conflicts.
            acid.record_written_row_keys(&tx_id, commit_write_keys.into_iter());
            acid.commit(&tx_id, now_ms);
            // C-3: Clear the connection→tx mapping on COMMIT so sql_execute no longer
            // applies the repeatable-read snapshot to standalone SELECTs.
            if let Ok(mut conn_map) = state.connection_tx_active.lock() {
                conn_map.remove(&connection_id);
            }
            // S2-WS2-05: flush committed DML (INSERT/UPDATE/DELETE) into PagedRowStore.
            // Write intents are registered before each write and released after the flush
            // so that concurrent transactions see the in-progress lock via begin_write_intent.
            {
                let mut rs = match state.row_store.lock() {
                    Ok(g) => g,
                    Err(_) => return Err(lock_poisoned_err("row_store")),
                };
                // Record snapshot xid before allocating the write xid
                let snapshot_xid = rs.current_xid();
                acid.set_row_store_snapshot(&tx_id, snapshot_xid);
                let xid = rs.begin_xid();
                for stmt in &statements {
                    let upper = stmt.trim_start().to_ascii_uppercase();
                    if upper.starts_with("INSERT") {
                        // Use extract_all_insert_rows to handle multi-row INSERT correctly.
                        // Each row is individually inserted and individually WAL-persisted.
                        for (raw_k, d, single_sql) in extract_all_insert_rows(stmt) {
                            let k = db_prefix_key(&db, &raw_k);
                            // Gap #3: record before-image for ROLLBACK support.
                            let before = rs.read_latest(&k).cloned();
                            record_undo(&state.tx_undo_log, &connection_id, &k, before);
                            let _ = rs.begin_write_intent(xid, &k);
                            if let Ok(mut wal) = state.wal_engine.lock() { wal.store_row(&db, &raw_k, xid, Some(&d)); }
                            rs.insert(xid, &k, d);
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, &single_sql);
                        }
                    } else if upper.starts_with("DELETE") {
                        if let Some(raw_k) = extract_delete_key_from_sql(stmt) {
                            let k = db_prefix_key(&db, &raw_k);
                            // Gap #3: record before-image for ROLLBACK support.
                            let before = rs.read_latest(&k).cloned();
                            record_undo(&state.tx_undo_log, &connection_id, &k, before);
                            let _ = rs.begin_write_intent(xid, &k);
                            rs.delete(xid, &k);
                            if let Ok(mut wal) = state.wal_engine.lock() { wal.store_row(&db, &raw_k, xid, None); }
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                        }
                    } else if upper.starts_with("UPDATE") {
                        if let Some((raw_k, d)) = extract_update_row_from_sql(stmt) {
                            let k = db_prefix_key(&db, &raw_k);
                            // Gap #3: record before-image for ROLLBACK support.
                            let before = rs.read_latest(&k).cloned();
                            record_undo(&state.tx_undo_log, &connection_id, &k, before);
                            let _ = rs.begin_write_intent(xid, &k);
                            if let Ok(mut wal) = state.wal_engine.lock() { wal.store_row(&db, &raw_k, xid, Some(&d)); }
                            rs.insert(xid, &k, d);
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                        }
                    }
                }
                // S2-WS2-02: record committed DML mutations in the WAL engine for
                // durability and recovery replay.
                if let Ok(mut wal) = state.wal_engine.lock() {
                    for stmt in &req.statements {
                        let upper = stmt.trim_start().to_ascii_uppercase();
                        if upper.starts_with("INSERT") {
                            if let Some((raw_k, d)) = extract_insert_row_from_sql(stmt) {
                                let k = db_prefix_key(&db, &raw_k);
                                let val = serde_json::to_string(&d).unwrap_or_default();
                                wal.append_mutation(&k, &val);
                            }
                        } else if upper.starts_with("DELETE") {
                            if let Some(raw_k) = extract_delete_key_from_sql(stmt) {
                                let k = db_prefix_key(&db, &raw_k);
                                wal.append_mutation(&k, "__deleted__");
                            }
                        } else if upper.starts_with("UPDATE") {
                            if let Some((raw_k, d)) = extract_update_row_from_sql(stmt) {
                                let k = db_prefix_key(&db, &raw_k);
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
                let mut origin = match state.sync_origin.lock() {
                    Ok(g) => g,
                    Err(_) => return Err(lock_poisoned_err("sync_origin")),
                };
                for stmt in &req.statements {
                    let upper = stmt.trim_start().to_ascii_uppercase();
                    if upper.starts_with("INSERT") {
                        if let Some((raw_k, _d)) = extract_insert_row_from_sql(stmt) {
                            let k = db_prefix_key(&db, &raw_k);
                            origin.append("row_store", &k, stmt, MutationOp::Insert);
                        }
                    } else if upper.starts_with("DELETE") {
                        if let Some(raw_k) = extract_delete_key_from_sql(stmt) {
                            let k = db_prefix_key(&db, &raw_k);
                            origin.append("row_store", &k, stmt, MutationOp::Delete);
                        }
                    } else if upper.starts_with("UPDATE") {
                        if let Some((raw_k, _d)) = extract_update_row_from_sql(stmt) {
                            let k = db_prefix_key(&db, &raw_k);
                            origin.append("row_store", &k, stmt, MutationOp::Update);
                        }
                    }
                }
            }
        } else if has_rollback {
            acid.rollback(&tx_id, now_ms);
            // C-3: Clear the connection→tx mapping on ROLLBACK as well.
            if let Ok(mut conn_map) = state.connection_tx_active.lock() {
                conn_map.remove(&connection_id);
            }
        }
        // Gap #3: on COMMIT clear undo log; on ROLLBACK apply it then clear.
        if has_commit {
            if let Ok(mut log) = state.tx_undo_log.lock() { log.remove(&connection_id); }
        } else if has_rollback {
            let undo_entries = {
                match state.tx_undo_log.lock() {
                    Ok(mut log) => log.remove(&connection_id).unwrap_or_default(),
                    Err(_) => Vec::new(),
                }
            };
            if !undo_entries.is_empty() {
                let mut rs = match state.row_store.lock() {
                    Ok(g) => g,
                    Err(_) => return Err(lock_poisoned_err("row_store")),
                };
                let undo_xid = rs.begin_xid();
                for (key, before_data) in undo_entries.into_iter().rev() {
                    match before_data {
                        Some(data) => { rs.insert(undo_xid, &key, data); }
                        None => { rs.delete(undo_xid, &key); }
                    }
                }
                rs.release_write_intents(undo_xid);
            }
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

#[tracing::instrument(skip_all, name = "sql.execute")]
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

    // Extract x-vng-database header for database-scoped query execution.
    // When present, all DDL/DML/SELECT operations are scoped to this database.
    let active_database: Option<String> = headers
        .get("x-vng-database")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());

    // Gap #2: db scope string — used to prefix all row keys for true DB isolation.
    // Empty when no database header is present (backward compat, single-DB deployments).
    let db: String = active_database.clone().unwrap_or_default();

    // RBAC database-level access check: if a database is specified and the
    // principal is not a DBA, verify they have an explicit database-level grant.
    if let Some(ref db) = active_database {
        if !crate::auth::principal_has_database_access(&principal, db, &state) {
            return Err((
                axum::http::StatusCode::FORBIDDEN,
                axum::Json(crate::AuthErrorResponse {
                    status: "error",
                    reason: format!("access denied to database '{db}'"),
                    locale: "en".to_string(),
                    localized_message: format!("You do not have access to database '{db}'"),
                }),
            ));
        }
    }

    let connection_id = acquire_sql_data_plane_connection(&state, &headers, &principal, "sql/execute")?;

    // Gap #9: acquire a per-database connection permit (enforces max_connections per database).
    let _db_permit = if !db.is_empty() {
        let sem = {
            let mut semaphores = match state.db_semaphores.lock() {
                Ok(g) => g,
                Err(_) => return Err(lock_poisoned_err("db_semaphores")),
            };
            semaphores
                .entry(db.clone())
                .or_insert_with(|| {
                    // No max_connections field in DdlCatalog yet — use the default.
                    Arc::new(tokio::sync::Semaphore::new(crate::DEFAULT_DB_MAX_CONNECTIONS))
                })
                .clone()
        };
        // Try to acquire without blocking; return 503 if at capacity.
        match sem.try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => {
                release_sql_data_plane_connection(&state, &connection_id);
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    axum::Json(crate::AuthErrorResponse {
                        status: "error",
                        reason: format!("database '{}' is at max_connections limit", db),
                        locale: "en".to_string(),
                        localized_message: format!(
                            "Database '{}' has reached its maximum connection limit",
                            db
                        ),
                    }),
                ));
            }
        }
    } else {
        None
    };

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
    // Handle CALL insert_rows('<table>', <count>) ONLY in demo mode.
    // Gate behind VNG_DEMO_MODE=true to avoid shadowing user-defined stored procedures.
    if std::env::var("VNG_DEMO_MODE").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false) {
        if let Some(early) = try_handle_call_insert_rows_demo(&state, &headers, &principal, &connection_id, &req, &db) {
            return early;
        }
    }

    // Virtual catalog interception: information_schema.* and pg_catalog.*
    // Must come before OLAP/OLTP routing so DBeaver, TablePlus, psql and
    // SQLAlchemy get synthesized metadata rows without touching the row store.
    if crate::helpers::information_schema::is_virtual_catalog_query(&req.sql_batch) {
        let (cols, rows) = crate::helpers::information_schema::synthesize_virtual_catalog_response(
            &req.sql_batch, &state,
        );
        append_runtime_audit_event(
            &state,
            AuditEventKind::Sql,
            &principal,
            "virtual_catalog_query",
            "ok",
            serde_json::json!({ "route_scope": "sql/execute", "batch_snippet": &req.sql_batch.chars().take(64).collect::<String>() }),
        );
        release_sql_data_plane_connection(&state, &connection_id);
        return Ok((
            StatusCode::OK,
            Json(SqlExecuteResponse {
                status: "ok",
                route_path: "virtual_catalog".to_string(),
                reason: "information_schema_intercept".to_string(),
                transaction: None,
                olap: None,
                rejected_statement_count: 0,
                udf_results: None,
                udf_guardrail_status: Some("passed".to_string()),
                udf_function_catalog: vec![],
                udf_guard_policies: vec![],
                udf_execution_plan: vec![],
                legacy_agg_results: None,
                planner_path: Some("virtual_catalog".to_string()),
                oltp_rows: None,
                olap_agg_results: None,
                columns: if cols.is_empty() { None } else { Some(cols) },
                rows: if rows.is_empty() { None } else { Some(rows) },
            }),
        ));
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
                    status: "error",
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
                status: "error",
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
                    status: "error",
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
            let mut catalog = match state.ddl_catalog.lock() {
                Ok(g) => g,
                Err(_) => return Err(lock_poisoned_err("ddl_catalog")),
            };
            let mut ddl_warning: Option<String> = None;
            for stmt in &ddl_snapshot {
                if let Some(info) = parse_ddl_info(stmt) {
                    match info.operation {
                        "create" if info.object_kind == "index" => {
                            // M-2: CREATE INDEX — deferred to after the catalog loop to avoid
                            // holding the catalog lock while acquiring row_store + index_manager.
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Ddl, stmt);
                        }
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
            // M-2: Execute CREATE INDEX statements after releasing the catalog lock.
            // This avoids holding ddl_catalog while acquiring row_store + index_manager.
            drop(catalog);
            for stmt in &ddl_snapshot {
                let lower = stmt.trim().to_ascii_lowercase();
                if lower.starts_with("create index ") || lower.starts_with("create unique index ") {
                    handle_create_index_ddl(&state, stmt, &db);
                }
            }
            catalog = match state.ddl_catalog.lock() {
                Ok(g) => g,
                Err(_) => return Err(lock_poisoned_err("ddl_catalog")),
            };
            // M-2: Handle GRANT / REVOKE / CREATE ROLE / DROP ROLE statements.
            // These run after the catalog loop so we don't hold ddl_catalog while
            // touching db_grants. CREATE ROLE / DROP ROLE are persisted to WAL only;
            // the in-memory RBAC config is managed via the admin API on restart.
            for stmt in &ddl_snapshot {
                let kind = SqlAnalyzer::classify_statement(stmt);
                match kind {
                    SqlStatementKind::Grant => {
                        handle_grant_sql(&state, stmt);
                        persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Ddl, stmt);
                    }
                    SqlStatementKind::Revoke => {
                        handle_revoke_sql(&state, stmt);
                        persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Ddl, stmt);
                    }
                    SqlStatementKind::CreateRole | SqlStatementKind::DropRole => {
                        // Persist to WAL; in-memory role management requires admin API restart.
                        tracing::info!(
                            "role management statement persisted to WAL (restart admin API to apply): {stmt}"
                        );
                        persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Ddl, stmt);
                    }
                    _ => {}
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
                            status: "error",
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
    // non-SELECT transactions — pure DML, mixed DDL+DML, etc.
    //
    // Two paths depending on Raft role:
    //   • Multi-node leader  → linearisable: append to Raft log first, wait for
    //     apply-loop quorum confirmation, then ACK client (§5.3).
    //   • Single-node leader / follower / non-Raft → direct: write to row_store
    //     immediately, then fire-and-forget append to Raft log so followers can
    //     replicate on the next heartbeat tick.
    if transaction.as_ref().map(|r| r.statements_executed > 0).unwrap_or(false) {
        let has_dml = ddl_snapshot.iter().any(|s| {
            let u = s.trim_start().to_ascii_uppercase();
            u.starts_with("INSERT") || u.starts_with("UPDATE") || u.starts_with("DELETE")
        });
        if has_dml {
            let total_peers = state.raft_peers.len();
            let is_multi_node_leader = {
                let node = match state.raft_state.lock() {
                    Ok(g) => g,
                    Err(_) => return Err(lock_poisoned_err("raft_state")),
                };
                node.role == crate::RaftRole::Leader && total_peers > 0
            };

            if is_multi_node_leader {
                // ── Linearisable path ────────────────────────────────────────────
                // 1. Append every DML command to the Raft log (no row_store write yet).
                let mut max_pending_index: u64 = 0;
                {
                    let mut node = match state.raft_state.lock() {
                        Ok(g) => g,
                        Err(_) => return Err(lock_poisoned_err("raft_state")),
                    };
                    for stmt in &ddl_snapshot {
                        let upper = stmt.trim_start().to_ascii_uppercase();
                        if upper.starts_with("INSERT") || upper.starts_with("UPDATE") || upper.starts_with("DELETE") {
                            let idx = node.append_command_pending(stmt.clone(), total_peers);
                            if idx > max_pending_index { max_pending_index = idx; }
                        }
                    }
                }
                // 2. Persist to WAL for local durability (before waiting for quorum).
                for stmt in &ddl_snapshot {
                    let upper = stmt.trim_start().to_ascii_uppercase();
                    if upper.starts_with("INSERT") {
                        for (_, _, single_sql) in extract_all_insert_rows(stmt) {
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, &single_sql);
                        }
                    } else if upper.starts_with("UPDATE") || upper.starts_with("DELETE") {
                        persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                    }
                }
                // 3. Wait for the apply loop to commit and apply (up to 2 s).
                if max_pending_index > 0 {
                    let mut rx = state.raft_last_applied_tx.subscribe();
                    let wait_ok = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        rx.wait_for(|&applied| applied >= max_pending_index),
                    ).await.is_ok();
                    if !wait_ok {
                        release_sql_data_plane_connection(&state, &connection_id);
                        return Ok((
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(SqlExecuteResponse {
                                status: "error",
                                route_path: route_path_name(decision.payload.path).to_string(),
                                reason: "raft_quorum_timeout".to_string(),
                                transaction, olap: None,
                                rejected_statement_count,
                                udf_results: None, udf_guardrail_status: None,
                                udf_function_catalog, udf_guard_policies, udf_execution_plan,
                                legacy_agg_results: None, planner_path: None,
                                oltp_rows: None, olap_agg_results: None,
                                columns: None, rows: None,
                            }),
                        ));
                    }
                }
            } else {
                // ── Direct path (single-node leader / follower / non-Raft) ────────
                let mut rs = match state.row_store.lock() {
                    Ok(g) => g,
                    Err(_) => return Err(lock_poisoned_err("row_store")),
                };
                let xid = rs.begin_xid();
                // Gap #7: Track per-table insert/delete deltas for incremental stats update.
                let mut stats_inserts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
                for stmt in &ddl_snapshot {
                    let upper = stmt.trim_start().to_ascii_uppercase();
                    if upper.starts_with("INSERT") {
                        for (raw_k, d, single_sql) in extract_all_insert_rows(stmt) {
                            let k = db_prefix_key(&db, &raw_k);
                            // Gap #3: record before-image for ROLLBACK support.
                            let before = rs.read_latest(&k).cloned();
                            // Only increment if this is a fresh insert (not an overwrite).
                            if before.is_none() {
                                if let Some(colon) = k.rfind(':') {
                                    *stats_inserts.entry(k[..colon].to_string()).or_insert(0) += 1;
                                }
                            }
                            record_undo(&state.tx_undo_log, &connection_id, &k, before);
                            let _ = rs.begin_write_intent(xid, &k);
                            { let mut wal = state.wal_engine.lock().expect("wal store_row"); wal.store_row(&db, &raw_k, xid, Some(&d)); }
                            rs.insert(xid, &k, d);
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, &single_sql);
                        }
                    } else if upper.starts_with("UPDATE") {
                        if let Some((raw_k, d)) = extract_update_row_from_sql(stmt) {
                            // Rule 7 (Codd) — set-at-a-time UPDATE: when no primary-key
                            // WHERE constraint was parsed (raw_k == table name), fall back
                            // to a full table scan filtered by the WHERE predicate.
                            let table_name = d.get("__table").map(|t| t.clone()).unwrap_or_default();
                            let is_scan_update = raw_k == table_name
                                && extract_bulk_update_target(stmt)
                                    .map(|(_, _, _, ref wc, _)| !wc.eq_ignore_ascii_case("id") && !wc.is_empty())
                                    .unwrap_or(false);

                            if is_scan_update {
                                if let Some((tbl, set_col, set_val, where_col, where_val)) =
                                    extract_bulk_update_target(stmt)
                                {
                                    let snapshot_xid = rs.current_xid();
                                    let table_prefix = format!("{tbl}:");
                                    let db_prefix_str = if db.is_empty() {
                                        String::new()
                                    } else {
                                        format!("{db}.")
                                    };
                                    let matching_keys: Vec<(String, std::collections::HashMap<String, String>)> = rs
                                        .scan_at_snapshot(snapshot_xid)
                                        .into_iter()
                                        .filter(|(k, row_data)| {
                                            let k_str: &str = k;
                                            let local_k = if db_prefix_str.is_empty() {
                                                k_str.to_string()
                                            } else {
                                                k_str.strip_prefix(&db_prefix_str).unwrap_or(k_str).to_string()
                                            };
                                            local_k.starts_with(&table_prefix)
                                                && row_data.get(&where_col).map(|v| v == &where_val).unwrap_or(false)
                                        })
                                        .map(|(k, row_data)| (k.to_string(), row_data.clone()))
                                        .collect();

                                    for (matched_k, existing) in matching_keys {
                                        let before = rs.read_latest(&matched_k).cloned();
                                        let mut updated = existing;
                                        updated.insert(set_col.clone(), set_val.clone());
                                        record_undo(&state.tx_undo_log, &connection_id, &matched_k, before);
                                        let _ = rs.begin_write_intent(xid, &matched_k);
                                        let raw_k_stripped = if db_prefix_str.is_empty() {
                                            matched_k.clone()
                                        } else {
                                            matched_k.strip_prefix(&db_prefix_str).unwrap_or(matched_k.as_str()).to_string()
                                        };
                                        { let mut wal = state.wal_engine.lock().expect("wal store_row bulk"); wal.store_row(&db, &raw_k_stripped, xid, Some(&updated)); }
                                        rs.insert(xid, &matched_k, updated);
                                    }
                                    persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                                }
                            } else {
                                let k = db_prefix_key(&db, &raw_k);
                                // Gap #3: record before-image for ROLLBACK support.
                                let before = rs.read_latest(&k).cloned();
                                // UPDATE keeps the row count the same — no stat delta needed.
                                record_undo(&state.tx_undo_log, &connection_id, &k, before);
                                let _ = rs.begin_write_intent(xid, &k);
                                { let mut wal = state.wal_engine.lock().expect("wal store_row"); wal.store_row(&db, &raw_k, xid, Some(&d)); }
                                rs.insert(xid, &k, d);
                                persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                            }
                        }
                    } else if upper.starts_with("DELETE") {
                        // Codd Rule 7: set-at-a-time DELETE — try bulk scan first when WHERE
                        // is on a non-key column; fall through to single-key delete otherwise.
                        let is_bulk_delete = extract_bulk_delete_target(stmt)
                            .map(|(_, ref wc, _)| !wc.eq_ignore_ascii_case("id") && !wc.is_empty())
                            .unwrap_or(false);
                        if is_bulk_delete {
                            if let Some((tbl, where_col, where_val)) = extract_bulk_delete_target(stmt) {
                                let snapshot_xid = rs.current_xid();
                                let table_prefix = format!("{tbl}:");
                                let db_prefix_str = if db.is_empty() { String::new() } else { format!("{db}.") };
                                let matching_keys: Vec<String> = rs
                                    .scan_at_snapshot(snapshot_xid)
                                    .into_iter()
                                    .filter(|(k, row_data)| {
                                        let key_matches = k.contains(&format!("{db_prefix_str}{table_prefix}"));
                                        let val_matches = row_data.get(&where_col).map(|v| *v == where_val).unwrap_or(false);
                                        key_matches && val_matches
                                    })
                                    .map(|(k, _)| k.to_string())
                                    .collect();
                                for matched_k in matching_keys {
                                    let before = rs.read_latest(&matched_k).cloned();
                                    if before.is_some() {
                                        if let Some(colon) = matched_k.rfind(':') {
                                            *stats_inserts.entry(matched_k[..colon].to_string()).or_insert(0) -= 1;
                                        }
                                    }
                                    record_undo(&state.tx_undo_log, &connection_id, &matched_k, before);
                                    let _ = rs.begin_write_intent(xid, &matched_k);
                                    rs.delete(xid, &matched_k);
                                    let raw_k = matched_k.trim_start_matches(&format!("{db_prefix_str}")).to_string();
                                    { let mut wal = state.wal_engine.lock().expect("wal store_row"); wal.store_row(&db, &raw_k, xid, None); }
                                }
                                persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                            }
                        } else if let Some(raw_k) = extract_delete_key_from_sql(stmt) {
                            let k = db_prefix_key(&db, &raw_k);
                            // Gap #3: record before-image for ROLLBACK support.
                            let before = rs.read_latest(&k).cloned();
                            // Only decrement if the row actually existed.
                            if before.is_some() {
                                if let Some(colon) = k.rfind(':') {
                                    *stats_inserts.entry(k[..colon].to_string()).or_insert(0) -= 1;
                                }
                            }
                            record_undo(&state.tx_undo_log, &connection_id, &k, before);
                            let _ = rs.begin_write_intent(xid, &k);
                            rs.delete(xid, &k);
                            { let mut wal = state.wal_engine.lock().expect("wal store_row"); wal.store_row(&db, &raw_k, xid, None); }
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                        }
                    }
                }
                rs.release_write_intents(xid);
                // Gap #7: Apply incremental stat deltas (O(tables_touched) instead of O(all_rows)).
                if !stats_inserts.is_empty() {
                    if let Ok(mut stats) = state.table_stats.lock() {
                        for (table_key, delta) in &stats_inserts {
                            let cur = stats.entry(table_key.clone()).or_insert(0);
                            *cur = (*cur as i64 + delta).max(0) as u64;
                        }
                    }
                }
                // H-1: Update StatsRegistry after DML commit.
                // Run async so we don't block the response path.
                {
                    let state_clone = state.clone();
                    let db_clone = db.clone();
                    tokio::spawn(async move {
                        let snapshot = if let Ok(rs) = state_clone.row_store.lock() {
                            rs.export_rows_snapshot()
                        } else {
                            return;
                        };
                        // Count rows per table (strip db prefix to get bare table name).
                        let mut table_counts: std::collections::HashMap<String, usize> =
                            std::collections::HashMap::new();
                        for (k, _) in &snapshot {
                            // Keys are stored as "db.table:pk" or "table:pk".
                            let after_db = if !db_clone.is_empty() {
                                k.strip_prefix(&format!("{db_clone}.")).unwrap_or(k)
                            } else {
                                k.as_str()
                            };
                            let table = after_db.split(':').next().unwrap_or(after_db);
                            *table_counts.entry(table.to_string()).or_default() += 1;
                        }
                        if let Ok(mut reg) = state_clone.stats_registry.lock() {
                            for (tbl, cnt) in table_counts {
                                reg.update_table(&tbl, cnt, std::collections::HashMap::new());
                            }
                        }
                    });
                }
                // Fire-and-forget: replicate to Raft log so followers catch up.
                // append_command pre-advances last_applied so the apply loop skips re-execution.
                {
                    let mut node = match state.raft_state.lock() {
                        Ok(g) => g,
                        Err(_) => return Err(lock_poisoned_err("raft_state")),
                    };
                    if node.role == crate::RaftRole::Leader {
                        for stmt in &ddl_snapshot {
                            let upper = stmt.trim_start().to_ascii_uppercase();
                            if upper.starts_with("INSERT") || upper.starts_with("UPDATE") || upper.starts_with("DELETE") {
                                node.append_command(stmt.clone(), total_peers);
                            }
                        }
                    }
                }
            }
        }
    }

    // Gap #3: ROLLBACK — apply undo log to restore before-images for this connection.
    let has_rollback = ddl_snapshot.iter().any(|s| {
        let u = s.trim_start().to_ascii_uppercase();
        u == "ROLLBACK" || u.starts_with("ROLLBACK;") || u.starts_with("ROLLBACK ")
    });
    let has_commit = ddl_snapshot.iter().any(|s| {
        let u = s.trim_start().to_ascii_uppercase();
        u == "COMMIT" || u.starts_with("COMMIT;") || u.starts_with("COMMIT ")
    });
    if has_rollback {
        let undo_entries = {
            match state.tx_undo_log.lock() {
                Ok(mut log) => log.remove(&connection_id).unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        };
        if !undo_entries.is_empty() {
            let mut rs = match state.row_store.lock() {
                Ok(g) => g,
                Err(_) => return Err(lock_poisoned_err("row_store")),
            };
            let undo_xid = rs.begin_xid();
            for (key, before_data) in undo_entries.into_iter().rev() {
                match before_data {
                    Some(data) => {
                        // Row existed before — restore it.
                        rs.insert(undo_xid, &key, data);
                    }
                    None => {
                        // Row was inserted fresh — delete it on rollback.
                        rs.delete(undo_xid, &key);
                    }
                }
            }
            rs.release_write_intents(undo_xid);
        }
    } else if has_commit {
        // Gap #3: COMMIT — clear the undo log for this connection.
        if let Ok(mut log) = state.tx_undo_log.lock() { log.remove(&connection_id); }
    }

    if !olap_statements.is_empty() {
        let query = olap_statements.join("; ");
        // C-3: look up any active repeatable-read snapshot for this connection.
        // M-6: if no ACID transaction is active but the request asks for repeatable_read,
        // capture a point-in-time snapshot from the current row-store xid.
        let rr_snapshot_xid: Option<u64> = {
            let conn_map = state.connection_tx_active.lock().ok();
            let from_acid = conn_map.and_then(|m| m.get(&connection_id).and_then(|tx_id| {
                state.acid_transactions.lock().ok()
                    .and_then(|a| a.rr_read_snapshot_xid(tx_id))
            }));
            if from_acid.is_some() {
                from_acid
            } else if req.isolation_level.as_deref() == Some("repeatable_read") {
                // Snapshot at request start for single-request RR consistency.
                state.row_store.lock().ok().map(|rs| rs.current_xid())
            } else {
                None
            }
        };
        let rs = match state.row_store.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned_err("row_store")),
        };
        let data_dir = state.runtime_config.storage.data_dir.clone();
        // C-1: fetch rows from RocksDB as primary read source when the engine persists rows.
        let rocksdb_rows_olap: Option<Vec<(String, std::collections::HashMap<String, String>)>> = {
            let wal = match state.wal_engine.lock() {
                Ok(g) => g,
                Err(_) => return Err(lock_poisoned_err("wal_engine")),
            };
            if wal.persists_rows() {
                let xid = rr_snapshot_xid.unwrap_or_else(|| rs.current_xid());
                Some(wal.scan_rows_for_db(&db, xid))
            } else {
                None
            }
        };
        olap = Some(execute_olap_query(query, req.max_rows, &rs, &db, &data_dir, rr_snapshot_xid, rocksdb_rows_olap));
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
                        source: "legacy_agg_olap_path",
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
            // C-3: use repeatable-read snapshot if this connection has one.
            let rr_snapshot_xid: Option<u64> = {
                let conn_map = state.connection_tx_active.lock().ok();
                conn_map.and_then(|m| m.get(&connection_id).and_then(|tx_id| {
                    state.acid_transactions.lock().ok()
                        .and_then(|a| a.rr_read_snapshot_xid(tx_id))
                }))
            };
            let rs = match state.row_store.lock() {
                Ok(g) => g,
                Err(_) => return Err(lock_poisoned_err("row_store")),
            };
            let limit = req.max_rows.unwrap_or(10_000).min(100_000);
            // C-1: fetch rows from RocksDB as primary read source when available.
            let rocksdb_rows_oltp: Option<Vec<(String, std::collections::HashMap<String, String>)>> = {
                let wal = match state.wal_engine.lock() {
                    Ok(g) => g,
                    Err(_) => return Err(lock_poisoned_err("wal_engine")),
                };
                if wal.persists_rows() {
                    let xid = rr_snapshot_xid.unwrap_or_else(|| rs.current_xid());
                    Some(wal.scan_rows_for_db(&db, xid))
                } else {
                    None
                }
            };
            let idx_mgr = state.index_manager.lock().ok();
            let rows = execute_oltp_select(&olap_statements, &rs, limit, &db, rr_snapshot_xid, rocksdb_rows_oltp, idx_mgr.as_deref());
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
                let rs = match state.row_store.lock() {
                    Ok(g) => g,
                    Err(_) => return Err(lock_poisoned_err("row_store")),
                };
                let table_names = collect_query_table_names(&sql);
                let all_rows = rs.export_rows_snapshot();
                drop(rs);
                let mut table_rows: std::collections::HashMap<String, Vec<(String, voltnuerongrid_store::mvcc::RowData)>> =
                    std::collections::HashMap::new();
                for name in &table_names {
                    let prefix = make_table_scan_prefix(&db, name);
                    let filtered: Vec<_> = all_rows
                        .iter()
                        .filter(|(k, _)| *k == name.as_str() || k.starts_with(&prefix))
                        // Strip db prefix so DataFusion resolves by plain table name.
                        .map(|(k, v)| {
                            let stripped = if db.is_empty() { k.clone() } else {
                                k.strip_prefix(&format!("{db}.")).unwrap_or(k).to_string()
                            };
                            (stripped, v.clone())
                        })
                        .collect();
                    table_rows.insert(name.clone(), filtered);
                }
                if table_rows.is_empty() {
                    table_rows.insert("rows".to_string(), all_rows);
                }
                match run_async_in_executor(df_select_owned(sql, table_rows, limit, String::new())) {
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
            let rs = match state.row_store.lock() {
                Ok(g) => g,
                Err(_) => return Err(lock_poisoned_err("row_store")),
            };
            let snapshot_xid = rs.current_xid();
            let db_prefix_filter = if db.is_empty() { String::new() } else { format!("{db}.") };
            // C-1: prefer RocksDB as primary read source when available.
            let all_rows: Vec<(String, std::collections::HashMap<String, String>)> = {
                let wal = match state.wal_engine.lock() {
                    Ok(g) => g,
                    Err(_) => return Err(lock_poisoned_err("wal_engine")),
                };
                if wal.persists_rows() {
                    // RocksDB rows are already db-scoped with no db prefix.
                    wal.scan_rows_for_db(&db, snapshot_xid)
                } else {
                    rs.scan_at_snapshot(snapshot_xid)
                        .into_iter()
                        .filter(|(k, _)| db_prefix_filter.is_empty() || k.starts_with(&db_prefix_filter))
                        .map(|(k, v)| {
                            let stripped = if db_prefix_filter.is_empty() { k.to_string() } else {
                                k.strip_prefix(&db_prefix_filter).unwrap_or(k).to_string()
                            };
                            (stripped, v.clone())
                        })
                        .collect()
                }
            };
            drop(rs);
            let limit = req.max_rows.unwrap_or(10_000).min(100_000);
            // final ordered column list and rows for this response
            let mut ordered_cols: Vec<String> = Vec::new();
            // M-8 Rule 1: track best observed PG type per column (updated across all rows).
            let mut col_pg_types: std::collections::HashMap<String, &'static str> = std::collections::HashMap::new();
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
                        let catalog = state.ddl_catalog.lock().unwrap_or_else(|e| e.into_inner());
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
                            // Rule 3 (Codd): column absent from row data emits null, not "".
                            for (idx, col_name) in ddl_cols.iter().enumerate() {
                                // Try real column name first, then positional fallback key.
                                // M-8 Rule 1: coerce to typed JSON value instead of raw String.
                                let json_val = match data.get(col_name)
                                    .or_else(|| data.get(&format!("col_{idx}")))
                                {
                                    Some(v) => infer_json_value(v),
                                    None => serde_json::Value::Null,
                                };
                                // Track best non-null PG type for this column.
                                if !matches!(json_val, serde_json::Value::Null) {
                                    col_pg_types.entry(col_name.clone())
                                        .and_modify(|t| { if *t == "text" { *t = json_value_pg_type(&json_val); } })
                                        .or_insert_with(|| json_value_pg_type(&json_val));
                                }
                                row_obj.insert(col_name.clone(), json_val);
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
                                // M-8 Rule 1: coerce to typed JSON value.
                                let json_val = infer_json_value(&data[*col]);
                                if !matches!(json_val, serde_json::Value::Null) {
                                    let col_s = (*col).to_string();
                                    col_pg_types.entry(col_s.clone())
                                        .and_modify(|t| { if *t == "text" { *t = json_value_pg_type(&json_val); } })
                                        .or_insert_with(|| json_value_pg_type(&json_val));
                                }
                                row_obj.insert((*col).to_string(), json_val);
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
                // M-8 Rule 1: use inferred PG types in column descriptors.
                let cols: Vec<serde_json::Value> = ordered_cols
                    .iter()
                    .map(|c| {
                        let dt = col_pg_types.get(c).copied().unwrap_or("text");
                        serde_json::json!({"name": c, "data_type": dt})
                    })
                    .collect();
                (Some(cols), Some(out_rows))
            }
        } else {
            (None, None)
        };

    let response = SqlExecuteResponse {
        status: "ok",
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
            // M-6: record client-requested isolation level and timeout hint for observability
            "requested_isolation_level": req.isolation_level.as_deref().unwrap_or("read_committed"),
            "statement_timeout_ms": req.statement_timeout_ms.unwrap_or(0),
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
    let acid = match state.acid_transactions.lock() {
        Ok(g) => g,
        Err(_) => return Err(lock_poisoned_err("acid_transactions")),
    };
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
    let acid = match state.acid_transactions.lock() {
        Ok(g) => g,
        Err(_) => return Err(lock_poisoned_err("acid_transactions")),
    };
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


// ── M-2: CREATE INDEX helper ─────────────────────────────────────────────

/// Parse `CREATE [UNIQUE] INDEX idx_name ON table_name (col1[, col2, ...])`.
/// Returns `(index_name, table_name, columns, is_unique)`.
fn parse_create_index_sql(sql: &str) -> Option<(String, String, Vec<String>, bool)> {
    let lower = sql.trim().to_ascii_lowercase();
    let (rest, is_unique) = if let Some(r) = lower.strip_prefix("create unique index ") {
        (r, true)
    } else if let Some(r) = lower.strip_prefix("create index ") {
        (r, false)
    } else {
        return None;
    };

    // rest = "idx_name on table_name (col1, col2)"
    // or    "if not exists idx_name on table_name (col1)"
    let rest = rest
        .strip_prefix("if not exists ")
        .unwrap_or(rest);

    let on_pos = rest.find(" on ")?;
    let idx_name = rest[..on_pos].trim().to_string();
    let after_on = rest[on_pos + 4..].trim();

    // after_on = "table_name (col1, col2)"
    let paren_pos = after_on.find('(')?;
    let table_name = after_on[..paren_pos].trim().to_string();
    let col_list = after_on[paren_pos + 1..].trim_end_matches(')').trim();

    let columns: Vec<String> = col_list
        .split(',')
        .map(|c| c.trim().split_whitespace().next().unwrap_or("").to_string())
        .filter(|c| !c.is_empty())
        .collect();

    if idx_name.is_empty() || table_name.is_empty() || columns.is_empty() {
        return None;
    }
    Some((idx_name, table_name, columns, is_unique))
}

/// M-2: Execute a `CREATE [UNIQUE] INDEX` statement by registering the index in
/// `IndexManager` and backfilling it from the current `PagedRowStore` snapshot.
fn handle_create_index_ddl(state: &AppState, sql: &str, db: &str) {
    let parsed = match parse_create_index_sql(sql) {
        Some(p) => p,
        None => {
            tracing::warn!("CREATE INDEX: could not parse statement: {sql}");
            return;
        }
    };
    let (idx_name, table_name, columns, is_unique) = parsed;

    // Register one BTree index per column (IndexDescriptor is single-column).
    // For multi-column indexes, each column gets its own index entry named "idx_name_col".
    let rs = state.row_store.lock().expect("row_store lock create_index backfill");
    let snapshot_xid = rs.current_xid();
    // Collect rows for this table from the row store.
    let table_prefix = if db.is_empty() {
        format!("{table_name}:")
    } else {
        format!("{db}.{table_name}:")
    };
    let rows: Vec<(String, voltnuerongrid_store::mvcc::RowData)> = rs
        .scan_at_snapshot(snapshot_xid)
        .into_iter()
        .filter(|(k, _)| k.starts_with(&table_prefix))
        .map(|(k, v)| {
            let raw_key = if db.is_empty() { k.to_string() } else {
                k.strip_prefix(&format!("{db}.")).unwrap_or(k).to_string()
            };
            (raw_key, v.clone())
        })
        .collect();
    drop(rs); // release row_store before acquiring index_manager

    let mut mgr = state.index_manager.lock().expect("index_manager lock create_index");
    for col in &columns {
        let entry_name = if columns.len() == 1 {
            idx_name.clone()
        } else {
            format!("{idx_name}_{col}")
        };
        let descriptor = voltnuerongrid_store::index::IndexDescriptor {
            name: entry_name.clone(),
            table: table_name.clone(),
            column: col.clone(),
            kind: voltnuerongrid_store::index::IndexKind::BTree,
            unique: is_unique,
        };
        match mgr.create_index(descriptor) {
            Ok(()) => {}
            Err(voltnuerongrid_store::index::IndexError::IndexAlreadyExists(_)) => {
                tracing::warn!("CREATE INDEX: index '{entry_name}' already exists — skipping");
                continue;
            }
            Err(e) => {
                tracing::error!("CREATE INDEX: failed to create index '{entry_name}': {e}");
                continue;
            }
        }

        // Backfill: insert existing rows into the new index.
        if let Some(idx) = mgr.get_mut(&entry_name) {
            for (row_key, data) in &rows {
                if let Some(col_val) = data.get(col.as_str()) {
                    if let Err(e) = idx.insert(col_val, row_key) {
                        tracing::warn!("CREATE INDEX backfill: {e}");
                    }
                }
            }
        }
        tracing::info!("CREATE INDEX: index '{entry_name}' on {table_name}({col}) created and backfilled with {} rows", rows.len());
    }
}

// ── M-2: GRANT / REVOKE / CREATE ROLE / DROP ROLE helpers ───────────────────

/// Parse and apply a GRANT statement to `state.db_grants`.
///
/// Accepted forms:
///   GRANT <role> ON DATABASE <db> TO <user>
///   GRANT <role> TO <user> ON DATABASE <db>
fn handle_grant_sql(state: &AppState, sql: &str) {
    let lower = sql.trim().to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.first() != Some(&"grant") {
        return;
    }
    let role = words.get(1).copied().unwrap_or("").to_string();
    let db_pos = words.iter().position(|&w| w == "database");
    let db = db_pos
        .and_then(|i| words.get(i + 1))
        .copied()
        .unwrap_or("")
        .to_string();
    if role.is_empty() || db.is_empty() {
        return;
    }
    if let Ok(mut grants) = state.db_grants.lock() {
        grants.entry(db).or_default().insert(role);
    }
}

/// Parse and apply a REVOKE statement to `state.db_grants`.
///
/// Accepted form:
///   REVOKE <role> FROM <user> ON DATABASE <db>
fn handle_revoke_sql(state: &AppState, sql: &str) {
    let lower = sql.trim().to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.first() != Some(&"revoke") {
        return;
    }
    let role = words.get(1).copied().unwrap_or("").to_string();
    let db_pos = words.iter().position(|&w| w == "database");
    let db = db_pos
        .and_then(|i| words.get(i + 1))
        .copied()
        .unwrap_or("")
        .to_string();
    if role.is_empty() || db.is_empty() {
        return;
    }
    if let Ok(mut grants) = state.db_grants.lock() {
        if let Some(roles) = grants.get_mut(&db) {
            roles.remove(&role);
        }
    }
}
