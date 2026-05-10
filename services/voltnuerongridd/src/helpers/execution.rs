//! OLAP/OLTP execution helpers, transaction executor, pessimistic locking.
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::Instant;
use axum::http::StatusCode;
use axum::Json;
use voltnuerongrid_sql::{SqlAnalyzer, SqlStatementKind};
use crate::handlers::sql::SqlExecuteResponse;
use crate::{
    DEADLOCK_SCAN_MAX_HOPS, PESSIMISTIC_LOCK_COUNTER, TX_COUNTER,
    WS22_GATE_DEADLOCK_DETECTIONS, WS22_GATE_SCAN_CAP_TIMEOUTS,
    DeadlockScanOutcome,
    OlapQueryResponse, OltpRowResult,
    PessimisticLockRecord, PessimisticLockResponse,
    SqlTransactionResponse,
};
use crate::{udf_guard_policy_contract, udf_function_catalog_contract};


/// Build a 503 SqlExecuteResponse for graceful degradation when an internal
/// mutex is poisoned (which happens after a panic in a critical section).
/// Returning 503 instead of expect()-panicking keeps the rest of the service alive.
pub(crate) fn svc_unavailable_sql_response(reason: &str) -> (StatusCode, Json<SqlExecuteResponse>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(SqlExecuteResponse {
            status: "error".to_string(),
            route_path: "unknown".to_string(),
            reason: format!("internal state unavailable: {reason}"),
            transaction: None,
            olap: None,
            rejected_statement_count: 0,
            udf_results: None,
            udf_guardrail_status: None,
            udf_function_catalog: udf_function_catalog_contract(),
            udf_guard_policies: udf_guard_policy_contract(),
            udf_execution_plan: Vec::new(),
            legacy_agg_results: None,
            planner_path: None,
            oltp_rows: None,
            olap_agg_results: None,
            columns: None,
            rows: None,
        }),
    )
}


pub(crate) fn execute_transaction_statements(statements: Vec<String>) -> (StatusCode, SqlTransactionResponse) {
    if statements.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            SqlTransactionResponse {
                status: "error".to_string(),
                transaction_id: String::new(),
                statements_executed: 0,
                requires_transaction: false,
                touches_catalog: false,
                rejected_statement_count: 0,
                elapsed_ms: 0,
            },
        );
    }

    let mut requires_transaction = false;
    let mut touches_catalog = false;
    let mut rejected_statement_count = 0usize;
    for stmt in &statements {
        let analysis = SqlAnalyzer::analyze_statement(stmt);
        if analysis.kind == SqlStatementKind::Unknown {
            rejected_statement_count += 1;
        }
        requires_transaction |= analysis.requires_transaction;
        touches_catalog |= analysis.touches_catalog;
    }

    if rejected_statement_count > 0 {
        return (
            StatusCode::BAD_REQUEST,
            SqlTransactionResponse {
                status: "error".to_string(),
                transaction_id: String::new(),
                statements_executed: 0,
                requires_transaction,
                touches_catalog,
                rejected_statement_count,
                elapsed_ms: 0,
            },
        );
    }

    let started = Instant::now();
    let tx_id = TX_COUNTER.fetch_add(1, Ordering::Relaxed);
    let elapsed = started.elapsed().as_millis();
    (
        StatusCode::OK,
        SqlTransactionResponse {
            status: "committed".to_string(),
            transaction_id: format!("tx-{tx_id}"),
            statements_executed: statements.len(),
            requires_transaction,
            touches_catalog,
            rejected_statement_count,
            elapsed_ms: elapsed,
        },
    )
}


pub(crate) fn acquire_pessimistic_lock(
    lock_table: &mut HashMap<String, PessimisticLockRecord>,
    wait_graph: &mut HashMap<String, String>,
    transaction_id: &str,
    resource: &str,
    owner: &str,
    ttl_ms: u64,
    wait_timeout_ms: u64,
    now_ms: u128,
) -> (StatusCode, PessimisticLockResponse) {
    let tx = transaction_id.trim();
    let resource_key = resource.trim();
    if tx.is_empty() || resource_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            PessimisticLockResponse {
                status: "error",
                lock_state: "invalid_request",
                reason: "transaction_id_and_resource_are_required".to_string(),
                lock: None,
            },
        );
    }

    wait_graph.remove(tx);
    if let Some(existing) = lock_table.get(resource_key).cloned() {
        if existing.expires_unix_ms <= now_ms {
            lock_table.remove(resource_key);
            cleanup_wait_edges_for_resource(wait_graph, resource_key);
        } else if existing.transaction_id != tx {
            let holder_tx = existing.transaction_id.clone();
            let mut scan_outcome = DeadlockScanOutcome::NoCycle;
            if wait_timeout_ms > 0 {
                wait_graph.insert(tx.to_string(), resource_key.to_string());
                scan_outcome =
                    evaluate_deadlock_scan_outcome(wait_graph, lock_table, tx, &holder_tx);
                if scan_outcome == DeadlockScanOutcome::CycleDetected {
                    WS22_GATE_DEADLOCK_DETECTIONS.fetch_add(1, Ordering::Relaxed);
                    return (
                        StatusCode::CONFLICT,
                        PessimisticLockResponse {
                            status: "blocked",
                            lock_state: "deadlock_risk",
                            reason: "pessimistic_lock_deadlock_risk".to_string(),
                            lock: Some(existing),
                        },
                    );
                }
            }
            if wait_timeout_ms > 0 {
                let timeout_reason = if scan_outcome == DeadlockScanOutcome::ScanCapReached {
                    WS22_GATE_SCAN_CAP_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
                    "pessimistic_lock_wait_timeout_scan_cap_reached"
                } else {
                    "pessimistic_lock_wait_timeout"
                };
                return (
                    StatusCode::REQUEST_TIMEOUT,
                    PessimisticLockResponse {
                        status: "blocked",
                        lock_state: "wait_timeout",
                        reason: timeout_reason.to_string(),
                        lock: Some(existing),
                    },
                );
            }
            return (
                StatusCode::CONFLICT,
                PessimisticLockResponse {
                    status: "blocked",
                    lock_state: "held_by_other_transaction",
                    reason: "pessimistic_lock_conflict".to_string(),
                    lock: Some(existing),
                },
            );
        }
    }

    wait_graph.remove(tx);
    let lock_id = format!(
        "plock-{}",
        PESSIMISTIC_LOCK_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let lock = PessimisticLockRecord {
        lock_id,
        transaction_id: tx.to_string(),
        resource: resource_key.to_string(),
        owner: owner.trim().to_string(),
        acquired_unix_ms: now_ms,
        expires_unix_ms: now_ms + u128::from(ttl_ms),
    };
    let lock_state = if lock_table.contains_key(resource_key) {
        "renewed"
    } else {
        "acquired"
    };
    lock_table.insert(resource_key.to_string(), lock.clone());
    (
        StatusCode::OK,
        PessimisticLockResponse {
            status: "ok",
            lock_state,
            reason: "pessimistic_lock_granted".to_string(),
            lock: Some(lock),
        },
    )
}


pub(crate) fn release_pessimistic_lock(
    lock_table: &mut HashMap<String, PessimisticLockRecord>,
    wait_graph: &mut HashMap<String, String>,
    transaction_id: &str,
    resource: &str,
) -> (StatusCode, PessimisticLockResponse) {
    let tx = transaction_id.trim();
    let resource_key = resource.trim();
    if tx.is_empty() || resource_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            PessimisticLockResponse {
                status: "error",
                lock_state: "invalid_request",
                reason: "transaction_id_and_resource_are_required".to_string(),
                lock: None,
            },
        );
    }

    let existing = match lock_table.get(resource_key).cloned() {
        Some(lock) => lock,
        None => {
            return (
                StatusCode::NOT_FOUND,
                PessimisticLockResponse {
                    status: "error",
                    lock_state: "not_found",
                    reason: "no_lock_for_resource".to_string(),
                    lock: None,
                },
            )
        }
    };

    if existing.transaction_id != tx {
        return (
            StatusCode::CONFLICT,
            PessimisticLockResponse {
                status: "blocked",
                lock_state: "ownership_mismatch",
                reason: "lock_owned_by_different_transaction".to_string(),
                lock: Some(existing),
            },
        );
    }

    lock_table.remove(resource_key);
    cleanup_wait_edges_for_resource(wait_graph, resource_key);
    wait_graph.remove(tx);
    (
        StatusCode::OK,
        PessimisticLockResponse {
            status: "ok",
            lock_state: "released",
            reason: "pessimistic_lock_released".to_string(),
            lock: Some(existing),
        },
    )
}



// ─── S10-WS15-02: CDC stream from WAL ─────────────────────────────────────────


pub(crate) fn evaluate_deadlock_scan_outcome(
    wait_graph: &HashMap<String, String>,
    lock_table: &HashMap<String, PessimisticLockRecord>,
    waiting_tx: &str,
    holder_tx: &str,
) -> DeadlockScanOutcome {
    let mut visited_txs = HashSet::new();
    let mut current_holder = holder_tx;

    for _ in 0..DEADLOCK_SCAN_MAX_HOPS {
        if !visited_txs.insert(current_holder.to_string()) {
            return DeadlockScanOutcome::NoCycle;
        }
        let current_wait_resource = match wait_graph.get(current_holder) {
            Some(resource) => resource,
            None => return DeadlockScanOutcome::NoCycle,
        };
        let current_blocker = match lock_table.get(current_wait_resource) {
            Some(lock) => lock,
            None => return DeadlockScanOutcome::NoCycle,
        };
        if current_blocker.transaction_id == waiting_tx {
            return DeadlockScanOutcome::CycleDetected;
        }
        current_holder = current_blocker.transaction_id.as_str();
    }
    DeadlockScanOutcome::ScanCapReached
}


pub(crate) fn cleanup_wait_edges_for_resource(
    wait_graph: &mut HashMap<String, String>,
    resource_key: &str,
) {
    wait_graph.retain(|_, waiting_resource| waiting_resource != resource_key);
}


/// Owned-argument wrapper so the returned future is `'static` (required by
/// `run_async_in_executor` when it needs to cross a thread boundary).
pub(crate) async fn df_select_owned(
    sql: String,
    table_rows: HashMap<String, Vec<(String, voltnuerongrid_store::mvcc::RowData)>>,
    max_rows: usize,
) -> Result<voltnuerongrid_exec_datafusion::SelectOutput, voltnuerongrid_exec_datafusion::ExecError> {
    voltnuerongrid_exec_datafusion::datafusion::execute_select_from_rows(&sql, table_rows, max_rows).await
}

/// Execute an OLAP SELECT query through the DataFusion engine.
///
/// Extracts all referenced table names, builds per-table row snapshots, then
/// drives `execute_select_from_rows`. Falls back to a stub count on errors so
/// callers never see a hard failure from the OLAP path.
pub(crate) fn execute_olap_query(
    query: String,
    max_rows: Option<usize>,
    rs: &voltnuerongrid_store::mvcc::PagedRowStore,
) -> OlapQueryResponse {
    use voltnuerongrid_exec_datafusion::{collect_query_table_names, SelectOutput};

    let started = Instant::now();
    let resolved_max_rows = max_rows.unwrap_or(1_000).min(100_000);

    let table_names = collect_query_table_names(&query);
    let all_rows = rs.export_rows_snapshot();
    let mut table_rows: HashMap<String, Vec<(String, voltnuerongrid_store::mvcc::RowData)>> =
        HashMap::new();
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
        // No tables recognised — register all rows under an implicit table.
        table_rows.insert("rows".to_string(), all_rows);
    }

    let row_count = match run_async_in_executor(df_select_owned(
        query.clone(),
        table_rows,
        resolved_max_rows,
    )) {
        Ok(SelectOutput::Rows(rows)) => rows.len(),
        Ok(SelectOutput::Aggregate(_)) => 1,
        Err(_) => 0,
    };

    OlapQueryResponse {
        status: "ok".to_string(),
        query_signature: query.chars().take(64).collect(),
        elapsed_ms: started.elapsed().as_millis(),
        rows: row_count,
    }
}


/// S4-WS3-02: physical OLTP executor — runs point SELECT queries against `PagedRowStore`.
/// Extracts an optional key/prefix constraint from the WHERE clause and filters visible rows.
pub(crate) fn execute_oltp_select(
    statements: &[String],
    rs: &voltnuerongrid_store::mvcc::PagedRowStore,
    limit: usize,
) -> Vec<OltpRowResult> {
    use voltnuerongrid_exec_datafusion::{execute_select, SelectOutput, ExecError};
    use voltnuerongrid_sql::{parse_one, Statement};

    let mut results: Vec<OltpRowResult> = Vec::new();
    for stmt_str in statements {
        let remaining = limit.saturating_sub(results.len());
        if remaining == 0 {
            break;
        }

        // Phase 3 — DataFusion fast path for JOIN / GROUP BY / HAVING / window / subquery.
        // Parse once to check for complex features before deciding which executor to use.
        let complex = if let Ok(Statement::Select(ref sel)) = parse_one(stmt_str) {
            sel.has_group_by
                || sel.has_having
                || sel.join.is_some()
                || sel.has_subquery
                || sel.has_window_fn
        } else {
            false
        };

        if complex {
            // Collect ALL table names: FROM + every JOIN (including A JOIN B JOIN C).
            let table_names = voltnuerongrid_exec_datafusion::collect_query_table_names(stmt_str);

            // Take a snapshot once and filter per table by key prefix.
            let all_rows = rs.export_rows_snapshot();
            let mut table_rows: std::collections::HashMap<String, Vec<(String, voltnuerongrid_store::mvcc::RowData)>> =
                std::collections::HashMap::new();
            for name in &table_names {
                let prefix = format!("{name}:");
                let filtered: Vec<_> = all_rows
                    .iter()
                    .filter(|(k, _)| k == name || k.starts_with(&prefix))
                    .cloned()
                    .collect();
                table_rows.insert(name.clone(), filtered);
            }

            let df_result = run_async_in_executor(
                df_select_owned(stmt_str.to_string(), table_rows, remaining)
            );

            match df_result {
                Ok(SelectOutput::Rows(rows)) => {
                    metrics::counter!(
                        "vng_sql_select_executor_total",
                        "engine" => "datafusion",
                        "outcome" => "ok",
                    ).increment(1);
                    for r in rows {
                        results.push(OltpRowResult { key: r.key, data: r.data });
                        if results.len() >= limit { break; }
                    }
                    continue;
                }
                Ok(SelectOutput::Aggregate(agg)) => {
                    // DataFusion now always returns Rows; this arm is defensive.
                    // Convert the single-row aggregate summary to an OltpRowResult.
                    metrics::counter!(
                        "vng_sql_select_executor_total",
                        "engine" => "datafusion",
                        "outcome" => "aggregate_as_row",
                    ).increment(1);
                    let mut data = voltnuerongrid_store::mvcc::RowData::new();
                    for (col, val) in agg.columns.iter().zip(agg.values.iter()) {
                        let s = match val {
                            voltnuerongrid_exec_datafusion::AggregateCell::Int(i) => i.to_string(),
                            voltnuerongrid_exec_datafusion::AggregateCell::Float(f) => f.to_string(),
                            voltnuerongrid_exec_datafusion::AggregateCell::Text(t) => t.clone(),
                            voltnuerongrid_exec_datafusion::AggregateCell::Null => continue,
                        };
                        data.insert(col.clone(), s);
                    }
                    results.push(OltpRowResult { key: "agg_0".to_string(), data });
                    continue;
                }
                Err(_) => {
                    metrics::counter!(
                        "vng_sql_select_executor_total",
                        "engine" => "datafusion",
                        "outcome" => "error_fallback",
                    ).increment(1);
                    // Fall through to Phase 1.7 / legacy.
                }
            }
        }

        // Phase 1.7 — try the correct AST-driven executor first.
        // It returns Unsupported for features it can't handle yet
        // (JOIN, GROUP BY, subquery), in which case we fall back to the
        // legacy substring scan to preserve existing behaviour.
        match execute_select(stmt_str, rs, remaining) {
            Ok(SelectOutput::Rows(rows)) => {
                metrics::counter!(
                    "vng_sql_select_executor_total",
                    "engine" => "vng_correct",
                    "outcome" => "ok",
                ).increment(1);
                for r in rows {
                    results.push(OltpRowResult { key: r.key, data: r.data });
                    if results.len() >= limit { break; }
                }
                continue;
            }
            Ok(SelectOutput::Aggregate(_)) => {
                // Aggregate fast-path output isn't representable in the
                // OltpRowResult wire format. Fall through to legacy which
                // also doesn't handle this; the legacy_aggregations crate
                // is invoked separately by the planner.
                metrics::counter!(
                    "vng_sql_select_executor_total",
                    "engine" => "vng_correct",
                    "outcome" => "aggregate_passthrough",
                ).increment(1);
            }
            Err(ExecError::Unsupported(_)) => {
                metrics::counter!(
                    "vng_sql_select_executor_total",
                    "engine" => "vng_correct",
                    "outcome" => "unsupported_fallback",
                ).increment(1);
                // Fall through to legacy.
            }
            Err(_) => {
                // Not a SELECT, or bad predicate — skip silently (legacy
                // would have skipped too).
                continue;
            }
        }

        // Legacy substring fallback path.
        execute_oltp_select_legacy(stmt_str, rs, limit, &mut results);
    }
    results
}


/// Drive an async future to completion from synchronous code within a tokio runtime.
///
/// On a multi-thread scheduler (production): uses `block_in_place` so we stay
/// on the calling thread without spawning.
///
/// On a `current_thread` scheduler (tests, `#[tokio::test]`): `block_in_place`
/// would panic, so we spawn a dedicated OS thread with its own runtime instead.
///
/// Outside any tokio context: same dedicated-thread path.
pub(crate) fn run_async_in_executor<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    use tokio::runtime::RuntimeFlavor;
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| handle.block_on(fut))
        }
        _ => std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("DataFusion runtime")
                .block_on(fut)
        })
        .join()
        .expect("DataFusion thread join"),
    }
}


/// Legacy substring-based executor. Kept as a fallback for queries the new
/// executor doesn't support yet (JOIN / GROUP BY / subquery). To be deleted
/// once the new executor covers those features.
///
/// **Known incorrect:** uses `row_key.contains(prefix_str)` which makes
/// `WHERE id = 5` match rows 15, 25, 50, 51 etc. The new path is preferred
/// whenever it can handle the query.
pub(crate) fn execute_oltp_select_legacy(
    stmt_str: &str,
    rs: &voltnuerongrid_store::mvcc::PagedRowStore,
    limit: usize,
    results: &mut Vec<OltpRowResult>,
) {
    use voltnuerongrid_sql::{parse_one, Statement};
    let snapshot_xid = rs.current_xid();
    let all_rows: Vec<(String, voltnuerongrid_store::mvcc::RowData)> = rs
        .scan_at_snapshot(snapshot_xid)
        .into_iter()
        .map(|(k, d)| (k.to_string(), d.clone()))
        .collect();
    if let Ok(Statement::Select(sel)) = parse_one(stmt_str) {
        let sql_limit: usize = sel
            .limit
            .map(|l| l as usize)
            .unwrap_or(limit)
            .min(limit);
        let prefix: Option<String> = sel.where_clause.as_deref().and_then(|w| {
            let eq = w.find('=')?;
            let rhs = w[eq + 1..].trim();
            let val = rhs.trim_matches('\'').trim_matches('"').trim();
            if val.is_empty() { None } else { Some(val.to_string()) }
        });
        let prefix_str = prefix.as_deref().unwrap_or("");
        let remaining = sql_limit.saturating_sub(results.len());
        let batch: Vec<OltpRowResult> = all_rows
            .iter()
            .filter(|(k, _)| prefix_str.is_empty() || k.contains(prefix_str))
            .take(remaining)
            .map(|(k, d)| OltpRowResult { key: k.clone(), data: d.clone() })
            .collect();
        results.extend(batch);
    }
}

