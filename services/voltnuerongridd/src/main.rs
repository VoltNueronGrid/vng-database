use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
// AR-6: these axum re-exports are unused by the binary itself but are pulled
// into the `tests` module via `use super::*`; keep them under `allow` so the
// non-test build stays warning-free without breaking the test build.
#[allow(unused_imports)]
use axum::http::{HeaderMap, StatusCode};
#[allow(unused_imports)]
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use voltnuerongrid_auth::{
    ConfiguredKmsProviderAdapter, KmsKeyResolution,
    RbacPrivilegeMatrix, SecurityConfigContract,
};
// AR-6: used by the `tests` module via `use super::*`; not referenced by the
// binary itself.
#[allow(unused_imports)]
use voltnuerongrid_audit::AuditEventKind;
use voltnuerongrid_audit::{AppendOnlyAuditSink, AuditEvent};
use voltnuerongrid_ai::AutonomousActionExecutionRecord;
use voltnuerongrid_exec::{HtapQueryRouter, QueryPath};
use voltnuerongrid_sql::{SqlAnalyzer, SqlStatementKind};
use voltnuerongrid_store::htap_sync::{
    InMemoryReplicationTransport, ReplicaReplayState, RowStoreSyncOrigin,
};
use voltnuerongrid_store::constraints::ConstraintManager;
use voltnuerongrid_store::ddl_catalog::DdlCatalog;
use voltnuerongrid_store::index::IndexManager;
use voltnuerongrid_store::mvcc::PagedRowStore;
use voltnuerongrid_store::BoxedDurabilityEngine;
use voltnuerongrid_store::triggers::TriggerRegistry;
use voltnuerongrid_store::trigger_emitter::{LoggingTriggerEmitter, TriggerEmitter};
use voltnuerongrid_driver_rust::ConnectionPoolManager;
use voltnuerongrid_ingest::{
    ManagedEventBusTransport, ManagedReplayCursorStore,
};
use voltnuerongrid_opt::DistributedCacheManager;
use voltnuerongrid_plugins::PluginLifecycleManager;

pub(crate) mod raft;
use raft::RaftNode;
pub(crate) use raft::{RaftAppendRequest, RaftAppendResponse, RaftDurableState, RaftInstallSnapshotRequest, RaftInstallSnapshotResponse, RaftLogEntry, RaftRole, RaftStatusSnapshot, RaftVoteRequest, RaftVoteResponse};

pub mod resilience;
pub mod observability;
pub(crate) mod auth;
pub(crate) mod config_init;
pub(crate) mod audit_helpers;
pub(crate) mod handlers;
pub(crate) mod helpers;
pub(crate) mod router;
pub(crate) mod user_store;
// ── Glob imports — many are only consumed by tests.rs via `use super::*` ─────
// These MUST NOT be removed: tests.rs relies on them being in the crate root
// namespace so `use super::*` / `use crate::*` can resolve all handler and
// helper symbols without individual imports in every test.
// See CLAUDE.md § "Known Issues / Gotchas" for the full explanation.
#[allow(unused_imports)] use auth::*;
use config_init::*;
#[allow(unused_imports)] use audit_helpers::*;
#[allow(unused_imports)] use handlers::cdc::*;
#[allow(unused_imports)] use handlers::catalog::*;
use handlers::autonomous::*;
#[allow(unused_imports)] use handlers::security::*;
#[allow(unused_imports)] use handlers::admin::*;
#[allow(unused_imports)] use handlers::driver::*;
use handlers::ingest::*;
use handlers::sql::*;
use handlers::sre::*;
#[allow(unused_imports)] use handlers::store::*;
#[allow(unused_imports)] use handlers::wal::*;
#[allow(unused_imports)] use handlers::audit::*;
#[allow(unused_imports)] use handlers::rows::*;
#[allow(unused_imports)] use handlers::raft::*;
use handlers::misc::*;
#[allow(unused_imports)] use handlers::backup::*;
#[allow(unused_imports)] use handlers::udf::*;
#[allow(unused_imports)] use handlers::plugins::*;
use router::build_router;
#[allow(unused_imports)] use helpers::time::*;
#[allow(unused_imports)] use helpers::env_helpers::*;
#[allow(unused_imports)] use helpers::sql_parse::*;
use helpers::dr_hook::*;
#[allow(unused_imports)] use helpers::execution::*;
#[allow(unused_imports)] use helpers::udf::*;
use helpers::cluster::*;
#[allow(unused_imports)] use helpers::boot::*;
use helpers::native_protocol::*;
use helpers::raft_loop::run_raft_tick_loop;
// ─── Re-export helpers so handler modules can use `crate::X` ─────────────────
// time
pub(crate) use helpers::time::{now_unix_ms, now_unix_ms_u64, now_epoch_ms_chaos};
// env
pub(crate) use helpers::env_helpers::{read_env_bool, read_env_usize, read_env_u64};
// sql_parse
pub(crate) use helpers::sql_parse::{
    build_http_envelope,
    extract_delete_key_from_sql, extract_update_row_from_sql,
    extract_column_names_from_ddl, extract_insert_row_from_sql,
    extract_all_insert_rows,
};
// execution
pub(crate) use helpers::execution::{
    execute_transaction_statements,
    acquire_pessimistic_lock, release_pessimistic_lock,
    execute_olap_query, execute_oltp_select,
    df_select_owned, run_async_in_executor,
};
// udf
pub(crate) use helpers::udf::{
    execute_udf_runtime_legacy, udf_function_catalog_contract,
    udf_guard_policy_contract, build_udf_execution_plan,
    UdfRegistry,
};
// cluster
pub(crate) use helpers::cluster::{
    pool_stats_response, pool_acquire_error_state,
    acquire_sql_data_plane_connection, release_sql_data_plane_connection,
    rotate_leader, record_transport_mutation,
    default_node_cpu_cores, default_node_ram_mb,
};
// dr_hook
pub(crate) use helpers::dr_hook::{
    failure_budget_snapshot, rate_limit_policy_snapshot,
    evaluate_failure_budget_alert, enqueue_dr_hook_task,
    execute_dr_hook, evaluate_rate_limit, build_retry_plan,
};
// boot
pub(crate) use helpers::boot::{
    persist_sql_statement, build_durability_engine,
    replay_ddl_into, replay_dml_into, replay_database_catalog_into,
    replay_triggers_into,
};
// ─── Re-export auth helpers so handler/helper modules can use `crate::fn` ────
// ─── Re-export audit helpers ──────────────────────────────────────────────────
pub(crate) use audit_helpers::append_runtime_audit_event;
// ─── Re-export native protocol helpers ───────────────────────────────────────
pub(crate) use helpers::native_protocol::{
    load_native_tls_acceptor, vng_native_listener_log,
};
// ─── Re-export cluster helpers ────────────────────────────────────────────────
pub(crate) use helpers::cluster::{
    build_failover_handoff_report,
};
// ─── Re-export dr_hook helpers ────────────────────────────────────────────────
pub(crate) use helpers::dr_hook::dequeue_dr_hook_task;
// ─── Re-export wal helpers ────────────────────────────────────────────────────
pub(crate) use handlers::wal::{contains_table_alias_sql, contains_column_alias_sql};
// ─── Re-export SRE types needed by helpers ────────────────────────────────────
pub(crate) use handlers::sre::{
    DrHookExecutionRecord, DrHookPolicyConfig, DrHookPolicyState,
    DrHookPolicyStateEnvelope, DrHookPolicyStateSnapshot, DrHookRetryPlanStep,
    DrHookRuntimeState, DrHookScheduledTask,
    FailureBudgetAlertResponse, FailureBudgetSnapshot,
    RateLimitPolicySnapshot,
};
// ─── Re-export SQL handler types needed by helpers ────────────────────────────
// Note: SqlTransactionResponse and PessimisticLockRecord are defined in main.rs, not sql.rs
pub(crate) use handlers::sql::{
    OlapQueryRequest, OlapQueryResponse, OltpRowResult,
    PessimisticLockResponse,
    UdfExecutionResult, UdfExecutionPlanStep, UdfFunctionCatalogEntry,
    UdfInvocationPlan, UdfLanguageGuardPolicy,
};
// ─── Re-export misc handler types needed by helpers ───────────────────────────
pub(crate) use handlers::misc::{
    NativeFrame, NativeListenerConfig,
    FailoverHandoffGapResponse, FailoverHandoffReportResponse,
};
// ─── Re-export auth helpers ────────────────────────────────────────────────────
pub(crate) use auth::locale_from_headers;


/// Default per-database connection concurrency limit (Gap #9).
/// Overridden by the database's max_connections DDL attribute once that
/// field is added to the catalog. sql.rs references this via `crate::`.
pub(crate) const DEFAULT_DB_MAX_CONNECTIONS: usize = 100;

pub(crate) static TX_COUNTER: AtomicU64 = AtomicU64::new(1);
#[allow(dead_code)] // reserved for future action telemetry tracing
static ACTION_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);
pub(crate) static DR_HOOK_COUNTER: AtomicU64 = AtomicU64::new(1);
pub(crate) static PESSIMISTIC_LOCK_COUNTER: AtomicU64 = AtomicU64::new(1);
/// REQ-22 / WS22: Gate-export counters (incremented in `acquire_pessimistic_lock` for trend artifacts).
pub(crate) static WS22_GATE_DEADLOCK_DETECTIONS: AtomicU64 = AtomicU64::new(0);
pub(crate) static WS22_GATE_SCAN_CAP_TIMEOUTS: AtomicU64 = AtomicU64::new(0);
pub(crate) static DRIVER_SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);
pub(crate) const DEADLOCK_SCAN_MAX_HOPS: usize = 8;

/// T-3: Prefix marking a Raft log command as a grouped transaction batch.
/// Encoding: `__vng_batch:<db>\n<stmt1>__vng_stmt__<stmt2>...`. The apply loop
/// applies every statement in the batch under a single Xid so the transaction
/// is atomic on followers (all-or-nothing via the single committed log entry).
pub(crate) const RAFT_BATCH_PREFIX: &str = "__vng_batch:";
/// T-3: Separator between statements inside a batched Raft command.
pub(crate) const RAFT_BATCH_STMT_SEP: &str = "\n__vng_stmt__\n";

/// T-3: Encode a transaction's effective DML statements into a single Raft
/// batch command string scoped to `db`. Returns `None` when there is no DML to
/// replicate (so callers can skip appending an empty entry).
pub(crate) fn encode_raft_batch_command(db: &str, statements: &[String]) -> Option<String> {
    let dml: Vec<&str> = statements
        .iter()
        .map(|s| s.trim())
        .filter(|s| {
            let u = s.to_ascii_uppercase();
            u.starts_with("INSERT") || u.starts_with("UPDATE") || u.starts_with("DELETE")
        })
        .collect();
    if dml.is_empty() {
        return None;
    }
    Some(format!(
        "{RAFT_BATCH_PREFIX}{db}\n{}",
        dml.join(RAFT_BATCH_STMT_SEP)
    ))
}

pub(crate) const CONTROL_PLANE_OPERATOR_ROLES: [OperatorRole; 4] = [
    OperatorRole::Dba,
    OperatorRole::Sre,
    OperatorRole::Security,
    OperatorRole::AiOperator,
];

#[derive(Clone)]
struct PessimisticLockContentionMetrics {
    deadlock_detections: Arc<AtomicU64>,
    scan_cap_timeouts: Arc<AtomicU64>,
    wait_timeouts: Arc<AtomicU64>,
    lock_grants: Arc<AtomicU64>,
    lock_conflicts: Arc<AtomicU64>,
    lock_releases: Arc<AtomicU64>,
}

impl PessimisticLockContentionMetrics {
    fn new() -> Self {
        Self {
            deadlock_detections: Arc::new(AtomicU64::new(0)),
            scan_cap_timeouts: Arc::new(AtomicU64::new(0)),
            wait_timeouts: Arc::new(AtomicU64::new(0)),
            lock_grants: Arc::new(AtomicU64::new(0)),
            lock_conflicts: Arc::new(AtomicU64::new(0)),
            lock_releases: Arc::new(AtomicU64::new(0)),
        }
    }
}

// â”€â”€ ACID Transaction State Machine (REQ-23) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
enum AcidTxState {
    Active,
    Committed,
    RolledBack,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AcidTxEntry {
    transaction_id: String,
    pub(crate) assigned_node_id: String,
    state: AcidTxState,
    isolation_level: String,
    started_at_unix_ms: u128,
    completed_at_unix_ms: Option<u128>,
    statement_count: usize,
    affected_tables: Vec<String>,
    savepoints: Vec<String>,
    /// REQ-23: For repeatable_read isolation, the timestamp at which the logical read
    /// snapshot was taken (BEGIN time). None for other isolation levels.
    read_snapshot_at_ms: Option<u128>,
    /// REQ-23: WAL — ordered log of (statement, affected_table_or_empty) tuples recorded
    /// during the transaction. Cleared on commit or rollback.
    wal_log: Vec<(String, String)>,
    /// S2-WS2-05: The `PagedRowStore::current_xid()` at the time this transaction
    /// began.  At COMMIT, any key whose latest row-store version has an Xid
    /// greater than this value was written by a concurrent transaction —
    /// triggering a write-write conflict (HTTP 409).
    row_store_snapshot_xid: Option<u64>,
    /// M-7: Row-level OCC for serializable isolation — the set of raw row keys
    /// written (INSERT/UPDATE/DELETE) by this transaction.  Populated at COMMIT
    /// time so that conflict detection can compare individual keys rather than
    /// coarse-grained table prefixes.
    pub(crate) written_row_keys: std::collections::HashSet<String>,
    /// M-7 (SSI): Read-set for serializable phantom detection.
    /// Records the row keys returned by SELECT statements executed within this
    /// serializable transaction.  At COMMIT, these are compared against the
    /// `written_row_keys` of any committed concurrent serializable transaction
    /// to detect read-write anti-dependencies (phantom reads).
    pub(crate) read_row_keys: std::collections::HashSet<String>,
}

#[derive(Default)]
pub(crate) struct AcidTransactionRegistry {
    pub(crate) transactions: HashMap<String, AcidTxEntry>,
}

impl AcidTransactionRegistry {
    /// Open a new transaction.
    ///
    /// `begin_snapshot_xid` — for `repeatable_read` transactions, pass
    /// `Some(rs.current_xid())` captured *before* calling this method.
    /// This is stored as the read snapshot: all SELECTs in the transaction
    /// will see row-store state as of this Xid, even after concurrent writes.
    fn begin(
        &mut self,
        tx_id: &str,
        assigned_node_id: &str,
        isolation_level: &str,
        now_ms: u128,
        begin_snapshot_xid: Option<u64>,
    ) {
        let read_snapshot_at_ms = if isolation_level == "repeatable_read" {
            Some(now_ms)
        } else {
            None
        };
        // For repeatable_read: snapshot Xid is captured at BEGIN time so reads
        // are stable for the lifetime of the transaction.
        // For optimistic (B-2): snapshot Xid is captured at BEGIN so version
        // validation at COMMIT compares writes against the version observed at BEGIN.
        // For other levels: stays None until set at COMMIT by set_row_store_snapshot().
        let row_store_snapshot_xid = if isolation_level == "repeatable_read"
            || isolation_level == "optimistic"
        {
            begin_snapshot_xid
        } else {
            None
        };
        self.transactions.insert(
            tx_id.to_string(),
            AcidTxEntry {
                transaction_id: tx_id.to_string(),
                assigned_node_id: assigned_node_id.to_string(),
                state: AcidTxState::Active,
                isolation_level: isolation_level.to_string(),
                started_at_unix_ms: now_ms,
                completed_at_unix_ms: None,
                statement_count: 0,
                affected_tables: Vec::new(),
                savepoints: Vec::new(),
                read_snapshot_at_ms,
                wal_log: Vec::new(),
                row_store_snapshot_xid,
                written_row_keys: std::collections::HashSet::new(),
                read_row_keys: std::collections::HashSet::new(),
            },
        );
    }

    /// C-3: Return the read snapshot Xid for a repeatable-read transaction.
    /// Returns `None` for non-repeatable-read transactions (callers should
    /// fall back to `rs.current_xid()` for read-committed semantics).
    pub(crate) fn rr_read_snapshot_xid(&self, tx_id: &str) -> Option<u64> {
        let entry = self.transactions.get(tx_id)?;
        if entry.isolation_level == "repeatable_read" {
            entry.row_store_snapshot_xid
        } else {
            None
        }
    }

    /// S2-WS2-05: Record the `PagedRowStore::current_xid()` at the moment
    /// the transaction first touches data.  Called from the `sql_transaction`
    /// handler once a `PagedRowStore` lock is acquired.
    fn set_row_store_snapshot(&mut self, tx_id: &str, snapshot_xid: u64) {
        if let Some(entry) = self.transactions.get_mut(tx_id) {
            if entry.row_store_snapshot_xid.is_none() {
                entry.row_store_snapshot_xid = Some(snapshot_xid);
            }
        }
    }

    /// S2-WS2-05: Retrieve the row-store snapshot xid for `tx_id`.
    fn row_store_snapshot_xid(&self, tx_id: &str) -> Option<u64> {
        self.transactions.get(tx_id)?.row_store_snapshot_xid
    }

    pub(crate) fn commit(&mut self, tx_id: &str, now_ms: u128) -> bool {
        if let Some(entry) = self.transactions.get_mut(tx_id) {
            if entry.state == AcidTxState::Active {
                entry.state = AcidTxState::Committed;
                entry.completed_at_unix_ms = Some(now_ms);
                entry.wal_log.clear();
                return true;
            }
        }
        false
    }

    pub(crate) fn rollback(&mut self, tx_id: &str, now_ms: u128) -> bool {
        if let Some(entry) = self.transactions.get_mut(tx_id) {
            if entry.state == AcidTxState::Active {
                entry.state = AcidTxState::RolledBack;
                entry.completed_at_unix_ms = Some(now_ms);
                entry.wal_log.clear();
                return true;
            }
        }
        false
    }

    fn add_savepoint(&mut self, tx_id: &str, savepoint: &str) -> bool {
        if let Some(entry) = self.transactions.get_mut(tx_id) {
            if entry.state == AcidTxState::Active {
                entry.savepoints.push(savepoint.to_string());
                return true;
            }
        }
        false
    }

    /// Release (drop) a named savepoint â€” returns true if the savepoint existed and was removed.
    fn release_savepoint(&mut self, tx_id: &str, savepoint: &str) -> bool {
        if let Some(entry) = self.transactions.get_mut(tx_id) {
            if entry.state == AcidTxState::Active {
                if let Some(pos) = entry.savepoints.iter().rposition(|s| s == savepoint) {
                    entry.savepoints.remove(pos);
                    return true;
                }
            }
        }
        false
    }

    /// ROLLBACK TO SAVEPOINT: rolls back to a named savepoint, preserving the transaction as Active.
    /// In this in-memory model we record the rollback in the savepoints list.
    fn rollback_to_savepoint(&mut self, tx_id: &str, savepoint: &str) -> bool {
        if let Some(entry) = self.transactions.get_mut(tx_id) {
            if entry.state == AcidTxState::Active && entry.savepoints.contains(&savepoint.to_string()) {
                // Record the rollback-to in the list for audit/trace purposes
                let marker = format!("rolled_back_to:{savepoint}");
                entry.savepoints.push(marker);
                return true;
            }
        }
        false
    }

    fn record_statement(&mut self, tx_id: &str, affected_table: Option<String>) {
        if let Some(entry) = self.transactions.get_mut(tx_id) {
            entry.statement_count += 1;
            let table = affected_table.clone().unwrap_or_default();
            if !table.is_empty() && !entry.affected_tables.contains(&table) {
                entry.affected_tables.push(table.clone());
            }
            // REQ-23 WAL: append (statement_N, table) to the write-ahead log
            let stmt_label = format!("statement_{}", entry.statement_count);
            entry.wal_log.push((stmt_label, table));
        }
    }

    /// M-7: Record the raw row keys written by `tx_id` at COMMIT time.
    /// Called from the sql_transaction COMMIT path with keys collected during
    /// the DML flush loop.
    pub(crate) fn record_written_row_keys(&mut self, tx_id: &str, keys: impl IntoIterator<Item = String>) {
        if let Some(entry) = self.transactions.get_mut(tx_id) {
            for k in keys {
                entry.written_row_keys.insert(k);
            }
        }
    }

    /// M-7 (SSI): Record the row keys read by a SELECT inside transaction `tx_id`.
    ///
    /// Only has effect when the transaction is active and uses `serializable`
    /// isolation — for other levels the read-set is ignored and not stored.
    pub(crate) fn record_read_row_keys(&mut self, tx_id: &str, keys: impl IntoIterator<Item = String>) {
        if let Some(entry) = self.transactions.get_mut(tx_id) {
            if entry.isolation_level == "serializable" && entry.state == AcidTxState::Active {
                for k in keys {
                    entry.read_row_keys.insert(k);
                }
            }
        }
    }

    /// M-7 (SSI): Detect read-write anti-dependencies for serializable phantom prevention.
    ///
    /// Two directions are checked:
    ///
    /// 1. **Phantom read** (`current.read_keys ∩ peer.written_keys ≠ ∅`):
    ///    The current transaction read a row that a concurrent committed
    ///    serializable transaction has since written — the read is now stale.
    ///
    /// 2. **Write-read anti-dependency** (`current.written_keys ∩ peer.read_keys ≠ ∅`):
    ///    A concurrent committed serializable transaction read a row that the
    ///    current transaction is about to overwrite — the peer's reads are stale.
    ///
    /// Returns `Some(conflicting_key)` on the first detected conflict.
    pub(crate) fn check_serializable_rw_conflict(
        &self,
        current_tx_id: &str,
        current_written_keys: &std::collections::HashSet<String>,
        current_read_keys: &std::collections::HashSet<String>,
    ) -> Option<String> {
        for (other_tx_id, other_entry) in &self.transactions {
            if other_tx_id == current_tx_id {
                continue;
            }
            if other_entry.isolation_level != "serializable" {
                continue;
            }
            if other_entry.state != AcidTxState::Committed {
                continue;
            }
            // Direction 1: phantom read — we read what they wrote.
            for k in current_read_keys {
                if other_entry.written_row_keys.contains(k) {
                    return Some(k.clone());
                }
            }
            // Direction 2: write-read — they read what we wrote.
            for k in current_written_keys {
                if other_entry.read_row_keys.contains(k) {
                    return Some(k.clone());
                }
            }
        }
        None
    }

    /// M-7: Row-level serializable conflict detection.
    ///
    /// Returns the first conflicting key if `current_written_keys` overlaps with
    /// the `written_row_keys` of any *committed* serializable transaction other
    /// than `current_tx_id`.  Comparing only committed transactions means we
    /// catch write-write anti-dependencies (a committed peer wrote the same row
    /// we are about to commit) without false-positives from concurrent-but-
    /// non-overlapping transactions that happened to touch the same table.
    pub(crate) fn check_serializable_conflict_row_level(
        &self,
        current_tx_id: &str,
        current_written_keys: &std::collections::HashSet<String>,
    ) -> Option<String> {
        if current_written_keys.is_empty() {
            return None;
        }
        for (other_tx_id, other_entry) in &self.transactions {
            if other_tx_id == current_tx_id {
                continue;
            }
            if other_entry.isolation_level != "serializable" {
                continue;
            }
            // Only compare against committed peers — still-active transactions
            // have not yet landed their writes, so comparing with them would be
            // premature and cause false conflicts.
            if other_entry.state != AcidTxState::Committed {
                continue;
            }
            for k in current_written_keys {
                if other_entry.written_row_keys.contains(k) {
                    return Some(k.clone());
                }
            }
        }
        None
    }

    /// B-2: Optimistic-locking conflict detection against committed peers.
    ///
    /// Returns the first key in `current_written_keys` that a *committed*
    /// `optimistic` transaction (other than `current_tx_id`) already wrote.
    /// This is the validate-on-commit step of optimistic concurrency control:
    /// two optimistic transactions that both write the same row cannot both
    /// commit — the later one is aborted with a typed version conflict. No row
    /// locks are taken at any point.
    pub(crate) fn check_optimistic_conflict(
        &self,
        current_tx_id: &str,
        current_written_keys: &std::collections::HashSet<String>,
    ) -> Option<String> {
        if current_written_keys.is_empty() {
            return None;
        }
        for (other_tx_id, other_entry) in &self.transactions {
            if other_tx_id == current_tx_id {
                continue;
            }
            if other_entry.isolation_level != "optimistic" {
                continue;
            }
            if other_entry.state != AcidTxState::Committed {
                continue;
            }
            for k in current_written_keys {
                if other_entry.written_row_keys.contains(k) {
                    return Some(k.clone());
                }
            }
        }
        None
    }

    pub(crate) fn active_transactions(&self) -> Vec<&AcidTxEntry> {
        self.transactions
            .values()
            .filter(|e| e.state == AcidTxState::Active)
            .collect()
    }

    pub(crate) fn all_transactions(&self) -> Vec<&AcidTxEntry> {
        self.transactions.values().collect()
    }

    pub(crate) fn reassign_active_node(&mut self, source_node_id: &str, target_node_id: &str) -> usize {
        let mut reassigned = 0usize;
        for entry in self.transactions.values_mut() {
            if entry.state == AcidTxState::Active && entry.assigned_node_id == source_node_id {
                entry.assigned_node_id = target_node_id.to_string();
                reassigned += 1;
            }
        }
        reassigned
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeadlockScanOutcome {
    CycleDetected,
    ScanCapReached,
    NoCycle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ClusterNodeRuntime {
    pub(crate) node_id: String,
    pub(crate) role: String,
    pub(crate) status: String,
    pub(crate) total_cpu_cores: u32,
    pub(crate) total_ram_mb: u64,
    pub(crate) draining: bool,
    pub(crate) last_heartbeat_ms: u64,
}

// ─── AI-3: Self-Heal rate-limiter state ──────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub(crate) struct SelfHealCounters {
    pub(crate) actions_this_hour: u64,
    pub(crate) window_start_ms: u64,
    pub(crate) max_per_hour: u64,
}

// ─── AI-4: Slow-query ring-buffer entry and tune recommendation ───────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SlowQueryEntry {
    pub(crate) query: String,
    pub(crate) duration_ms: u64,
    pub(crate) table_name: Option<String>,
    pub(crate) timestamp_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TuneRecommendation {
    pub(crate) action: String,
    pub(crate) table: Option<String>,
    pub(crate) column: Option<String>,
    pub(crate) reason: String,
    pub(crate) estimated_speedup: Option<f64>,
}

// ─── AI-5: DEK version record for KMS key rotation ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DekVersion {
    pub(crate) version: u64,
    pub(crate) kms_key_ref: String,
    pub(crate) wrapped_at_ms: u128,
    pub(crate) active: bool,
}

// ─── AI-6: Configurable diagnosis rule (loadable from dr-hook-runtime.json) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DiagnosisRule {
    #[serde(default)]
    pub(crate) failure_type: Option<String>,
    #[serde(default)]
    pub(crate) keywords: Vec<String>,
    pub(crate) root_cause: String,
    pub(crate) confidence: String,
    pub(crate) recommended_action: String,
}


// ─── AR-1: AppState sub-state structs ────────────────────────────────────────
// Each sub-struct groups logically related fields that were previously scattered
// flat on AppState. AppState now holds one field per group plus four
// top-level identity/config fields that every handler touches.

/// Authentication, RBAC, KMS, and user-session management state.
#[derive(Clone)]
pub(crate) struct AuthState {
    pub(crate) admin_api_key: Option<String>,
    pub(crate) security_config: Arc<SecurityConfigContract>,
    pub(crate) allowed_operator_roles: Arc<HashSet<OperatorRole>>,
    pub(crate) operator_role_bindings: Arc<HashMap<String, OperatorRole>>,
    pub(crate) tenant_user_bindings: Arc<HashMap<String, TenantUserBinding>>,
    pub(crate) rbac_privilege_matrix: Arc<RbacPrivilegeMatrix>,
    pub(crate) kms_runtime: Arc<Mutex<KmsRuntimeState>>,
    pub(crate) db_grants: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    pub(crate) user_store: Arc<Mutex<crate::user_store::UserStore>>,
    pub(crate) session_store: Arc<Mutex<crate::user_store::SessionStore>>,
    pub(crate) session_signer: Arc<Mutex<crate::user_store::SessionSigner>>,
}

/// Raft consensus, cluster topology, replication, and sync state.
#[derive(Clone)]
pub(crate) struct ClusterState {
    pub(crate) leader_node_id: Arc<Mutex<String>>,
    pub(crate) cluster_nodes: Arc<Mutex<HashMap<String, ClusterNodeRuntime>>>,
    pub(crate) cluster_failure_signals: Arc<Mutex<Vec<ClusterFailureSignal>>>,
    /// S7-WS6-02: Raft consensus node state (elections, log replication, snapshots).
    pub(crate) raft_state: Arc<Mutex<RaftNode>>,
    /// Raft peer base URLs loaded from `VNG_RAFT_PEERS`. Empty on single-node deployments.
    pub(crate) raft_peers: Arc<Vec<String>>,
    /// Shared secret for intra-cluster Raft RPCs, loaded from `VNG_CLUSTER_TOKEN`.
    pub(crate) cluster_token: Arc<Option<String>>,
    /// Watch channel that broadcasts the latest `last_applied` Raft index.
    pub(crate) raft_last_applied_tx: Arc<tokio::sync::watch::Sender<u64>>,
    /// URL of the current Raft leader, learned from AppendEntries headers.
    pub(crate) current_leader_url: Arc<Mutex<Option<String>>>,
    /// In-progress chunked snapshot transfer sessions keyed by `session_id`.
    #[allow(dead_code)]
    pub(crate) snapshot_chunk_sessions: Arc<Mutex<HashMap<String, SnapshotChunkSession>>>,
    pub(crate) sync_origin: Arc<Mutex<RowStoreSyncOrigin>>,
    pub(crate) replication_transport: Arc<Mutex<InMemoryReplicationTransport>>,
    pub(crate) replica_replay_states: Arc<Mutex<HashMap<String, ReplicaReplayState>>>,
    /// C-4: Per-peer HTAP replication cursor (peer base URL → last shipped sequence).
    pub(crate) htap_peer_cursors: Arc<Mutex<HashMap<String, u64>>>,
}

/// Storage engine, MVCC, SQL catalog, transaction management, and locking.
#[derive(Clone)]
pub(crate) struct StorageState {
    /// MVCC page-based row store (S2-WS2-04: PagedRowStore).
    pub(crate) row_store: Arc<Mutex<PagedRowStore>>,
    /// S2-WS2-02: WAL durability engine — records every committed DML mutation.
    pub(crate) wal_engine: Arc<Mutex<BoxedDurabilityEngine>>,
    /// S4-WS3-04: In-memory OLAP replica.
    pub(crate) olap_store: Arc<Mutex<HashMap<String, HashMap<String, String>>>>,
    pub(crate) ddl_catalog: Arc<Mutex<DdlCatalog>>,
    /// Phase 1.3 — first-class `Database` catalog.
    pub(crate) database_catalog: Arc<Mutex<voltnuerongrid_meta::DatabaseCatalog>>,
    pub(crate) acid_transactions: Arc<Mutex<AcidTransactionRegistry>>,
    pub(crate) index_manager: Arc<Mutex<IndexManager>>,
    pub(crate) constraint_manager: Arc<Mutex<ConstraintManager>>,
    /// Gap #3: Per-connection undo log for ROLLBACK support.
    pub(crate) tx_undo_log: Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<(String, Option<voltnuerongrid_store::mvcc::RowData>)>>>>,
    /// Gap #9: Per-database connection semaphores.
    pub(crate) db_semaphores: Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>>>,
    /// Gap #6/#7: Per-table row count statistics.
    pub(crate) table_stats: Arc<Mutex<std::collections::HashMap<String, u64>>>,
    /// H-1: Table statistics registry for the cost-based query optimizer.
    pub(crate) stats_registry: Arc<Mutex<voltnuerongrid_exec::StatsRegistry>>,
    /// C-3: connection_id → tx_id for repeatable-read transaction tracking.
    pub(crate) connection_tx_active: Arc<Mutex<HashMap<String, String>>>,
    /// L-2: Stored-procedure registry, pre-populated with built-ins at boot.
    pub(crate) proc_registry: Arc<Mutex<helpers::stored_proc::ProcedureRegistry>>,
    /// ISSUE-03: DML trigger registry.
    pub(crate) trigger_registry: Arc<Mutex<TriggerRegistry>>,
    pub(crate) trigger_emitter: Arc<dyn TriggerEmitter>,
    /// PART-1: partition registry — maps table_name → partition column.
    pub(crate) partition_registry: Arc<Mutex<HashMap<String, String>>>,
    /// B-4: Range-partition segment configuration per table (table → boundaries).
    pub(crate) partition_segments: Arc<Mutex<HashMap<String, helpers::partition::RangePartitionConfig>>>,
    /// C-2: shard registry — maps table_name → shard config (DISTRIBUTE BY HASH).
    pub(crate) shard_registry: Arc<Mutex<HashMap<String, helpers::dataplane::ShardTableConfig>>>,
    pub(crate) pessimistic_locks: Arc<Mutex<HashMap<String, PessimisticLockRecord>>>,
    pub(crate) pessimistic_lock_waits: Arc<Mutex<HashMap<String, String>>>,
    pub(crate) pessimistic_lock_metrics: PessimisticLockContentionMetrics,
    /// S10-WS15-02: Per-table CDC cursor positions.
    pub(crate) cdc_cursors: Arc<Mutex<HashMap<String, u64>>>,
}

/// Data ingestion pipelines, CDC connectors, and outbox state.
#[derive(Clone)]
pub(crate) struct IngestState {
    pub(crate) ingest_csv_records: Arc<Mutex<HashMap<String, Vec<voltnuerongrid_ingest::IngestRecord>>>>,
    pub(crate) ingest_json_records: Arc<Mutex<HashMap<String, Vec<voltnuerongrid_ingest::IngestRecord>>>>,
    pub(crate) ingest_parquet_records: Arc<Mutex<HashMap<String, Vec<voltnuerongrid_ingest::IngestRecord>>>>,
    pub(crate) ingest_excel_records: Arc<Mutex<HashMap<String, Vec<voltnuerongrid_ingest::IngestRecord>>>>,
    pub(crate) ingest_outbox_streams: Arc<Mutex<HashMap<String, String>>>,
    pub(crate) ingest_event_bus: Arc<Mutex<ManagedEventBusTransport>>,
    pub(crate) ingest_outbox_cursors: Arc<Mutex<ManagedReplayCursorStore>>,
    /// S5-WS4A-02: Broker adapter flush counters (broker_type → flush_count).
    pub(crate) broker_flush_counts: Arc<Mutex<HashMap<String, u64>>>,
    /// S5-E4A-01: Connector SDK runtime registry.
    pub(crate) connector_registry: Arc<Mutex<Vec<ConnectorPlugin>>>,
}

/// AI / autonomous operations, self-healing, and rate limiting state.
#[derive(Clone)]
pub(crate) struct AiState {
    pub(crate) autonomous_mode: AutonomousMode,
    pub(crate) emergency_stop: Arc<AtomicEmergencyStop>,
    pub(crate) guardrails: Arc<Vec<GuardrailRule>>,
    /// S9-WS8-02: AI model gateway isolation policy.
    pub(crate) model_gateway_policy: Arc<Mutex<ModelGatewayPolicy>>,
    /// S9-WS8-02: Per-model request counters for rate limiting.
    pub(crate) ai_request_counters: Arc<Mutex<HashMap<String, u64>>>,
    /// S9-WS8-02: Sliding-window rate limiter — per-model window start timestamp (ms).
    pub(crate) ai_rate_window_starts: Arc<Mutex<HashMap<String, u64>>>,
    // AI-3: Self-heal rate-limiter counters.
    pub(crate) self_heal_counters: Arc<Mutex<SelfHealCounters>>,
    // AI-4: Slow-query ring buffer (max 1000 most-recent entries).
    pub(crate) slow_query_log: Arc<Mutex<VecDeque<SlowQueryEntry>>>,
    // AI-4: Latest advisor tune recommendations.
    pub(crate) tune_recommendations: Arc<Mutex<Vec<TuneRecommendation>>>,
    // AI-5: DEK version history for KMS key rotation.
    pub(crate) dek_versions: Arc<Mutex<Vec<DekVersion>>>,
    // AI-5: Current TLS certificate SHA-256 fingerprint (updated on rotation).
    pub(crate) cert_fingerprint: Arc<Mutex<Option<String>>>,
    // AI-6: Configurable incident diagnosis rules (loaded from dr-hook-runtime.json).
    pub(crate) diagnosis_rules: Arc<Mutex<Vec<DiagnosisRule>>>,
    // AI-1: Per-operator Chat-to-SQL request counters for rate limiting.
    pub(crate) chat_sql_counters: Arc<Mutex<HashMap<String, u64>>>,
    pub(crate) action_records: Arc<Mutex<Vec<AutonomousActionExecutionRecord>>>,
}

/// Observability, DR hooks, plugins, extensions, drivers, autoscale, and audit.
#[derive(Clone)]
pub(crate) struct OpsState {
    pub(crate) audit_sink: Arc<Mutex<AppendOnlyAuditSink>>,
    /// S9-WS8A-02: Optional path to a JSON-lines audit log file.
    pub(crate) audit_log_path: Option<String>,
    /// B-5: Operational lifecycle event stream (ring buffer, observability).
    pub(crate) operational_events: Arc<Mutex<helpers::op_events::OperationalEventStream>>,
    pub(crate) dr_hook_records: Arc<Mutex<Vec<DrHookExecutionRecord>>>,
    pub(crate) dr_hook_policy_state: Arc<Mutex<DrHookPolicyState>>,
    pub(crate) dr_hook_policy_config: Arc<DrHookPolicyConfig>,
    pub(crate) dr_hook_state_path: Option<String>,
    pub(crate) dr_hook_queue: Arc<Mutex<VecDeque<DrHookScheduledTask>>>,
    /// S7-WS6-04: Chaos/game-day fault injection state.
    pub(crate) chaos_state: Arc<Mutex<ChaosState>>,
    // SCALE-1: autoscale policy + current status.
    pub(crate) autoscale_policy: Arc<Mutex<handlers::autoscale::AutoscalePolicy>>,
    pub(crate) autoscale_status: Arc<Mutex<handlers::autoscale::AutoscaleStatus>>,
    pub(crate) plugin_lifecycle: Arc<Mutex<PluginLifecycleManager>>,
    // PLUG-4: versioned plugin marketplace registry.
    pub(crate) plugin_registry: Arc<Mutex<helpers::plugins::PluginRegistry>>,
    /// UDF runtime: registered WASM/JS/Python user-defined functions.
    pub(crate) udf_registry: Arc<Mutex<UdfRegistry>>,
    pub(crate) distributed_cache: Arc<Mutex<DistributedCacheManager>>,
    // CACHE-1: path to the on-disk cache snapshot file.
    pub(crate) cache_snapshot_path: Arc<String>,
    pub(crate) driver_pool: Arc<Mutex<ConnectionPoolManager>>,
    /// S8-WS10-02: Driver wire protocol session registry.
    pub(crate) driver_sessions: Arc<Mutex<HashMap<String, DriverSession>>>,
    /// S6-WS5-04: TDE runtime toggle override.
    pub(crate) tde_override: Arc<Mutex<Option<bool>>>,
    // PLUG-1: flat-scan cosine-similarity vector index.
    pub(crate) vector_index: Arc<Mutex<helpers::vector::VectorIndex>>,
    // PLUG-2: in-memory inverted full-text search index.
    pub(crate) fts_index: Arc<Mutex<helpers::fts::FtsIndex>>,
    // PLUG-3: R-tree geospatial index.
    pub(crate) geo_index: Arc<Mutex<helpers::geo::GeoIndex>>,
}

/// Shared, cheaply-clonable application state injected into every axum handler.
///
/// Fields are decomposed into six logical sub-structs (AR-1):
/// - `auth`    — authentication, RBAC, KMS, user/session stores
/// - `cluster` — Raft consensus, cluster topology, replication
/// - `storage` — MVCC row-store, WAL, SQL catalog, transactions, locking
/// - `ingest`  — ingestion pipelines, CDC connectors, outbox
/// - `ai`      — autonomous operations, self-healing, rate limiting
/// - `ops`     — audit, DR hooks, plugins, drivers, autoscale, cache
///
/// Four top-level fields (`node_id`, `cluster_mode`, `node_url`,
/// `runtime_config`) are kept flat because they are read on every request.
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) node_id: String,
    pub(crate) cluster_mode: String,
    /// This node's own advertised base URL, loaded from `VNG_NODE_URL`.
    #[allow(dead_code)]
    pub(crate) node_url: Arc<Option<String>>,
    /// Phase 0 — runtime config selected at boot. Read-only after startup.
    pub(crate) runtime_config: Arc<voltnuerongrid_config::RuntimeConfig>,
    /// Authentication, RBAC, KMS, and user-session management.
    pub(crate) auth: AuthState,
    /// Raft consensus, cluster topology, and replication.
    pub(crate) cluster: ClusterState,
    /// Storage engine, MVCC, SQL catalog, transactions, and locking.
    pub(crate) storage: StorageState,
    /// Data ingestion pipelines and CDC.
    pub(crate) ingest: IngestState,
    /// AI / autonomous operations and self-healing.
    pub(crate) ai: AiState,
    /// Observability, DR, plugins, drivers, and autoscale.
    pub(crate) ops: OpsState,
}

#[derive(Clone, Default)]
pub(crate) struct KmsRuntimeState {
    pub(crate) providers: Vec<ConfiguredKmsProviderAdapter>,
    pub(crate) unavailable_envs: HashSet<String>,
    pub(crate) last_resolution: Option<KmsKeyResolution>,
    pub(crate) last_error: Option<String>,
    pub(crate) last_simulation_note: Option<String>,
}


#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomousMode {
    Disabled,
    Advisory,
    Supervised,
    Autonomous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum OperatorRole {
    Dba,
    Sre,
    Security,
    AiOperator,
}

impl OperatorRole {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dba" | "admin" => Some(Self::Dba),
            "sre" => Some(Self::Sre),
            "security" | "secops" => Some(Self::Security),
            "ai_operator" | "ai-operator" | "autonomous" => Some(Self::AiOperator),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Dba => "dba",
            Self::Sre => "sre",
            Self::Security => "security",
            Self::AiOperator => "ai_operator",
        }
    }
}

/// Accumulates row chunks from a leader snapshot-transfer session.
/// Keyed in `AppState::snapshot_chunk_sessions` by `session_id`.
#[allow(dead_code)] // fields written on chunk receipt; consumed when is_last = true
#[derive(Clone)]
pub(crate) struct SnapshotChunkSession {
    pub(crate) term: u64,
    pub(crate) leader_id: String,
    pub(crate) snapshot_index: u64,
    pub(crate) snapshot_term: u64,
    pub(crate) rows: Vec<(String, serde_json::Value)>,
    pub(crate) next_expected_chunk: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OperatorIdentity {
    pub(crate) operator_id: String,
    pub(crate) role: OperatorRole,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TenantUserBinding {
    pub(crate) tenant_id: String,
    pub(crate) role: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TenantUserIdentity {
    pub(crate) user_id: String,
    pub(crate) tenant_id: String,
    pub(crate) role: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeAccessPrincipal {
    Operator(OperatorIdentity),
    TenantUser(TenantUserIdentity),
}

impl AutonomousMode {
    fn from_env(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "disabled" => Self::Disabled,
            "advisory" => Self::Advisory,
            "autonomous" => Self::Autonomous,
            _ => Self::Supervised,
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::Disabled => 0,
            Self::Advisory => 1,
            Self::Supervised => 2,
            Self::Autonomous => 3,
        }
    }
}

#[derive(Clone)]
struct AtomicEmergencyStop {
    enabled: Arc<std::sync::atomic::AtomicBool>,
}

impl AtomicEmergencyStop {
    fn new(initial: bool) -> Self {
        Self {
            enabled: Arc::new(std::sync::atomic::AtomicBool::new(initial)),
        }
    }

    fn get(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    fn set(&self, value: bool) {
        self.enabled.store(value, Ordering::SeqCst);
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct AuthErrorResponse {
    pub(crate) status: &'static str,
    pub(crate) reason: String,
    pub(crate) locale: String,
    pub(crate) localized_message: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransportKind {
    Http,
    Native,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanonicalCommandName {
    Health,
    SqlAnalyze,
    SqlRoute,
    SqlExecute,
    SqlTransaction,
    IngestSchemaRegistry,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct CanonicalCommandEnvelope<TPayload> {
    pub(crate) request_id: String,
    pub(crate) transport: TransportKind,
    pub(crate) command: CanonicalCommandName,
    pub(crate) session_context: Option<String>,
    pub(crate) transport_metadata: std::collections::HashMap<String, String>,
    pub(crate) payload: TPayload,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct CanonicalSuccess<TPayload> {
    pub(crate) payload: TPayload,
    pub(crate) request_id: String,
    pub(crate) transport: TransportKind,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct CanonicalError {
    pub(crate) request_id: String,
    pub(crate) transport: TransportKind,
    pub(crate) kind: &'static str,
    pub(crate) message: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeFrameType {
    Hello,
    HelloAck,
    Auth,
    AuthAck,
    Command,
    Result,
    Error,
    Ping,
    Pong,
    StreamChunk,
    StreamEnd,
    Cancel,
    Goodbye,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeCommandKind {
    Health,
    SqlAnalyze,
    SqlRoute,
    SqlExecute,
    SqlTransaction,
    IngestSchemaRegistry,
    Unknown,
}

#[derive(Debug, Clone)]
pub(crate) struct SqlTransactionGatewayContext {
    pub(crate) statements: Vec<String>,
    pub(crate) isolation_level: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct TransportGateway;

impl TransportGateway {
    fn new() -> Self {
        Self
    }

    fn health_response(&self, _transport: TransportKind, state: &AppState) -> HealthResponse {
        HealthResponse {
            status: "ok",
            node_id: state.node_id.clone(),
            cluster_mode: state.cluster_mode.clone(),
        }
    }

    fn sql_analyze_response(
        &self,
        envelope: &CanonicalCommandEnvelope<SqlAnalyzeRequest>,
    ) -> CanonicalSuccess<SqlAnalyzeResponse> {
        let parsed = SqlAnalyzer::parse_batch(&envelope.payload.sql_batch);
        let mut rejected = 0usize;
        let mut statements = Vec::with_capacity(parsed.len());
        for statement in parsed {
            let analysis = SqlAnalyzer::analyze_statement(&statement.raw);
            let accepted = analysis.kind != SqlStatementKind::Unknown;
            if !accepted {
                rejected += 1;
            }
            statements.push(AnalyzedStatement {
                statement: statement.raw,
                kind: format!("{:?}", analysis.kind),
                requires_transaction: analysis.requires_transaction,
                touches_catalog: analysis.touches_catalog,
                accepted,
            });
        }

        CanonicalSuccess {
            payload: SqlAnalyzeResponse {
                status: "ok",
                total_statements: statements.len(),
                rejected_statements: rejected,
                statements,
            },
            request_id: envelope.request_id.clone(),
            transport: envelope.transport,
        }
    }

    fn sql_route_response(
        &self,
        envelope: &CanonicalCommandEnvelope<SqlRouteRequest>,
    ) -> CanonicalSuccess<SqlRouteResponse> {
        let decision = HtapQueryRouter::route_batch(&envelope.payload.sql_batch);
        // S3-WS1-05: augment each routed statement with cost-model hints from QueryPlanner
        use voltnuerongrid_exec::{QueryPath, QueryPlanner};
        use voltnuerongrid_sql::parse_one;
        let mut batch_estimated_rows: u64 = 0;
        let mut batch_relative_cost: f64 = 0.0;
        let statements: Vec<RoutedStatementResponse> = decision
            .statements
            .into_iter()
            .map(|s| {
                let (planner_path, estimated_rows, relative_cost) = match parse_one(&s.statement) {
                    Ok(stmt) => {
                        let plan = QueryPlanner::plan(&stmt);
                        let cost = QueryPlanner::estimate_cost(&plan);
                        let pp = match cost.recommended_path {
                            QueryPath::Oltp => "oltp",
                            QueryPath::Olap => "olap",
                            QueryPath::Hybrid => "hybrid",
                            QueryPath::Unknown => "unknown",
                        };
                        (pp.to_string(), cost.estimated_rows, cost.relative_cost)
                    }
                    Err(_) => ("unknown".to_string(), 0u64, 0.0f64),
                };
                batch_estimated_rows += estimated_rows;
                batch_relative_cost += relative_cost;
                RoutedStatementResponse {
                    statement: s.statement,
                    path: route_path_name(s.path).to_string(),
                    planner_path,
                    estimated_rows,
                    relative_cost,
                }
            })
            .collect();
        CanonicalSuccess {
            payload: SqlRouteResponse {
                status: "ok",
                route_path: route_path_name(decision.path).to_string(),
                reason: decision.reason,
                statements,
                batch_estimated_rows,
                batch_relative_cost,
            },
            request_id: envelope.request_id.clone(),
            transport: envelope.transport,
        }
    }

    fn sql_execute_route_decision(
        &self,
        envelope: &CanonicalCommandEnvelope<SqlExecuteRequest>,
    ) -> CanonicalSuccess<voltnuerongrid_exec::BatchRouteDecision> {
        CanonicalSuccess {
            payload: HtapQueryRouter::route_batch(&envelope.payload.sql_batch),
            request_id: envelope.request_id.clone(),
            transport: envelope.transport,
        }
    }

    fn sql_transaction_context(
        &self,
        envelope: &CanonicalCommandEnvelope<SqlTransactionRequest>,
    ) -> CanonicalSuccess<SqlTransactionGatewayContext> {
        CanonicalSuccess {
            payload: SqlTransactionGatewayContext {
                statements: envelope.payload.statements.clone(),
                isolation_level: envelope.payload.isolation_level.clone(),
            },
            request_id: envelope.request_id.clone(),
            transport: envelope.transport,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CommandDispatcher {
    gateway: TransportGateway,
}

impl CommandDispatcher {
    pub(crate) fn new() -> Self {
        Self {
            gateway: TransportGateway::new(),
        }
    }

    fn dispatch_health(&self, state: &AppState) -> HealthResponse {
        self.gateway.health_response(TransportKind::Http, state)
    }

    fn dispatch_health_for_transport(
        &self,
        state: &AppState,
        transport: TransportKind,
    ) -> HealthResponse {
        self.gateway.health_response(transport, state)
    }

    pub(crate) fn dispatch_sql_analyze(
        &self,
        envelope: &CanonicalCommandEnvelope<SqlAnalyzeRequest>,
    ) -> CanonicalSuccess<SqlAnalyzeResponse> {
        self.gateway.sql_analyze_response(envelope)
    }

    pub(crate) fn dispatch_sql_route(
        &self,
        envelope: &CanonicalCommandEnvelope<SqlRouteRequest>,
    ) -> CanonicalSuccess<SqlRouteResponse> {
        self.gateway.sql_route_response(envelope)
    }

    pub(crate) fn dispatch_sql_execute_route_decision(
        &self,
        envelope: &CanonicalCommandEnvelope<SqlExecuteRequest>,
    ) -> CanonicalSuccess<voltnuerongrid_exec::BatchRouteDecision> {
        self.gateway.sql_execute_route_decision(envelope)
    }

    pub(crate) fn dispatch_sql_transaction_context(
        &self,
        envelope: &CanonicalCommandEnvelope<SqlTransactionRequest>,
    ) -> CanonicalSuccess<SqlTransactionGatewayContext> {
        self.gateway.sql_transaction_context(envelope)
    }

    fn dispatch_ingest_schema_registry(
        &self,
        state: &AppState,
        envelope: &CanonicalCommandEnvelope<()>,
    ) -> CanonicalSuccess<IngestSchemaRegistryResponse> {
        let payload = collect_ingest_schema_registry_response(state);
        CanonicalSuccess {
            payload,
            request_id: envelope.request_id.clone(),
            transport: envelope.transport,
        }
    }
}

impl NativeAdapter {
    fn dispatch_frame(
        frame: &NativeFrame,
        state: &AppState,
        dispatcher: &CommandDispatcher,
    ) -> NativeFrame {
        let dispatched = if frame.frame_type != NativeFrameType::Command {
            Err(CanonicalError {
                request_id: frame.request_id.clone(),
                transport: TransportKind::Native,
                kind: "protocol",
                message: "expected COMMAND frame for native dispatch".to_string(),
            })
        } else {
            match frame.command {
                Some(NativeCommandKind::Health) => {
                    Self::dispatch_health_frame(frame, state, dispatcher)
                }
                Some(NativeCommandKind::SqlAnalyze) => {
                    Self::dispatch_sql_analyze_frame(frame, dispatcher)
                }
                Some(NativeCommandKind::SqlRoute) => Self::dispatch_sql_route_frame(frame, dispatcher),
                Some(NativeCommandKind::SqlExecute) => {
                    Self::dispatch_sql_execute_route_decision_frame(frame, dispatcher)
                }
                Some(NativeCommandKind::SqlTransaction) => {
                    Self::dispatch_sql_transaction_context_frame(frame, dispatcher)
                }
                Some(NativeCommandKind::IngestSchemaRegistry) => {
                    Self::dispatch_ingest_schema_registry_frame(frame, state, dispatcher)
                }
                Some(NativeCommandKind::Unknown) => Err(CanonicalError {
                    request_id: frame.request_id.clone(),
                    transport: TransportKind::Native,
                    kind: "protocol",
                    message: "unsupported native command: unknown".to_string(),
                }),
                None => Err(CanonicalError {
                    request_id: frame.request_id.clone(),
                    transport: TransportKind::Native,
                    kind: "protocol",
                    message: "missing command for COMMAND frame".to_string(),
                }),
            }
        };

        match dispatched {
            Ok(frame) => frame,
            Err(error) => Self::error_to_error_frame(&error),
        }
    }

    fn dispatch_health_frame(
        frame: &NativeFrame,
        state: &AppState,
        dispatcher: &CommandDispatcher,
    ) -> Result<NativeFrame, CanonicalError> {
        let envelope = Self::from_command_frame(frame, CanonicalCommandName::Health, ())?;
        let payload = dispatcher.dispatch_health_for_transport(state, envelope.transport);
        let success = CanonicalSuccess {
            payload,
            request_id: envelope.request_id,
            transport: envelope.transport,
        };
        Ok(Self::success_to_result_frame(&success))
    }

    fn dispatch_ingest_schema_registry_frame(
        frame: &NativeFrame,
        state: &AppState,
        dispatcher: &CommandDispatcher,
    ) -> Result<NativeFrame, CanonicalError> {
        let envelope =
            Self::from_command_frame(frame, CanonicalCommandName::IngestSchemaRegistry, ())?;
        let success = dispatcher.dispatch_ingest_schema_registry(state, &envelope);
        Ok(Self::success_to_result_frame(&success))
    }

    fn dispatch_sql_analyze_frame(
        frame: &NativeFrame,
        dispatcher: &CommandDispatcher,
    ) -> Result<NativeFrame, CanonicalError> {
        let payload_json = frame.payload_json.clone().ok_or_else(|| CanonicalError {
            request_id: frame.request_id.clone(),
            transport: TransportKind::Native,
            kind: "protocol",
            message: "missing payload for sql.analyze frame".to_string(),
        })?;
        let payload: SqlAnalyzeRequest =
            serde_json::from_value(payload_json).map_err(|err| CanonicalError {
                request_id: frame.request_id.clone(),
                transport: TransportKind::Native,
                kind: "serialization",
                message: format!("invalid sql.analyze payload: {err}"),
            })?;
        let envelope = Self::from_command_frame(frame, CanonicalCommandName::SqlAnalyze, payload)?;
        let success = dispatcher.dispatch_sql_analyze(&envelope);
        Ok(Self::success_to_result_frame(&success))
    }

    fn dispatch_sql_route_frame(
        frame: &NativeFrame,
        dispatcher: &CommandDispatcher,
    ) -> Result<NativeFrame, CanonicalError> {
        let payload_json = frame.payload_json.clone().ok_or_else(|| CanonicalError {
            request_id: frame.request_id.clone(),
            transport: TransportKind::Native,
            kind: "protocol",
            message: "missing payload for sql.route frame".to_string(),
        })?;
        let payload: SqlRouteRequest =
            serde_json::from_value(payload_json).map_err(|err| CanonicalError {
                request_id: frame.request_id.clone(),
                transport: TransportKind::Native,
                kind: "serialization",
                message: format!("invalid sql.route payload: {err}"),
            })?;
        let envelope = Self::from_command_frame(frame, CanonicalCommandName::SqlRoute, payload)?;
        let success = dispatcher.dispatch_sql_route(&envelope);
        Ok(Self::success_to_result_frame(&success))
    }

    fn dispatch_sql_execute_route_decision_frame(
        frame: &NativeFrame,
        dispatcher: &CommandDispatcher,
    ) -> Result<NativeFrame, CanonicalError> {
        let payload_json = frame.payload_json.clone().ok_or_else(|| CanonicalError {
            request_id: frame.request_id.clone(),
            transport: TransportKind::Native,
            kind: "protocol",
            message: "missing payload for sql.execute frame".to_string(),
        })?;
        let payload: SqlExecuteRequest =
            serde_json::from_value(payload_json).map_err(|err| CanonicalError {
                request_id: frame.request_id.clone(),
                transport: TransportKind::Native,
                kind: "serialization",
                message: format!("invalid sql.execute payload: {err}"),
            })?;
        let envelope = Self::from_command_frame(frame, CanonicalCommandName::SqlExecute, payload)?;
        let success = dispatcher.dispatch_sql_execute_route_decision(&envelope);
        Ok(NativeFrame {
            frame_type: NativeFrameType::Result,
            request_id: success.request_id,
            session_id: None,
            command: None,
            payload_json: Some(json!({
                "path": route_path_name(success.payload.path),
                "reason": success.payload.reason,
                "statements": success.payload.statements.iter().map(|s| {
                    json!({
                        "statement": s.statement,
                        "path": route_path_name(s.path),
                    })
                }).collect::<Vec<_>>(),
            })),
        })
    }

    fn dispatch_sql_transaction_context_frame(
        frame: &NativeFrame,
        dispatcher: &CommandDispatcher,
    ) -> Result<NativeFrame, CanonicalError> {
        let payload_json = frame.payload_json.clone().ok_or_else(|| CanonicalError {
            request_id: frame.request_id.clone(),
            transport: TransportKind::Native,
            kind: "protocol",
            message: "missing payload for sql.transaction frame".to_string(),
        })?;
        let payload: SqlTransactionRequest =
            serde_json::from_value(payload_json).map_err(|err| CanonicalError {
                request_id: frame.request_id.clone(),
                transport: TransportKind::Native,
                kind: "serialization",
                message: format!("invalid sql.transaction payload: {err}"),
            })?;
        let envelope =
            Self::from_command_frame(frame, CanonicalCommandName::SqlTransaction, payload)?;
        let success = dispatcher.dispatch_sql_transaction_context(&envelope);
        Ok(NativeFrame {
            frame_type: NativeFrameType::Result,
            request_id: success.request_id,
            session_id: None,
            command: None,
            payload_json: Some(json!({
                "statement_count": success.payload.statements.len(),
                "isolation_level": success.payload.isolation_level,
            })),
        })
    }
}

// ─── S8-WS10-02: Driver wire protocol structs ─────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct DriverSession {
    pub(crate) driver_name: String,
    pub(crate) driver_version: String,
    pub(crate) connected_at_ms: u64,
    pub(crate) assigned_node_id: String,
    pub(crate) pooled_connection_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SqlTransactionResponse {
    pub(crate) status: String,
    pub(crate) transaction_id: String,
    pub(crate) statements_executed: usize,
    pub(crate) requires_transaction: bool,
    pub(crate) touches_catalog: bool,
    pub(crate) rejected_statement_count: usize,
    pub(crate) elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PessimisticLockRecord {
    pub(crate) lock_id: String,
    pub(crate) transaction_id: String,
    resource: String,
    owner: String,
    acquired_unix_ms: u128,
    expires_unix_ms: u128,
}

// ─── S5-E4A-01: Connector SDK runtime structs ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ConnectorPlugin {
    connector_id: String,
    connector_type: String,
    version: String,
    signed: bool,
    registered_at_ms: u64,
}

// ─── S11-WS1-12: Connector health structs ───────────────────────────────────

#[derive(Debug, Serialize)]
struct ConnectorHealthEntry {
    connector_id: String,
    connector_type: String,
    version: String,
    signed: bool,
    healthy: bool,
}

#[derive(Debug, Serialize)]
struct ConnectorHealthResponse {
    status: &'static str,
    total: usize,
    healthy: usize,
    entries: Vec<ConnectorHealthEntry>,
}

// REQ-27: Redis-compat cache command DTOs (function + tests stay in main.rs)
#[derive(Deserialize)]
struct RedisCacheCommandRequest {
    cmd: String,
    key: Option<String>,
    value: Option<serde_json::Value>,
    partition_id: Option<String>,
    ttl_ms: Option<u64>,
    expire_ms: Option<u64>,
    delta: Option<f64>,
    keys: Option<Vec<String>>,
    start: Option<i64>,
    stop: Option<i64>,
    field: Option<String>,
}

#[derive(Serialize)]
struct RedisCacheCommandResponse {
    status: &'static str,
    cmd: String,
    value: Option<serde_json::Value>,
    exists: Option<bool>,
    removed: Option<bool>,
    flushed_count: Option<usize>,
    keys: Option<Vec<String>>,
    error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct PoolStatsResponse {
    total_connections: usize,
    idle_connections: usize,
    active_connections: usize,
    failed_connections: usize,
    circuit_breaker_state: String,
    storm_active: bool,
    current_rps: u64,
    total_acquired: u64,
    total_released: u64,
    total_rejected: u64,
    total_circuit_opens: u64,
}

impl NativeListenerConfig {
    fn from_env() -> Self {
        let enabled = read_env_bool("VNG_NATIVE_LISTENER_ENABLED", true);
        let bind = env::var("VNG_NATIVE_BIND")
            .unwrap_or_else(|_| "127.0.0.1:7542".to_string())
            .trim()
            .to_string();
        let tls_enabled = read_env_bool("VNG_NATIVE_TLS_ENABLED", false);
        let tls_cert_path = env::var("VNG_NATIVE_TLS_CERT_PATH")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let tls_key_path = env::var("VNG_NATIVE_TLS_KEY_PATH")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let tls_client_ca_path = env::var("VNG_NATIVE_TLS_CLIENT_CA_PATH")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let max_connections = read_env_usize("VNG_NATIVE_MAX_CONNECTIONS", 2048);
        let idle_timeout_ms = read_env_u64("VNG_NATIVE_IDLE_TIMEOUT_MS", 60000);
        let handshake_timeout_ms = read_env_u64("VNG_NATIVE_HANDSHAKE_TIMEOUT_MS", 5000);
        let heartbeat_interval_ms = read_env_u64("VNG_NATIVE_HEARTBEAT_INTERVAL_MS", 15000);
        let max_frame_bytes = read_env_usize("VNG_NATIVE_MAX_FRAME_BYTES", 1_048_576);
        let compression_enabled = read_env_bool("VNG_NATIVE_COMPRESSION_ENABLED", false);
        let compression_threshold_bytes =
            read_env_usize("VNG_NATIVE_COMPRESSION_THRESHOLD_BYTES", 4096);
        // NT-S6-001: bearer token for native transport Auth frames
        let bearer_token = env::var("VNG_NATIVE_BEARER_TOKEN")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        Self {
            enabled,
            bind,
            tls_enabled,
            tls_cert_path,
            tls_key_path,
            tls_client_ca_path,
            max_connections,
            idle_timeout_ms,
            handshake_timeout_ms,
            heartbeat_interval_ms,
            max_frame_bytes,
            compression_enabled,
            compression_threshold_bytes,
            bearer_token,
        }
    }

    fn validate(self) -> Self {
        if self.max_connections == 0 {
            eprintln!(
                "Invalid VNG_NATIVE_MAX_CONNECTIONS=0; defaulting to 2048 for safety"
            );
            return Self {
                max_connections: 2048,
                ..self
            };
        }
        if self.compression_threshold_bytes > self.max_frame_bytes {
            eprintln!(
                "Invalid native compression threshold > max frame size; defaulting threshold to 4096"
            );
            return Self {
                compression_threshold_bytes: 4096,
                ..self
            };
        }
        self
    }
}

#[tokio::main]
async fn main() {
    // Phase 0.4 — initialise observability before anything else. This sets up
    // tracing (env-filter) and the Prometheus metrics recorder. See
    // `observability.rs` for env vars (VNG_LOG, VNG_LOG_FORMAT).
    observability::init_observability();
    tracing::info!(target: "vng.boot", "voltnuerongridd starting");

    // Phase 0 — load + validate runtime config. Fail fast if the operator
    // selected an unsupported backend (e.g. VNG_STORAGE_ENGINE=vng before the
    // native storage engine is implemented). See `voltnuerongrid-config` and
    // `gaps-may26-1.md` for the rationale.
    let cfg_path = env::var("VNG_CONFIG_PATH").unwrap_or_else(|_| "./vng.config.json".to_string());
    let cfg_file = std::fs::read_to_string(&cfg_path).ok();
    let runtime_config = match voltnuerongrid_config::RuntimeConfig::from_env_and_file(
        &voltnuerongrid_config::ProcessEnv,
        cfg_file.as_deref(),
    ) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(target: "vng.boot", error = %e, "failed to load runtime config");
            eprintln!("VNG config error: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = runtime_config.validate() {
        tracing::error!(target: "vng.boot", error = %e, "runtime config rejected");
        eprintln!("VNG config error: {e}");
        std::process::exit(2);
    }
    tracing::info!(
        target: "vng.boot",
        storage_engine = ?runtime_config.storage.engine,
        sql_engine = ?runtime_config.sql.engine,
        data_dir = %runtime_config.storage.data_dir,
        "runtime config validated"
    );

    let node_id = env::var("VNG_NODE_ID")
        .unwrap_or_else(|_| "node-1".to_string())
        .trim()
        .to_string();
    let cluster_mode = env::var("VNG_CLUSTER_MODE")
        .unwrap_or_else(|_| "single".to_string())
        .trim()
        .to_string();
    let http_bind = env::var("VNG_HTTP_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .trim()
        .to_string();
    let native_listener_config = NativeListenerConfig::from_env().validate();
    let autonomous_mode = AutonomousMode::from_env(
        &env::var("VNG_AUTONOMOUS_MODE").unwrap_or_else(|_| "supervised".to_string()),
    );
    let admin_api_key = env::var("VNG_ADMIN_API_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let emergency_stop = AtomicEmergencyStop::new(
        env::var("VNG_AUTONOMOUS_EMERGENCY_STOP")
            .unwrap_or_else(|_| "false".to_string())
            .trim()
            .eq_ignore_ascii_case("true"),
    );
    let dr_hook_state_path = env::var("VNG_DR_HOOK_STATE_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| Some("state/dr-hook-runtime.json".to_string()));
    let loaded_policy_state = load_dr_hook_policy_state(dr_hook_state_path.as_deref());
    let allowed_operator_roles = Arc::new(load_allowed_operator_roles());
    let operator_role_bindings = Arc::new(load_operator_role_bindings(&allowed_operator_roles));
    let tenant_user_bindings = Arc::new(default_tenant_user_bindings());
    let rbac_privilege_matrix = Arc::new(load_rbac_privilege_matrix());
    let security_config = Arc::new(load_runtime_security_config(&allowed_operator_roles));
    let kms_runtime = Arc::new(Mutex::new(load_kms_runtime_state(&security_config)));
    let addr: SocketAddr = http_bind
        .parse()
        .unwrap_or_else(|_| "127.0.0.1:8080".parse().expect("fallback socket parse"));

    // Phase 2 — pick the durability engine from runtime_config.
    // RocksDB by default; falls back to in-memory only if explicitly
    // configured with `storage.engine = vng` (native engine not yet shipped)
    // or if the rocksdb open fails (which we surface and exit on, not
    // silently degrade — silently dropping durability would violate the
    // whole point of Phase 2).
    let wal_engine_boxed = build_durability_engine(&runtime_config);
    metrics::counter!(
        "vng_durability_engine_boot",
        "engine" => wal_engine_boxed.engine_kind().to_string(),
    ).increment(1);
    let wal_engine = Arc::new(Mutex::new(wal_engine_boxed));
    // Clone before the struct literal consumes `wal_engine` via the field assignment.
    let wal_engine_for_catalog = wal_engine.clone();
    let wal_engine_for_users = wal_engine.clone();
    let wal_engine_for_triggers = wal_engine.clone();

    let (raft_last_applied_tx, _raft_last_applied_rx) = tokio::sync::watch::channel(0u64);
    let raft_last_applied_tx = Arc::new(raft_last_applied_tx);

    let state = AppState {
        node_id: node_id.clone(),
        cluster_mode,
        node_url: Arc::new(std::env::var("VNG_NODE_URL").ok()),
        runtime_config: Arc::new(runtime_config),
        auth: AuthState {
            admin_api_key,
            security_config,
            allowed_operator_roles,
            operator_role_bindings,
            tenant_user_bindings,
            rbac_privilege_matrix,
            kms_runtime,
            db_grants: Arc::new(Mutex::new(HashMap::new())),
            user_store: Arc::new(Mutex::new({
                let mut us = crate::user_store::UserStore::new();
                crate::helpers::boot::replay_user_store_into(&mut us, &wal_engine_for_users);
                us
            })),
            session_store: Arc::new(Mutex::new(crate::user_store::SessionStore::new())),
            session_signer: Arc::new(Mutex::new({
                let secret = std::env::var("VNG_SESSION_SECRET")
                    .or_else(|_| std::env::var("VNG_CLUSTER_TOKEN"))
                    .unwrap_or_else(|_| "vng-default-session-secret".to_string());
                crate::user_store::SessionSigner::new(&secret, 86_400)
            })),
        },
        cluster: ClusterState {
            leader_node_id: Arc::new(Mutex::new("node-1".to_string())),
            cluster_nodes: Arc::new(Mutex::new(initial_cluster_nodes(&node_id))),
            cluster_failure_signals: Arc::new(Mutex::new(Vec::new())),
            raft_state: Arc::new(Mutex::new(RaftNode::new(&node_id))),
            raft_peers: Arc::new(load_raft_peers()),
            cluster_token: Arc::new(load_cluster_token()),
            raft_last_applied_tx: raft_last_applied_tx.clone(),
            current_leader_url: Arc::new(Mutex::new(None)),
            snapshot_chunk_sessions: Arc::new(Mutex::new(HashMap::new())),
            sync_origin: Arc::new(Mutex::new(RowStoreSyncOrigin::new())),
            replication_transport: Arc::new(Mutex::new(InMemoryReplicationTransport::new())),
            replica_replay_states: Arc::new(Mutex::new(HashMap::new())),
            htap_peer_cursors: Arc::new(Mutex::new(HashMap::new())),
        },
        storage: StorageState {
            row_store: Arc::new(Mutex::new({
                let mut rs = PagedRowStore::default();
                let row_cap: usize = std::env::var("VNG_ROW_STORE_MAX_ROWS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                if row_cap > 0 {
                    rs.set_max_rows_cap(row_cap);
                }
                let (use_rocksdb, max_xid) = {
                    let guard = wal_engine.lock().expect("wal_engine lock for boot persists_rows");
                    (guard.persists_rows(), guard.max_row_xid())
                };
                if use_rocksdb {
                    rs.fast_forward_xid(max_xid + 1);
                } else {
                    replay_dml_into(&mut rs, &wal_engine);
                }
                rs
            })),
            wal_engine: Arc::clone(&wal_engine),
            olap_store: Arc::new(Mutex::new(HashMap::new())),
            ddl_catalog: Arc::new(Mutex::new({
                let mut cat = DdlCatalog::new();
                replay_ddl_into(&mut cat, &wal_engine);
                cat
            })),
            database_catalog: Arc::new(Mutex::new({
                let mut db_cat = voltnuerongrid_meta::DatabaseCatalog::new();
                replay_database_catalog_into(&mut db_cat, &wal_engine_for_catalog);
                db_cat
            })),
            acid_transactions: Arc::new(Mutex::new(AcidTransactionRegistry::default())),
            index_manager: Arc::new(Mutex::new(IndexManager::new())),
            constraint_manager: Arc::new(Mutex::new(ConstraintManager::new())),
            tx_undo_log: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            db_semaphores: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            table_stats: Arc::new(Mutex::new(std::collections::HashMap::new())),
            stats_registry: Arc::new(Mutex::new(voltnuerongrid_exec::StatsRegistry::new())),
            connection_tx_active: Arc::new(Mutex::new(HashMap::new())),
            proc_registry: {
                let mut reg = helpers::stored_proc::ProcedureRegistry::new();
                reg.register_builtins();
                Arc::new(Mutex::new(reg))
            },
            trigger_registry: Arc::new(Mutex::new({
                let mut reg = TriggerRegistry::new();
                replay_triggers_into(&mut reg, &wal_engine_for_triggers);
                reg
            })),
            trigger_emitter: Arc::new(LoggingTriggerEmitter),
            partition_registry: Arc::new(Mutex::new(HashMap::new())),
            partition_segments: Arc::new(Mutex::new(HashMap::new())),
            shard_registry: Arc::new(Mutex::new(HashMap::new())),
            pessimistic_locks: Arc::new(Mutex::new(HashMap::new())),
            pessimistic_lock_waits: Arc::new(Mutex::new(HashMap::new())),
            pessimistic_lock_metrics: PessimisticLockContentionMetrics::new(),
            cdc_cursors: Arc::new(Mutex::new(HashMap::new())),
        },
        ingest: IngestState {
            ingest_csv_records: Arc::new(Mutex::new(HashMap::new())),
            ingest_json_records: Arc::new(Mutex::new(HashMap::new())),
            ingest_parquet_records: Arc::new(Mutex::new(HashMap::new())),
            ingest_excel_records: Arc::new(Mutex::new(HashMap::new())),
            ingest_outbox_streams: Arc::new(Mutex::new(HashMap::new())),
            ingest_event_bus: Arc::new(Mutex::new(load_ingest_event_bus())),
            ingest_outbox_cursors: Arc::new(Mutex::new(load_ingest_outbox_cursor_store())),
            broker_flush_counts: Arc::new(Mutex::new(HashMap::new())),
            connector_registry: Arc::new(Mutex::new(Vec::new())),
        },
        ai: AiState {
            autonomous_mode,
            emergency_stop: Arc::new(emergency_stop),
            guardrails: Arc::new(default_guardrail_rules()),
            model_gateway_policy: Arc::new(Mutex::new(ModelGatewayPolicy::default())),
            ai_request_counters: Arc::new(Mutex::new(HashMap::new())),
            ai_rate_window_starts: Arc::new(Mutex::new(HashMap::new())),
            self_heal_counters: Arc::new(Mutex::new(SelfHealCounters {
                actions_this_hour: 0,
                window_start_ms: 0,
                max_per_hour: env::var("VNG_MAX_SELF_HEAL_PER_HOUR")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10),
            })),
            slow_query_log: Arc::new(Mutex::new(VecDeque::with_capacity(1000))),
            tune_recommendations: Arc::new(Mutex::new(Vec::new())),
            dek_versions: Arc::new(Mutex::new(Vec::new())),
            cert_fingerprint: Arc::new(Mutex::new(
                env::var("VNG_TLS_CERT_PATH").ok()
                    .filter(|v| !v.trim().is_empty())
                    .and_then(|p| std::fs::read(&p).ok())
                    .map(|bytes| compute_sha256_fingerprint(&bytes)),
            )),
            diagnosis_rules: Arc::new(Mutex::new(
                load_diagnosis_rules_from_state(
                    env::var("VNG_DR_HOOK_STATE_PATH")
                        .ok()
                        .or_else(|| Some("state/dr-hook-runtime.json".to_string()))
                        .as_deref(),
                ),
            )),
            chat_sql_counters: Arc::new(Mutex::new(HashMap::new())),
            action_records: Arc::new(Mutex::new(Vec::new())),
        },
        ops: OpsState {
            audit_sink: Arc::new(Mutex::new(AppendOnlyAuditSink::new())),
            audit_log_path: std::env::var("VNG_AUDIT_LOG_PATH").ok(),
            operational_events: Arc::new(Mutex::new(helpers::op_events::OperationalEventStream::new())),
            dr_hook_records: Arc::new(Mutex::new(Vec::new())),
            dr_hook_policy_state: Arc::new(Mutex::new(loaded_policy_state)),
            dr_hook_policy_config: Arc::new(default_dr_hook_policy_config()),
            dr_hook_state_path,
            dr_hook_queue: Arc::new(Mutex::new(VecDeque::new())),
            chaos_state: Arc::new(Mutex::new(ChaosState::default())),
            autoscale_policy: Arc::new(Mutex::new(handlers::autoscale::AutoscalePolicy::default())),
            autoscale_status: Arc::new(Mutex::new(handlers::autoscale::AutoscaleStatus::default())),
            plugin_lifecycle: Arc::new(Mutex::new(PluginLifecycleManager::new(256))),
            plugin_registry: Arc::new(Mutex::new(helpers::plugins::PluginRegistry::new())),
            udf_registry: Arc::new(Mutex::new(UdfRegistry::new())),
            distributed_cache: Arc::new(Mutex::new(DistributedCacheManager::with_default_policy())),
            cache_snapshot_path: Arc::new(
                std::env::var("VNG_CACHE_SNAPSHOT_PATH")
                    .unwrap_or_else(|_| "state/cache-snapshot.json".to_string())
            ),
            driver_pool: Arc::new(Mutex::new(ConnectionPoolManager::with_default_policy())),
            driver_sessions: Arc::new(Mutex::new(HashMap::new())),
            tde_override: Arc::new(Mutex::new(None)),
            vector_index: Arc::new(Mutex::new(helpers::vector::VectorIndex::new())),
            fts_index: Arc::new(Mutex::new(helpers::fts::FtsIndex::new())),
            geo_index: Arc::new(Mutex::new(helpers::geo::GeoIndex::new())),
        },
    };

    tokio::spawn(run_dr_hook_scheduler(state.clone()));
    tokio::spawn(crate::handlers::autonomous_ctl::run_ops_agent_scheduler(state.clone()));

    // CACHE-1: Restore distributed cache from on-disk snapshot (if available).
    crate::handlers::misc::load_cache_snapshot(&state);

    // P1: On boot, load persisted rows from the durability engine into
    // PagedRowStore as a warm read-through cache.
    //
    // - RocksDB engine (persists_rows() == true): call load_persisted_rows_into
    //   to populate PagedRowStore with rows from per-DB CFs.  The XID counter
    //   was already fast-forwarded above so MVCC snapshot scans are correct.
    //   This also enables SERIALIZABLE conflict detection to work across restarts.
    //
    // - In-memory engine (persists_rows() == false): load from legacy WAL text
    //   files (scan_persisted_rows scans the in-memory accumulated record set).
    {
        let persists_rows = state.storage.wal_engine
            .lock()
            .expect("wal_engine lock for boot persists_rows check")
            .persists_rows();
        // Both paths call load_persisted_rows_into — for RocksDB it reads from
        // per-DB CFs; for in-memory it reads the accumulated legacy records.
        helpers::boot::load_persisted_rows_into(&state.storage.row_store, &state.storage.wal_engine);
        if persists_rows {
            tracing::info!(target: "vng.durability", "P1: warm row cache loaded from RocksDB per-DB CFs");
        }
    }

    // M-3: Restore committed serializable write-sets from the previous process run.
    // Without this, cross-restart serializable isolation is only correct within a
    // single process lifetime — a transaction committed just before a crash would
    // not conflict-check correctly with transactions starting after the restart.
    {
        let data_dir = state.runtime_config.storage.data_dir.clone();
        let sets = helpers::raft_loop::load_committed_write_sets(&data_dir);
        if !sets.is_empty() {
            let mut acid = state.storage.acid_transactions.lock().expect("acid restore lock");
            let restored = sets.len();
            for ws in sets {
                acid.transactions.insert(
                    ws.tx_id.clone(),
                    AcidTxEntry {
                        transaction_id: ws.tx_id,
                        assigned_node_id: "restored".to_string(),
                        state: AcidTxState::Committed,
                        isolation_level: "serializable".to_string(),
                        started_at_unix_ms: ws.committed_at_ms,
                        completed_at_unix_ms: Some(ws.committed_at_ms),
                        statement_count: 0,
                        affected_tables: Vec::new(),
                        savepoints: Vec::new(),
                        read_snapshot_at_ms: None,
                        wal_log: Vec::new(),
                        row_store_snapshot_xid: None,
                        written_row_keys: ws.written_keys.into_iter().collect(),
                        read_row_keys: ws.read_keys.into_iter().collect(),
                    },
                );
            }
            tracing::info!(
                target: "vng.acid",
                count = restored,
                "serializable write-sets restored from disk for cross-restart SSI"
            );
        }
    }

    // H-2: Restore durable Raft state (current_term, voted_for, log entries)
    // from the previous process run.  This prevents a restarted node from
    // double-voting in an old term or losing committed log entries.
    {
        let data_dir = state.runtime_config.storage.data_dir.clone();
        if let Some(durable) = helpers::raft_loop::load_raft_state(&data_dir) {
            let mut node = state.cluster.raft_state.lock().expect("raft restore lock");
            let log_len = durable.log.len();
            let restored_term = durable.current_term;
            node.restore_durable(durable);
            tracing::info!(
                target: "vng.raft",
                current_term = restored_term,
                log_length = log_len,
                "raft durable state restored from disk"
            );
        }
    }

    tokio::spawn(run_raft_tick_loop(state.clone()));

    // Gap #6 + L-5: Background Parquet flush task — exports row data to disk for OLAP queries.
    // L-5: Run one immediate flush at startup so OLAP queries have Parquet files from the
    // first request instead of waiting up to `flush_interval_secs` (default 60 s).
    {
        let flush_state = state.clone();
        let flush_interval_secs = std::env::var("VNG_PARQUET_FLUSH_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(60);
        let data_dir = state.runtime_config.storage.data_dir.clone();

        // L-5: immediate startup flush — populates Parquet before the first OLAP query.
        if !data_dir.is_empty() {
            let startup_rows = {
                let rs = state.storage.row_store.lock().expect("row_store parquet startup flush");
                let xid = rs.current_xid();
                rs.scan_at_snapshot(xid)
                    .into_iter()
                    .map(|(k, d)| (k.to_string(), d.clone()))
                    .collect::<Vec<_>>()
            };
            let flushed = helpers::parquet_flush::flush_rows_to_parquet(&startup_rows, &data_dir);
            if flushed > 0 {
                tracing::info!(
                    target: "vng.parquet",
                    tables = flushed,
                    "parquet startup flush complete (L-5)"
                );
            }
        }

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                std::time::Duration::from_secs(flush_interval_secs)
            );
            interval.tick().await; // skip the first immediate tick (startup flush already ran)
            loop {
                interval.tick().await;
                let rows = {
                    let rs = flush_state.storage.row_store.lock().expect("row_store parquet flush");
                    let xid = rs.current_xid();
                    rs.scan_at_snapshot(xid)
                        .into_iter()
                        .map(|(k, d)| (k.to_string(), d.clone()))
                        .collect::<Vec<_>>()
                };
                let flushed = helpers::parquet_flush::flush_rows_to_parquet(&rows, &data_dir);
                if flushed > 0 {
                    tracing::info!(
                        target: "vng.parquet",
                        tables = flushed,
                        "parquet flush complete"
                    );
                }
            }
        });
    }

    let app = build_router(state.clone());
    if native_listener_config.enabled {
        vng_native_listener_log(
            "listener_config",
            json!({
                "bind": native_listener_config.bind,
                "tls_enabled": native_listener_config.tls_enabled,
                "tls_client_ca_configured": native_listener_config.tls_client_ca_path.is_some(),
                "max_connections": native_listener_config.max_connections,
                "max_frame_bytes": native_listener_config.max_frame_bytes,
                "compression_enabled": native_listener_config.compression_enabled,
                "compression_threshold_bytes": native_listener_config.compression_threshold_bytes,
                "idle_timeout_ms": native_listener_config.idle_timeout_ms,
                "handshake_timeout_ms": native_listener_config.handshake_timeout_ms,
                "heartbeat_interval_ms": native_listener_config.heartbeat_interval_ms,
            }),
        );
        let nl_state = state.clone();
        let nl_cfg = native_listener_config.clone();
        tokio::spawn(run_native_listener(nl_cfg, nl_state));
    } else {
        println!("native listener disabled (set VNG_NATIVE_LISTENER_ENABLED=true to enable)");
    }

    println!("voltnuerongridd listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind listener");
    axum::serve(listener, app).await.expect("server failed");

    // Flush any in-flight OTEL spans before the process exits.
    observability::shutdown_otel();
}

pub(crate) async fn native_read_framed<S: AsyncRead + Unpin>(
    socket: &mut S,
    max_payload_bytes: usize,
) -> Result<Vec<u8>, std::io::Error> {
    let mut len_buf = [0u8; 4];
    socket.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > max_payload_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("native frame exceeds max_payload_bytes ({len} > {max_payload_bytes})"),
        ));
    }
    let mut buf = vec![0u8; len];
    socket.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn native_write_framed_json<S: AsyncWrite + Unpin>(
    socket: &mut S,
    value: &serde_json::Value,
) -> Result<(), std::io::Error> {
    let payload = serde_json::to_vec(value).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("native wire json encode failed: {e}"),
        )
    })?;
    let len_u32 = u32::try_from(payload.len()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "native wire payload length overflow",
        )
    })?;
    socket.write_all(&len_u32.to_be_bytes()).await?;
    socket.write_all(&payload).await?;
    Ok(())
}

pub(crate) async fn run_native_connection<S: AsyncRead + AsyncWrite + Send + Unpin>(
    mut socket: S,
    state: AppState,
    config: NativeListenerConfig,
) {
    let dispatcher = CommandDispatcher::new();
    let idle = Duration::from_millis(config.idle_timeout_ms.max(1000));
    loop {
        let frame_bytes = match tokio::time::timeout(idle, native_read_framed(&mut socket, config.max_frame_bytes)).await
        {
            Ok(Ok(b)) => b,
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Ok(Err(err)) => {
                vng_native_listener_log(
                    "read_error",
                    json!({ "message": err.to_string(), "kind": format!("{:?}", err.kind()) }),
                );
                break;
            }
            Err(_) => {
                vng_native_listener_log("read_idle_timeout", json!({ "idle_timeout_ms": config.idle_timeout_ms }));
                break;
            }
        };
        let value: serde_json::Value = match serde_json::from_slice(&frame_bytes) {
            Ok(v) => v,
            Err(err) => {
                let err_frame = wire_protocol_error_frame(
                    "decode-error",
                    &format!("invalid json frame: {err}"),
                );
                let _ = native_write_framed_json(&mut socket, &err_frame).await;
                continue;
            }
        };
        let frame_type = value
            .get("frame_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let request_id = value
            .get("request_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        match frame_type {
            "Hello" => {
                let session_hint = value.get("payload").and_then(|p| p.get("session_id"));
                let ack = native_wire_hello_ack(&request_id, session_hint);
                if native_write_framed_json(&mut socket, &ack).await.is_err() {
                    break;
                }
            }
            "Auth" => {
                let payload = value.get("payload").cloned().unwrap_or(json!({}));
                let ok = native_auth_payload_matches_runtime(&state, &config, &payload);
                let sess = value
                    .get("session_id")
                    .and_then(|v| v.as_str());
                let ack = native_wire_auth_ack(&request_id, sess, ok);
                if native_write_framed_json(&mut socket, &ack).await.is_err() {
                    break;
                }
                if !ok {
                    break;
                }
            }
            "Command" => {
                let internal = match wire_json_to_native_dispatch_frame(&value) {
                    Ok(f) => f,
                    Err(msg) => {
                        let err_frame = wire_protocol_error_frame(&request_id, &msg);
                        if native_write_framed_json(&mut socket, &err_frame).await.is_err() {
                            break;
                        }
                        continue;
                    }
                };
                let out = NativeAdapter::dispatch_frame(&internal, &state, &dispatcher);
                let wire = internal_native_frame_to_driver_wire_json(&out, "v1");
                if native_write_framed_json(&mut socket, &wire).await.is_err() {
                    break;
                }
            }
            other => {
                let err_frame = wire_protocol_error_frame(
                    &request_id,
                    &format!("unsupported frame_type for data plane: {other}"),
                );
                if native_write_framed_json(&mut socket, &err_frame).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Parse a SQL DELETE statement and return the row key to tombstone.
/// Pattern: DELETE FROM <table> WHERE <col> = '<key>'
/// Demo helper for the studio's "Generate N rows" button.
///
/// Recognises `CALL insert_rows('<table>', <count>)` and writes synthetic rows.
/// This is **not** a real stored-procedure runtime — column values are produced
/// by name-based heuristics (`*_id` → row index, `*_name` → templated string,
/// `*_status` → cycle through a fixed list, etc.).
///
/// Returns `Some(...)` if the request matched the demo pattern (caller should
/// short-circuit with that result). Returns `None` if the request is not a
/// demo CALL — let normal SQL execution take over.
///
/// **Tracked gap:** §4.3 in `gaps-may26-1.md`. Replace once `CREATE PROCEDURE`
/// and the UDF runtime are wired through the SQL parser/executor properly.
#[cfg(feature = "demo")]
pub(crate) fn try_handle_call_insert_rows_demo(
    state: &AppState,
    _headers: &HeaderMap,
    principal: &RuntimeAccessPrincipal,
    connection_id: &str,
    req: &SqlExecuteRequest,
    db: &str,
) -> Option<Result<(StatusCode, Json<SqlExecuteResponse>), (StatusCode, Json<AuthErrorResponse>)>> {
    let raw = req.sql_batch.trim();
    let upper = raw.to_ascii_uppercase();
    if !upper.starts_with("CALL ") {
        return None;
    }
    let inner = raw[5..].trim();
    let fn_lower = inner.to_ascii_lowercase();
    if !(fn_lower.starts_with("insert_rows(") || fn_lower.starts_with("insert_rows (")) {
        return None;
    }
    let (open, close) = match (inner.find('('), inner.rfind(')')) {
        (Some(o), Some(c)) if c > o => (o, c),
        _ => return None,
    };
    let args_str = inner[open + 1..close].trim();
    let mut parts: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut in_quote = false;
    for ch in args_str.chars() {
        match ch {
            '\'' | '"' => { in_quote = !in_quote; buf.push(ch); }
            ',' if !in_quote => { parts.push(buf.trim().to_string()); buf.clear(); }
            _ => buf.push(ch),
        }
    }
    if !buf.trim().is_empty() { parts.push(buf.trim().to_string()); }
    if parts.len() != 2 {
        return None;
    }
    let table_name = parts[0].trim().trim_matches('\'').trim_matches('"').to_ascii_lowercase();
    let num_records: usize = parts[1].trim().parse().unwrap_or(0);
    let start_ms = now_unix_ms();

    // Determine next row id (highest existing rowid for this table + 1).
    // Lock acquisition uses match-on-Result so a poisoned mutex returns 503
    // instead of taking the whole service down (fixes pattern from .cursorrules).
    let scan_prefix = crate::helpers::sql_parse::make_table_scan_prefix(db, &table_name);
    let existing_max: usize = match state.storage.row_store.lock() {
        Ok(rs) => {
            let snap = rs.current_xid();
            rs.scan_at_snapshot(snap)
                .iter()
                .filter(|(k, _)| k.starts_with(&scan_prefix))
                .filter_map(|(k, _)| k.splitn(2, ':').nth(1).and_then(|v| v.parse::<usize>().ok()))
                .max()
                .unwrap_or(0)
        }
        Err(_) => return Some(Ok(svc_unavailable_sql_response("row_store mutex poisoned"))),
    };
    let ddl_cols = match state.storage.ddl_catalog.lock() {
        Ok(catalog) => catalog
            .get(&table_name)
            .map(|e| extract_column_names_from_ddl(&e.original_statement))
            .unwrap_or_default(),
        Err(_) => return Some(Ok(svc_unavailable_sql_response("ddl_catalog mutex poisoned"))),
    };
    let col_count = if ddl_cols.is_empty() { 3 } else { ddl_cols.len() };
    let mut inserted = 0usize;
    for i in 1..=num_records {
        let row_id = existing_max + i;
        let key = crate::helpers::sql_parse::make_row_key(db, &table_name, &row_id.to_string());
        let mut row_data: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        row_data.insert("__table".to_string(), table_name.clone());
        for col_idx in 0..col_count {
            let col_name = ddl_cols.get(col_idx).cloned().unwrap_or_else(|| format!("col_{col_idx}"));
            let val = synthesize_demo_value(&col_name, &table_name, row_id);
            row_data.insert(col_name, val);
        }
        match state.storage.row_store.lock() {
            Ok(mut rs) => {
                let xid = rs.begin_xid();
                rs.insert(xid, &key, row_data);
                inserted += 1;
            }
            Err(_) => return Some(Ok(svc_unavailable_sql_response("row_store mutex poisoned"))),
        }
    }
    let elapsed = now_unix_ms().saturating_sub(start_ms);
    append_runtime_audit_event(
        state, AuditEventKind::Sql, principal, "sql_execute", "ok",
        json!({ "call": "insert_rows", "table": table_name, "inserted": inserted, "demo": true }),
    );
    release_sql_data_plane_connection(state, connection_id);
    let udf_function_catalog = udf_function_catalog_contract();
    let udf_guard_policies = udf_guard_policy_contract();
    let udf_execution_plan = build_udf_execution_plan(&req.sql_batch);
    Some(Ok((StatusCode::OK, Json(SqlExecuteResponse {
        status: "ok".to_string(),
        route_path: "oltp".to_string(),
        reason: format!("inserted {inserted} demo rows into {table_name}"),
        transaction: Some(SqlTransactionResponse {
            status: "committed".to_string(),
            transaction_id: format!("call-{start_ms}"),
            statements_executed: inserted,
            requires_transaction: false,
            touches_catalog: false,
            rejected_statement_count: 0,
            elapsed_ms: elapsed,
        }),
        olap: None,
        rejected_statement_count: 0,
        udf_results: None,
        udf_guardrail_status: Some("passed".to_string()),
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
    }))))
}

/// Heuristic value generator for the insert_rows demo.
/// Pure function — no state, easy to unit-test.
/// Also used by the Studio "Generate N rows" UI endpoint (a real feature),
/// so this helper is always compiled regardless of the `demo` feature.
pub(crate) fn synthesize_demo_value(col_name: &str, table_name: &str, row_id: usize) -> String {
    if col_name.ends_with("_id") || col_name == "id" {
        row_id.to_string()
    } else if col_name.contains("name") {
        format!("Generated {table_name} {row_id}")
    } else if col_name.contains("date") || col_name.contains("_at") {
        format!("2024-{:02}-{:02}", (row_id % 12) + 1, (row_id % 28) + 1)
    } else if col_name.contains("price") || col_name.contains("amount") || col_name.contains("cost") {
        format!("{:.2}", 10.0 + (row_id as f64) * 0.5)
    } else if col_name.contains("qty") || col_name.contains("count") || col_name.contains("level") {
        ((row_id % 500) + 1).to_string()
    } else if col_name.contains("email") {
        format!("gen{row_id}@example.com")
    } else if col_name.contains("phone") {
        format!("555-{row_id:04}")
    } else if col_name.contains("status") {
        ["active", "pending", "done", "cancelled"][row_id % 4].to_string()
    } else if col_name.contains("rating") {
        ((row_id % 5) + 1).to_string()
    } else if col_name.contains("comment") || col_name.contains("description") || col_name.contains("body") {
        format!("Auto-generated record {row_id} for {table_name}")
    } else {
        format!("value_{row_id}")
    }
}

pub(crate) fn route_path_name(path: QueryPath) -> &'static str {
    match path {
        QueryPath::Oltp => "oltp",
        QueryPath::Olap => "olap",
        QueryPath::Hybrid => "hybrid",
        QueryPath::Unknown => "unknown",
    }
}

pub(crate) fn latest_dr_hook_records(state: &AppState, max_items: usize) -> Vec<DrHookExecutionRecord> {
    match state.ops.dr_hook_records.lock() {
        Ok(records) => {
            let len = records.len();
            let start = len.saturating_sub(max_items);
            records[start..].to_vec()
        }
        Err(_) => Vec::new(),
    }
}

// ─── AI-5: SHA-256 fingerprint helper ────────────────────────────────────────

pub(crate) fn compute_sha256_fingerprint(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(bytes);
    hash.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

// ─── AI-6: Load configurable diagnosis rules from extended dr-hook JSON ───────

#[derive(Deserialize)]
struct DiagnosisRulesEnvelope {
    #[serde(default)]
    diagnosis_rules: Vec<DiagnosisRule>,
}

pub(crate) fn load_diagnosis_rules_from_state(path: Option<&str>) -> Vec<DiagnosisRule> {
    let Some(p) = path else { return Vec::new() };
    let Ok(contents) = std::fs::read_to_string(p) else { return Vec::new() };
    serde_json::from_str::<DiagnosisRulesEnvelope>(&contents)
        .map(|e| e.diagnosis_rules)
        .unwrap_or_default()
}

// ─── S6-001: Object-scoped query history endpoint ─────────────────────────────

#[derive(Debug, Deserialize)]
struct ObjectHistoryQuery {
    table: Option<String>,
    schema: Option<String>,
    database: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ObjectHistoryEntry {
    sql: String,
    executed_at_ms: u128,
    status: String,
}

#[derive(Debug, Serialize)]
struct ObjectHistoryResponse {
    entries: Vec<ObjectHistoryEntry>,
    total: usize,
}

// ─── S6-002: Dump structure and data streaming endpoint ───────────────────────

#[derive(Debug, Deserialize)]
struct DumpExportQuery {
    table: Option<String>,
    format: Option<String>,
    limit: Option<u64>,
}

#[derive(Debug, Serialize)]
struct DumpExportResponse {
    format: String,
    content: String,
    rows_exported: u64,
    warning: String,
}


// ─── S6-005: Full-text search endpoint (feature-gated) ───────────────────────

#[derive(Debug, Deserialize)]
struct FulltextSearchRequest {
    query: String,
    table: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct FulltextSearchResponse {
    hits: Vec<serde_json::Value>,
    total: usize,
}

#[derive(Debug, Serialize)]
struct FulltextNotEnabledError {
    error: String,
    enable_with: String,
}

#[cfg(test)]
mod tests;
