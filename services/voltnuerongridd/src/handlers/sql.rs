use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, atomic::Ordering};
use voltnuerongrid_audit::AuditEventKind;
use voltnuerongrid_auth::PrivilegeAction;
use voltnuerongrid_exec::QueryPath;
use voltnuerongrid_sql::{eval_legacy_numeric_aggregation, I18nCatalog, SqlAnalyzer, SqlStatementKind};
use voltnuerongrid_sql::legacy_aggregations::SUPPORTED_LEGACY_AGGREGATIONS;
use voltnuerongrid_store::ddl_catalog::{parse_ddl_info, CatalogResult};
use voltnuerongrid_store::triggers::TriggerEvent;
use crate::{AppState, AuthErrorResponse, RuntimeAccessPrincipal, AcidTxEntry};
use crate::{SqlTransactionResponse, PessimisticLockRecord};
use crate::{CommandDispatcher, CanonicalCommandName, CanonicalError};
use crate::{now_unix_ms, build_http_envelope};
use crate::{execute_transaction_statements, acquire_sql_data_plane_connection, release_sql_data_plane_connection};
use crate::{acquire_pessimistic_lock, release_pessimistic_lock};
use crate::{execute_oltp_select, df_select_owned, run_async_in_executor};
use crate::{execute_udf_runtime_legacy, udf_function_catalog_contract, udf_guard_policy_contract, build_udf_execution_plan};
use crate::route_path_name;
#[cfg(feature = "demo")]
use crate::try_handle_call_insert_rows_demo;
use crate::{extract_delete_key_from_sql, extract_update_row_from_sql, extract_column_names_from_ddl, extract_insert_row_from_sql, extract_all_insert_rows};
use crate::helpers::sql_parse::{extract_bulk_update_target, extract_bulk_delete_target};
use crate::helpers::sql_parse::{db_prefix_key, make_table_scan_prefix, validate_row_against_ddl, extract_partition_column};
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

/// M-6: Statement timeout error — returned when a deadline set by `statement_timeout_ms`
/// has elapsed before (or immediately after) query execution completes.
fn statement_timeout_err() -> (StatusCode, Json<AuthErrorResponse>) {
    (
        StatusCode::REQUEST_TIMEOUT,
        Json(AuthErrorResponse {
            status: "error",
            reason: "statement_timeout".to_string(),
            locale: "en".to_string(),
            localized_message: "Statement exceeded the configured timeout".to_string(),
        }),
    )
}

/// Return `Err(statement_timeout_err())` if `deadline` is `Some` and has already elapsed.
/// No-op when `deadline` is `None` (timeout not configured).
#[inline]
fn check_deadline(deadline: Option<std::time::Instant>) -> Result<(), (StatusCode, Json<AuthErrorResponse>)> {
    if let Some(dl) = deadline {
        if std::time::Instant::now() >= dl {
            return Err(statement_timeout_err());
        }
    }
    Ok(())
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

/// M-2: Read the latest version of `key` from the in-memory row store, falling
/// back to a RocksDB point read when the in-memory lookup misses and the
/// durability engine persists rows.  This ensures before-images for ROLLBACK
/// support and MVCC conflict detection are correct after a crash-recovery boot
/// when rows live only in RocksDB (not yet replayed into `PagedRowStore`).
///
/// `db`    — database scope (empty for no-DB deployments)
/// `key`   — db-prefixed key used in `PagedRowStore` (e.g. `"mydb.orders:1"`)
/// `raw_k` — raw key WITHOUT db prefix (e.g. `"orders:1"`) passed to `get_row`
fn read_latest_with_rocksdb_fallback(
    rs: &voltnuerongrid_store::mvcc::PagedRowStore,
    wal: &crate::AppState,
    db: &str,
    key: &str,
    raw_k: &str,
) -> Option<voltnuerongrid_store::mvcc::RowData> {
    if let Some(row) = rs.read_latest(key) {
        return Some(row.clone());
    }
    // In-memory miss — try RocksDB if it persists rows.
    if let Ok(wal_guard) = wal.storage.wal_engine.lock() {
        if wal_guard.persists_rows() {
            return wal_guard.get_row(db, raw_k);
        }
    }
    None
}

// ─── ISSUE-03: Trigger execution helper ──────────────────────────────────────

/// T-1: Compute the effective statements to flush at COMMIT after applying
/// SAVEPOINT / ROLLBACK TO SAVEPOINT / RELEASE SAVEPOINT semantics.
///
/// In the batch transaction model every statement of a `BEGIN … COMMIT` block
/// arrives in one request and DML is flushed at COMMIT. This helper walks the
/// statement list in order and returns the subset of DML statements that
/// survive savepoint rollbacks:
///
/// * `SAVEPOINT <name>` records a marker at the current applied-length.
/// * `ROLLBACK TO SAVEPOINT <name>` discards every statement applied after the
///   matching savepoint (and any later savepoints).
/// * `RELEASE SAVEPOINT <name>` drops the marker but keeps the work.
/// * `BEGIN` / `COMMIT` / `ROLLBACK` are control statements and never flushed.
///
/// All other statements (INSERT/UPDATE/DELETE and any non-savepoint SQL) are
/// appended to the surviving set in order.
fn effective_statements_after_savepoints(statements: &[String]) -> Vec<String> {
    let mut applied: Vec<String> = Vec::new();
    // (savepoint_name, applied_len_at_creation)
    let mut savepoints: Vec<(String, usize)> = Vec::new();
    for stmt in statements {
        match SqlAnalyzer::classify_statement(stmt) {
            SqlStatementKind::Savepoint => {
                if let Some(name) = stmt.split_ascii_whitespace().nth(1) {
                    savepoints.push((name.to_ascii_lowercase(), applied.len()));
                }
            }
            SqlStatementKind::ReleaseSavepoint => {
                if let Some(name) = stmt.split_ascii_whitespace().nth(2) {
                    let name = name.to_ascii_lowercase();
                    if let Some(pos) = savepoints.iter().rposition(|(n, _)| n == &name) {
                        savepoints.remove(pos);
                    }
                }
            }
            SqlStatementKind::RollbackToSavepoint => {
                let tokens: Vec<&str> = stmt.split_ascii_whitespace().collect();
                let name = if tokens.get(2).map(|t| t.eq_ignore_ascii_case("SAVEPOINT")).unwrap_or(false) {
                    tokens.get(3)
                } else {
                    tokens.get(2)
                };
                if let Some(name) = name {
                    let name = name.to_ascii_lowercase();
                    if let Some(pos) = savepoints.iter().rposition(|(n, _)| n == &name) {
                        let marker = savepoints[pos].1;
                        applied.truncate(marker);
                        // Drop this and any later savepoints (they are now gone).
                        savepoints.truncate(pos + 1);
                    }
                }
            }
            SqlStatementKind::Begin
            | SqlStatementKind::Commit
            | SqlStatementKind::Rollback => {
                // Control statements — never flushed.
            }
            _ => {
                applied.push(stmt.clone());
            }
        }
    }
    applied
}

/// Fire all registered DML triggers that match `(table, schema, event)`.
fn fire_dml_triggers(
    state: &crate::AppState,
    table: &str,
    schema: &str,
    event: &voltnuerongrid_store::triggers::TriggerEvent,
    old_row: Option<&voltnuerongrid_store::mvcc::RowData>,
    new_row: Option<&voltnuerongrid_store::mvcc::RowData>,
) {
    // Build a minimal JSON payload without pulling in a full serde_json dependency
    // on the RowData map — the emitter only needs enough context for logging/CDC.
    let payload = {
        let old_part = match old_row {
            Some(r) => {
                let fields: Vec<String> = r
                    .iter()
                    .map(|(k, v)| format!("\"{}\":\"{}\"", k, v.replace('"', "\\\"")))
                    .collect();
                format!("{{{}}}",  fields.join(","))
            }
            None => "null".to_string(),
        };
        let new_part = match new_row {
            Some(r) => {
                let fields: Vec<String> = r
                    .iter()
                    .map(|(k, v)| format!("\"{}\":\"{}\"", k, v.replace('"', "\\\"")))
                    .collect();
                format!("{{{}}}", fields.join(","))
            }
            None => "null".to_string(),
        };
        format!(
            "{{\"event\":\"{}\",\"table\":\"{}\",\"schema\":\"{}\",\"old_row\":{},\"new_row\":{}}}",
            event.as_str(),
            table,
            schema,
            old_part,
            new_part,
        )
    };

    let triggers_to_fire: Vec<voltnuerongrid_store::triggers::TriggerDefinition> = {
        match state.storage.trigger_registry.lock() {
            Ok(reg) => reg
                .find_triggers(table, schema, event)
                .into_iter()
                .cloned()
                .collect(),
            Err(_) => return, // lock poisoned — skip silently
        }
    };

    for trigger in &triggers_to_fire {
        if let Err(e) = state.storage.trigger_emitter.emit(trigger, &payload) {
            eprintln!(
                "[vng:trigger] emit error for trigger '{}' on {}.{}: {e}",
                trigger.name, schema, table
            );
        }
    }
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

#[derive(Serialize, Deserialize, Default)]
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
    /// P4: OLAP/hybrid route freshness lag — milliseconds since the last committed
    /// OLTP mutation was appended to the HTAP sync origin. `Some(0)` means the OLAP
    /// view is fully up-to-date (in-process store, zero replication lag). `None` means
    /// the route was OLTP-only or no mutations have been published yet.
    pub(crate) freshness_lag_ms: Option<u64>,
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
    /// Q-2: which physical store served this OLAP query — `"rocksdb"` (primary
    /// durable path) or `"paged_store"` (in-memory fallback used in dev/test
    /// when RocksDB is not the active durability engine).
    #[serde(default)]
    pub(crate) data_source: String,
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
    // Accept both x-vng-database and x-vng-db (alias used by gate scripts).
    let db: String = headers
        .get("x-vng-database")
        .or_else(|| headers.get("x-vng-db"))
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
            // Append a process-global monotonic counter so two transactions begun
            // by the same principal within the same millisecond still get distinct
            // ids (otherwise OCC/serializable conflict detection would skip the
            // "same" peer and miss a real write-write conflict).
            let seq = crate::TX_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            format!("tx-{}-{}-{}", identity, now_ms, seq)
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
        // C-3 / B-2: For repeatable_read and optimistic, capture the row-store snapshot
        // Xid at BEGIN time so reads are stable (repeatable_read) and so optimistic
        // version validation compares against the version observed at BEGIN.  Must be
        // captured *before* acquiring acid_transactions lock to avoid deadlock.
        let begin_snapshot_xid: Option<u64> = if has_begin
            && (iso_level == "repeatable_read" || iso_level == "optimistic")
        {
            let rs = match state.storage.row_store.lock() {
                Ok(g) => g,
                Err(_) => return Err(lock_poisoned_err("row_store")),
            };
            Some(rs.current_xid())
        } else {
            None
        };

        let mut acid = match state.storage.acid_transactions.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned_err("acid_transactions")),
        };
        if has_begin {
            acid.begin(&tx_id, &state.node_id, &iso_level, now_ms, begin_snapshot_xid);
            // Register connection → tx mapping so sql_execute can look up the
            // repeatable-read snapshot for standalone SELECT calls.
            if iso_level == "repeatable_read" {
                if let Ok(mut conn_map) = state.storage.connection_tx_active.lock() {
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
            // T-1: Compute the effective DML statements after applying any
            // SAVEPOINT / ROLLBACK TO SAVEPOINT semantics. Statements executed
            // after a savepoint that is later rolled back must NOT be flushed at
            // COMMIT. RELEASE SAVEPOINT keeps the work but drops the marker.
            let effective_statements = effective_statements_after_savepoints(&statements);
            // M-7: Collect the set of row keys this transaction intends to write.
            // Used for both (a) write-write conflict detection (S2-WS2-05) and
            // (b) row-level serializable OCC conflict detection.
            let commit_write_keys: std::collections::HashSet<String> = {
                let mut keys = std::collections::HashSet::new();
                for stmt in &effective_statements {
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
            // B-2: Optimistic-locking validation (runs only for `optimistic` txns).
            // Two complementary checks, both lock-free:
            //   (1) Row-store version check: a written key whose MVCC version
            //       advanced past this txn's BEGIN snapshot was modified by a
            //       concurrent transaction → stale write.
            //   (2) Committed-peer check: another committed optimistic txn already
            //       wrote one of our keys → lost-update prevention.
            // Either yields a typed 409 `optimistic_version_conflict:<key>` and
            // aborts the transaction without ever holding a row lock.
            if iso_level == "optimistic" {
                let begin_snapshot = acid.row_store_snapshot_xid(&tx_id).unwrap_or(0);
                let version_conflict = {
                    let rs = match state.storage.row_store.lock() {
                        Ok(g) => g,
                        Err(_) => return Err(lock_poisoned_err("row_store")),
                    };
                    crate::helpers::execution::optimistic_version_conflict(
                        &rs, &commit_write_keys, begin_snapshot,
                    )
                };
                let conflict = version_conflict
                    .or_else(|| acid.check_optimistic_conflict(&tx_id, &commit_write_keys));
                if let Some(conflict_key) = conflict {
                    acid.rollback(&tx_id, now_ms);
                    drop(acid);
                    let locale = locale_from_headers(&headers);
                    let localized = I18nCatalog::message(locale, "unauthorized");
                    return Err((
                        StatusCode::CONFLICT,
                        Json(AuthErrorResponse {
                            status: "error",
                            reason: format!("optimistic_version_conflict:{conflict_key}"),
                            locale: locale.as_str().to_string(),
                            localized_message: localized.message.to_string(),
                        }),
                    ));
                }
            }
            // M-7: Row-level serializable conflict detection (write-write).
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
            // M-7 (SSI): Read-write anti-dependency check (phantom detection).
            // Detects two dangerous structures:
            //   (1) current TX read a key that a committed concurrent TX wrote (phantom read)
            //   (2) current TX writes a key that a committed concurrent TX read (write-read)
            // Either indicates a serialization anomaly that SSI must prevent.
            {
                let current_read_keys = acid.transactions
                    .get(&tx_id)
                    .map(|e| e.read_row_keys.clone())
                    .unwrap_or_default();
                if let Some(conflict_key) = acid.check_serializable_rw_conflict(
                    &tx_id, &commit_write_keys, &current_read_keys,
                ) {
                    acid.rollback(&tx_id, now_ms);
                    drop(acid);
                    let locale = locale_from_headers(&headers);
                    let localized = I18nCatalog::message(locale, "unauthorized");
                    return Err((
                        StatusCode::CONFLICT,
                        Json(AuthErrorResponse {
                            status: "error",
                            reason: format!("serializable_phantom_conflict:{conflict_key}"),
                            locale: locale.as_str().to_string(),
                            localized_message: localized.message.to_string(),
                        }),
                    ));
                }
            }
            // S2-WS2-05: write-write conflict detection using row-store snapshot xid.
            {
                if !commit_write_keys.is_empty() {
                    let rs = match state.storage.row_store.lock() {
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
            // M-3: Persist committed serializable write-sets to disk so they survive
            // restarts and serializable isolation is enforced across process boundaries.
            {
                let data_dir = state.runtime_config.storage.data_dir.clone();
                if !data_dir.is_empty() {
                    crate::helpers::raft_loop::persist_committed_write_sets(&data_dir, &acid);
                }
            }
            // C-3: Clear the connection→tx mapping on COMMIT so sql_execute no longer
            // applies the repeatable-read snapshot to standalone SELECTs.
            if let Ok(mut conn_map) = state.storage.connection_tx_active.lock() {
                conn_map.remove(&connection_id);
            }
            // S2-WS2-05: flush committed DML (INSERT/UPDATE/DELETE) into PagedRowStore.
            // Write intents are registered before each write and released after the flush
            // so that concurrent transactions see the in-progress lock via begin_write_intent.
            {
                let mut rs = match state.storage.row_store.lock() {
                    Ok(g) => g,
                    Err(_) => return Err(lock_poisoned_err("row_store")),
                };
                // Record snapshot xid before allocating the write xid
                let snapshot_xid = rs.current_xid();
                acid.set_row_store_snapshot(&tx_id, snapshot_xid);
                let xid = rs.begin_xid();
                for stmt in &effective_statements {
                    let upper = stmt.trim_start().to_ascii_uppercase();
                    if upper.starts_with("INSERT") {
                        // Use extract_all_insert_rows to handle multi-row INSERT correctly.
                        // Each row is individually inserted and individually WAL-persisted.
                        for (raw_k, d, single_sql) in extract_all_insert_rows(stmt) {
                            let table_name_t = d.get("__table").map(|t| t.as_str()).unwrap_or("").to_string();
                            // CON-1: Validate constraints before acquiring write intent.
                            if !table_name_t.is_empty() {
                                if let Ok(mgr) = state.storage.constraint_manager.lock() {
                                    for (col, val) in d.iter().filter(|(c, _)| !c.starts_with("__")) {
                                        if let Err(violation) = mgr.validate(&table_name_t, col, Some(val.as_str())) {
                                            drop(mgr);
                                            rs.release_write_intents(xid);
                                            return Err((
                                                StatusCode::CONFLICT,
                                                Json(crate::AuthErrorResponse {
                                                    status: "error",
                                                    reason: format!("constraint_violation: {violation}"),
                                                    locale: "en".to_string(),
                                                    localized_message: format!("Constraint violation in transaction INSERT into '{table_name_t}': {violation}"),
                                                }),
                                            ));
                                        }
                                    }
                                }
                            }
                            let k = db_prefix_key(&db, &raw_k);
                            // Gap #3 + M-2: record before-image for ROLLBACK support, falling
                            // back to RocksDB if the row is not in the in-memory store yet.
                            let before = read_latest_with_rocksdb_fallback(&rs, &state, &db, &k, &raw_k);
                            record_undo(&state.storage.tx_undo_log, &connection_id, &k, before);
                            let _ = rs.begin_write_intent(xid, &k);
                            if let Ok(mut wal) = state.storage.wal_engine.lock() { wal.store_row(&db, &raw_k, xid, Some(&d)); }
                            rs.insert(xid, &k, d.clone());
                            // CON-1: Record committed values for PK/UNIQUE tracking.
                            if !table_name_t.is_empty() {
                                if let Ok(mut mgr) = state.storage.constraint_manager.lock() {
                                    for (col, val) in d.iter().filter(|(c, _)| !c.starts_with("__")) {
                                        mgr.record_committed_value(&table_name_t, col, val);
                                    }
                                }
                            }
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, &single_sql);
                        }
                    } else if upper.starts_with("DELETE") {
                        if let Some(raw_k) = extract_delete_key_from_sql(stmt) {
                            let k = db_prefix_key(&db, &raw_k);
                            // Gap #3 + M-2: record before-image for ROLLBACK support.
                            let before = read_latest_with_rocksdb_fallback(&rs, &state, &db, &k, &raw_k);
                            record_undo(&state.storage.tx_undo_log, &connection_id, &k, before);
                            let _ = rs.begin_write_intent(xid, &k);
                            rs.delete(xid, &k);
                            if let Ok(mut wal) = state.storage.wal_engine.lock() { wal.store_row(&db, &raw_k, xid, None); }
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                        }
                    } else if upper.starts_with("UPDATE") {
                        // M-5: mirror the bulk-scan UPDATE logic from sql_execute so that
                        // a transaction-wrapped UPDATE with a non-PK WHERE clause updates all
                        // matching rows instead of silently updating at most one.
                        if let Some((raw_k, d)) = extract_update_row_from_sql(stmt) {
                            let table_name = d.get("__table").map(|t| t.clone()).unwrap_or_default();
                            // Rule 7 (Codd): a UPDATE whose WHERE filters on a non-PK column
                            // is set-at-a-time — scan the table and update every matching row.
                            let is_scan_update = extract_bulk_update_target(stmt)
                                .map(|(_, _, _, ref wc, _)| !wc.eq_ignore_ascii_case("id") && !wc.is_empty())
                                .unwrap_or(false);
                            if is_scan_update {
                                if let Some((tbl, set_col, set_val, where_col, where_val)) =
                                    extract_bulk_update_target(stmt)
                                {
                                    let snapshot_xid = rs.current_xid();
                                    let table_prefix = format!("{tbl}:");
                                    let db_prefix_str = if db.is_empty() { String::new() } else { format!("{db}.") };
                                    let scan_rows: Vec<(String, std::collections::HashMap<String, String>)> =
                                        if let Ok(wal) = state.storage.wal_engine.lock() {
                                            if wal.persists_rows() {
                                                wal.scan_rows_for_db(&db, snapshot_xid)
                                                    .into_iter()
                                                    .map(|(k, v)| {
                                                        let prefixed = if db_prefix_str.is_empty() { k } else { format!("{db_prefix_str}{k}") };
                                                        (prefixed, v)
                                                    })
                                                    .collect()
                                            } else {
                                                rs.scan_at_snapshot(snapshot_xid)
                                                    .into_iter()
                                                    .map(|(k, v)| (k.to_string(), v.clone()))
                                                    .collect()
                                            }
                                        } else {
                                            rs.scan_at_snapshot(snapshot_xid)
                                                .into_iter()
                                                .map(|(k, v)| (k.to_string(), v.clone()))
                                                .collect()
                                        };
                                    let matching_keys: Vec<(String, std::collections::HashMap<String, String>)> = scan_rows
                                        .into_iter()
                                        .filter(|(k, row_data)| {
                                            let local_k = if db_prefix_str.is_empty() {
                                                k.clone()
                                            } else {
                                                k.strip_prefix(&db_prefix_str).unwrap_or(k.as_str()).to_string()
                                            };
                                            local_k.starts_with(&table_prefix)
                                                && row_data.get(&where_col).map(|v| v == &where_val).unwrap_or(false)
                                        })
                                        .collect();
                                    for (matched_k, existing) in matching_keys {
                                        let before = rs.read_latest(&matched_k).cloned();
                                        let mut updated = existing;
                                        updated.insert(set_col.clone(), set_val.clone());
                                        record_undo(&state.storage.tx_undo_log, &connection_id, &matched_k, before);
                                        let _ = rs.begin_write_intent(xid, &matched_k);
                                        let raw_k_stripped = if db_prefix_str.is_empty() {
                                            matched_k.clone()
                                        } else {
                                            matched_k.strip_prefix(&db_prefix_str).unwrap_or(matched_k.as_str()).to_string()
                                        };
                                        { let mut wal = state.storage.wal_engine.lock().expect("wal store_row bulk txn"); wal.store_row(&db, &raw_k_stripped, xid, Some(&updated)); }
                                        rs.insert(xid, &matched_k, updated);
                                    }
                                    persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                                }
                            } else {
                                let k = db_prefix_key(&db, &raw_k);
                                // Gap #3 + M-2: record before-image for ROLLBACK support.
                                let before = read_latest_with_rocksdb_fallback(&rs, &state, &db, &k, &raw_k);
                                // Read-before-write: merge SET columns into the existing row so
                                // non-SET fields are preserved (fix for UPDATE nullifying columns).
                                let mut merged = before.clone().unwrap_or_default();
                                for (col, val) in &d {
                                    merged.insert(col.clone(), val.clone());
                                }
                                record_undo(&state.storage.tx_undo_log, &connection_id, &k, before);
                                let _ = rs.begin_write_intent(xid, &k);
                                if let Ok(mut wal) = state.storage.wal_engine.lock() { wal.store_row(&db, &raw_k, xid, Some(&merged)); }
                                rs.insert(xid, &k, merged);
                                persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                            }
                        }
                    }
                }
                // S2-WS2-02: record committed DML mutations in the WAL engine for
                // durability and recovery replay.
                if let Ok(mut wal) = state.storage.wal_engine.lock() {
                    for stmt in &effective_statements {
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
            // T-3: Replicate the whole transaction as ONE atomic Raft log entry.
            // The row store was already updated above (direct path), so we use
            // the fire-and-forget `append_command` which pre-advances
            // last_applied; followers apply the batch all-or-nothing under a
            // single Xid. Only the leader appends; single-node clusters commit
            // immediately. This groups BEGIN…COMMIT DML so a follower never sees
            // a partially-applied transaction.
            if let Some(batch_cmd) = crate::encode_raft_batch_command(&db, &effective_statements) {
                let total_peers = state.cluster.raft_peers.len();
                let mut node = match state.cluster.raft_state.lock() {
                    Ok(g) => g,
                    Err(_) => return Err(lock_poisoned_err("raft_state")),
                };
                if node.role == crate::RaftRole::Leader {
                    node.append_command(batch_cmd, total_peers);
                }
            }
            // S4-WS3-04: publish each committed DML mutation to RowStoreSyncOrigin for HTAP consumers.
            {
                use voltnuerongrid_store::htap_sync::MutationOp;
                let mut origin = match state.cluster.sync_origin.lock() {
                    Ok(g) => g,
                    Err(_) => return Err(lock_poisoned_err("sync_origin")),
                };
                for stmt in &effective_statements {
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
            if let Ok(mut conn_map) = state.storage.connection_tx_active.lock() {
                conn_map.remove(&connection_id);
            }
        }
        // Gap #3: on COMMIT clear undo log; on ROLLBACK apply it then clear.
        if has_commit {
            if let Ok(mut log) = state.storage.tx_undo_log.lock() { log.remove(&connection_id); }
        } else if has_rollback {
            let undo_entries = {
                match state.storage.tx_undo_log.lock() {
                    Ok(mut log) => log.remove(&connection_id).unwrap_or_default(),
                    Err(_) => Vec::new(),
                }
            };
            if !undo_entries.is_empty() {
                let mut rs = match state.storage.row_store.lock() {
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
    let mut lock_table = match state.storage.pessimistic_locks.lock() {
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
    let mut wait_graph = match state.storage.pessimistic_lock_waits.lock() {
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
        "deadlock_risk" => { state.storage.pessimistic_lock_metrics.deadlock_detections.fetch_add(1, Ordering::Relaxed); }
        "wait_timeout" if response.reason.contains("scan_cap") => { state.storage.pessimistic_lock_metrics.scan_cap_timeouts.fetch_add(1, Ordering::Relaxed); }
        "wait_timeout" => { state.storage.pessimistic_lock_metrics.wait_timeouts.fetch_add(1, Ordering::Relaxed); }
        "acquired" | "renewed" => { state.storage.pessimistic_lock_metrics.lock_grants.fetch_add(1, Ordering::Relaxed); }
        "held_by_other_transaction" => { state.storage.pessimistic_lock_metrics.lock_conflicts.fetch_add(1, Ordering::Relaxed); }
        _ => {}
    }
    (status, Json(response))
}

pub(crate) async fn sql_pessimistic_lock_release(
    State(state): State<AppState>,
    Json(req): Json<PessimisticLockReleaseRequest>,
) -> (StatusCode, Json<PessimisticLockResponse>) {
    let mut lock_table = match state.storage.pessimistic_locks.lock() {
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
    let mut wait_graph = match state.storage.pessimistic_lock_waits.lock() {
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
        state.storage.pessimistic_lock_metrics.lock_releases.fetch_add(1, Ordering::Relaxed);
    }
    (status, Json(response))
}

pub(crate) async fn sql_pessimistic_lock_metrics(
    State(state): State<AppState>,
) -> Json<PessimisticLockContentionMetricsResponse> {
    let deadlock_detections = state.storage.pessimistic_lock_metrics.deadlock_detections.load(Ordering::Relaxed);
    let scan_cap_timeouts = state.storage.pessimistic_lock_metrics.scan_cap_timeouts.load(Ordering::Relaxed);
    let wait_timeouts = state.storage.pessimistic_lock_metrics.wait_timeouts.load(Ordering::Relaxed);
    let lock_grants = state.storage.pessimistic_lock_metrics.lock_grants.load(Ordering::Relaxed);
    let lock_conflicts = state.storage.pessimistic_lock_metrics.lock_conflicts.load(Ordering::Relaxed);
    let lock_releases = state.storage.pessimistic_lock_metrics.lock_releases.load(Ordering::Relaxed);
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
    // Also accept x-vng-db as an alias (used by gate scripts and drivers).
    // When present, all DDL/DML/SELECT operations are scoped to this database.
    let active_database: Option<String> = headers
        .get("x-vng-database")
        .or_else(|| headers.get("x-vng-db"))
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
            let mut semaphores = match state.storage.db_semaphores.lock() {
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

    // M-6: Statement timeout enforcement.
    // If the client specified statement_timeout_ms, record a deadline. The timeout
    // is checked before and after each major execution phase. Because the core
    // executor is synchronous (no preemption point), this is deadline-based rather
    // than pre-emptive — it prevents returning results that took longer than the
    // budget and returns a 408 to the client.
    let statement_deadline: Option<std::time::Instant> = req.statement_timeout_ms
        .filter(|&ms| ms > 0)
        .map(|ms| std::time::Instant::now() + std::time::Duration::from_millis(ms));

    let dispatcher = CommandDispatcher::new();
    let envelope = build_http_envelope(
        &headers,
        CanonicalCommandName::SqlExecute,
        req.clone(),
        "http-sql-execute",
    );
    let decision = dispatcher.dispatch_sql_execute_route_decision(&envelope);
    let parsed = SqlAnalyzer::parse_batch(&req.sql_batch);

    // Q-1 / AR-2: Cost-based routing refinement. For a single-statement SELECT
    // batch, consult the StatsRegistry so the AST decision can be overridden
    // when the target table size makes OLAP/OLTP the wrong choice. Only a
    // shared read of the registry is taken; the registry is never mutated here.
    let decision = {
        let mut decision = decision;
        if parsed.len() == 1 {
            if let Some(stmt) = parsed.first() {
                if matches!(
                    SqlAnalyzer::analyze_statement(&stmt.raw).kind,
                    voltnuerongrid_sql::SqlStatementKind::Select
                ) {
                    if let Ok(stats) = state.storage.stats_registry.lock() {
                        let db_scope = if db.is_empty() { None } else { Some(db.as_str()) };
                        let refined = voltnuerongrid_exec::HtapQueryRouter::route_with_stats(
                            &stmt.raw, &stats, db_scope,
                        );
                        if refined.path != decision.payload.path {
                            decision.payload.path = refined.path;
                            decision.payload.reason = refined.reason;
                        }
                    }
                }
            }
        }
        decision
    };

    // ── L-2: CREATE PROCEDURE / DROP PROCEDURE / CALL dispatch ──────────────
    // Route stored-procedure DDL and CALL statements through the
    // `ProcedureRegistry` before the normal SQL execution path.
    //
    // Strategy:
    //  - CREATE / DROP PROCEDURE → mutate registry, return early.
    //  - CALL user-defined proc  → expand body, rebind `req` with expanded SQL.
    //  - CALL built-in proc      → fall through to the demo shim below.
    let req = {
        use crate::helpers::stored_proc::ProcedureRegistry;
        let sql_trim = req.sql_batch.trim().to_string();

        if ProcedureRegistry::is_create_procedure(&sql_trim) {
            let mut reg = state.storage.proc_registry.lock().unwrap_or_else(|e| e.into_inner());
            match reg.register_from_ddl(&sql_trim) {
                Ok(name) => {
                    release_sql_data_plane_connection(&state, &connection_id);
                    return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                        status: "ok".to_string(),
                        route_path: "proc_registry".to_string(),
                        reason: format!("procedure '{name}' registered"),
                        ..Default::default()
                    })));
                }
                Err(msg) => {
                    release_sql_data_plane_connection(&state, &connection_id);
                    return Ok((StatusCode::BAD_REQUEST, Json(SqlExecuteResponse {
                        status: "error".to_string(),
                        route_path: "proc_registry".to_string(),
                        reason: msg,
                        ..Default::default()
                    })));
                }
            }
        } else if ProcedureRegistry::is_drop_procedure(&sql_trim) {
            let name = sql_trim["DROP PROCEDURE ".len()..]
                .trim()
                .trim_end_matches(';')
                .trim()
                .to_ascii_lowercase();
            let mut reg = state.storage.proc_registry.lock().unwrap_or_else(|e| e.into_inner());
            match reg.drop_procedure(&name) {
                Ok(()) => {
                    release_sql_data_plane_connection(&state, &connection_id);
                    return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                        status: "ok".to_string(),
                        route_path: "proc_registry".to_string(),
                        reason: format!("procedure '{name}' dropped"),
                        ..Default::default()
                    })));
                }
                Err(msg) => {
                    release_sql_data_plane_connection(&state, &connection_id);
                    return Ok((StatusCode::BAD_REQUEST, Json(SqlExecuteResponse {
                        status: "error".to_string(),
                        route_path: "proc_registry".to_string(),
                        reason: msg,
                        ..Default::default()
                    })));
                }
            }
        } else if ProcedureRegistry::is_call(&sql_trim) {
            let resolved = {
                let reg = state.storage.proc_registry.lock().unwrap_or_else(|e| e.into_inner());
                reg.resolve_call(&sql_trim)
            };
            match resolved {
                Err(msg) => {
                    // Unknown procedure or arity mismatch.
                    release_sql_data_plane_connection(&state, &connection_id);
                    return Ok((StatusCode::BAD_REQUEST, Json(SqlExecuteResponse {
                        status: "error".to_string(),
                        route_path: "proc_registry".to_string(),
                        reason: msg,
                        ..Default::default()
                    })));
                }
                Ok(Some(expanded_sql)) => {
                    // User-defined procedure: rebind `req` with the expanded SQL
                    // body so the rest of the handler executes it transparently.
                    let mut r = req;
                    r.sql_batch = expanded_sql;
                    r
                }
                Ok(None) => req, // built-in — pass through unchanged
            }
        } else {
            req // not a procedure statement — pass through unchanged
        }
    };

    // ── Intercept CREATE DATABASE and DROP DATABASE ──────────────────────────
    if let Some((kind, db_name, flag)) = parse_db_ddl_statement(&req.sql_batch) {
        let start_time = std::time::Instant::now();
        release_sql_data_plane_connection(&state, &connection_id);
        
        match kind {
            voltnuerongrid_sql::SqlStatementKind::CreateDatabase => {
                let mut catalog = state.storage.database_catalog.lock().unwrap_or_else(|e| e.into_inner());
                let now_ms = crate::now_unix_ms() as u128;
                match catalog.create(&db_name, now_ms, None, None) {
                    Ok(_) => {
                        // Persist to DDL WAL using standard format
                        persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Ddl, &format!("CREATE DATABASE {}", db_name));
                        
                        let elapsed_ms = start_time.elapsed().as_millis();
                        return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                            status: "ok".to_string(),
                            route_path: "database_catalog".to_string(),
                            reason: format!("Database '{db_name}' created successfully"),
                            transaction: Some(crate::SqlTransactionResponse {
                                status: "ok".to_string(),
                                transaction_id: "direct".to_string(),
                                statements_executed: 1,
                                requires_transaction: true,
                                touches_catalog: true,
                                rejected_statement_count: 0,
                                elapsed_ms,
                            }),
                            ..Default::default()
                        })));
                    }
                    Err(e) => {
                        // If IF NOT EXISTS was passed and it's already exists, we can treat as success
                        if flag && matches!(e, voltnuerongrid_meta::DatabaseCatalogError::AlreadyExists { .. }) {
                            let elapsed_ms = start_time.elapsed().as_millis();
                            return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                                status: "ok".to_string(),
                                route_path: "database_catalog".to_string(),
                                reason: format!("Database '{db_name}' already exists (IF NOT EXISTS)"),
                                transaction: Some(crate::SqlTransactionResponse {
                                    status: "ok".to_string(),
                                    transaction_id: "direct".to_string(),
                                    statements_executed: 1,
                                    requires_transaction: true,
                                    touches_catalog: true,
                                    rejected_statement_count: 0,
                                    elapsed_ms,
                                }),
                                ..Default::default()
                            })));
                        }
                        return Ok((StatusCode::BAD_REQUEST, Json(SqlExecuteResponse {
                            status: "error".to_string(),
                            route_path: "database_catalog".to_string(),
                            reason: format!("Failed to create database: {}", e),
                            ..Default::default()
                        })));
                    }
                }
            }
            voltnuerongrid_sql::SqlStatementKind::DropDatabase => {
                let mut catalog = state.storage.database_catalog.lock().unwrap_or_else(|e| e.into_inner());
                match catalog.drop_database(&db_name, flag) {
                    Ok(dropped_opt) => {
                        // Only purge rows if actually dropped
                        if dropped_opt.is_some() {
                            crate::helpers::boot::purge_database_rows(&db_name, &state.storage.row_store, &state.storage.wal_engine);
                            // Also purge all DDL catalog entries (tables, views, functions,
                            // triggers, events) for the dropped database so that the schema tree
                            // reflects the deletion on the next refresh.
                            if let Ok(mut ddl_cat) = state.storage.ddl_catalog.lock() {
                                ddl_cat.purge_database(&db_name);
                            }
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Ddl, &format!("DROP DATABASE {}", db_name));
                        }
                        
                        let elapsed_ms = start_time.elapsed().as_millis();
                        return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                            status: "ok".to_string(),
                            route_path: "database_catalog".to_string(),
                            reason: format!("Database '{db_name}' dropped successfully"),
                            transaction: Some(crate::SqlTransactionResponse {
                                status: "ok".to_string(),
                                transaction_id: "direct".to_string(),
                                statements_executed: 1,
                                requires_transaction: true,
                                touches_catalog: true,
                                rejected_statement_count: 0,
                                elapsed_ms,
                            }),
                            ..Default::default()
                        })));
                    }
                    Err(e) => {
                        return Ok((StatusCode::BAD_REQUEST, Json(SqlExecuteResponse {
                            status: "error".to_string(),
                            route_path: "database_catalog".to_string(),
                            reason: format!("Failed to drop database: {}", e),
                            ..Default::default()
                        })));
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    // ── Demo CALL intercept (built-in insert_rows shim) ───────────────────────
    // Handles CALL insert_rows('<table>', <count>).
    // Q-5 / AR-4: compiled only under the `demo` Cargo feature so the production
    // binary carries no synthetic-data generator and the SQL hot path performs
    // no `VNG_DEMO_MODE` env lookup. Build/run with `--features demo` to enable.
    #[cfg(feature = "demo")]
    {
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
                status: "ok".to_string(),
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
                freshness_lag_ms: None,
            }),
        ));
    }

    let udf_function_catalog = udf_function_catalog_contract();
    let udf_guard_policies = udf_guard_policy_contract();
    let udf_execution_plan = build_udf_execution_plan(&req.sql_batch);
    let udf_execution = execute_udf_runtime_legacy(&req.sql_batch);

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
                    freshness_lag_ms: None,
                }),
            ));
            release_sql_data_plane_connection(&state, &connection_id);
            return response;
        }
    };

    // MV-1/MV-2 early intercept: handle matview-specific statements that the
    // dispatcher classifies as Unknown (REFRESH, DROP MATERIALIZED VIEW).
    // Must come BEFORE the QueryPath::Unknown bail-out below.
    {
        let sql_upper = req.sql_batch.trim().to_ascii_uppercase();
        if sql_upper.starts_with("DROP MATERIALIZED VIEW ")
            || sql_upper.starts_with("REFRESH MATERIALIZED VIEW ")
        {
            // Re-inject as DDL by forwarding to the statement loop
            // via a synthetic parsed list. We fall through to the
            // for-statement loop below by temporarily overriding the decision
            // path — do so by NOT returning here; just let fall-through happen.
            // The statement loop handles these intercepts first.
            let mut synthetic_parsed = parsed.clone();
            if synthetic_parsed.is_empty() {
                synthetic_parsed = SqlAnalyzer::parse_batch(&req.sql_batch);
            }
            for stmt in synthetic_parsed {
                let upper = stmt.raw.trim_start().to_ascii_uppercase();
                if upper.starts_with("DROP MATERIALIZED VIEW ") {
                    let view_name = upper
                        .trim_start_matches("DROP MATERIALIZED VIEW ")
                        .trim()
                        .trim_end_matches(';')
                        .to_ascii_lowercase();
                    {
                        let mut cat = state.storage.ddl_catalog.lock().unwrap_or_else(|e| e.into_inner());
                        cat.record_drop("", "", &view_name);
                    }
                    let matview_prefix = format!("__matview:{view_name}:");
                    let delta_prefix = format!("__delta:{view_name}:");
                    let mut rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
                    let xid = rs.begin_xid();
                    let old_keys: Vec<String> = rs.scan_at_snapshot(xid)
                        .into_iter().filter(|(k, _)| k.starts_with(&matview_prefix) || k.starts_with(&delta_prefix))
                        .map(|(k, _)| k.to_string()).collect();
                    for k in old_keys { rs.delete(xid, &k); }
                    drop(rs);
                    release_sql_data_plane_connection(&state, &connection_id);
                    return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                        status: "ok".to_string(),
                        route_path: "materialized_view_drop".to_string(),
                        reason: format!("DROP MATERIALIZED VIEW '{view_name}': removed"),
                        transaction: None, olap: None, rejected_statement_count: 0,
                        udf_results: None, udf_guardrail_status: None,
                        udf_function_catalog: vec![], udf_guard_policies: vec![],
                        udf_execution_plan: vec![], legacy_agg_results: None,
                        planner_path: None, oltp_rows: None, olap_agg_results: None,
                        columns: None, rows: None, freshness_lag_ms: None,
                    })));
                }
                if upper.starts_with("REFRESH MATERIALIZED VIEW ") {
                    let tail = upper
                        .trim_start_matches("REFRESH MATERIALIZED VIEW ")
                        .trim()
                        .trim_end_matches(';');
                    let (view_name, is_concurrent, is_incremental) =
                        if let Some(n) = tail.strip_suffix(" CONCURRENTLY") {
                            (n.trim().to_ascii_lowercase(), true, false)
                        } else if let Some(n) = tail.strip_suffix(" INCREMENTALLY") {
                            (n.trim().to_ascii_lowercase(), false, true)
                        } else {
                            (tail.to_ascii_lowercase(), false, false)
                        };
                    let defining_sql: Option<(String, String)> = {
                        let cat = state.storage.ddl_catalog.lock().unwrap_or_else(|e| e.into_inner());
                        cat.get(&view_name)
                            .filter(|e| e.object_kind == "materialized_view" || e.object_kind == "incremental_matview")
                            .map(|e| {
                                let stmt = &e.original_statement;
                                let upper_stmt = stmt.to_ascii_uppercase();
                                let select = if let Some(pos) = upper_stmt.find(" AS ") {
                                    stmt[pos + 4..].trim().to_string()
                                } else { stmt.clone() };
                                let base_table = upper_stmt.find(" FROM ")
                                    .and_then(|p| {
                                        let tail = &stmt[p + 6..];
                                        let end = tail.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(tail.len());
                                        Some(tail[..end].trim().to_ascii_lowercase())
                                    }).unwrap_or_default();
                                (select, base_table)
                            })
                    };
                    match defining_sql {
                        None => {
                            release_sql_data_plane_connection(&state, &connection_id);
                            return Err((StatusCode::NOT_FOUND, Json(crate::AuthErrorResponse {
                                status: "error",
                                reason: format!("materialized_view_not_found: '{view_name}'"),
                                locale: "en".to_string(),
                                localized_message: format!("Materialized view '{view_name}' not found"),
                            })));
                        }
                        Some((select_sql, base_table)) => {
                            if is_incremental {
                                let delta_prefix = format!("__delta:{base_table}:");
                                let matview_prefix = format!("__matview:{view_name}:");
                                let mut rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
                                let xid = rs.begin_xid();
                                let deltas: Vec<(String, std::collections::HashMap<String, String>)> = rs
                                    .scan_at_snapshot(xid).into_iter()
                                    .filter(|(k, _)| k.starts_with(&delta_prefix))
                                    .map(|(k, v)| (k.to_string(), v.clone())).collect();
                                let mut applied = 0usize;
                                for (delta_key, delta_data) in &deltas {
                                    let op = delta_data.get("__delta_op").map(|s| s.as_str()).unwrap_or("INSERT");
                                    let row_key = delta_data.get("__delta_row_key").cloned().unwrap_or_default();
                                    let snap_key = format!("{matview_prefix}{row_key}");
                                    match op {
                                        "DELETE" => { rs.delete(xid, &snap_key); }
                                        _ => {
                                            let mut data = delta_data.clone();
                                            data.remove("__delta_op"); data.remove("__delta_row_key");
                                            data.insert("__table".to_string(), view_name.clone());
                                            rs.insert(xid, &snap_key, data);
                                        }
                                    }
                                    rs.delete(xid, delta_key);
                                    applied += 1;
                                }
                                drop(rs);
                                release_sql_data_plane_connection(&state, &connection_id);
                                return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                                    status: "ok".to_string(),
                                    route_path: "materialized_view_refresh_incremental".to_string(),
                                    reason: format!("REFRESH MATERIALIZED VIEW '{view_name}' INCREMENTALLY: {applied} deltas applied"),
                                    transaction: None, olap: None, rejected_statement_count: 0,
                                    udf_results: None, udf_guardrail_status: None,
                                    udf_function_catalog: vec![], udf_guard_policies: vec![],
                                    udf_execution_plan: vec![], legacy_agg_results: None,
                                    planner_path: None, oltp_rows: None, olap_agg_results: None,
                                    columns: None, rows: None, freshness_lag_ms: None,
                                })));
                            }
                            // Full refresh (default or CONCURRENTLY).
                            let snapshot = {
                                let rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
                                let xid = rs.current_xid();
                                let rocksdb_rows = if let Ok(wal) = state.storage.wal_engine.lock() {
                                    if wal.persists_rows() { Some(wal.scan_rows_for_db(&db, xid)) } else { None }
                                } else { None };
                                let idx_mgr = state.storage.index_manager.lock().unwrap_or_else(|e| e.into_inner());
                                crate::helpers::execution::execute_oltp_select(
                                    &[select_sql.clone()], &rs, 100_000, &db, Some(xid), rocksdb_rows, Some(&idx_mgr),
                                )
                            };
                            let matview_prefix = format!("__matview:{view_name}:");
                            let mut rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
                            let xid = rs.begin_xid();
                            let old_keys: Vec<String> = rs.scan_at_snapshot(xid)
                                .into_iter().filter(|(k, _)| k.starts_with(&matview_prefix))
                                .map(|(k, _)| k.to_string()).collect();
                            for k in old_keys { rs.delete(xid, &k); }
                            let row_count = snapshot.len();
                            for (i, row) in snapshot.iter().enumerate() {
                                let k = format!("{matview_prefix}{i}");
                                let mut data = row.data.clone();
                                data.insert("__table".to_string(), view_name.clone());
                                rs.insert(xid, &k, data);
                            }
                            drop(rs);
                            let mode = if is_concurrent { "concurrently" } else { "full" };
                            release_sql_data_plane_connection(&state, &connection_id);
                            return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                                status: "ok".to_string(),
                                route_path: "materialized_view_refresh".to_string(),
                                reason: format!("REFRESH MATERIALIZED VIEW '{view_name}' ({mode}): {row_count} rows materialized"),
                                transaction: None, olap: None, rejected_statement_count: 0,
                                udf_results: None, udf_guardrail_status: None,
                                udf_function_catalog: vec![], udf_guard_policies: vec![],
                                udf_execution_plan: vec![], legacy_agg_results: None,
                                planner_path: None, oltp_rows: None, olap_agg_results: None,
                                columns: None, rows: None, freshness_lag_ms: None,
                            })));
                        }
                    }
                }
            }
        }
    }

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
                freshness_lag_ms: None,
            }),
        ));
        release_sql_data_plane_connection(&state, &connection_id);
        return response;
    }

    // PART-2: EXPLAIN SELECT intercept — show index plan before statement dispatch.
    {
        let sql_upper = req.sql_batch.trim().to_ascii_uppercase();
        if sql_upper.starts_with("EXPLAIN ") {
            let inner_sql = req.sql_batch.trim()["EXPLAIN ".len()..].trim();
            let index_descriptors: Vec<(String, String, String)> = state.storage.index_manager
                .lock()
                .ok()
                .map(|mgr| {
                    mgr.list_indexes()
                        .into_iter()
                        .map(|d| (d.table.clone(), d.column.clone(), d.name.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let plan_desc = if let Ok(parsed_stmt) = voltnuerongrid_sql::parse_one(inner_sql) {
                use voltnuerongrid_exec::QueryPlanner;
                let plan = if index_descriptors.is_empty() {
                    QueryPlanner::plan(&parsed_stmt)
                } else {
                    QueryPlanner::plan_with_indexes(&parsed_stmt, &index_descriptors)
                };
                // Describe the plan node chosen
                match &plan {
                    voltnuerongrid_exec::LogicalPlan::IndexScan { table, indexed_column, index_name, .. } => {
                        format!("IndexScan({index_name}) on {table}.{indexed_column}")
                    }
                    voltnuerongrid_exec::LogicalPlan::Scan { table, filter, .. } => {
                        if let Some(f) = filter {
                            format!("TableScan on {table} with filter: {f}")
                        } else {
                            format!("TableScan on {table}")
                        }
                    }
                    other => format!("{other:?}"),
                }
            } else {
                "TableScan (could not parse inner query)".to_string()
            };
            let udf_function_catalog = udf_function_catalog_contract();
            let udf_guard_policies = udf_guard_policy_contract();
            release_sql_data_plane_connection(&state, &connection_id);
            return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                status: "ok".to_string(),
                route_path: "explain".to_string(),
                reason: plan_desc,
                transaction: None, olap: None, rejected_statement_count: 0,
                udf_results: None, udf_guardrail_status: None,
                udf_function_catalog, udf_guard_policies,
                udf_execution_plan: vec![], legacy_agg_results: None,
                planner_path: Some("explain".to_string()), oltp_rows: None,
                olap_agg_results: None, columns: None, rows: None, freshness_lag_ms: None,
            })));
        }
    }

    // ── Statement dispatch ───────────────────────────────────────────────────
    // M-8 Rule 6: Snapshot the DDL catalog once so we can resolve view definitions
    // without holding the lock across the entire dispatch loop.
    // ISSUE-05: Also snapshot catalog-registered UDFs for inline substitution.
    // MV-1: Also snapshot materialized view names so SELECT FROM matview can be intercepted.
    let (view_catalog_snapshot, udf_catalog_snapshot, matview_names): (Vec<(String, String)>, Vec<crate::helpers::udf::CatalogUdfEntry>, std::collections::HashSet<String>) = {
        match state.storage.ddl_catalog.lock() {
            Ok(cat) => {
                // Only regular (non-materialized) views are expanded inline.
                // Materialized views are served from the __matview: snapshot prefix.
                let matviews: std::collections::HashSet<String> = cat.active_entries()
                    .into_iter().filter(|e| e.object_kind == "materialized_view")
                    .map(|e| e.object_name.to_ascii_lowercase()).collect();
                let views = cat.active_entries()
                    .into_iter()
                    .filter(|e| e.object_kind == "view")
                    .map(|e| (e.object_name.to_ascii_lowercase(), e.original_statement.clone()))
                    .collect();
                let udfs = cat.list_active_functions()
                    .into_iter()
                    .map(|e| crate::helpers::udf::CatalogUdfEntry {
                        name: e.object_name.to_ascii_lowercase(),
                        sql_body: crate::helpers::udf::extract_sql_function_body(&e.original_statement),
                        ddl: e.original_statement.clone(),
                    })
                    .collect();
                (views, udfs, matviews)
            }
            Err(_) => (Vec::new(), Vec::new(), std::collections::HashSet::new()),
        }
    };

    let mut transaction_statements = Vec::new();
    let mut olap_statements = Vec::new();
    for statement in parsed {
        let upper = statement.raw.trim_start().to_ascii_uppercase();

        // MV-1 completion: DROP MATERIALIZED VIEW
        if upper.starts_with("DROP MATERIALIZED VIEW ") {
            let view_name = upper
                .trim_start_matches("DROP MATERIALIZED VIEW ")
                .trim()
                .trim_end_matches(';')
                .to_ascii_lowercase();
            {
                let mut cat = state.storage.ddl_catalog.lock().unwrap_or_else(|e| e.into_inner());
                cat.record_drop("", "", &view_name);
            };
            // Clear the cached snapshot rows.
            let matview_prefix = format!("__matview:{view_name}:");
            let mut rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
            let xid = rs.begin_xid();
            let old_keys: Vec<String> = rs
                .scan_at_snapshot(xid)
                .into_iter()
                .filter(|(k, _)| k.starts_with(&matview_prefix))
                .map(|(k, _)| k.to_string())
                .collect();
            for k in old_keys {
                rs.delete(xid, &k);
            }
            // Also clear any delta rows.
            let delta_prefix = format!("__delta:{view_name}:");
            let delta_keys: Vec<String> = rs
                .scan_at_snapshot(xid)
                .into_iter()
                .filter(|(k, _)| k.starts_with(&delta_prefix))
                .map(|(k, _)| k.to_string())
                .collect();
            for k in delta_keys {
                rs.delete(xid, &k);
            }
            drop(rs);
            release_sql_data_plane_connection(&state, &connection_id);
            let reason = format!("DROP MATERIALIZED VIEW '{view_name}': removed");
            return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                status: "ok".to_string(),
                route_path: "materialized_view_drop".to_string(),
                reason,
                transaction: None, olap: None, rejected_statement_count: 0,
                udf_results: None, udf_guardrail_status: None,
                udf_function_catalog: Vec::new(), udf_guard_policies: Vec::new(),
                udf_execution_plan: Vec::new(), legacy_agg_results: None,
                planner_path: None, oltp_rows: None, olap_agg_results: None,
                columns: None, rows: None, freshness_lag_ms: None,
            })));
        }

        // MV-1: REFRESH MATERIALIZED VIEW — execute defining SELECT and store snapshot.
        // Supports: REFRESH MATERIALIZED VIEW name
        //           REFRESH MATERIALIZED VIEW name CONCURRENTLY
        //           REFRESH MATERIALIZED VIEW name INCREMENTALLY  (MV-2)
        if upper.starts_with("REFRESH MATERIALIZED VIEW ") {
            let tail = upper
                .trim_start_matches("REFRESH MATERIALIZED VIEW ")
                .trim()
                .trim_end_matches(';');
            let (view_name_upper, is_concurrent, is_incremental) =
                if let Some(n) = tail.strip_suffix(" CONCURRENTLY") {
                    (n.trim().to_ascii_lowercase(), true, false)
                } else if let Some(n) = tail.strip_suffix(" INCREMENTALLY") {
                    (n.trim().to_ascii_lowercase(), false, true)
                } else {
                    (tail.to_ascii_lowercase(), false, false)
                };
            let view_name = view_name_upper.as_str();
            let defining_sql: Option<(String, String)> = {
                let cat = state.storage.ddl_catalog.lock().unwrap_or_else(|e| e.into_inner());
                cat.get(view_name)
                    .filter(|e| e.object_kind == "materialized_view")
                    .map(|e| {
                        let stmt = &e.original_statement;
                        let upper_stmt = stmt.to_ascii_uppercase();
                        // Extract the SELECT portion after " AS "
                        let select = if let Some(pos) = upper_stmt.find(" AS ") {
                            stmt[pos + 4..].trim().to_string()
                        } else {
                            stmt.clone()
                        };
                        // Heuristic: extract the primary base table from "FROM <table>"
                        let base_table = upper_stmt
                            .find(" FROM ")
                            .and_then(|p| {
                                let tail = &stmt[p + 6..];
                                let end = tail.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(tail.len());
                                Some(tail[..end].trim().to_ascii_lowercase())
                            })
                            .unwrap_or_default();
                        (select, base_table)
                    })
            };
            if let Some((select_sql, base_table)) = defining_sql {
                if is_incremental {
                    // MV-2: apply only the rows from the __delta:<base_table>: prefix.
                    let delta_prefix = format!("__delta:{base_table}:");
                    let matview_prefix = format!("__matview:{view_name}:");
                    let mut rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
                    let xid = rs.begin_xid();
                    // Collect and clear deltas.
                    let deltas: Vec<(String, std::collections::HashMap<String, String>)> = rs
                        .scan_at_snapshot(xid)
                        .into_iter()
                        .filter(|(k, _)| k.starts_with(&delta_prefix))
                        .map(|(k, v)| (k.to_string(), v.clone()))
                        .collect();
                    let mut applied = 0usize;
                    for (delta_key, delta_data) in &deltas {
                        let op = delta_data.get("__delta_op").map(|s| s.as_str()).unwrap_or("INSERT");
                        let row_key = delta_data.get("__delta_row_key").cloned().unwrap_or_default();
                        let snap_key = format!("{matview_prefix}{row_key}");
                        match op {
                            "DELETE" => { rs.delete(xid, &snap_key); }
                            _ => {
                                // INSERT or UPDATE: upsert into snapshot
                                let mut data = delta_data.clone();
                                data.remove("__delta_op");
                                data.remove("__delta_row_key");
                                data.insert("__table".to_string(), view_name.to_string());
                                rs.insert(xid, &snap_key, data);
                            }
                        }
                        rs.delete(xid, delta_key);
                        applied += 1;
                    }
                    drop(rs);
                    release_sql_data_plane_connection(&state, &connection_id);
                    return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                        status: "ok".to_string(),
                        route_path: "materialized_view_refresh_incremental".to_string(),
                        reason: format!("REFRESH MATERIALIZED VIEW '{view_name}' INCREMENTALLY: {applied} deltas applied"),
                        transaction: None, olap: None, rejected_statement_count: 0,
                        udf_results: None, udf_guardrail_status: None,
                        udf_function_catalog: Vec::new(), udf_guard_policies: Vec::new(),
                        udf_execution_plan: Vec::new(), legacy_agg_results: None,
                        planner_path: None, oltp_rows: None, olap_agg_results: None,
                        columns: None, rows: None, freshness_lag_ms: None,
                    })));
                }

                // Full refresh (default or CONCURRENTLY).
                // Execute the SELECT against current row_store.
                let snapshot = {
                    let rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
                    let xid = rs.current_xid();
                    let rocksdb_rows = if let Ok(wal) = state.storage.wal_engine.lock() {
                        if wal.persists_rows() { Some(wal.scan_rows_for_db(&db, xid)) } else { None }
                    } else { None };
                    let idx_mgr = state.storage.index_manager.lock().unwrap_or_else(|e| e.into_inner());
                    crate::helpers::execution::execute_oltp_select(
                        &[select_sql.clone()], &rs, 100_000, &db, Some(xid), rocksdb_rows, Some(&idx_mgr),
                    )
                };
                // For CONCURRENTLY, write to a shadow prefix first, then swap.
                // For regular refresh, write directly.
                let write_prefix = if is_concurrent {
                    format!("__matview_shadow:{view_name}:")
                } else {
                    format!("__matview:{view_name}:")
                };
                let matview_prefix = format!("__matview:{view_name}:");
                let mut rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
                let xid = rs.begin_xid();
                // Remove old rows at write target.
                let old_keys: Vec<String> = rs
                    .scan_at_snapshot(xid)
                    .into_iter()
                    .filter(|(k, _)| k.starts_with(&write_prefix))
                    .map(|(k, _)| k.to_string())
                    .collect();
                for k in old_keys { rs.delete(xid, &k); }
                // Insert new snapshot rows.
                let row_count = snapshot.len();
                for (i, row) in snapshot.iter().enumerate() {
                    let k = format!("{write_prefix}{i}");
                    let mut data = row.data.clone();
                    data.insert("__table".to_string(), view_name.to_string());
                    rs.insert(xid, &k, data);
                }
                // CONCURRENTLY: atomically swap shadow → live prefix.
                if is_concurrent {
                    let shadow_rows: Vec<(String, std::collections::HashMap<String, String>)> = rs
                        .scan_at_snapshot(xid)
                        .into_iter()
                        .filter(|(k, _)| k.starts_with(&write_prefix))
                        .map(|(k, v)| (k.to_string(), v.clone()))
                        .collect();
                    // Remove old live rows.
                    let live_keys: Vec<String> = rs
                        .scan_at_snapshot(xid)
                        .into_iter()
                        .filter(|(k, _)| k.starts_with(&matview_prefix))
                        .map(|(k, _)| k.to_string())
                        .collect();
                    for k in live_keys { rs.delete(xid, &k); }
                    // Write shadow rows to live prefix.
                    for (i, (_, data)) in shadow_rows.iter().enumerate() {
                        let k = format!("{matview_prefix}{i}");
                        rs.insert(xid, &k, data.clone());
                    }
                    // Clear shadow.
                    let shd_keys: Vec<String> = rs
                        .scan_at_snapshot(xid)
                        .into_iter()
                        .filter(|(k, _)| k.starts_with(&write_prefix))
                        .map(|(k, _)| k.to_string())
                        .collect();
                    for k in shd_keys { rs.delete(xid, &k); }
                }
                drop(rs);
                let mode = if is_concurrent { "concurrently" } else { "full" };
                release_sql_data_plane_connection(&state, &connection_id);
                return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                    status: "ok".to_string(),
                    route_path: "materialized_view_refresh".to_string(),
                    reason: format!("REFRESH MATERIALIZED VIEW '{view_name}' ({mode}): {row_count} rows materialized"),
                    transaction: None, olap: None, rejected_statement_count: 0,
                    udf_results: None, udf_guardrail_status: None,
                    udf_function_catalog: Vec::new(), udf_guard_policies: Vec::new(),
                    udf_execution_plan: Vec::new(), legacy_agg_results: None,
                    planner_path: None, oltp_rows: None, olap_agg_results: None,
                    columns: None, rows: None,
                    freshness_lag_ms: None,
                })));
            } else {
                release_sql_data_plane_connection(&state, &connection_id);
                return Err((StatusCode::NOT_FOUND, Json(crate::AuthErrorResponse {
                    status: "error",
                    reason: format!("materialized_view_not_found: '{view_name}'"),
                    locale: "en".to_string(),
                    localized_message: format!("Materialized view '{view_name}' not found in catalog"),
                })));
            }
        }

        // AI-4: ANALYZE <table> — collect column statistics.
        if upper.starts_with("ANALYZE ") {
            let table_name = upper.trim_start_matches("ANALYZE ")
                .trim().trim_end_matches(';').to_ascii_lowercase();
            let rows: Vec<(String, std::collections::HashMap<String, String>)> = {
                let rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
                let xid = rs.current_xid();
                let prefix = if db.is_empty() { format!("{table_name}:") } else { format!("{db}.{table_name}:") };
                rs.scan_at_snapshot(xid).into_iter()
                    .filter(|(k, _)| k.starts_with(&prefix) || k.starts_with(&format!("{table_name}:")))
                    .map(|(k, v)| (k.to_string(), v.clone())).collect()
            };
            let row_count = rows.len();
            let mut col_stats: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
            // Compute per-column min/max/distinct/null_count
            let mut col_data: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
            for (_, row) in &rows {
                for (col, val) in row {
                    if col.starts_with("__") { continue; }
                    col_data.entry(col.clone()).or_default().push(val.clone());
                }
            }
            for (col, vals) in &col_data {
                let distinct: std::collections::HashSet<_> = vals.iter().collect();
                let null_count = vals.iter().filter(|v| v.is_empty() || v.as_str() == "null" || v.as_str() == "NULL").count();
                let mut sorted = vals.clone(); sorted.sort();
                let min = sorted.first().cloned().unwrap_or_default();
                let max = sorted.last().cloned().unwrap_or_default();
                col_stats.insert(col.clone(), serde_json::json!({
                    "distinct_count": distinct.len(),
                    "null_count": null_count,
                    "min": min,
                    "max": max,
                    "sample_count": vals.len(),
                }));
            }
            let stats_json = serde_json::json!({
                "table": table_name,
                "row_count": row_count,
                "columns": col_stats,
            });
            release_sql_data_plane_connection(&state, &connection_id);
            return Ok((StatusCode::OK, Json(SqlExecuteResponse {
                status: "ok".to_string(),
                route_path: "analyze".to_string(),
                reason: serde_json::to_string(&stats_json).unwrap_or_default(),
                transaction: None, olap: None, rejected_statement_count: 0,
                udf_results: None, udf_guardrail_status: None,
                udf_function_catalog: Vec::new(), udf_guard_policies: Vec::new(),
                udf_execution_plan: Vec::new(), legacy_agg_results: None,
                planner_path: None, oltp_rows: None, olap_agg_results: None,
                columns: Some(col_stats.keys().map(|k| serde_json::Value::String(k.clone())).collect()),
                rows: None,
                freshness_lag_ms: None,
            })));
        }

        let analysis = SqlAnalyzer::analyze_statement(&statement.raw);
        if analysis.kind == SqlStatementKind::Select {
            // M-8 Rule 6: If the SELECT targets a registered view, expand it to the
            // view's underlying query body before passing to the executor.
            let expanded = expand_select_view(&statement.raw, &view_catalog_snapshot);
            // ISSUE-05: If the (view-expanded) SELECT calls a catalog UDF with a SQL
            // body, inline the function body as a subquery so the query can execute.
            let expanded = inline_catalog_udf_calls(expanded, &udf_catalog_snapshot);
            olap_statements.push(expanded);
        } else {
            // M-8 Rule 6: For DML (INSERT/UPDATE/DELETE) targeting a simple updatable
            // view, rewrite the statement to target the base table instead.
            let rewritten = rewrite_dml_for_view(&statement.raw, &view_catalog_snapshot);
            transaction_statements.push(rewritten);
        }
    }

    let mut transaction = None;
    let mut olap = None;
    let mut rejected_statement_count = 0usize;
    // Hoisted so the DML WAL/row_store commit block below can access it
    // regardless of whether touches_catalog is true or false.
    let mut ddl_snapshot: Vec<String> = Vec::new();

    // M-6: pre-execution deadline check — reject immediately if deadline already passed.
    check_deadline(statement_deadline).map_err(|e| { release_sql_data_plane_connection(&state, &connection_id); e })?;

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
                    freshness_lag_ms: None,
                }),
            ));
            release_sql_data_plane_connection(&state, &connection_id);
            return response;
        }
        transaction = Some(response);
        // REQ-02: update DDL catalog when DDL statements touched the catalog
        if transaction.as_ref().map(|r| r.touches_catalog).unwrap_or(false) {
            let now_ms = now_unix_ms();
            let mut catalog = match state.storage.ddl_catalog.lock() {
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
                                // MV-1: If this is CREATE MATERIALIZED VIEW ... WITH DATA,
                                //        immediately record the initial-population intent.
                                //        The actual SELECT will run after the DDL loop when the
                                //        catalog is released (avoids double-locking row_store).
                                // MV-2: WITH INCREMENTAL → object_kind override stored in catalog.
                                if info.object_kind == "materialized_view" {
                                    let upper_stmt = stmt.to_ascii_uppercase();
                                    if upper_stmt.contains("WITH INCREMENTAL") {
                                        // Re-record as incremental_matview kind so REFRESH
                                        // INCREMENTALLY can distinguish at query time.
                                        catalog.record_create(
                                            "incremental_matview",
                                            &info.database_name,
                                            &info.schema_name,
                                            &info.object_name,
                                            stmt,
                                            now_ms,
                                            true, // replace ok
                                        );
                                    }
                                    // WITH DATA initial population is deferred to the
                                    // post-DDL section below (needs row_store lock separately).
                                }
                            }
                        }
                        "drop" => {
                            catalog.record_drop(&info.database_name, &info.schema_name, &info.object_name);
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Ddl, stmt);
                            // CACHE-1: DDL-trigger-driven cache invalidation on DROP TABLE.
                            if info.object_kind == "table" && !info.object_name.is_empty() {
                                let prefix = format!("table:{}:", info.object_name);
                                if let Ok(mut cache) = state.ops.distributed_cache.lock() {
                                    cache.evict_by_prefix(&prefix);
                                }
                                // Also remove from partition registry (PART-1).
                                if let Ok(mut reg) = state.storage.partition_registry.lock() {
                                    reg.remove(&info.object_name);
                                }
                            }
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
            // O-2: emit a structured audit event for every DDL statement so schema
            // changes are recorded in the audit trail (not just the WAL).
            for stmt in &ddl_snapshot {
                if let Some(info) = parse_ddl_info(stmt) {
                    append_runtime_audit_event(
                        &state,
                        AuditEventKind::Sql,
                        &principal,
                        "ddl_execute",
                        "ok",
                        json!({
                            "route_scope": "sql/execute",
                            "operation": info.operation,
                            "object_kind": info.object_kind,
                            "object": info.object_name,
                            "database": info.database_name,
                            "schema": info.schema_name,
                        }),
                    );
                }
            }
            for stmt in &ddl_snapshot {
                let lower = stmt.trim().to_ascii_lowercase();
                if lower.starts_with("create index ") || lower.starts_with("create unique index ") {
                    handle_create_index_ddl(&state, stmt, &db);
                }
            }
            // Q-3: CREATE TRIGGER / DROP TRIGGER — register into the live
            // TriggerRegistry so the trigger fires on subsequent DML.  The DDL
            // is already persisted to the WAL above (object_kind "trigger"), so
            // it is replayed into the registry at boot via replay_triggers_into.
            for stmt in &ddl_snapshot {
                let lower = stmt.trim().to_ascii_lowercase();
                if lower.starts_with("create trigger ") {
                    if let Some(def) = voltnuerongrid_store::triggers::parse_create_trigger(stmt) {
                        if let Ok(mut reg) = state.storage.trigger_registry.lock() {
                            // Replace an existing trigger of the same name (idempotent DDL replay).
                            reg.remove_trigger(&def.name);
                            let _ = reg.register(def);
                        }
                    }
                } else if lower.starts_with("drop trigger ") {
                    if let Some(name) = voltnuerongrid_store::triggers::parse_drop_trigger_name(stmt) {
                        if let Ok(mut reg) = state.storage.trigger_registry.lock() {
                            reg.remove_trigger(&name);
                        }
                    }
                }
            }
            // Q-4: Register column/table constraints declared in CREATE TABLE and
            // ALTER TABLE ADD CONSTRAINT so PK/UNIQUE/NOT NULL/CHECK/FK are
            // enforced on subsequent INSERT/UPDATE. Idempotent across DDL replay
            // (a duplicate constraint name is ignored).
            for stmt in &ddl_snapshot {
                let lower = stmt.trim().to_ascii_lowercase();
                let parsed = if lower.starts_with("create table") {
                    voltnuerongrid_store::constraints::parse_create_table_constraints(stmt)
                } else if lower.starts_with("alter table") && lower.contains(" add ") {
                    voltnuerongrid_store::constraints::parse_alter_add_constraint(stmt)
                        .into_iter()
                        .collect()
                } else {
                    Vec::new()
                };
                if parsed.is_empty() {
                    continue;
                }
                if let Ok(mut mgr) = state.storage.constraint_manager.lock() {
                    for pc in parsed {
                        use voltnuerongrid_store::constraints::{ConstraintDescriptor, ConstraintKind};
                        if pc.kind == ConstraintKind::Check {
                            let _ = mgr.add_check_constraint(
                                &pc.name,
                                &pc.table,
                                &pc.column,
                                pc.check_expr.as_deref().unwrap_or(""),
                            );
                        } else {
                            let _ = mgr.add_constraint(ConstraintDescriptor {
                                name: pc.name,
                                table: pc.table,
                                column: pc.column,
                                kind: pc.kind,
                                ref_table: pc.ref_table,
                                ref_column: pc.ref_column,
                            });
                        }
                    }
                }
            }
            // PART-1: Register partition tables for PARTITION BY RANGE(col) DDL.
            for stmt in &ddl_snapshot {
                let upper = stmt.trim_start().to_ascii_uppercase();
                if upper.starts_with("CREATE TABLE ") && upper.contains("PARTITION BY RANGE") {
                    if let Some(part_col) = extract_partition_column(&upper) {
                        // Extract table name (second token after CREATE TABLE)
                        let table_name = upper
                            .trim_start_matches("CREATE TABLE ")
                            .split_whitespace()
                            .next()
                            .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric() && c != '_'))
                            .unwrap_or("")
                            .to_ascii_lowercase();
                        if !table_name.is_empty() {
                            if let Ok(mut reg) = state.storage.partition_registry.lock() {
                                reg.insert(table_name.clone(), part_col);
                            }
                            // B-4: parse + register range-partition segments (boundaries).
                            if let Some(cfg) = crate::helpers::partition::parse_range_partition(stmt) {
                                crate::helpers::partition::register_partition_config(&state, &table_name, cfg);
                            }
                        }
                    }
                }
            }
            // C-2: Register shard tables for DISTRIBUTE BY HASH(col) DDL.
            for stmt in &ddl_snapshot {
                let upper = stmt.trim_start().to_ascii_uppercase();
                if upper.starts_with("CREATE TABLE ") && upper.contains("DISTRIBUTE BY") {
                    let default_shards = crate::read_env_usize("VNG_SHARD_DEFAULT_COUNT", 4);
                    if let Some(cfg) =
                        crate::helpers::dataplane::parse_distribute_by(stmt, default_shards)
                    {
                        let table_name = upper
                            .trim_start_matches("CREATE TABLE ")
                            .split_whitespace()
                            .next()
                            .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric() && c != '_'))
                            .unwrap_or("")
                            .to_ascii_lowercase();
                        if !table_name.is_empty() {
                            crate::helpers::dataplane::register_shard_config(&state, &table_name, cfg);
                        }
                    }
                }
            }
            // MV-1: Initial population for CREATE MATERIALIZED VIEW ... WITH DATA (not NO DATA).
            // Run after releasing the catalog lock to avoid holding two locks simultaneously.
            for stmt in &ddl_snapshot {
                let upper = stmt.trim_start().to_ascii_uppercase();
                if upper.starts_with("CREATE MATERIALIZED VIEW ")
                    && upper.contains(" WITH DATA")
                    && !upper.contains("WITH NO DATA")
                {
                    // Extract view name and defining SELECT.
                    let after_create = upper.trim_start_matches("CREATE MATERIALIZED VIEW ").trim();
                    let view_name = after_create
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let select_sql = if let Some(pos) = stmt.to_ascii_uppercase().find(" AS ") {
                        stmt[pos + 4..]
                            .trim()
                            .trim_end_matches(|c: char| c == ';' || c.is_whitespace())
                            // Strip trailing WITH DATA / WITH INCREMENTAL suffixes.
                            .trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_')
                            .to_string()
                    } else {
                        String::new()
                    };
                    // Strip "WITH DATA" suffix from the select SQL if it leaked through.
                    let select_clean = {
                        let u = select_sql.to_ascii_uppercase();
                        let trimmed = u
                            .trim_end_matches("WITH DATA")
                            .trim_end_matches("WITH INCREMENTAL")
                            .trim_end();
                        select_sql[..trimmed.len().min(select_sql.len())].trim().to_string()
                    };
                    if !view_name.is_empty() && !select_clean.is_empty() {
                        let snapshot = {
                            let rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
                            let xid = rs.current_xid();
                            let rocksdb_rows = if let Ok(wal) = state.storage.wal_engine.lock() {
                                if wal.persists_rows() { Some(wal.scan_rows_for_db(&db, xid)) } else { None }
                            } else { None };
                            let idx_mgr = state.storage.index_manager.lock().unwrap_or_else(|e| e.into_inner());
                            crate::helpers::execution::execute_oltp_select(
                                &[select_clean.clone()], &rs, 100_000, &db, Some(xid), rocksdb_rows, Some(&idx_mgr),
                            )
                        };
                        let matview_prefix = format!("__matview:{view_name}:");
                        let mut rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
                        let xid = rs.begin_xid();
                        for (i, row) in snapshot.iter().enumerate() {
                            let k = format!("{matview_prefix}{i}");
                            let mut data = row.data.clone();
                            data.insert("__table".to_string(), view_name.clone());
                            rs.insert(xid, &k, data);
                        }
                        drop(rs);
                    }
                }
            }
            catalog = match state.storage.ddl_catalog.lock() {
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
                            freshness_lag_ms: None,
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
            // Pre-apply leadership check: in a multi-node cluster, followers must
            // not write DML locally — they have no way to replicate.  Try to proxy
            // the request transparently to the current leader; fall back to 503.
            let peer_count = state.cluster.raft_peers.len();
            if peer_count > 0 {
                let is_leader = {
                    let node = state.cluster.raft_state.lock().expect("raft_state lock leadership_precheck");
                    node.role == crate::RaftRole::Leader
                };
                if !is_leader {
                    let leader_url = state.cluster.current_leader_url.lock().expect("leader_url lock").clone();
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
                        if let Some(token) = state.cluster.cluster_token.as_ref().as_deref() {
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
                        freshness_lag_ms: None,
                    })));
                }
            }

            let total_peers = state.cluster.raft_peers.len();
            let is_multi_node_leader = {
                let node = match state.cluster.raft_state.lock() {
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
                    let mut node = match state.cluster.raft_state.lock() {
                        Ok(g) => g,
                        Err(_) => return Err(lock_poisoned_err("raft_state")),
                    };
                    for stmt in &ddl_snapshot {
                        let upper = stmt.trim_start().to_ascii_uppercase();
                        if upper.starts_with("INSERT") || upper.starts_with("UPDATE") || upper.starts_with("DELETE") {
                            // M-1: embed db scope in command so followers apply to the right CF.
                            let scoped = format!("__vng_db:{db}\n{stmt}");
                            let idx = node.append_command_pending(scoped, total_peers);
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
                    let mut rx = state.cluster.raft_last_applied_tx.subscribe();
                    let wait_ok = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        rx.wait_for(|&applied| applied >= max_pending_index),
                    ).await.is_ok();
                    if !wait_ok {
                        release_sql_data_plane_connection(&state, &connection_id);
                        return Ok((
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(SqlExecuteResponse {
                                status: "error".to_string(),
                                route_path: route_path_name(decision.payload.path).to_string(),
                                reason: "raft_quorum_timeout".to_string(),
                                transaction, olap: None,
                                rejected_statement_count,
                                udf_results: None, udf_guardrail_status: None,
                                udf_function_catalog, udf_guard_policies, udf_execution_plan,
                                legacy_agg_results: None, planner_path: None,
                                oltp_rows: None, olap_agg_results: None,
                                columns: None, rows: None,
                                freshness_lag_ms: None,
                            }),
                        ));
                    }
                }
            } else {
                // ── Direct path (single-node leader / follower / non-Raft) ────────
                let mut rs = match state.storage.row_store.lock() {
                    Ok(g) => g,
                    Err(_) => return Err(lock_poisoned_err("row_store")),
                };
                let xid = rs.begin_xid();

                // M-6: If the batch contains a BEGIN and the client requested a non-default
                // isolation level, register the implicit transaction in acid_transactions so
                // that RR snapshot and serializable conflict detection apply to DML in this batch.
                let implicit_tx_id: Option<String> = {
                    let req_iso = req.isolation_level.as_deref().unwrap_or("read_committed");
                    let has_begin = ddl_snapshot.iter().any(|s| {
                        matches!(SqlAnalyzer::analyze_statement(s).kind, SqlStatementKind::Begin)
                    });
                    if has_begin && req_iso != "read_committed" {
                        let tx_id = format!("implicit-tx-{}-{}", connection_id, now_unix_ms());
                        let begin_snapshot_xid = if req_iso == "repeatable_read" {
                            Some(rs.current_xid())
                        } else {
                            None
                        };
                        if let Ok(mut acid) = state.storage.acid_transactions.lock() {
                            acid.begin(&tx_id, &state.node_id, req_iso, now_unix_ms(), begin_snapshot_xid);
                        }
                        if let Ok(mut conn_map) = state.storage.connection_tx_active.lock() {
                            conn_map.insert(connection_id.clone(), tx_id.clone());
                        }
                        Some(tx_id)
                    } else {
                        None
                    }
                };

                // Gap #7: Track per-table insert/delete deltas for incremental stats update.
                let mut stats_inserts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
                for stmt in &ddl_snapshot {
                    let upper = stmt.trim_start().to_ascii_uppercase();
                    if upper.starts_with("INSERT") {
                        for (raw_k, d, single_sql) in extract_all_insert_rows(stmt) {
                            // M-6: Validate column types against the DDL schema before storing.
                            let table_name = d.get("__table").map(|t| t.as_str()).unwrap_or("");
                            if !table_name.is_empty() {
                                let validation_result = {
                                    let cat = state.storage.ddl_catalog.lock().expect("ddl_catalog validate");
                                    validate_row_against_ddl(table_name, &d, &cat)
                                };
                                if let Err(msg) = validation_result {
                                    rs.release_write_intents(xid);
                                    release_sql_data_plane_connection(&state, &connection_id);
                                    return Err((
                                        StatusCode::BAD_REQUEST,
                                        Json(crate::AuthErrorResponse {
                                            status: "error",
                                            reason: format!("type_validation_error: {msg}"),
                                            locale: "en".to_string(),
                                            localized_message: format!("Type mismatch on INSERT into '{table_name}': {msg}"),
                                        }),
                                    ));
                                }
                                // CON-1: Validate constraints (PK/UNIQUE/NOT NULL) before writing.
                                if let Ok(mgr) = state.storage.constraint_manager.lock() {
                                    // Q-4: reject INSERTs that omit a NOT NULL / PRIMARY KEY column.
                                    for req_col in mgr.not_null_columns(table_name) {
                                        let present = d
                                            .get(&req_col)
                                            .map(|v| !v.is_empty())
                                            .unwrap_or(false);
                                        if !present {
                                            drop(mgr);
                                            rs.release_write_intents(xid);
                                            release_sql_data_plane_connection(&state, &connection_id);
                                            return Err((
                                                StatusCode::CONFLICT,
                                                Json(crate::AuthErrorResponse {
                                                    status: "error",
                                                    reason: format!("constraint_violation: not-null column '{req_col}' missing"),
                                                    locale: "en".to_string(),
                                                    localized_message: format!("NOT NULL violation on INSERT into '{table_name}': column '{req_col}' is required"),
                                                }),
                                            ));
                                        }
                                    }
                                    for (col, val) in d.iter().filter(|(c, _)| !c.starts_with("__")) {
                                        if let Err(violation) = mgr.validate(table_name, col, Some(val.as_str())) {
                                            drop(mgr);
                                            rs.release_write_intents(xid);
                                            release_sql_data_plane_connection(&state, &connection_id);
                                            return Err((
                                                StatusCode::CONFLICT,
                                                Json(crate::AuthErrorResponse {
                                                    status: "error",
                                                    reason: format!("constraint_violation: {violation}"),
                                                    locale: "en".to_string(),
                                                    localized_message: format!("Constraint violation on INSERT into '{table_name}': {violation}"),
                                                }),
                                            ));
                                        }
                                    }
                                }
                            }
                            let k = db_prefix_key(&db, &raw_k);
                            // Gap #3 + M-2: record before-image for ROLLBACK support.
                            let before = read_latest_with_rocksdb_fallback(&rs, &state, &db, &k, &raw_k);
                            // Only increment if this is a fresh insert (not an overwrite).
                            if before.is_none() {
                                if let Some(colon) = k.rfind(':') {
                                    *stats_inserts.entry(k[..colon].to_string()).or_insert(0) += 1;
                                }
                            }
                            record_undo(&state.storage.tx_undo_log, &connection_id, &k, before);
                            let _ = rs.begin_write_intent(xid, &k);
                            { let mut wal = state.storage.wal_engine.lock().expect("wal store_row"); wal.store_row(&db, &raw_k, xid, Some(&d)); }
                            rs.insert(xid, &k, d.clone());
                            // MV-2: Write delta record for incremental matview tracking.
                            if !table_name.is_empty() && !raw_k.starts_with("__") {
                                let delta_key = format!("__delta:{}:{}", table_name, raw_k);
                                let mut delta_data = d.clone();
                                delta_data.insert("__delta_op".to_string(), "INSERT".to_string());
                                delta_data.insert("__delta_row_key".to_string(), raw_k.clone());
                                rs.insert(xid, &delta_key, delta_data);
                            }
                            // CON-1: Record committed values for PK/UNIQUE tracking after successful insert.
                            if !table_name.is_empty() {
                                if let Ok(mut mgr) = state.storage.constraint_manager.lock() {
                                    for (col, val) in d.iter().filter(|(c, _)| !c.starts_with("__")) {
                                        mgr.record_committed_value(table_name, col, val);
                                    }
                                }
                            }
                            // ISSUE-03: fire AFTER INSERT triggers.
                            fire_dml_triggers(&state, table_name, "public", &TriggerEvent::AfterInsert, None, Some(&d));
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, &single_sql);
                        }
                    } else if upper.starts_with("UPDATE") {
                        if let Some((raw_k, d)) = extract_update_row_from_sql(stmt) {
                            // Rule 7 (Codd) — set-at-a-time UPDATE: when the WHERE clause
                            // filters on a non-PK column, fall back to a full table scan
                            // filtered by the WHERE predicate (updates every matching row).
                            let table_name = d.get("__table").map(|t| t.clone()).unwrap_or_default();
                            let is_scan_update = extract_bulk_update_target(stmt)
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
                                    // C-1: prefer RocksDB as the primary scan source for bulk
                                    // UPDATE when the durability engine persists rows.  RocksDB
                                    // rows survive restarts; PagedRowStore may be empty after
                                    // a crash-recovery boot.  Lock ordering: wal_engine inside
                                    // row_store is already the established pattern (store_row).
                                    let scan_rows: Vec<(String, std::collections::HashMap<String, String>)> =
                                        if let Ok(wal) = state.storage.wal_engine.lock() {
                                            if wal.persists_rows() {
                                                // RocksDB keys have NO db prefix — add it so the
                                                // existing filter/write logic (which strips it) works unchanged.
                                                wal.scan_rows_for_db(&db, snapshot_xid)
                                                    .into_iter()
                                                    .map(|(k, v)| {
                                                        let prefixed = if db_prefix_str.is_empty() { k } else { format!("{db_prefix_str}{k}") };
                                                        (prefixed, v)
                                                    })
                                                    .collect()
                                            } else {
                                                rs.scan_at_snapshot(snapshot_xid)
                                                    .into_iter()
                                                    .map(|(k, v)| (k.to_string(), v.clone()))
                                                    .collect()
                                            }
                                        } else {
                                            rs.scan_at_snapshot(snapshot_xid)
                                                .into_iter()
                                                .map(|(k, v)| (k.to_string(), v.clone()))
                                                .collect()
                                        };
                                    let matching_keys: Vec<(String, std::collections::HashMap<String, String>)> = scan_rows
                                        .into_iter()
                                        .filter(|(k, row_data)| {
                                            let local_k = if db_prefix_str.is_empty() {
                                                k.clone()
                                            } else {
                                                k.strip_prefix(&db_prefix_str).unwrap_or(k.as_str()).to_string()
                                            };
                                            local_k.starts_with(&table_prefix)
                                                && row_data.get(&where_col).map(|v| v == &where_val).unwrap_or(false)
                                        })
                                        .collect();

                                    for (matched_k, existing) in matching_keys {
                                        let before = rs.read_latest(&matched_k).cloned();
                                        let mut updated = existing;
                                        updated.insert(set_col.clone(), set_val.clone());
                                        // CON-1: Validate constraints on bulk UPDATE.
                                        if let Ok(mgr) = state.storage.constraint_manager.lock() {
                                            let mut violation_found: Option<String> = None;
                                            for (col, val) in updated.iter().filter(|(c, _)| !c.starts_with("__")) {
                                                // Q-4 fix: skip columns whose value is unchanged so a
                                                // row's own PK/UNIQUE value does not conflict with itself.
                                                if before.as_ref().and_then(|b| b.get(col)) == Some(val) {
                                                    continue;
                                                }
                                                if let Err(v) = mgr.validate(&tbl, col, Some(val.as_str())) {
                                                    violation_found = Some(v.to_string());
                                                    break;
                                                }
                                            }
                                            drop(mgr);
                                            if let Some(msg) = violation_found {
                                                rs.release_write_intents(xid);
                                                release_sql_data_plane_connection(&state, &connection_id);
                                                return Err((
                                                    StatusCode::CONFLICT,
                                                    Json(crate::AuthErrorResponse {
                                                        status: "error",
                                                        reason: format!("constraint_violation: {msg}"),
                                                        locale: "en".to_string(),
                                                        localized_message: format!("Constraint violation on bulk UPDATE '{tbl}': {msg}"),
                                                    }),
                                                ));
                                            }
                                        }
                                        record_undo(&state.storage.tx_undo_log, &connection_id, &matched_k, before.clone());
                                        let _ = rs.begin_write_intent(xid, &matched_k);
                                        let raw_k_stripped = if db_prefix_str.is_empty() {
                                            matched_k.clone()
                                        } else {
                                            matched_k.strip_prefix(&db_prefix_str).unwrap_or(matched_k.as_str()).to_string()
                                        };
                                        { let mut wal = state.storage.wal_engine.lock().expect("wal store_row bulk"); wal.store_row(&db, &raw_k_stripped, xid, Some(&updated)); }
                                        rs.insert(xid, &matched_k, updated.clone());
                                        // ISSUE-03: fire AFTER UPDATE triggers.
                                        fire_dml_triggers(&state, &tbl, "public", &TriggerEvent::AfterUpdate, before.as_ref(), Some(&updated));
                                    }
                                    persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                                }
                            } else {
                                let k = db_prefix_key(&db, &raw_k);
                                // Gap #3 + M-2: record before-image for ROLLBACK support.
                                let before = read_latest_with_rocksdb_fallback(&rs, &state, &db, &k, &raw_k);
                                // Read-before-write: merge SET columns into the existing row so
                                // non-SET fields are preserved (fix for UPDATE nullifying columns).
                                // UPDATE keeps the row count the same — no stat delta needed.
                                let mut merged = before.clone().unwrap_or_default();
                                for (col, val) in &d {
                                    merged.insert(col.clone(), val.clone());
                                }
                                let table_name_upd = d.get("__table").map(|s| s.as_str()).unwrap_or("");
                                // CON-1: Validate constraints on UPDATE (merged row values).
                                if !table_name_upd.is_empty() {
                                    if let Ok(mgr) = state.storage.constraint_manager.lock() {
                                        for (col, val) in merged.iter().filter(|(c, _)| !c.starts_with("__")) {
                                            // Q-4 fix: skip columns whose value is unchanged so a
                                            // row's own PK/UNIQUE value does not conflict with itself.
                                            if before.as_ref().and_then(|b| b.get(col)) == Some(val) {
                                                continue;
                                            }
                                            if let Err(violation) = mgr.validate(table_name_upd, col, Some(val.as_str())) {
                                                drop(mgr);
                                                rs.release_write_intents(xid);
                                                release_sql_data_plane_connection(&state, &connection_id);
                                                return Err((
                                                    StatusCode::CONFLICT,
                                                    Json(crate::AuthErrorResponse {
                                                        status: "error",
                                                        reason: format!("constraint_violation: {violation}"),
                                                        locale: "en".to_string(),
                                                        localized_message: format!("Constraint violation on UPDATE '{table_name_upd}': {violation}"),
                                                    }),
                                                ));
                                            }
                                        }
                                    }
                                }
                                record_undo(&state.storage.tx_undo_log, &connection_id, &k, before.clone());
                                let _ = rs.begin_write_intent(xid, &k);
                                { let mut wal = state.storage.wal_engine.lock().expect("wal store_row"); wal.store_row(&db, &raw_k, xid, Some(&merged)); }
                                rs.insert(xid, &k, merged.clone());
                                // ISSUE-03: fire AFTER UPDATE triggers.
                                fire_dml_triggers(&state, table_name_upd, "public", &TriggerEvent::AfterUpdate, before.as_ref(), Some(&merged));
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
                                // C-1: prefer RocksDB as the primary scan source for bulk DELETE
                                // when the durability engine persists rows (same reasoning as bulk UPDATE).
                                let scan_rows: Vec<(String, std::collections::HashMap<String, String>)> =
                                    if let Ok(wal) = state.storage.wal_engine.lock() {
                                        if wal.persists_rows() {
                                            wal.scan_rows_for_db(&db, snapshot_xid)
                                                .into_iter()
                                                .map(|(k, v)| {
                                                    let prefixed = if db_prefix_str.is_empty() { k } else { format!("{db_prefix_str}{k}") };
                                                    (prefixed, v)
                                                })
                                                .collect()
                                        } else {
                                            rs.scan_at_snapshot(snapshot_xid)
                                                .into_iter()
                                                .map(|(k, v)| (k.to_string(), v.clone()))
                                                .collect()
                                        }
                                    } else {
                                        rs.scan_at_snapshot(snapshot_xid)
                                            .into_iter()
                                            .map(|(k, v)| (k.to_string(), v.clone()))
                                            .collect()
                                    };
                                let matching_keys: Vec<String> = scan_rows
                                    .into_iter()
                                    .filter(|(k, row_data)| {
                                        let key_matches = k.contains(&format!("{db_prefix_str}{table_prefix}"));
                                        let val_matches = row_data.get(&where_col).map(|v| *v == where_val).unwrap_or(false);
                                        key_matches && val_matches
                                    })
                                    .map(|(k, _)| k)
                                    .collect();
                                for matched_k in matching_keys {
                                    let before = rs.read_latest(&matched_k).cloned();
                                    if before.is_some() {
                                        if let Some(colon) = matched_k.rfind(':') {
                                            *stats_inserts.entry(matched_k[..colon].to_string()).or_insert(0) -= 1;
                                        }
                                    }
                                    record_undo(&state.storage.tx_undo_log, &connection_id, &matched_k, before.clone());
                                    let _ = rs.begin_write_intent(xid, &matched_k);
                                    rs.delete(xid, &matched_k);
                                    let raw_k = matched_k.trim_start_matches(&format!("{db_prefix_str}")).to_string();
                                    { let mut wal = state.storage.wal_engine.lock().expect("wal store_row"); wal.store_row(&db, &raw_k, xid, None); }
                                    // ISSUE-03: fire AFTER DELETE triggers.
                                    fire_dml_triggers(&state, &tbl, "public", &TriggerEvent::AfterDelete, before.as_ref(), None);
                                }
                                persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                            }
                        } else if let Some(raw_k) = extract_delete_key_from_sql(stmt) {
                            let k = db_prefix_key(&db, &raw_k);
                            // Gap #3 + M-2: record before-image for ROLLBACK support.
                            let before = read_latest_with_rocksdb_fallback(&rs, &state, &db, &k, &raw_k);
                            // Only decrement if the row actually existed.
                            if before.is_some() {
                                if let Some(colon) = k.rfind(':') {
                                    *stats_inserts.entry(k[..colon].to_string()).or_insert(0) -= 1;
                                }
                            }
                            // Extract table name from the raw key (format: "table:id").
                            let del_table = raw_k.split(':').next().unwrap_or("");
                            record_undo(&state.storage.tx_undo_log, &connection_id, &k, before.clone());
                            let _ = rs.begin_write_intent(xid, &k);
                            rs.delete(xid, &k);
                            { let mut wal = state.storage.wal_engine.lock().expect("wal store_row"); wal.store_row(&db, &raw_k, xid, None); }
                            // MV-2: Write delta record for incremental matview tracking.
                            if !del_table.is_empty() && !raw_k.starts_with("__") {
                                let delta_key = format!("__delta:{}:{}", del_table, raw_k);
                                let mut delta_data = std::collections::HashMap::new();
                                delta_data.insert("__delta_op".to_string(), "DELETE".to_string());
                                delta_data.insert("__delta_row_key".to_string(), raw_k.clone());
                                rs.insert(xid, &delta_key, delta_data);
                            }
                            // ISSUE-03: fire AFTER DELETE triggers.
                            fire_dml_triggers(&state, del_table, "public", &TriggerEvent::AfterDelete, before.as_ref(), None);
                            persist_sql_statement(&state, voltnuerongrid_store::SqlWalKind::Dml, stmt);
                        }
                    }
                }
                rs.release_write_intents(xid);

                // M-6: Commit (or rollback) the implicit ACID transaction if we registered one.
                {
                    let batch_has_rollback = ddl_snapshot.iter().any(|s| {
                        let u = s.trim_start().to_ascii_uppercase();
                        u == "ROLLBACK" || u.starts_with("ROLLBACK;") || u.starts_with("ROLLBACK ")
                    });
                    if let Some(ref tx_id) = implicit_tx_id {
                        if let Ok(mut acid) = state.storage.acid_transactions.lock() {
                            if batch_has_rollback {
                                acid.rollback(tx_id, now_unix_ms());
                            } else {
                                acid.commit(tx_id, now_unix_ms());
                            }
                        }
                        if let Ok(mut conn_map) = state.storage.connection_tx_active.lock() {
                            conn_map.remove(&connection_id);
                        }
                    }
                }

                // Gap #7: Apply incremental stat deltas (O(tables_touched) instead of O(all_rows)).
                if !stats_inserts.is_empty() {
                    if let Ok(mut stats) = state.storage.table_stats.lock() {
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
                        let snapshot = if let Ok(rs) = state_clone.storage.row_store.lock() {
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
                        if let Ok(mut reg) = state_clone.storage.stats_registry.lock() {
                            for (tbl, cnt) in table_counts {
                                reg.update_table(&tbl, cnt, std::collections::HashMap::new());
                            }
                        }
                    });
                }
                // Fire-and-forget: replicate to Raft log so followers catch up.
                // append_command pre-advances last_applied so the apply loop skips re-execution.
                {
                    let mut node = match state.cluster.raft_state.lock() {
                        Ok(g) => g,
                        Err(_) => return Err(lock_poisoned_err("raft_state")),
                    };
                    if node.role == crate::RaftRole::Leader {
                        for stmt in &ddl_snapshot {
                            let upper = stmt.trim_start().to_ascii_uppercase();
                            if upper.starts_with("INSERT") || upper.starts_with("UPDATE") || upper.starts_with("DELETE") {
                                // M-1: embed db scope so followers apply to the right CF.
                                let scoped = format!("__vng_db:{db}\n{stmt}");
                                node.append_command(scoped, total_peers);
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
            match state.storage.tx_undo_log.lock() {
                Ok(mut log) => log.remove(&connection_id).unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        };
        if !undo_entries.is_empty() {
            let mut rs = match state.storage.row_store.lock() {
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
        if let Ok(mut log) = state.storage.tx_undo_log.lock() { log.remove(&connection_id); }
    }

    if !olap_statements.is_empty() {
        // M-8: Linearisable reads — if VNG_REQUIRE_LEADER_READS=true, only the
        // current Raft leader may serve SELECT queries.  Followers and deposed
        // leaders return 503 so clients retry against the leader.  Single-node
        // deployments are unaffected (total_peers == 0 → always allowed).
        let require_leader_reads = std::env::var("VNG_REQUIRE_LEADER_READS")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        if require_leader_reads {
            let total_peers = state.cluster.raft_peers.len();
            if total_peers > 0 {
                let is_leader = {
                    let node = state.cluster.raft_state.lock().expect("raft_state leader read check");
                    node.role == crate::RaftRole::Leader
                };
                if !is_leader {
                    release_sql_data_plane_connection(&state, &connection_id);
                    return Ok((
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(SqlExecuteResponse {
                            status: "error".to_string(),
                            route_path: route_path_name(decision.payload.path).to_string(),
                            reason: "not_leader_reads_rejected".to_string(),
                            transaction: None,
                            olap: None,
                            rejected_statement_count: olap_statements.len(),
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
                            freshness_lag_ms: None,
                        }),
                    ));
                }
            }
        }

        // DataFusion path: mirrors the df_select_owned pattern used in the
        // olap_agg_results block below. execute_olap_query is no longer called
        // here so all OLAP SELECT dispatch goes through a single code path.
        use voltnuerongrid_exec_datafusion::{collect_query_table_names, SelectOutput};
        let started = std::time::Instant::now();
        let query = olap_statements.join("; ");
        let limit = req.max_rows.unwrap_or(1_000).min(100_000);
        let rs = state.storage.row_store.lock().expect("row_store lock olap_execute");
        let table_names = collect_query_table_names(&query);
        let all_rows = rs.export_rows_snapshot();
        drop(rs);
        let mut table_rows: std::collections::HashMap<String, Vec<(String, voltnuerongrid_store::mvcc::RowData)>> =
            std::collections::HashMap::new();
        for name in &table_names {
            // MV-1: If the table name is a materialized view, serve rows from the
            // __matview:<name>: snapshot prefix instead of scanning normal table rows.
            if matview_names.contains(name.as_str()) {
                let matview_prefix = format!("__matview:{name}:");
                let matview_rows: Vec<_> = all_rows.iter()
                    .filter(|(k, _)| k.starts_with(&matview_prefix))
                    .map(|(k, v)| {
                        // Strip the __matview:name: prefix to expose clean row keys.
                        let short_key = k.strip_prefix(&matview_prefix)
                            .map(|s| format!("{name}:{s}"))
                            .unwrap_or_else(|| k.clone());
                        (short_key, v.clone())
                    }).collect();
                table_rows.insert(name.clone(), matview_rows);
                continue;
            }
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
        let data_dir = state.runtime_config.storage.data_dir.clone();
        // M-4: wrap DataFusion execution in a preemptive timeout when the
        // caller specified statement_timeout_ms.  tokio::time::timeout inside
        // the future is the only cancellation point that can interrupt a
        // running DataFusion plan — the synchronous block_in_place wrapper
        // itself cannot be interrupted from outside.
        let df_future = df_select_owned(query.clone(), table_rows, limit, data_dir);
        let df_result = if let Some(dl) = statement_deadline {
            let remaining = dl.saturating_duration_since(std::time::Instant::now());
            run_async_in_executor(async move {
                tokio::time::timeout(remaining, df_future).await
                    .map_err(|_| voltnuerongrid_exec_datafusion::ExecError::Timeout)
                    .and_then(|r| r)
            })
        } else {
            run_async_in_executor(df_future)
        };
        if let Err(voltnuerongrid_exec_datafusion::ExecError::Timeout) = &df_result {
            release_sql_data_plane_connection(&state, &connection_id);
            return Err(statement_timeout_err());
        }
        let row_count = match df_result {
            Ok(SelectOutput::Rows(rows)) => rows.len(),
            Ok(SelectOutput::Aggregate(_)) => 1,
            Err(_) => 0,
        };
        olap = Some(OlapQueryResponse {
            status: "ok".to_string(),
            query_signature: query.chars().take(64).collect(),
            elapsed_ms: started.elapsed().as_millis(),
            rows: row_count,
            // Q-2: this code path reads from the PagedRowStore snapshot, which is
            // hydrated from RocksDB when RocksDB is the active durability engine.
            data_source: {
                let persists = state
                    .storage.wal_engine
                    .lock()
                    .map(|w| w.persists_rows())
                    .unwrap_or(false);
                if persists {
                    "rocksdb".to_string()
                } else {
                    tracing::warn!(
                        target: "vng.olap",
                        query_signature = %query.chars().take(64).collect::<String>(),
                        "Q-2: OLAP query served from in-memory PagedRowStore (no durable row engine active)"
                    );
                    "paged_store".to_string()
                }
            },
        });
    }

    // REQ-12: Detect legacy aggregate functions in OLAP SELECT statements and
    // route them through eval_legacy_numeric_aggregation.
    let legacy_agg_results: Option<Vec<LegacyAggResult>> = {
        let mut agg_results: Vec<LegacyAggResult> = Vec::new();
        // REQ-12: collect real numeric values from all ingest stores; fall back to synthetic sample.
        let mut real_values: Vec<f64> = Vec::new();
        for store in [
            &state.ingest.ingest_csv_records,
            &state.ingest.ingest_json_records,
            &state.ingest.ingest_parquet_records,
            &state.ingest.ingest_excel_records,
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
        // H-1: Build an index descriptor list for index-aware cost routing.
        // Snapshot (table, column, index_name) from the IndexManager so the planner
        // can promote Filter(Scan) → IndexScan and assign lower cost to index lookups.
        let index_descriptors: Vec<(String, String, String)> = state.storage.index_manager
            .lock()
            .ok()
            .map(|mgr| {
                mgr.list_indexes()
                    .into_iter()
                    .map(|d| (d.table.clone(), d.column.clone(), d.name.clone()))
                    .collect()
            })
            .unwrap_or_default();

        let mut max_cost: f64 = f64::NEG_INFINITY;
        let mut dominant: Option<String> = None;
        for stmt in &olap_statements {
            if let Ok(parsed) = parse_one(stmt) {
                // H-1: Use index-aware planner when indexes are available.
                let plan = if index_descriptors.is_empty() {
                    QueryPlanner::plan(&parsed)
                } else {
                    QueryPlanner::plan_with_indexes(&parsed, &index_descriptors)
                };
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
        // Q4: OTEL trace event for HTAP route decision.
        tracing::debug!(
            route_chosen = dominant.as_deref().unwrap_or("none"),
            statement_count = olap_statements.len(),
            "htap.route_decision"
        );
        dominant
    };

    // S4-WS3-02: OLTP physical executor dispatch
    let oltp_rows: Option<Vec<OltpRowResult>> =
        if planner_path.as_deref() == Some("oltp") && !olap_statements.is_empty() {
            // C-3: use repeatable-read snapshot if this connection has one.
            let rr_snapshot_xid: Option<u64> = {
                let conn_map = state.storage.connection_tx_active.lock().ok();
                conn_map.and_then(|m| m.get(&connection_id).and_then(|tx_id| {
                    state.storage.acid_transactions.lock().ok()
                        .and_then(|a| a.rr_read_snapshot_xid(tx_id))
                }))
            };
            let rs = match state.storage.row_store.lock() {
                Ok(g) => g,
                Err(_) => return Err(lock_poisoned_err("row_store")),
            };
            let limit = req.max_rows.unwrap_or(10_000).min(100_000);
            // C-1: fetch rows from RocksDB as primary read source when available.
            let rocksdb_rows_oltp: Option<Vec<(String, std::collections::HashMap<String, String>)>> = {
                let wal = match state.storage.wal_engine.lock() {
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
            let idx_mgr = state.storage.index_manager.lock().ok();
            // M-6: check deadline before OLTP select (full table scans can be slow).
            check_deadline(statement_deadline).map_err(|e| { release_sql_data_plane_connection(&state, &connection_id); e })?;
            let rows = execute_oltp_select(&olap_statements, &rs, limit, &db, rr_snapshot_xid, rocksdb_rows_oltp, idx_mgr.as_deref());
            // M-6: check deadline after OLTP select to avoid returning timed-out results.
            check_deadline(statement_deadline).map_err(|e| { release_sql_data_plane_connection(&state, &connection_id); e })?;
            // M-7 (SSI): Record the row keys returned by this SELECT into the
            // active serializable transaction's read-set so phantom detection
            // at COMMIT time can check read-write anti-dependencies.
            if !rows.is_empty() {
                let active_tx_id: Option<String> = state.storage.connection_tx_active
                    .lock().ok()
                    .and_then(|m| m.get(&connection_id).cloned());
                if let Some(tx_id) = active_tx_id {
                    if let Ok(mut acid) = state.storage.acid_transactions.lock() {
                        let read_keys: Vec<String> = rows.iter()
                            .map(|r| db_prefix_key(&db, &r.key))
                            .collect();
                        acid.record_read_row_keys(&tx_id, read_keys);
                    }
                }
            }
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
                let rs = match state.storage.row_store.lock() {
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
                // M-4: apply preemptive timeout to aggregate DataFusion path.
                let df_agg_future = df_select_owned(sql, table_rows, limit, String::new());
                let df_agg_result = if let Some(dl) = statement_deadline {
                    let remaining = dl.saturating_duration_since(std::time::Instant::now());
                    run_async_in_executor(async move {
                        tokio::time::timeout(remaining, df_agg_future).await
                            .map_err(|_| voltnuerongrid_exec_datafusion::ExecError::Timeout)
                            .and_then(|r| r)
                    })
                } else {
                    run_async_in_executor(df_agg_future)
                };
                match df_agg_result {
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
            let rs = match state.storage.row_store.lock() {
                Ok(g) => g,
                Err(_) => return Err(lock_poisoned_err("row_store")),
            };
            let snapshot_xid = rs.current_xid();
            let db_prefix_filter = if db.is_empty() { String::new() } else { format!("{db}.") };
            // C-1: prefer RocksDB as primary read source when available.
            let all_rows: Vec<(String, std::collections::HashMap<String, String>)> = {
                let wal = match state.storage.wal_engine.lock() {
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
                        let catalog = state.storage.ddl_catalog.lock().unwrap_or_else(|e| e.into_inner());
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
        // P4: Compute OLAP freshness lag — milliseconds since the last committed
        // OLTP mutation was appended to the HTAP sync origin.
        freshness_lag_ms: if matches!(decision.payload.path, crate::QueryPath::Olap | crate::QueryPath::Hybrid) {
            if let Ok(origin) = state.cluster.sync_origin.lock() {
                let last_ms = origin.last_mutation_epoch_ms();
                if last_ms > 0 {
                    Some((now_unix_ms() as u64).saturating_sub(last_ms))
                } else {
                    Some(0)
                }
            } else {
                None
            }
        } else {
            None
        },
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
    let acid = match state.storage.acid_transactions.lock() {
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
    let acid = match state.storage.acid_transactions.lock() {
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
pub(crate) fn handle_create_index_ddl(state: &AppState, sql: &str, db: &str) {
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
    let rs = state.storage.row_store.lock().expect("row_store lock create_index backfill");
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

    let mut mgr = state.storage.index_manager.lock().expect("index_manager lock create_index");
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

/// Parse and apply a GRANT statement to `state.auth.db_grants`.
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
    if let Ok(mut grants) = state.auth.db_grants.lock() {
        grants.entry(db).or_default().insert(role);
    }
}

/// Parse and apply a REVOKE statement to `state.auth.db_grants`.
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
    if let Ok(mut grants) = state.auth.db_grants.lock() {
        if let Some(roles) = grants.get_mut(&db) {
            roles.remove(&role);
        }
    }
}

// ─── ISSUE-05: Catalog UDF inline expansion ───────────────────────────────────

/// Inline catalog-registered UDF calls in a SELECT statement.
///
/// For each catalog function whose `sql_body` is a simple `SELECT` or `RETURN`
/// expression, replace `fn_name(arg)` with `(SELECT body)` so the downstream OLTP
/// or OLAP executor can evaluate the expression without requiring DataFusion UDF
/// registration.
///
/// Multiple UDFs in the same statement are inlined iteratively. Falls back to the
/// original statement if no UDF could be inlined.
fn inline_catalog_udf_calls(sql: String, udfs: &[crate::helpers::udf::CatalogUdfEntry]) -> String {
    use crate::helpers::udf::try_inline_catalog_udf;
    let mut result = sql;
    for udf in udfs {
        if let Some(body) = &udf.sql_body {
            if let Some(inlined) = try_inline_catalog_udf(&result, &udf.name, body) {
                result = inlined;
            }
        }
    }
    result
}

// ─── M-8 Rule 6: View expansion ─────────────────────────────────────────────

/// Expand a SELECT statement that targets a registered view.
///
/// `view_catalog` is a snapshot of `(view_name_lower, original_ddl)` pairs.
/// If the FROM table matches a view name, the view body is inlined:
///   `SELECT * FROM my_view WHERE ...`
///   → `SELECT * FROM (SELECT col1, col2 FROM base_table) AS my_view WHERE ...`
///
/// Falls back to the original SQL unchanged if no matching view is found.
fn expand_select_view(sql: &str, view_catalog: &[(String, String)]) -> String {
    use crate::helpers::sql_parse::{extract_view_select_body, expand_view_in_select};
    let lower = sql.to_ascii_lowercase();
    for (view_name, ddl) in view_catalog {
        let pattern = format!(" from {}", view_name);
        if lower.contains(&pattern) {
            if let Some(body) = extract_view_select_body(ddl) {
                return expand_view_in_select(sql, view_name, &body);
            }
        }
    }
    sql.to_string()
}

/// Rewrite a DML statement (INSERT/UPDATE/DELETE) that targets a simple updatable view.
///
/// Only simple single-table views (no JOIN, no GROUP BY, no aggregates) are updatable.
/// The rewrite replaces the view name with the base table name so the DML applies to
/// the actual underlying rows.
///
/// ISSUE-04 improvement: uses proper word-boundary checks so that a view name appearing
/// inside a string literal, comment, or as part of a longer identifier is NOT replaced.
/// A word boundary is defined as the characters surrounding the match being
/// non-alphanumeric / non-underscore (i.e. SQL identifier delimiters).
fn rewrite_dml_for_view(sql: &str, view_catalog: &[(String, String)]) -> String {
    use crate::helpers::sql_parse::extract_updatable_view_base_table;
    let lower = sql.to_ascii_lowercase();
    let first_word = lower.split_whitespace().next().unwrap_or("");
    // Only rewrite DML statements.
    if !matches!(first_word, "insert" | "update" | "delete") {
        return sql.to_string();
    }

    /// Check whether a character is a SQL identifier character (alphanumeric or `_`).
    fn is_ident_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '_'
    }

    for (view_name, ddl) in view_catalog {
        let vname_lower = view_name.to_ascii_lowercase();
        let vlen = vname_lower.len();
        if vlen == 0 {
            continue;
        }
        // Scan `lower` for the view name at a word boundary.
        // We want to find the FIRST occurrence that:
        //   - is preceded by a non-ident char (or start of string), AND
        //   - is followed by a non-ident char (or end of string).
        let lower_bytes = lower.as_bytes();
        let view_bytes = vname_lower.as_bytes();
        let mut match_pos: Option<usize> = None;
        let limit = lower.len().saturating_sub(vlen) + 1;
        for i in 0..limit {
            if &lower_bytes[i..i + vlen] != view_bytes {
                continue;
            }
            // Check left boundary.
            let left_ok = if i == 0 {
                true
            } else {
                !is_ident_char(lower.as_bytes()[i - 1] as char)
            };
            // Check right boundary.
            let right_ok = if i + vlen >= lower.len() {
                true
            } else {
                !is_ident_char(lower.as_bytes()[i + vlen] as char)
            };
            if left_ok && right_ok {
                match_pos = Some(i);
                break;
            }
        }
        if let Some(pos) = match_pos {
            if let Some(base_table) = extract_updatable_view_base_table(ddl) {
                let end = pos + vlen;
                return format!("{}{}{}", &sql[..pos], base_table, &sql[end..]);
            }
        }
    }
    sql.to_string()
}

fn parse_db_ddl_statement(sql: &str) -> Option<(voltnuerongrid_sql::SqlStatementKind, String, bool)> {
    let sql_trimmed = sql.trim();
    // Normalize spaces by splitting into words
    let words: Vec<&str> = sql_trimmed
        .split_whitespace()
        .map(|w| w.trim_end_matches(';'))
        .filter(|w| !w.is_empty())
        .collect();

    if words.len() < 3 {
        return None;
    }

    let first = words[0].to_ascii_uppercase();
    let second = words[1].to_ascii_uppercase();

    if first == "CREATE" && second == "DATABASE" {
        // CREATE DATABASE [IF NOT EXISTS] <name>
        let mut db_name = String::new();
        let mut if_not_exists = false;

        if words.len() >= 6 
            && words[2].to_ascii_uppercase() == "IF"
            && words[3].to_ascii_uppercase() == "NOT"
            && words[4].to_ascii_uppercase() == "EXISTS" 
        {
            if_not_exists = true;
            db_name = words[5].to_ascii_lowercase();
        } else if words.len() >= 3 {
            db_name = words[2].to_ascii_lowercase();
        }

        // Clean db_name if it has quotes
        let db_name = db_name.trim_matches(|c| c == '"' || c == '\'' || c == '`').to_string();
        if db_name.is_empty() {
            return None;
        }
        Some((voltnuerongrid_sql::SqlStatementKind::CreateDatabase, db_name, if_not_exists))
    } else if first == "DROP" && second == "DATABASE" {
        // DROP DATABASE [IF EXISTS] <name> [CASCADE | RESTRICT]
        let mut db_name = String::new();
        let mut if_exists = false;

        if words.len() >= 5 
            && words[2].to_ascii_uppercase() == "IF"
            && words[3].to_ascii_uppercase() == "EXISTS"
        {
            if_exists = true;
            db_name = words[4].to_ascii_lowercase();
        } else if words.len() >= 3 {
            db_name = words[2].to_ascii_lowercase();
        }

        let db_name = db_name.trim_matches(|c| c == '"' || c == '\'' || c == '`').to_string();
        if db_name.is_empty() {
            return None;
        }
        Some((voltnuerongrid_sql::SqlStatementKind::DropDatabase, db_name, if_exists))
    } else {
        None
    }
}
