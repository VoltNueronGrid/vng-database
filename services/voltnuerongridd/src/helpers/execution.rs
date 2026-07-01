//! OLAP/OLTP execution helpers, transaction executor, pessimistic locking.
use std::collections::{HashMap, HashSet};
use voltnuerongrid_store::index::IndexManager;
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
#[allow(dead_code)]
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
            freshness_lag_ms: None,
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
    // B-3: incremental wait-for graph traversal. Each transaction waits on at
    // most one resource (`wait_graph: tx -> resource`), so following the chain
    // from `holder_tx` is O(hops) — we visit only the wait-for edges reachable
    // from the requesting transaction, never the whole lock table. The hop
    // budget is configurable via `VNG_DEADLOCK_SCAN_MAX_HOPS` (default
    // `DEADLOCK_SCAN_MAX_HOPS`).
    let max_hops = deadlock_scan_max_hops();
    let mut visited_txs = HashSet::new();
    let mut current_holder = holder_tx;

    for _ in 0..max_hops {
        // Revisiting a transaction means the wait-for chain reachable from the
        // requester contains a cycle. Even when that cycle does not pass through
        // `waiting_tx` itself, the requester is parked behind a set of
        // transactions that can never make progress — i.e. a deadlock for the
        // requester. Report it as a cycle so the caller aborts instead of
        // blocking forever.
        if !visited_txs.insert(current_holder.to_string()) {
            return DeadlockScanOutcome::CycleDetected;
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

/// B-3: Effective deadlock scan hop budget. Reads `VNG_DEADLOCK_SCAN_MAX_HOPS`
/// at call time (so it can be tuned without a restart in tests), falling back to
/// the compile-time `DEADLOCK_SCAN_MAX_HOPS` default when unset or invalid.
pub(crate) fn deadlock_scan_max_hops() -> usize {
    std::env::var("VNG_DEADLOCK_SCAN_MAX_HOPS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEADLOCK_SCAN_MAX_HOPS)
}


pub(crate) fn cleanup_wait_edges_for_resource(
    wait_graph: &mut HashMap<String, String>,
    resource_key: &str,
) {
    wait_graph.retain(|_, waiting_resource| waiting_resource != resource_key);
}

/// B-2: Optimistic-locking version check. For an optimistic transaction that
/// captured `begin_snapshot_xid` at BEGIN, return the first written key that a
/// concurrent transaction modified *after* that snapshot (i.e. a stale-version
/// write). Returns `None` when every write is conflict-free.
///
/// This holds no row locks — it is a pure read of the MVCC version chain at
/// COMMIT time, which is the defining property of optimistic concurrency
/// control (validate-on-commit rather than lock-on-write).
pub(crate) fn optimistic_version_conflict(
    rs: &voltnuerongrid_store::mvcc::PagedRowStore,
    written_keys: &HashSet<String>,
    begin_snapshot_xid: u64,
) -> Option<String> {
    for key in written_keys {
        if rs.was_modified_after(key, begin_snapshot_xid) {
            return Some(key.clone());
        }
    }
    None
}


/// Owned-argument wrapper so the returned future is `'static` (required by
/// `run_async_in_executor` when it needs to cross a thread boundary).
///
/// When `data_dir` is non-empty the query engine prefers on-disk Parquet files
/// over the supplied in-memory `table_rows` for tables that already have a
/// flushed Parquet snapshot (`{data_dir}/parquet/_default/{table}.parquet`).
pub(crate) async fn df_select_owned(
    sql: String,
    table_rows: HashMap<String, Vec<(String, voltnuerongrid_store::mvcc::RowData)>>,
    max_rows: usize,
    data_dir: String,
) -> Result<voltnuerongrid_exec_datafusion::SelectOutput, voltnuerongrid_exec_datafusion::ExecError> {
    voltnuerongrid_exec_datafusion::datafusion::execute_select_prefer_parquet(
        &sql, table_rows, max_rows, &data_dir,
    ).await
}

/// Execute an OLAP SELECT query through the DataFusion engine.
///
/// Extracts all referenced table names, builds per-table row snapshots, then
/// drives the DataFusion executor.  When `data_dir` is non-empty the executor
/// prefers on-disk Parquet snapshots over the in-memory rows — see
/// [`execute_select_prefer_parquet`] for the fallback logic.  Pass `data_dir = ""`
/// to always use in-memory rows (useful in tests / single-node dev).
///
/// Falls back to a stub count on errors so callers never see a hard failure
/// from the OLAP path.
pub(crate) fn execute_olap_query(
    query: String,
    max_rows: Option<usize>,
    rs: &voltnuerongrid_store::mvcc::PagedRowStore,
    db: &str,
    data_dir: &str,
    // C-3: Optional repeatable-read snapshot Xid.  When `Some`, reads return
    // row-store state as of that Xid rather than the current head.
    snapshot_xid: Option<u64>,
    // C-1: When Some, these RocksDB-sourced rows are used as the primary read
    // source instead of the in-memory PagedRowStore scan.  Keys are raw row_keys
    // WITHOUT the db prefix (e.g. "orders:row-1").
    rocksdb_rows: Option<Vec<(String, HashMap<String, String>)>>,
) -> OlapQueryResponse {
    use voltnuerongrid_exec_datafusion::{collect_query_table_names, SelectOutput};
    use crate::helpers::sql_parse::make_table_scan_prefix;

    let started = Instant::now();
    let resolved_max_rows = max_rows.unwrap_or(1_000).min(100_000);

    let table_names = collect_query_table_names(&query);
    // C-1: prefer RocksDB rows when available (primary read path); fall back to
    // in-memory PagedRowStore scan for dev/test environments without RocksDB.
    // Q-2: emit an observable signal (warn span event + data_source tag) so the
    // fallback is never silent.
    let data_source: &str;
    let all_rows: Vec<(String, voltnuerongrid_store::mvcc::RowData)> = if let Some(rdb_rows) = rocksdb_rows {
        data_source = "rocksdb";
        // RocksDB rows have no db prefix — convert HashMap to RowData directly.
        rdb_rows.into_iter().map(|(k, cols)| (k, cols)).collect()
    } else {
        data_source = "paged_store";
        tracing::warn!(
            target: "vng.olap",
            db = %db,
            query_signature = %query.chars().take(64).collect::<String>(),
            "Q-2: OLAP query falling back to in-memory PagedRowStore (RocksDB rows unavailable); results are not durably sourced"
        );
        // In-memory fallback: use repeatable-read snapshot or current head.
        let effective_xid = snapshot_xid.unwrap_or_else(|| rs.current_xid());
        rs.scan_at_snapshot(effective_xid)
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    };
    let mut table_rows: HashMap<String, Vec<(String, voltnuerongrid_store::mvcc::RowData)>> =
        HashMap::new();
    for name in &table_names {
        let prefix = make_table_scan_prefix(db, name);
        let unqualified = name.as_str();
        let filtered: Vec<_> = all_rows
            .iter()
            .filter(|(k, _)| *k == unqualified || k.starts_with(&prefix))
            // Strip the db prefix from the key before passing to DataFusion so
            // table resolution inside the executor still matches the plain table name.
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
        // No tables recognised — register all rows under an implicit table.
        table_rows.insert("rows".to_string(), all_rows);
    }

    let row_count = match run_async_in_executor(df_select_owned(
        query.clone(),
        table_rows,
        resolved_max_rows,
        data_dir.to_string(),
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
        data_source: data_source.to_string(),
    }
}


/// S4-WS3-02: physical OLTP executor — runs point SELECT queries against `PagedRowStore`.
/// Extracts an optional key/prefix constraint from the WHERE clause and filters visible rows.
/// `db` scopes the scan to rows stored under the given database prefix (empty = no scoping).
/// `snapshot_xid`: when `Some`, enforces repeatable-read semantics by reading at that Xid.
/// `rocksdb_rows`: C-1 — when `Some`, used as primary row source for DataFusion path instead
/// of in-memory PagedRowStore scan.  Keys are raw row_keys WITHOUT the db prefix.
pub(crate) fn execute_oltp_select(
    statements: &[String],
    rs: &voltnuerongrid_store::mvcc::PagedRowStore,
    limit: usize,
    db: &str,
    snapshot_xid: Option<u64>,
    rocksdb_rows: Option<Vec<(String, HashMap<String, String>)>>,
    index_manager: Option<&IndexManager>,
) -> Vec<OltpRowResult> {
    use voltnuerongrid_exec_datafusion::{execute_select, SelectOutput, ExecError};
    use voltnuerongrid_sql::{parse_one, Statement};
    use crate::helpers::sql_parse::make_table_scan_prefix;

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

            // C-1: prefer RocksDB rows when available; fall back to in-memory scan.
            // Clone rocksdb_rows so we can use it across loop iterations.
            let all_rows: Vec<(String, voltnuerongrid_store::mvcc::RowData)> = if let Some(ref rdb) = rocksdb_rows {
                rdb.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            } else {
                let eff_xid = snapshot_xid.unwrap_or_else(|| rs.current_xid());
                rs.scan_at_snapshot(eff_xid)
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect()
            };
            let mut table_rows: std::collections::HashMap<String, Vec<(String, voltnuerongrid_store::mvcc::RowData)>> =
                std::collections::HashMap::new();
            for name in &table_names {
                let prefix = make_table_scan_prefix(db, name);
                let filtered: Vec<_> = all_rows
                    .iter()
                    .filter(|(k, _)| k == name || k.starts_with(&prefix))
                    // Strip db prefix so DataFusion table resolution works on plain name.
                    .map(|(k, v)| {
                        let stripped = if db.is_empty() { k.clone() } else {
                            k.strip_prefix(&format!("{db}.")).unwrap_or(k).to_string()
                        };
                        (stripped, v.clone())
                    })
                    .collect();
                table_rows.insert(name.clone(), filtered);
            }

            let df_result = run_async_in_executor(
                df_select_owned(stmt_str.to_string(), table_rows, remaining, String::new())
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

        // C-1: Phase 1.7 reads from PagedRowStore directly.  When RocksDB is the
        // primary durability engine (rocksdb_rows.is_some()), PagedRowStore may be
        // empty after restart because we skip boot-time replay.  Skip Phase 1.7 in
        // that case and fall through to the legacy path which already correctly
        // uses rocksdb_rows as its row source (see execute_oltp_select_legacy).
        if rocksdb_rows.is_some() {
            // RocksDB is primary — go directly to legacy which uses rocksdb_rows.
            execute_oltp_select_legacy(stmt_str, rs, limit, &mut results, db, snapshot_xid, rocksdb_rows.as_deref(), index_manager);
            continue;
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

        // Legacy substring fallback path — pass through snapshot and RocksDB rows if set.
        execute_oltp_select_legacy(stmt_str, rs, limit, &mut results, db, snapshot_xid, rocksdb_rows.as_deref(), index_manager);
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


/// Legacy executor. Kept as a fallback for queries the new executor doesn't
/// support yet (JOIN / GROUP BY / subquery). To be deleted once the new
/// executor covers those features.
///
/// WHERE filtering uses exact column-value match only; unknown columns never
/// produce false-positive matches. The new path is preferred whenever it can
/// handle the query.
pub(crate) fn execute_oltp_select_legacy(
    stmt_str: &str,
    rs: &voltnuerongrid_store::mvcc::PagedRowStore,
    limit: usize,
    results: &mut Vec<OltpRowResult>,
    db: &str,
    override_snapshot_xid: Option<u64>,
    // C-1: When Some, used as primary row source; keys are raw row_keys WITHOUT db prefix.
    rocksdb_rows: Option<&[(String, HashMap<String, String>)]>,
    // H-2: When Some, consulted for index-accelerated lookups on WHERE col = 'val'.
    index_manager: Option<&IndexManager>,
) {
    use voltnuerongrid_sql::{parse_one, Statement};
    // C-1: When rocksdb_rows is Some (RocksDB engine), all_rows comes from RocksDB
    // and PagedRowStore is the write-buffer only. When rocksdb_rows is None (in-memory
    // engine), all_rows comes from PagedRowStore as before.
    let all_rows: Vec<(String, voltnuerongrid_store::mvcc::RowData)> = if let Some(rdb) = rocksdb_rows {
        // RocksDB rows are already db-scoped and have no db prefix.
        rdb.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    } else {
        let snapshot_xid = override_snapshot_xid.unwrap_or_else(|| rs.current_xid());
        rs.scan_at_snapshot(snapshot_xid)
            .into_iter()
            // Only include rows that belong to this database scope.
            .filter(|(k, _)| {
                if db.is_empty() {
                    // No db scope — include all rows (backward compat).
                    true
                } else {
                    k.starts_with(&format!("{db}."))
                }
            })
            // Strip db prefix so the rest of the scan logic works on plain `"table:value"` keys.
            .map(|(k, d)| {
                let stripped = if db.is_empty() { k.to_string() } else {
                    k.strip_prefix(&format!("{db}.")).unwrap_or(&k).to_string()
                };
                (stripped, d.clone())
            })
            .collect()
    };
    if let Ok(Statement::Select(sel)) = parse_one(stmt_str) {
        let sql_limit: usize = sel
            .limit
            .map(|l| l as usize)
            .unwrap_or(limit)
            .min(limit);

        // M-5: Parse all AND-separated predicates from the WHERE clause.
        // Split on " AND " (case-insensitive) and parse each piece as `col = 'val'`.
        let where_filters: Vec<(String, String)> = sel.where_clause.as_deref()
            .map(|w| {
                // Split on AND (case-insensitive) by uppercasing the text first.
                let upper = w.to_ascii_uppercase();
                let parts: Vec<&str> = {
                    let mut result = Vec::new();
                    let mut last = 0usize;
                    let and_token = " AND ";
                    let mut search = upper.as_str();
                    let mut offset = 0usize;
                    while let Some(pos) = search.find(and_token) {
                        result.push(&w[last..last + pos]);
                        last += pos + and_token.len();
                        offset += pos + and_token.len();
                        search = &upper[offset..];
                    }
                    result.push(&w[last..]);
                    result
                };
                parts.into_iter().filter_map(|piece| {
                    let eq = piece.find('=')?;
                    let lhs = piece[..eq].trim().to_ascii_lowercase();
                    let rhs = piece[eq + 1..].trim();
                    let val = rhs.trim_matches('\'').trim_matches('"').trim();
                    if lhs.is_empty() || val.is_empty() {
                        None
                    } else {
                        Some((lhs, val.to_string()))
                    }
                }).collect()
            })
            .unwrap_or_default();

        // Determine the table name from the parsed AST (for index lookup).
        let table_name_str: Option<String> = sel.table.clone();

        // Determine whether this is SELECT * or a specific column list.
        let select_star = sel.columns.is_empty()
            || sel.columns.iter().any(|c| c == "*");

        let remaining = sql_limit.saturating_sub(results.len());

        // H-2: Index-accelerated single-equality-predicate path.
        if let (Some(mgr), Some(table_name)) = (index_manager, table_name_str.as_deref()) {
            if where_filters.len() == 1 {
                let (ref col, ref val) = where_filters[0];
                let matching_desc = mgr.list_indexes().into_iter().find(|d| {
                    d.table == table_name && &d.column == col
                });
                if let Some(desc) = matching_desc {
                    if let Some(idx) = mgr.get(&desc.name) {
                        let row_keys: std::collections::HashSet<&str> =
                            idx.lookup(val).into_iter().collect();
                        let batch: Vec<OltpRowResult> = all_rows
                            .iter()
                            .filter(|(k, _)| row_keys.contains(k.as_str()))
                            .take(remaining)
                            .map(|(k, d)| OltpRowResult { key: k.clone(), data: d.clone() })
                            .collect();
                        // M-5: apply column projection.
                        let projected = apply_projection(batch, &sel.columns, select_star);
                        results.extend(projected);
                        return; // skip the full scan below
                    }
                }
            }
        }

        // Full scan with multi-predicate AND filter.
        let batch: Vec<OltpRowResult> = all_rows
            .iter()
            .filter(|(_, d)| {
                if where_filters.is_empty() {
                    true
                } else {
                    where_filters.iter().all(|(col, val)| {
                        d.get(col.as_str())
                            .map(|v| v.eq_ignore_ascii_case(val))
                            .unwrap_or(false)
                    })
                }
            })
            .take(remaining)
            .map(|(k, d)| OltpRowResult { key: k.clone(), data: d.clone() })
            .collect();

        // M-5: apply column projection.
        let projected = apply_projection(batch, &sel.columns, select_star);
        results.extend(projected);
    }
}

/// M-5: Apply column projection to a batch of rows.
/// When `select_star` is true, returns rows unchanged.
/// Otherwise filters each row's data to only the requested columns.
fn apply_projection(
    batch: Vec<OltpRowResult>,
    columns: &[String],
    select_star: bool,
) -> Vec<OltpRowResult> {
    if select_star {
        return batch;
    }
    batch.into_iter().map(|r| {
        let projected_data: HashMap<String, String> = columns.iter()
            .filter_map(|col| {
                if col == "*" { return None; }
                r.data.get(col.as_str()).map(|v| (col.clone(), v.clone()))
            })
            .collect();
        OltpRowResult { key: r.key, data: projected_data }
    }).collect()
}

