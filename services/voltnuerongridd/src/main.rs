use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Semaphore;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{Next, from_fn};
use axum::http::Request;
use axum::response::Response;
use base64::Engine;
use axum::routing::{get, post, options};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use voltnuerongrid_auth::{
    ConfiguredKmsProviderAdapter, KmsKeyResolution,
    PrivilegeAction, RbacPrivilegeMatrix, ResourceGrant, SecurityConfigContract,
};
use voltnuerongrid_audit::{AppendOnlyAuditSink, AuditEvent, AuditEventKind};
use voltnuerongrid_ai::{AutonomousActionDecision, AutonomousActionExecutionRecord};
use voltnuerongrid_exec::{HtapQueryRouter, QueryPath};
use voltnuerongrid_sql::{
    eval_legacy_numeric_aggregation, I18nCatalog, SqlAnalyzer, SqlStatementKind, SupportedLocale,
};
use voltnuerongrid_sql::legacy_aggregations::SUPPORTED_LEGACY_AGGREGATIONS;
use voltnuerongrid_store::htap_sync::{
    InMemoryReplicationTransport, MutationOp, ReplicaReplayState, RowStoreSyncOrigin,
};
use voltnuerongrid_store::constraints::ConstraintManager;
use voltnuerongrid_store::ddl_catalog::{parse_ddl_info, CatalogResult, DdlCatalog};
use voltnuerongrid_store::index::IndexManager;
use voltnuerongrid_store::mvcc::PagedRowStore;
use voltnuerongrid_store::{BoxedDurabilityEngine, DurabilityConfig};
use voltnuerongrid_mcp::{McpRequest, McpServerCapabilities, process_request};
use voltnuerongrid_driver_rust::{ConnectionPoolManager, PoolAcquireError};
use voltnuerongrid_ingest::{
    IngestionConnector, ManagedEventBusTransport, ManagedReplayCursorStore,
    ReplayCursorStore, StreamDirection,
};
use voltnuerongrid_opt::DistributedCacheManager;
use voltnuerongrid_plugins::PluginLifecycleManager;

mod raft;
use raft::{RaftAppendRequest, RaftAppendResponse, RaftLogEntry, RaftNode, RaftRole, RaftStatusSnapshot, RaftVoteRequest, RaftVoteResponse};

pub mod resilience;
pub mod observability;
pub(crate) mod auth;
pub(crate) mod config_init;
pub(crate) mod audit_helpers;
pub(crate) mod handlers;
use auth::*;
use config_init::*;
use audit_helpers::*;
use handlers::cdc::*;
use handlers::catalog::*;
use handlers::autonomous::*;
use handlers::security::*;
use handlers::admin::*;
use handlers::driver::*;
use handlers::ingest::*;
use handlers::sql::*;
use handlers::sre::*;
use handlers::store::*;
use handlers::wal::*;
use handlers::audit::*;
use handlers::rows::*;
use handlers::raft::*;
use handlers::misc::*;

static TX_COUNTER: AtomicU64 = AtomicU64::new(1);
static ACTION_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);
pub(crate) static DR_HOOK_COUNTER: AtomicU64 = AtomicU64::new(1);
static PESSIMISTIC_LOCK_COUNTER: AtomicU64 = AtomicU64::new(1);
/// REQ-22 / WS22: Gate-export counters (incremented in `acquire_pessimistic_lock` for trend artifacts).
static WS22_GATE_DEADLOCK_DETECTIONS: AtomicU64 = AtomicU64::new(0);
static WS22_GATE_SCAN_CAP_TIMEOUTS: AtomicU64 = AtomicU64::new(0);
pub(crate) static DRIVER_SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);
const DEADLOCK_SCAN_MAX_HOPS: usize = 8;

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
}

#[derive(Default)]
pub(crate) struct AcidTransactionRegistry {
    pub(crate) transactions: HashMap<String, AcidTxEntry>,
}

impl AcidTransactionRegistry {
    fn begin(&mut self, tx_id: &str, assigned_node_id: &str, isolation_level: &str, now_ms: u128) {
        let read_snapshot_at_ms = if isolation_level == "repeatable_read" {
            Some(now_ms)
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
                row_store_snapshot_xid: None,
            },
        );
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

    /// REQ-23: For serializable isolation, check whether any other Active serializable
    /// transaction has already written to the same table(s) as `tx_id`.
    /// Returns the conflicting table name if a conflict is found, otherwise None.
    fn check_serializable_conflict(&self, tx_id: &str) -> Option<String> {
        let entry = self.transactions.get(tx_id)?;
        if entry.isolation_level != "serializable" {
            return None;
        }
        if entry.affected_tables.is_empty() {
            return None;
        }
        for (other_id, other) in &self.transactions {
            if other_id == tx_id {
                continue;
            }
            if other.state != AcidTxState::Active {
                continue;
            }
            if other.isolation_level != "serializable" {
                continue;
            }
            for table in &entry.affected_tables {
                if other.affected_tables.contains(table) {
                    return Some(table.clone());
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
enum DeadlockScanOutcome {
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


#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) node_id: String,
    pub(crate) cluster_mode: String,
    pub(crate) admin_api_key: Option<String>,
    pub(crate) security_config: Arc<SecurityConfigContract>,
    pub(crate) allowed_operator_roles: Arc<HashSet<OperatorRole>>,
    pub(crate) operator_role_bindings: Arc<HashMap<String, OperatorRole>>,
    pub(crate) tenant_user_bindings: Arc<HashMap<String, TenantUserBinding>>,
    pub(crate) rbac_privilege_matrix: Arc<RbacPrivilegeMatrix>,
    pub(crate) kms_runtime: Arc<Mutex<KmsRuntimeState>>,
    pub(crate) leader_node_id: Arc<Mutex<String>>,
    pub(crate) cluster_nodes: Arc<Mutex<HashMap<String, ClusterNodeRuntime>>>,
    pub(crate) audit_sink: Arc<Mutex<AppendOnlyAuditSink>>,
    pub(crate) action_records: Arc<Mutex<Vec<AutonomousActionExecutionRecord>>>,
    pub(crate) dr_hook_records: Arc<Mutex<Vec<DrHookExecutionRecord>>>,
    pub(crate) dr_hook_policy_state: Arc<Mutex<DrHookPolicyState>>,
    pub(crate) dr_hook_policy_config: Arc<DrHookPolicyConfig>,
    pub(crate) dr_hook_state_path: Option<String>,
    pub(crate) dr_hook_queue: Arc<Mutex<VecDeque<DrHookScheduledTask>>>,
    pub(crate) cluster_failure_signals: Arc<Mutex<Vec<ClusterFailureSignal>>>,
    pub(crate) sync_origin: Arc<Mutex<RowStoreSyncOrigin>>,
    pub(crate) replication_transport: Arc<Mutex<InMemoryReplicationTransport>>,
    pub(crate) replica_replay_states: Arc<Mutex<HashMap<String, ReplicaReplayState>>>,
    pub(crate) pessimistic_locks: Arc<Mutex<HashMap<String, PessimisticLockRecord>>>,
    pub(crate) pessimistic_lock_waits: Arc<Mutex<HashMap<String, String>>>,
    pub(crate) pessimistic_lock_metrics: PessimisticLockContentionMetrics,
    pub(crate) index_manager: Arc<Mutex<IndexManager>>,
    pub(crate) constraint_manager: Arc<Mutex<ConstraintManager>>,
    pub(crate) ingest_csv_records: Arc<Mutex<HashMap<String, Vec<voltnuerongrid_ingest::IngestRecord>>>>,
    pub(crate) ingest_json_records: Arc<Mutex<HashMap<String, Vec<voltnuerongrid_ingest::IngestRecord>>>>,
    pub(crate) ingest_parquet_records: Arc<Mutex<HashMap<String, Vec<voltnuerongrid_ingest::IngestRecord>>>>,
    pub(crate) ingest_excel_records: Arc<Mutex<HashMap<String, Vec<voltnuerongrid_ingest::IngestRecord>>>>,
    pub(crate) ingest_outbox_streams: Arc<Mutex<HashMap<String, String>>>,
    pub(crate) ingest_event_bus: Arc<Mutex<ManagedEventBusTransport>>,
    pub(crate) ingest_outbox_cursors: Arc<Mutex<ManagedReplayCursorStore>>,
    pub(crate) distributed_cache: Arc<Mutex<DistributedCacheManager>>,
    pub(crate) driver_pool: Arc<Mutex<ConnectionPoolManager>>,
    pub(crate) plugin_lifecycle: Arc<Mutex<PluginLifecycleManager>>,
    pub(crate) autonomous_mode: AutonomousMode,
    pub(crate) emergency_stop: Arc<AtomicEmergencyStop>,
    pub(crate) guardrails: Arc<Vec<GuardrailRule>>,
    pub(crate) ddl_catalog: Arc<Mutex<DdlCatalog>>,
    pub(crate) acid_transactions: Arc<Mutex<AcidTransactionRegistry>>,
    /// MVCC page-based row store (S2-WS2-04: PagedRowStore scaffold).
    pub(crate) row_store: Arc<Mutex<PagedRowStore>>,
    /// S9-WS8-02: AI model gateway isolation policy.
    pub(crate) model_gateway_policy: Arc<Mutex<ModelGatewayPolicy>>,
    /// S4-WS3-04: In-memory OLAP replica — receives mutations via `POST /api/v1/store/htap/apply`.
    /// Maps primary_key → row data (last-writer-wins).
    pub(crate) olap_store: Arc<Mutex<HashMap<String, HashMap<String, String>>>>,
    /// S9-WS8A-02: Optional path to a JSON-lines audit log file.
    /// Resolved from `VNG_AUDIT_LOG_PATH` env var at start-up.
    pub(crate) audit_log_path: Option<String>,
    /// S7-WS6-02: Raft consensus node state (single-node scaffold).
    pub(crate) raft_state: Arc<Mutex<RaftNode>>,
    /// S9-WS8-02: Per-model-identity request counters for rate limiting.
    /// Maps model_id → request count in current window.
    pub(crate) ai_request_counters: Arc<Mutex<HashMap<String, u64>>>,
    /// S2-WS2-02: WAL durability engine — records every committed DML mutation.
    pub(crate) wal_engine: Arc<Mutex<BoxedDurabilityEngine>>,
    /// S7-WS6-04: Chaos/game-day fault injection state.
    pub(crate) chaos_state: Arc<Mutex<ChaosState>>,
    /// S8-WS10-02: Driver wire protocol session registry.
    pub(crate) driver_sessions: Arc<Mutex<HashMap<String, DriverSession>>>,
    /// S5-WS4A-02: Broker adapter flush counters (broker_type → flush_count).
    pub(crate) broker_flush_counts: Arc<Mutex<HashMap<String, u64>>>,
    /// S9-WS8-02: Sliding-window rate limiter — per-model window start timestamp (ms).
    pub(crate) ai_rate_window_starts: Arc<Mutex<HashMap<String, u64>>>,
    /// S5-E4A-01: Connector SDK runtime registry.
    pub(crate) connector_registry: Arc<Mutex<Vec<ConnectorPlugin>>>,
    /// S6-WS5-04: TDE runtime toggle override.
    pub(crate) tde_override: Arc<Mutex<Option<bool>>>,
    /// S10-WS15-02: Per-table CDC cursor positions (table_name → last consumed sequence).
    pub(crate) cdc_cursors: Arc<Mutex<HashMap<String, u64>>>,
    /// Phase 1.3 — first-class `Database` catalog. Replaces the implicit
    /// `database_name` string fragment used in `DdlCatalog` keys. New code
    /// should consult this for existence/uniqueness checks; legacy code
    /// continues to use `DdlCatalog` keys until the multi-database migration
    /// completes (see `remaining.md` Phase 1.3).
    pub(crate) database_catalog: Arc<Mutex<voltnuerongrid_meta::DatabaseCatalog>>,
    /// Phase 0 — runtime config selected at boot. Read-only after startup.
    pub(crate) runtime_config: Arc<voltnuerongrid_config::RuntimeConfig>,
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

/// Write a SQL statement to the durability engine (RocksDB or in-memory).
/// The engine is the single source of truth from Phase 2.2 onward.
pub(crate) fn persist_sql_statement(
    state: &AppState,
    kind: voltnuerongrid_store::SqlWalKind,
    sql: &str,
) {
    if let Ok(mut wal) = state.wal_engine.lock() {
        let _ = wal.append_sql(kind, sql);
    } else {
        tracing::error!(target: "vng.wal", "wal_engine mutex poisoned in persist_sql_statement");
    }
    metrics::counter!(
        "vng_wal_append_total",
        "kind" => kind.as_str(),
    ).increment(1);
}

/// Phase 2 — pick the durability engine based on `runtime_config.storage`.
///
/// Selection rules:
/// - `StorageEngine::Rocksdb` (default) → open RocksDB at the configured
///   `data_dir`. The `wal_fsync_on_commit` flag is propagated via the
///   `VNG_WAL_FSYNC_ON_COMMIT` env var (the engine reads it directly to
///   keep its open() signature stable across feature flags).
///   On failure to open RocksDB the process **exits**, not falls back to
///   in-memory. Silently degrading durability would defeat the whole
///   point of Phase 2; an obvious crash is preferred.
/// - `StorageEngine::Vng` → currently mapped to in-memory with a warning,
///   because the native VNG engine is not yet shipped.
fn build_durability_engine(
    cfg: &voltnuerongrid_config::RuntimeConfig,
) -> BoxedDurabilityEngine {
    use voltnuerongrid_config::StorageEngine;

    let durability_cfg = DurabilityConfig {
        wal_enabled: true,
        checkpoint_interval_seconds: 60,
        max_wal_records_before_checkpoint: 1_000,
    };

    match cfg.storage.engine {
        StorageEngine::Rocksdb => {
            // Propagate the fsync flag to the rocksdb engine via env var so
            // its open() signature can stay simple (the engine reads it once
            // at boot — main.rs sets it before construction).
            std::env::set_var(
                "VNG_WAL_FSYNC_ON_COMMIT",
                if cfg.storage.wal_fsync_on_commit { "1" } else { "0" },
            );
            let path = std::path::PathBuf::from(&cfg.storage.data_dir).join("rocksdb");
            tracing::info!(
                target: "vng.durability",
                path = %path.display(),
                fsync = cfg.storage.wal_fsync_on_commit,
                "opening RocksDB durability engine"
            );
            match BoxedDurabilityEngine::rocksdb(&path, durability_cfg) {
                Ok(engine) => {
                    tracing::info!(
                        target: "vng.durability",
                        kind = engine.engine_kind(),
                        latest_sequence = engine.latest_sequence(),
                        checkpoint_count = engine.checkpoint_count(),
                        "durability engine opened"
                    );
                    engine
                }
                Err(e) => {
                    eprintln!(
                        "[vng-durability] FATAL: failed to open RocksDB at {}: {}",
                        path.display(),
                        e
                    );
                    eprintln!(
                        "[vng-durability] refusing to fall back to in-memory — \
                         silently dropping durability would mask data loss. \
                         Fix the path or set storage.engine = vng to opt out."
                    );
                    std::process::exit(2);
                }
            }
        }
        StorageEngine::Vng => {
            tracing::warn!(
                target: "vng.durability",
                "storage.engine = vng — native VNG engine is not yet implemented; \
                 falling back to non-durable in-memory engine. Set \
                 storage.engine = rocksdb for production durability."
            );
            BoxedDurabilityEngine::in_memory(durability_cfg)
        }
    }
}

// ─── Phase 2.1: engine-first replay helpers ─────────────────────────────────
//
// Boot replay precedence:
// 1. If the durability engine persists SQL streams (RocksDB) AND has any
//    statements in the requested kind, drive replay from there.
// 2. Otherwise, fall back to the legacy text WAL files. The first successful
//    engine-backed replay (after migration) lets the operator delete the
//    text files.
//
// The engine-first path is the reason for the SqlWalKind extension to the
// trait. Once all deployments have migrated, the legacy path can be removed.

/// Replay DDL into a freshly-created catalog from the durability engine.
fn replay_ddl_into(
    catalog: &mut DdlCatalog,
    engine: &Arc<Mutex<voltnuerongrid_store::BoxedDurabilityEngine>>,
) {
    use voltnuerongrid_store::SqlWalKind;
    let now_ms = now_unix_ms();

    let stmts: Vec<String> = {
        let guard = engine.lock().expect("wal_engine lock for replay_ddl");
        if guard.persists_sql() && guard.sql_count(SqlWalKind::Ddl) > 0 {
            guard.iter_sql(SqlWalKind::Ddl)
        } else {
            Vec::new()
        }
    };
    for sql in &stmts {
        apply_ddl_to_catalog(catalog, sql, now_ms);
    }
    if !stmts.is_empty() {
        eprintln!("[vng-wal] replayed {} DDL statement(s) from durability engine", stmts.len());
        metrics::counter!(
            "vng_wal_replay_total",
            "kind" => "ddl",
            "source" => "engine",
        ).increment(stmts.len() as u64);
    }
}

/// Replay DML into a freshly-created row store from the durability engine.
fn replay_dml_into(
    rs: &mut PagedRowStore,
    engine: &Arc<Mutex<voltnuerongrid_store::BoxedDurabilityEngine>>,
) {
    use voltnuerongrid_store::SqlWalKind;

    let stmts: Vec<String> = {
        let guard = engine.lock().expect("wal_engine lock for replay_dml");
        if guard.persists_sql() && guard.sql_count(SqlWalKind::Dml) > 0 {
            guard.iter_sql(SqlWalKind::Dml)
        } else {
            Vec::new()
        }
    };
    if stmts.is_empty() {
        return;
    }
    let xid = rs.begin_xid();
    for sql in &stmts {
        apply_dml_to_rowstore(rs, xid, sql);
    }
    eprintln!("[vng-wal] replayed {} DML statement(s) from durability engine", stmts.len());
    metrics::counter!(
        "vng_wal_replay_total",
        "kind" => "dml",
        "source" => "engine",
    ).increment(stmts.len() as u64);
}

/// Apply a single DDL statement to the catalog.
fn apply_ddl_to_catalog(catalog: &mut DdlCatalog, sql: &str, now_ms: u128) {
    if let Some(info) = parse_ddl_info(sql) {
        match info.operation {
            "create" => { let _ = catalog.record_create(&info.object_kind, &info.database_name, &info.schema_name, &info.object_name, sql, now_ms, info.replace_ok); }
            "drop"   => { catalog.record_drop(&info.database_name, &info.schema_name, &info.object_name); }
            "alter"  => { catalog.record_alter(&info.database_name, &info.schema_name, &info.object_name, sql, now_ms); }
            _ => {}
        }
    }
}

/// Apply a single DML statement to the row store.
fn apply_dml_to_rowstore(rs: &mut PagedRowStore, xid: voltnuerongrid_store::mvcc::Xid, sql: &str) {
    let upper = sql.trim_start().to_ascii_uppercase();
    if upper.starts_with("INSERT") {
        for (k, d, _) in extract_all_insert_rows(sql) {
            rs.insert(xid, &k, d);
        }
    } else if upper.starts_with("DELETE") {
        if let Some(k) = extract_delete_key_from_sql(sql) {
            rs.delete(xid, &k);
        }
    } else if upper.starts_with("UPDATE") {
        if let Some((k, d)) = extract_update_row_from_sql(sql) {
            rs.insert(xid, &k, d);
        }
    }
}

fn default_rbac_privilege_matrix() -> RbacPrivilegeMatrix {
    let mut matrix = RbacPrivilegeMatrix::new();

    for role in [OperatorRole::Dba] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "sql.runtime".to_string(),
                scopes: vec!["sql/*".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Execute],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.failover".to_string(),
                scopes: vec!["cluster".to_string()],
                actions: vec![
                    PrivilegeAction::Read,
                    PrivilegeAction::Execute,
                    PrivilegeAction::Manage,
                ],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.sre".to_string(),
                scopes: vec!["sre/*".to_string()],
                actions: vec![
                    PrivilegeAction::Read,
                    PrivilegeAction::Execute,
                    PrivilegeAction::Manage,
                ],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.dr_hooks".to_string(),
                scopes: vec!["dr_hooks/*".to_string()],
                actions: vec![
                    PrivilegeAction::Read,
                    PrivilegeAction::Execute,
                    PrivilegeAction::Manage,
                ],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "storage.catalog".to_string(),
                scopes: vec!["store/*".to_string()],
                actions: vec![
                    PrivilegeAction::Read,
                    PrivilegeAction::Write,
                    PrivilegeAction::Manage,
                ],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "ingest.connectors".to_string(),
                scopes: vec!["ingest/*".to_string()],
                actions: vec![
                    PrivilegeAction::Read,
                    PrivilegeAction::Write,
                    PrivilegeAction::Manage,
                ],
            },
        );
    }

    for role in ["tenant_analyst", "tenant_admin"] {
        matrix.grant_role(
            role,
            ResourceGrant {
                resource: "sql.runtime".to_string(),
                scopes: vec!["tenants/{tenant}/sql/*".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Execute],
            },
        );
        matrix.grant_role(
            role,
            ResourceGrant {
                resource: "ingest.connectors".to_string(),
                scopes: vec!["tenants/{tenant}/ingest/*".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Write],
            },
        );
        matrix.grant_role(
            role,
            ResourceGrant {
                resource: "storage.catalog".to_string(),
                scopes: vec![
                    "tenants/{tenant}/store/indexes".to_string(),
                    "tenants/{tenant}/store/indexes/lookup".to_string(),
                    "tenants/{tenant}/store/constraints/validate".to_string(),
                ],
                actions: vec![PrivilegeAction::Read],
            },
        );
        matrix.grant_role(
            role,
            ResourceGrant {
                resource: "observability.audit".to_string(),
                scopes: vec!["tenants/{tenant}/audit/events".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
        matrix.grant_role(
            role,
            ResourceGrant {
                resource: "observability.autonomous_records".to_string(),
                scopes: vec!["tenants/{tenant}/autonomous/records".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
    }

    matrix.grant_role(
        "tenant_admin",
        ResourceGrant {
            resource: "storage.catalog".to_string(),
            scopes: vec![
                "tenants/{tenant}/store/indexes".to_string(),
                "tenants/{tenant}/store/constraints".to_string(),
            ],
            actions: vec![PrivilegeAction::Manage],
        },
    );

    for role in [OperatorRole::Dba, OperatorRole::Sre] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "observability.audit".to_string(),
                scopes: vec!["audit/*".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
    }

    for role in [OperatorRole::Dba, OperatorRole::Sre, OperatorRole::Security] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.sre".to_string(),
                scopes: vec!["sre/reliability", "sre/failure_budget", "sre/gate"].into_iter().map(String::from).collect(),
                actions: vec![PrivilegeAction::Read],
            },
        );
    }

    for role in [OperatorRole::Dba, OperatorRole::Sre] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.failover".to_string(),
                scopes: vec!["cluster".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Execute],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.dr_hooks".to_string(),
                scopes: vec!["dr_hooks/*".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Execute],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "cluster.sre".to_string(),
                scopes: vec!["sre/*".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Execute],
            },
        );
    }

    for role in [OperatorRole::Dba, OperatorRole::Security, OperatorRole::AiOperator] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "autonomous.guardrails".to_string(),
                scopes: vec!["autonomous/*".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
    }

    for role in [OperatorRole::Dba, OperatorRole::Security] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "autonomous.guardrails".to_string(),
                scopes: vec!["autonomous/emergency_stop".to_string()],
                actions: vec![PrivilegeAction::Manage],
            },
        );
    }

    for role in [OperatorRole::Dba, OperatorRole::AiOperator] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "autonomous.actions".to_string(),
                scopes: vec!["autonomous/actions".to_string()],
                actions: vec![PrivilegeAction::Execute],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "observability.autonomous_records".to_string(),
                scopes: vec!["autonomous/records".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
    }

    for role in [OperatorRole::Dba, OperatorRole::Security] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "observability.audit".to_string(),
                scopes: vec!["audit/*".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "security.kms".to_string(),
                scopes: vec![
                    "security/kms".to_string(),
                    "security/kms/outage".to_string(),
                    "security/tls/status".to_string(),
                    "security/tls/rotate".to_string(),
                    "security/tde/status".to_string(),
                    "security/tde/toggle".to_string(),
                ],
                actions: vec![
                    PrivilegeAction::Read,
                    PrivilegeAction::Manage,
                ],
            },
        );
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "security.supply_chain".to_string(),
                scopes: vec!["security/plugins/provenance/*".to_string()],
                actions: vec![PrivilegeAction::Read, PrivilegeAction::Manage],
            },
        );
    }

    matrix.grant_role(
        OperatorRole::Sre.as_str(),
        ResourceGrant {
            resource: "security.kms".to_string(),
            scopes: vec![
                "security/kms".to_string(),
                "security/tls/status".to_string(),
                "security/tde/status".to_string(),
            ],
            actions: vec![PrivilegeAction::Read],
        },
    );

    // S9-WS8-02: AI model gateway policy enforcement.
    for role in [OperatorRole::Dba, OperatorRole::Security, OperatorRole::AiOperator] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "ai.governance".to_string(),
                scopes: vec!["ai/policy".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
    }
    for role in [OperatorRole::Dba, OperatorRole::Security] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "ai.governance".to_string(),
                scopes: vec!["ai/policy".to_string()],
                actions: vec![PrivilegeAction::Manage],
            },
        );
    }

    // S6-WS5-03 / S6-WS5-04: TLS and TDE status endpoints — already covered
    // by security.kms Read grants which we reuse for TLS/TDE status.

    // S9-WS8A-02: Audit export endpoint — accessible to DBA and Security operators.
    for role in [OperatorRole::Dba, OperatorRole::Security] {
        matrix.grant_role(
            role.as_str(),
            ResourceGrant {
                resource: "audit.read".to_string(),
                scopes: vec!["audit/export".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );
    }

    matrix
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
enum NativeFrameType {
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
enum NativeCommandKind {
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

pub(crate) fn extract_request_id(headers: &HeaderMap, fallback: &str) -> String {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

pub(crate) fn build_http_envelope<TPayload>(
    headers: &HeaderMap,
    command: CanonicalCommandName,
    payload: TPayload,
    fallback_request_id: &str,
) -> CanonicalCommandEnvelope<TPayload> {
    let request_id = extract_request_id(headers, fallback_request_id);
    let session_context = headers
        .get("x-vng-session-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let mut transport_metadata = std::collections::HashMap::new();
    transport_metadata.insert("protocol".to_string(), "http".to_string());
    if let Some(session_id) = session_context.clone() {
        transport_metadata.insert("session_id".to_string(), session_id);
    }
    CanonicalCommandEnvelope {
        request_id,
        transport: TransportKind::Http,
        command,
        session_context,
        transport_metadata,
        payload,
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SqlTransactionResponse {
    pub(crate) status: &'static str,
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
                "Invalid VNG_NATIVE_MAX_CONNECTIONS=0; defaulting to 2048 for scaffold safety"
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

fn load_native_tls_acceptor(
    cert_path: &str,
    key_path: &str,
    client_ca_path: Option<&str>,
) -> Result<Arc<tokio_rustls::TlsAcceptor>, String> {
    use rustls::RootCertStore;
    use rustls::ServerConfig;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls::server::WebPkiClientVerifier;
    use rustls_pemfile::{certs, private_key};
    use std::fs::File;
    use std::io::BufReader;

    let mut cert_r = BufReader::new(
        File::open(cert_path).map_err(|e| format!("open cert {cert_path}: {e}"))?,
    );
    let cert_chain: Vec<CertificateDer<'static>> = certs(&mut cert_r)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("parse cert PEM: {e}"))?;
    if cert_chain.is_empty() {
        return Err("no certificates found in PEM file".to_string());
    }

    let mut key_r = BufReader::new(
        File::open(key_path).map_err(|e| format!("open key {key_path}: {e}"))?,
    );
    let key: PrivateKeyDer<'static> = private_key(&mut key_r)
        .map_err(|e| format!("parse key PEM: {e}"))?
        .ok_or_else(|| "no private keys in PEM file".to_string())?;

    let cfg = if let Some(ca_path) = client_ca_path {
        let mut ca_r = BufReader::new(
            File::open(ca_path).map_err(|e| format!("open client CA {ca_path}: {e}"))?,
        );
        let ca_certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut ca_r)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("parse client CA PEM: {e}"))?;
        if ca_certs.is_empty() {
            return Err("no certificates in client CA PEM file".to_string());
        }
        let mut root_store = RootCertStore::empty();
        let (added, _ignored) = root_store.add_parsable_certificates(ca_certs);
        if added == 0 {
            return Err("no valid client CA trust anchors parsed".to_string());
        }
        let verifier = WebPkiClientVerifier::builder(Arc::new(root_store))
            .build()
            .map_err(|e| format!("client cert verifier: {e}"))?;
        ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(cert_chain, key)
            .map_err(|e| format!("rustls server config: {e}"))?
    } else {
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain, key)
            .map_err(|e| format!("rustls server config: {e}"))?
    };

    Ok(Arc::new(tokio_rustls::TlsAcceptor::from(Arc::new(cfg))))
}

fn read_env_bool(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(value) => value.trim().eq_ignore_ascii_case("true"),
        Err(_) => default,
    }
}

fn read_env_usize(name: &str, default: usize) -> usize {
    match env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        Some(value) => match value.parse::<usize>() {
            Ok(parsed) => parsed,
            Err(_) => {
                eprintln!("Invalid {name}={value}; using default {default}");
                default
            }
        },
        None => default,
    }
}

fn read_env_u64(name: &str, default: u64) -> u64 {
    match env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        Some(value) => match value.parse::<u64>() {
            Ok(parsed) => parsed,
            Err(_) => {
                eprintln!("Invalid {name}={value}; using default {default}");
                default
            }
        },
        None => default,
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
    let rbac_privilege_matrix = Arc::new(default_rbac_privilege_matrix());
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

    let state = AppState {
        node_id: node_id.clone(),
        cluster_mode,
        admin_api_key,
        security_config,
        allowed_operator_roles,
        operator_role_bindings,
        tenant_user_bindings,
        rbac_privilege_matrix,
        kms_runtime,
        leader_node_id: Arc::new(Mutex::new("node-1".to_string())),
        cluster_nodes: Arc::new(Mutex::new(initial_cluster_nodes(&node_id))),
        audit_sink: Arc::new(Mutex::new(AppendOnlyAuditSink::new())),
        action_records: Arc::new(Mutex::new(Vec::new())),
        dr_hook_records: Arc::new(Mutex::new(Vec::new())),
        dr_hook_policy_state: Arc::new(Mutex::new(loaded_policy_state)),
        dr_hook_policy_config: Arc::new(default_dr_hook_policy_config()),
        dr_hook_state_path,
        dr_hook_queue: Arc::new(Mutex::new(VecDeque::new())),
        cluster_failure_signals: Arc::new(Mutex::new(Vec::new())),
        sync_origin: Arc::new(Mutex::new(RowStoreSyncOrigin::new())),
        replication_transport: Arc::new(Mutex::new(InMemoryReplicationTransport::new())),
        replica_replay_states: Arc::new(Mutex::new(HashMap::new())),
        pessimistic_locks: Arc::new(Mutex::new(HashMap::new())),
        pessimistic_lock_waits: Arc::new(Mutex::new(HashMap::new())),
        pessimistic_lock_metrics: PessimisticLockContentionMetrics::new(),
        index_manager: Arc::new(Mutex::new(IndexManager::new())),
        constraint_manager: Arc::new(Mutex::new(ConstraintManager::new())),
        ingest_csv_records: Arc::new(Mutex::new(HashMap::new())),
        ingest_json_records: Arc::new(Mutex::new(HashMap::new())),
        ingest_parquet_records: Arc::new(Mutex::new(HashMap::new())),
        ingest_excel_records: Arc::new(Mutex::new(HashMap::new())),
        ingest_outbox_streams: Arc::new(Mutex::new(HashMap::new())),
        ingest_event_bus: Arc::new(Mutex::new(load_ingest_event_bus())),
        ingest_outbox_cursors: Arc::new(Mutex::new(load_ingest_outbox_cursor_store())),
        distributed_cache: Arc::new(Mutex::new(DistributedCacheManager::with_default_policy())),
        driver_pool: Arc::new(Mutex::new(ConnectionPoolManager::with_default_policy())),
        plugin_lifecycle: Arc::new(Mutex::new(PluginLifecycleManager::new(256))),
        autonomous_mode,
        emergency_stop: Arc::new(emergency_stop),
        guardrails: Arc::new(default_guardrail_rules()),
        ddl_catalog: Arc::new(Mutex::new({
            let mut cat = DdlCatalog::new();
            replay_ddl_into(&mut cat, &wal_engine);
            cat
        })),
        acid_transactions: Arc::new(Mutex::new(AcidTransactionRegistry::default())),
        row_store: Arc::new(Mutex::new({
            let mut rs = PagedRowStore::default();
            replay_dml_into(&mut rs, &wal_engine);
            rs
        })),
        model_gateway_policy: Arc::new(Mutex::new(ModelGatewayPolicy::default())),
        wal_engine,
        chaos_state: Arc::new(Mutex::new(ChaosState::default())),
        olap_store: Arc::new(Mutex::new(HashMap::new())),
        audit_log_path: std::env::var("VNG_AUDIT_LOG_PATH").ok(),
        raft_state: Arc::new(Mutex::new(RaftNode::new("node-1"))),
        ai_request_counters: Arc::new(Mutex::new(HashMap::new())),
        driver_sessions: Arc::new(Mutex::new(HashMap::new())),
        broker_flush_counts: Arc::new(Mutex::new(HashMap::new())),
        ai_rate_window_starts: Arc::new(Mutex::new(HashMap::new())),
        connector_registry: Arc::new(Mutex::new(Vec::new())),
        tde_override: Arc::new(Mutex::new(None)),
        cdc_cursors: Arc::new(Mutex::new(HashMap::new())),
        // Phase 1.3 — first-class DatabaseCatalog. Empty at boot; populated
        // via CREATE DATABASE. Future Phase 2 work will restore this from
        // RocksDB instead of starting empty.
        database_catalog: Arc::new(Mutex::new(voltnuerongrid_meta::DatabaseCatalog::new())),
        // Phase 0 — read-only runtime config selected at boot.
        runtime_config: Arc::new(runtime_config),
    };

    tokio::spawn(run_dr_hook_scheduler(state.clone()));

    async fn add_cors(req: Request<axum::body::Body>, next: Next) -> Response {
        let mut res = next.run(req).await;
        res.headers_mut().insert(
            "Access-Control-Allow-Origin",
            axum::http::HeaderValue::from_static("*"),
        );
        res.headers_mut().insert(
            "Access-Control-Allow-Methods",
            axum::http::HeaderValue::from_static("GET,POST,OPTIONS"),
        );
        res.headers_mut().insert(
            "Access-Control-Allow-Headers",
            axum::http::HeaderValue::from_static("content-type,x-vng-admin-key,x-vng-operator-id,x-vng-tenant-id,x-vng-user-id"),
        );
        res
    }

    async fn options_preflight() -> Response {
        let res = axum::http::Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Methods", "GET,POST,OPTIONS")
            .header(
                "Access-Control-Allow-Headers",
                "content-type,x-vng-admin-key,x-vng-operator-id,x-vng-tenant-id,x-vng-user-id",
            )
            .body(axum::body::Body::empty())
            .unwrap();
        res
    }

    /// Phase 0.4 follow-up: emit `vng_http_requests_total` for every route, labeled
    /// by method, route template (when matched), and response status class.
    /// Also emits `vng_http_request_duration_seconds` as a histogram.
    /// Skips itself for `/metrics` to avoid recursive label cardinality.
    async fn track_http_metrics(req: Request<axum::body::Body>, next: Next) -> Response {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let started = std::time::Instant::now();

        let response = next.run(req).await;

        // Don't tag the metrics endpoint itself — Prometheus scrapes are
        // not interesting application traffic and would skew histograms.
        if path != "/metrics" {
            let status = response.status().as_u16();
            // Coarse status class (2xx / 3xx / 4xx / 5xx) keeps cardinality small.
            let status_class = match status {
                100..=199 => "1xx",
                200..=299 => "2xx",
                300..=399 => "3xx",
                400..=499 => "4xx",
                500..=599 => "5xx",
                _ => "other",
            };
            // Coarsen unbounded paths to template-like buckets so we don't
            // create one time-series per random ID. This is a pragmatic
            // approximation; a future PR can plug into axum's matched-path
            // info for precise route templates.
            let route_label = coarsen_route_for_metrics(&path);

            metrics::counter!(
                "vng_http_requests_total",
                "method" => method.as_str().to_string(),
                "route" => route_label.clone(),
                "status_class" => status_class,
            )
            .increment(1);

            metrics::histogram!(
                "vng_http_request_duration_seconds",
                "method" => method.as_str().to_string(),
                "route" => route_label,
            )
            .record(started.elapsed().as_secs_f64());
        }

        response
    }

    /// Map a request path to a low-cardinality route label.
    /// e.g. `/api/v1/admin/databases/foo` → `/api/v1/admin/databases/:name`.
    /// Conservative: only does well-known transformations; returns the path
    /// as-is if no rule matches.
    fn coarsen_route_for_metrics(path: &str) -> String {
        // Path-id parameters that we know about so far. Add to this list as
        // routes with dynamic segments are introduced.
        for (prefix, replacement) in &[
            ("/api/v1/admin/databases/", "/api/v1/admin/databases/:name"),
        ] {
            if path.starts_with(prefix) && path.len() > prefix.len() {
                return replacement.to_string();
            }
        }
        path.to_string()
    }

    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_handler))
        .route("/api/v1/sql/transaction", post(sql_transaction))
        .route(
            "/api/v1/sql/locks/pessimistic/acquire",
            post(sql_pessimistic_lock_acquire),
        )
        .route(
            "/api/v1/sql/locks/pessimistic/release",
            post(sql_pessimistic_lock_release),
        )
        .route(
            "/api/v1/sql/locks/pessimistic/metrics",
            get(sql_pessimistic_lock_metrics),
        )
        .route("/api/v1/sql/analyze", post(sql_analyze))
        .route("/api/v1/sql/route", post(sql_route))
        .route("/api/v1/sql/execute", post(sql_execute))
        .route("/api/v1/olap/query", post(olap_query))
        .route("/api/v1/failover/status", get(failover_status))
        .route("/api/v1/failover/simulate", post(failover_simulate))
        .route("/api/v1/admin/cluster/topology", get(admin_cluster_topology))
        .route("/api/v1/admin/schema/tree", get(admin_schema_tree))
        // Phase 1.3 — first-class database lifecycle.
        .route("/api/v1/admin/databases", get(admin_databases_list).post(admin_databases_create))
        .route("/api/v1/admin/databases/:name", axum::routing::delete(admin_databases_drop))
        .route("/api/v1/admin/databases/:name/metadata", get(admin_databases_metadata))
        .route("/api/v1/admin/databases/:name/metadata/:table", get(admin_databases_metadata_rows))
        // Phase 0 — surface the boot-time runtime config (read-only).
        .route("/api/v1/admin/runtime-config", get(admin_runtime_config))
        .route("/api/v1/admin/sql/transactions/control", post(admin_sql_transaction_control))
        .route("/api/v1/admin/sql/locks/control", post(admin_sql_lock_control))
        .route("/api/v1/admin/cluster/nodes/manage", post(admin_cluster_node_manage))
        .route("/api/v1/sre/reliability/status", get(sre_reliability_status))
        .route("/api/v1/sre/rate-limit/check", post(sre_rate_limit_check))
        .route(
            "/api/v1/sre/failure-budget/alerts",
            get(sre_failure_budget_alerts),
        )
        .route("/api/v1/sre/dr/hooks/policy", get(sre_dr_hook_policy))
        .route("/api/v1/sre/dr/hooks/retry-plan", get(sre_dr_hook_retry_plan))
        .route("/api/v1/sre/dr/hooks/schedule", post(sre_dr_hook_schedule))
        .route("/api/v1/sre/dr/hooks/trigger", post(sre_dr_hook_trigger))
        .route("/api/v1/sre/dr/hooks/status", get(sre_dr_hook_status))
        .route("/api/v1/sre/failure/signal", post(sre_failure_signal))
        .route("/api/v1/sre/failure/reconcile", post(sre_failure_reconcile))
        .route("/api/v1/sre/gate/evaluate", get(sre_gate_evaluate))
        .route("/api/v1/sre/gate/export", post(sre_gate_export))
        .route("/api/v1/sre/cache/set", post(sre_cache_set))
        .route("/api/v1/sre/cache/get", get(sre_cache_get))
        .route("/api/v1/sre/cache/invalidate", post(sre_cache_invalidate))
        .route("/api/*path", options(options_preflight))
        .layer(from_fn(add_cors))
        .layer(from_fn(track_http_metrics))
        .route("/api/v1/sre/cache/rebalance", post(sre_cache_rebalance))
        .route("/api/v1/sre/cache/metrics", get(sre_cache_metrics))
        // REQ-27: Redis-compat cache command interface
        .route("/api/v1/cache/redis/command", post(cache_redis_command))
        .route("/api/v1/sre/driver/pool/acquire", post(sre_driver_pool_acquire))
        .route("/api/v1/sre/driver/pool/release", post(sre_driver_pool_release))
        .route("/api/v1/sre/driver/pool/failure", post(sre_driver_pool_failure))
        .route("/api/v1/sre/driver/pool/recover", post(sre_driver_pool_recover))
        .route("/api/v1/sre/driver/pool/stats", get(sre_driver_pool_stats))
        .route(
            "/api/v1/security/plugins/provenance/register",
            post(security_plugins_provenance_register),
        )
        .route("/api/v1/audit/events", get(audit_events))
        // S9-WS8A-02: tamper-evident audit chain verification
        .route("/api/v1/audit/chain/verify", get(audit_chain_verify))
        .route("/api/v1/audit/snapshot", get(audit_snapshot))
        // S9-WS8A-01: Audit CLI summary
        .route("/api/v1/audit/cli/summary", get(audit_cli_summary))
        .route("/api/v1/security/kms/status", get(security_kms_status))
        .route(
            "/api/v1/security/kms/outage/simulate",
            post(security_kms_outage_simulate),
        )
        .route(
            "/api/v1/security/kms/outage/reconcile",
            post(security_kms_outage_reconcile),
        )
        .route("/api/v1/i18n/messages", get(i18n_messages))
        .route(
            "/api/v1/autonomous/actions/records",
            get(autonomous_action_records),
        )
        .route("/api/v1/autonomous/guardrails", get(autonomous_guardrails))
        .route(
            "/api/v1/autonomous/emergency-stop",
            post(autonomous_emergency_stop),
        )
        .route(
            "/api/v1/autonomous/actions/authorize",
            post(authorize_autonomous_action),
        )
        // WS2 Index + Constraint endpoints
        .route("/api/v1/store/indexes", get(store_list_indexes))
        .route("/api/v1/store/indexes/create", post(store_create_index))
        .route("/api/v1/store/indexes/drop", post(store_drop_index))
        .route("/api/v1/store/indexes/lookup", post(store_index_lookup))
        .route(
            "/api/v1/store/constraints/add",
            post(store_add_constraint),
        )
        .route(
            "/api/v1/store/constraints/validate",
            post(store_validate_constraint),
        )
        // S4-WS3-04: HTAP OLAP consumer apply + scan
        .route("/api/v1/store/htap/apply", post(store_htap_apply))
        .route("/api/v1/store/htap/olap/scan", get(store_htap_olap_scan))
        .route("/api/v1/store/htap/lag", get(htap_lag))
        // S4-WS3-04: HTAP force-sync — drain sync_origin into olap_store
        .route("/api/v1/store/htap/sync", post(htap_force_sync))
        // S4-WS3-04: HTAP detailed status
        .route("/api/v1/store/htap/status", get(htap_status))
        // S11-WS1-11: HTAP OLAP store statistics
        .route("/api/v1/store/htap/stats", get(htap_stats))
        // S9-WS8A-02: Audit export (all buffered events + file-backed status)
        .route("/api/v1/audit/export", get(audit_export))
        // S9-WS8A-02: Audit purge — flush in-memory audit sink
        .route("/api/v1/audit/purge", post(audit_purge))
        // S7-WS6-02: Raft consensus RPC + status endpoints
        .route("/api/v1/cluster/raft/status", get(raft_status))
        .route("/api/v1/cluster/raft/vote", post(raft_vote))
        .route("/api/v1/cluster/raft/append", post(raft_append))
        .route("/api/v1/cluster/raft/tick", post(raft_tick))
        .route("/api/v1/cluster/raft/log", get(raft_log))
        // S7-WS6-02: raft commit progress
        .route("/api/v1/cluster/raft/commit", get(raft_commit_progress))
        // S7-WS6-02: Raft point-in-time snapshot
        .route("/api/v1/cluster/raft/snapshot", get(raft_snapshot))
        .route("/api/v1/cluster/raft/heartbeat", post(raft_heartbeat))
        // S7-WS6-02: Raft election timer status
        .route("/api/v1/cluster/raft/election/status", get(raft_election_status))
        .route("/api/v1/cluster/raft/members", get(raft_member_list))
        // S7-WS6-03: Raft current leader
        .route("/api/v1/cluster/raft/leader", get(raft_leader))
        // S7-WS6-03: Raft fencing token
        .route("/api/v1/cluster/raft/fence", get(raft_fence))
        // S7-WS6-01: Raft vote statistics
        .route("/api/v1/cluster/raft/vote/stats", get(raft_vote_stats))
        .route("/api/v1/store/rows/scan", post(store_rows_scan))
        // S4-WS3-04: HTAP sync export for OLAP consumers
        .route("/api/v1/store/htap/export", post(store_htap_export))
        // S4-WS3-03: vectorized columnar scan
        .route("/api/v1/store/columnar/scan", get(store_columnar_scan))
        .route("/api/v1/store/columnar/project", get(store_columnar_project))
        // S4-WS3-03: Columnar vectorized aggregate
        .route("/api/v1/store/columnar/aggregate", get(store_columnar_aggregate))
        // S6-WS5-03: TLS runtime status
        .route("/api/v1/security/tls/status", get(security_tls_status))
        .route("/api/v1/security/tls/rotate", post(security_tls_rotate))
        // S6-WS5-03: TLS certificate info
        .route("/api/v1/security/tls/cert/info", get(security_tls_cert_info))
        // S6-WS5-04: TDE/encryption-at-rest status
        .route("/api/v1/security/tde/status", get(security_tde_status))
        // S6-WS5-04: TDE toggle override
        .route("/api/v1/security/tde/toggle", post(security_tde_toggle))
        // S6-WS5-04: TDE runtime override status
        .route("/api/v1/security/tde/override-status", get(security_tde_override_status))
        // S9-WS8-02: AI model gateway policy
        .route("/api/v1/ai/policy", get(ai_policy))
        // S2-WS2-02: WAL durability status + recovery replay
        .route("/api/v1/store/wal/status", get(wal_status))
        .route("/api/v1/store/wal/recover", post(wal_recover))
        .route("/api/v1/store/wal/checkpoint", post(wal_force_checkpoint))
        .route("/api/v1/store/wal/stats", get(wal_stats))
        // S2-WS2-02: WAL compact
        .route("/api/v1/store/wal/compact", post(wal_compact))
        // S2-WS2-02: WAL bounds (oldest/newest sequence)
        .route("/api/v1/store/wal/bounds", get(wal_bounds))
        // S2-WS2-02: WAL replay (filtered read-back)
        .route("/api/v1/store/wal/replay", get(wal_replay))
        // S2-WS2-02: WAL tail (last N records)
        .route("/api/v1/store/wal/tail", get(wal_tail))
        // S2-WS2-03: WAL mutations (recent key-value changes)
        .route("/api/v1/store/wal/mutations", get(wal_mutations))
        // S11-WS1-13: WAL latest sequence info
        .route("/api/v1/store/wal/seq", get(wal_seq))
        // S11-WS1-14: WAL head records (first N entries)
        .route("/api/v1/store/wal/head", get(wal_head))
        // S11-WS1-15: WAL range (records within a sequence range)
        .route("/api/v1/store/wal/range", get(wal_range))
        // S11-WS1-16: WAL size estimate
        .route("/api/v1/store/wal/size", get(wal_size))
        // S11-WS1-17: WAL latest single record
        .route("/api/v1/store/wal/latest", get(wal_latest))
        // S11-WS1-18: WAL records filtered by key prefix
        .route("/api/v1/store/wal/by-key", get(wal_by_key))
        // S11-WS1-19: Latest WAL checkpoint info
        .route("/api/v1/store/wal/checkpoint/latest", get(wal_checkpoint_latest))
        // S11-WS1-20: WAL insert/delete delta counts
        .route("/api/v1/store/wal/delta", get(wal_delta))
        // S11-WS1-21: Unique key count across all WAL records
        .route("/api/v1/store/wal/unique/keys", get(wal_unique_keys))
        // S11-WS1-22: WAL age (oldest/newest sequence span)
        .route("/api/v1/store/wal/age", get(wal_age))
        // S11-WS1-23: List all unique keys in the WAL
        .route("/api/v1/store/wal/keys/list", get(wal_keys_list))
        // S2-WS2-02: WAL checkpoint history
        .route("/api/v1/store/wal/checkpoint/history", get(wal_checkpoint_history))
        // S11-WS1-10: WAL truncate up to sequence
        .route("/api/v1/store/wal/truncate", post(wal_truncate))
        // S2-WS2-02: WAL segment list (checkpoint groups)
        .route("/api/v1/store/wal/segment/list", get(wal_segment_list))
        // S2-WS2-02: WAL replay count (filtered record count, no body)
        .route("/api/v1/store/wal/replay/count", get(wal_replay_count))
        // S7-WS6-04: Chaos/game-day fault injection
        .route("/api/v1/cluster/chaos/inject", post(chaos_inject))
        .route("/api/v1/cluster/chaos/clear", post(chaos_clear))
        .route("/api/v1/cluster/chaos/status", get(chaos_status))
        .route("/api/v1/cluster/chaos/health", get(chaos_health))
        // S7-WS6-04: Chaos event history
        .route("/api/v1/cluster/chaos/history", get(chaos_history))
        // S7-WS6-04: Chaos fire drill
        .route("/api/v1/cluster/chaos/fire-drill", post(chaos_fire_drill))
        // S8-WS10-02: Driver wire protocol info + session connect + disconnect
        .route("/api/v1/driver/protocol/info", get(driver_protocol_info))
        .route("/api/v1/driver/connect", post(driver_connect))
        .route("/api/v1/driver/disconnect", post(driver_disconnect))
        .route("/api/v1/driver/sessions", get(driver_session_list))
        // S8-WS10-02: driver pool health
        .route("/api/v1/driver/health", get(driver_health))
        // S8-WS10-02: driver query pass-through
        .route("/api/v1/driver/query", post(driver_query))
        // S8-WS10-02: driver session ping/keepalive
        .route("/api/v1/driver/ping", post(driver_ping))
        // S8-WS10-02: driver pool stats (operator-facing)
        .route("/api/v1/driver/pool/stats", get(driver_pool_stats))
        // S10-WS15-02: CDC stream from WAL
        .route("/api/v1/store/cdc/stream", get(cdc_stream))
        .route("/api/v1/store/cdc/stream/filter", get(cdc_stream_filter))
        // S10-WS15-02: CDC stream latest N events
        .route("/api/v1/store/cdc/stream/latest", get(cdc_stream_latest))
        .route("/api/v1/store/cdc/cursor", get(cdc_cursor_status))
        .route("/api/v1/store/cdc/cursor/advance", post(cdc_cursor_advance))
        // S10-WS15-02: CDC cursor rewind
        .route("/api/v1/store/cdc/cursor/rewind", post(cdc_cursor_rewind))
        // S10-WS15-02: CDC cursor list (all tracked table positions)
        .route("/api/v1/store/cdc/cursor/list", get(cdc_cursor_list))
        // S10-WS15-02: CDC aggregate metrics
        .route("/api/v1/store/cdc/metrics", get(cdc_metrics))
        // S2-WS2-04: Row store point-in-time snapshot export
        .route("/api/v1/store/rows/snapshot", get(row_store_snapshot))
        // S2-WS2-04: Row store operational stats
        .route("/api/v1/store/rows/stats", get(row_store_stats))
        // S2-WS2-04: Row store key-prefix count
        .route("/api/v1/store/rows/count", get(row_store_count))
        // S2-WS2-04: Row store delete by key
        .route("/api/v1/store/rows/delete", post(row_store_delete))
        // S11-WS1-10: Row store key list
        .route("/api/v1/store/rows/keys", get(store_rows_keys))
        // S11-WS1-11: Row store version / current transaction ID
        .route("/api/v1/store/rows/version", get(row_store_version))
        // S11-WS1-14: Rows modified after a given transaction ID
        .route("/api/v1/store/rows/modified", get(rows_modified))
        // S11-WS1-15: Row store current XID info
        .route("/api/v1/store/rows/xid", get(rows_xid))
        // S11-WS1-16: Visible row count at current snapshot
        .route("/api/v1/store/rows/visible", get(rows_visible))
        // S11-WS1-17: Total row count (all versions)
        .route("/api/v1/store/rows/total", get(rows_total))
        // S11-WS1-18: Count of distinct row keys
        .route("/api/v1/store/rows/keys/count", get(rows_keys_count))
        // S11-WS1-20: Count of tombstone (deleted) rows
        .route("/api/v1/store/rows/tombstone/count", get(rows_tombstone_count))
        // S11-WS1-21: Row store XID history (current + next + total)
        .route("/api/v1/store/rows/xid/history", get(rows_xid_history))
        // S11-WS1-22: First key in the row store (alphabetically)
        .route("/api/v1/store/rows/first/key", get(rows_first_key))
        // S11-WS1-23: Last alphabetically-sorted key in the row store
        .route("/api/v1/store/rows/last/key", get(rows_last_key))
        // S11-WS1-24: Count of distinct row values in the store
        .route("/api/v1/store/rows/count/distinct", get(rows_count_distinct))
        // S11-WS1-24: Check if a given key exists in the row store
        .route("/api/v1/store/rows/key/exists", get(rows_key_exists))
        // S11-WS1-25: Search rows by value
        .route("/api/v1/store/rows/value/search", get(rows_value_search))
        // S11-WS1-25: Count total WAL records
        .route("/api/v1/store/wal/record/count", get(wal_record_count))
        // S11-WS1-26: Count rows optionally filtered by key prefix
        .route("/api/v1/store/rows/count/range", get(rows_count_range))
        // S11-WS1-26: WAL checkpoint age (oldest/newest seqno)
        .route("/api/v1/store/wal/checkpoint/age", get(wal_checkpoint_age))
        // S11-WS1-27: Total payload field count across all rows
        .route("/api/v1/store/rows/payload/size", get(rows_payload_size))
        // S11-WS1-27: WAL flush count (total writes)
        .route("/api/v1/store/wal/flush/count", get(wal_flush_count))
        // S3-WS1-28: rows/field/count + wal/entry/latest
        .route("/api/v1/store/rows/field/count", get(rows_field_count))
        .route("/api/v1/store/wal/entry/latest", get(wal_entry_latest))
        // S3-WS1-29: wal/write/count + rows/key/longest
        .route("/api/v1/store/wal/write/count", get(wal_write_count))
        .route("/api/v1/store/rows/key/longest", get(rows_key_longest))
        // S3-WS1-30: rows/key/shortest (wal/age exists from S22)
        .route("/api/v1/store/rows/key/shortest", get(rows_key_shortest))
        // S3-WS1-31: wal/min/seq + rows/count/all
        .route("/api/v1/store/wal/min/seq", get(wal_min_seq))
        .route("/api/v1/store/rows/count/all", get(rows_count_all))
        // S3-WS1-32: wal/max/seq + rows/snapshot/size
        .route("/api/v1/store/wal/max/seq", get(wal_max_seq))
        .route("/api/v1/store/rows/snapshot/size", get(rows_snapshot_size))
        // S3-WS1-33: wal/entry/count + rows/version/latest
        .route("/api/v1/store/wal/entry/count", get(wal_entry_count))
        .route("/api/v1/store/rows/version/latest", get(rows_version_latest))
        // S3-WS1-34: wal/size/bytes + rows/distinct/count
        .route("/api/v1/store/wal/size/bytes", get(wal_size_bytes))
        .route("/api/v1/store/rows/distinct/count", get(rows_distinct_count))
        // S3-WS1-35: wal/delete/count + rows/key/median
        .route("/api/v1/store/wal/delete/count", get(wal_delete_count))
        .route("/api/v1/store/rows/key/median", get(rows_key_median))
        // S3-WS1-36: wal/validate + rows/checksum
        .route("/api/v1/store/wal/validate", get(wal_validate))
        .route("/api/v1/store/rows/checksum", get(rows_checksum))
        // S3-WS1-37: wal/entry/oldest + rows/field/types
        .route("/api/v1/store/wal/entry/oldest", get(wal_entry_oldest))
        .route("/api/v1/store/rows/field/types", get(rows_field_types))
        // S3-WS1-38: wal/seq/span + rows/key/empty/count
        .route("/api/v1/store/wal/seq/span", get(wal_seq_span))
        .route("/api/v1/store/rows/key/empty/count", get(rows_key_empty_count))
        // S3-WS1-39: wal/record/active + rows/key/min
        .route("/api/v1/store/wal/record/active", get(wal_record_active))
        .route("/api/v1/store/rows/key/min", get(rows_key_min))
        // S3-WS1-40: wal/record/mutations + rows/field/cardinality
        .route("/api/v1/store/wal/record/mutations", get(wal_record_mutations))
        .route("/api/v1/store/rows/field/cardinality", get(rows_field_cardinality))
        // S3-WS1-41: wal/record/deleted + rows/key/max
        .route("/api/v1/store/wal/record/deleted", get(wal_record_deleted))
        .route("/api/v1/store/rows/key/max", get(rows_key_max))
        // S3-WS1-42: wal/mutation/span + rows/value/non_null/count
        .route("/api/v1/store/wal/mutation/span", get(wal_mutation_span))
        .route("/api/v1/store/rows/value/non_null/count", get(rows_value_non_null_count))
        // S3-WS1-43: wal/mutation/count/non_deleted + rows/value/empty/count
        .route("/api/v1/store/wal/mutation/count/non_deleted", get(wal_mutation_non_deleted_count))
        .route("/api/v1/store/rows/value/empty/count", get(rows_value_empty_count))
        // S3-WS1-44: wal/non_deleted/span + rows/value/non_empty/count
        .route("/api/v1/store/wal/non_deleted/span", get(wal_non_deleted_span))
        .route("/api/v1/store/rows/value/non_empty/count", get(rows_value_non_empty_count))
        // S3-WS1-45: wal/non_deleted/count + rows/key/non_empty/count
        .route("/api/v1/store/wal/non_deleted/count", get(wal_non_deleted_count))
        .route("/api/v1/store/rows/key/non_empty/count", get(rows_key_non_empty_count))
        // S3-WS1-46: wal/non_deleted/latest + rows/value/non_blank/count
        .route("/api/v1/store/wal/non_deleted/latest", get(wal_non_deleted_latest))
        .route("/api/v1/store/rows/value/non_blank/count", get(rows_value_non_blank_count))
        // S3-WS1-47: wal/non_deleted/oldest + rows/key/non_blank/count
        .route("/api/v1/store/wal/non_deleted/oldest", get(wal_non_deleted_oldest))
        .route("/api/v1/store/rows/key/non_blank/count", get(rows_key_non_blank_count))
        // S3-WS1-48: wal/non_deleted/newest + rows/value/blank/count
        .route("/api/v1/store/wal/non_deleted/newest", get(wal_non_deleted_newest))
        .route("/api/v1/store/rows/value/blank/count", get(rows_value_blank_count))
        // S3-WS1-49: wal/record/total + rows/key/duplicates/count
        .route("/api/v1/store/wal/record/total", get(wal_record_total))
        .route("/api/v1/store/rows/key/duplicates/count", get(rows_key_duplicates_count))
        // S3-WS1-50: wal/value/duplicates/count + rows/value/duplicates/count
        .route("/api/v1/store/wal/value/duplicates/count", get(wal_value_duplicates_count))
        .route("/api/v1/store/rows/value/duplicates/count", get(rows_value_duplicates_count))
        // S3-WS1-51: wal/value/distinct/count + rows/value/distinct/count
        .route("/api/v1/store/wal/value/distinct/count", get(wal_value_distinct_count))
        .route("/api/v1/store/rows/value/distinct/count", get(rows_value_distinct_count))
        // S3-WS1-52: wal/value/unique/count + rows/value/unique/count
        .route("/api/v1/store/wal/value/unique/count", get(wal_value_unique_count))
        .route("/api/v1/store/rows/value/unique/count", get(rows_value_unique_count))
        // S3-WS1-53: wal/value/trimmed/count + rows/value/trimmed/count
        .route("/api/v1/store/wal/value/trimmed/count", get(wal_value_trimmed_count))
        .route("/api/v1/store/rows/value/trimmed/count", get(rows_value_trimmed_count))
        // S3-WS1-54: wal/value/case_variant/count + rows/value/case_variant/count
        .route("/api/v1/store/wal/value/case_variant/count", get(wal_value_case_variant_count))
        .route("/api/v1/store/rows/value/case_variant/count", get(rows_value_case_variant_count))
        .route("/api/v1/store/wal/order_by/desc_direction/count", get(wal_order_by_desc_direction_count))
        .route("/api/v1/store/rows/order_by/desc_direction/count", get(rows_order_by_desc_direction_count))
        .route("/api/v1/store/wal/order_by/random/count", get(wal_order_by_random_count))
        .route("/api/v1/store/rows/order_by/random/count", get(rows_order_by_random_count))
        .route("/api/v1/store/wal/order_by/random_seeded/count", get(wal_order_by_random_seeded_count))
        .route("/api/v1/store/rows/order_by/random_seeded/count", get(rows_order_by_random_seeded_count))
        .route("/api/v1/store/wal/order_by/asc_direction/count", get(wal_order_by_asc_direction_count))
        .route("/api/v1/store/rows/order_by/asc_direction/count", get(rows_order_by_asc_direction_count))
        .route("/api/v1/store/wal/order_by/rand_alias/count", get(wal_order_by_rand_alias_count))
        .route("/api/v1/store/rows/order_by/rand_alias/count", get(rows_order_by_rand_alias_count))
        .route("/api/v1/store/wal/order_by/multi_column/count", get(wal_order_by_multi_column_count))
        .route("/api/v1/store/rows/order_by/multi_column/count", get(rows_order_by_multi_column_count))
        .route("/api/v1/store/wal/pagination/limit_offset/count", get(wal_pagination_limit_offset_count))
        .route("/api/v1/store/rows/pagination/limit_offset/count", get(rows_pagination_limit_offset_count))
        .route("/api/v1/store/wal/pagination/offset_only/count", get(wal_pagination_offset_only_count))
        .route("/api/v1/store/rows/pagination/offset_only/count", get(rows_pagination_offset_only_count))
        .route("/api/v1/store/wal/having_without_group_by/count", get(wal_having_without_group_by_count))
        .route("/api/v1/store/rows/having_without_group_by/count", get(rows_having_without_group_by_count))
        .route("/api/v1/store/wal/having_with_group_by/count", get(wal_having_with_group_by_count))
        .route("/api/v1/store/rows/having_with_group_by/count", get(rows_having_with_group_by_count))
        .route("/api/v1/store/wal/group_by/rollup/count", get(wal_group_by_rollup_count))
        .route("/api/v1/store/rows/group_by/rollup/count", get(rows_group_by_rollup_count))
        .route("/api/v1/store/wal/group_by/cube/count", get(wal_group_by_cube_count))
        .route("/api/v1/store/rows/group_by/cube/count", get(rows_group_by_cube_count))
        .route("/api/v1/store/wal/select/distinct_on/count", get(wal_select_distinct_on_count))
        .route("/api/v1/store/rows/select/distinct_on/count", get(rows_select_distinct_on_count))
        .route("/api/v1/store/wal/for/update/count", get(wal_for_update_count))
        .route("/api/v1/store/rows/for/update/count", get(rows_for_update_count))
        .route("/api/v1/store/wal/left/join/count", get(wal_left_join_count))
        .route("/api/v1/store/rows/left/join/count", get(rows_left_join_count))
        .route("/api/v1/store/wal/right/join/count", get(wal_right_join_count))
        .route("/api/v1/store/rows/right/join/count", get(rows_right_join_count))
        .route("/api/v1/store/wal/full_outer/join/count", get(wal_full_outer_join_count))
        .route("/api/v1/store/rows/full_outer/join/count", get(rows_full_outer_join_count))
        .route("/api/v1/store/wal/inner/join/count", get(wal_inner_join_count))
        .route("/api/v1/store/rows/inner/join/count", get(rows_inner_join_count))
        .route("/api/v1/store/wal/straight/join/count", get(wal_straight_join_count))
        .route("/api/v1/store/rows/straight/join/count", get(rows_straight_join_count))
        .route("/api/v1/store/wal/semi/join/count", get(wal_semi_join_count))
        .route("/api/v1/store/rows/semi/join/count", get(rows_semi_join_count))
        .route("/api/v1/store/wal/anti/join/count", get(wal_anti_join_count))
        .route("/api/v1/store/rows/anti/join/count", get(rows_anti_join_count))
        .route("/api/v1/store/wal/cross/apply/count", get(wal_cross_apply_count))
        .route("/api/v1/store/rows/cross/apply/count", get(rows_cross_apply_count))
        .route("/api/v1/store/wal/outer/apply/count", get(wal_outer_apply_count))
        .route("/api/v1/store/rows/outer/apply/count", get(rows_outer_apply_count))
        .route("/api/v1/store/wal/apply/count", get(wal_apply_count))
        .route("/api/v1/store/rows/apply/count", get(rows_apply_count))
        .route("/api/v1/store/wal/left/semi/join/count", get(wal_left_semi_join_count))
        .route("/api/v1/store/rows/left/semi/join/count", get(rows_left_semi_join_count))
        .route("/api/v1/store/wal/left/anti/join/count", get(wal_left_anti_join_count))
        .route("/api/v1/store/rows/left/anti/join/count", get(rows_left_anti_join_count))
        .route("/api/v1/store/wal/right/semi/join/count", get(wal_right_semi_join_count))
        .route("/api/v1/store/rows/right/semi/join/count", get(rows_right_semi_join_count))
        .route("/api/v1/store/wal/right/anti/join/count", get(wal_right_anti_join_count))
        .route("/api/v1/store/rows/right/anti/join/count", get(rows_right_anti_join_count))
        .route("/api/v1/store/wal/full/semi/join/count", get(wal_full_semi_join_count))
        .route("/api/v1/store/rows/full/semi/join/count", get(rows_full_semi_join_count))
        .route("/api/v1/store/wal/full/anti/join/count", get(wal_full_anti_join_count))
        .route("/api/v1/store/rows/full/anti/join/count", get(rows_full_anti_join_count))
        .route("/api/v1/store/wal/union/all/count", get(wal_union_all_count))
        .route("/api/v1/store/rows/union/all/count", get(rows_union_all_count))
        .route("/api/v1/store/wal/aggregate/distinct/count", get(wal_aggregate_distinct_count))
        .route("/api/v1/store/rows/aggregate/distinct/count", get(rows_aggregate_distinct_count))
        .route("/api/v1/store/wal/table/alias/count", get(wal_table_alias_count))
        .route("/api/v1/store/rows/table/alias/count", get(rows_table_alias_count))
        .route("/api/v1/store/wal/sql/column/alias/count", get(wal_column_alias_count))
        .route("/api/v1/store/rows/sql/column/alias/count", get(rows_column_alias_count))
        // S11-WS1-19: Scan all rows visible at current snapshot
        .route("/api/v1/store/rows/scan/visible", get(rows_scan_visible))
        // S11-WS1-12: Row store page-level stats
        .route("/api/v1/store/rows/page/stats", get(rows_page_stats))
        // S5-WS4A-02: Broker adapter status + flush
        .route("/api/v1/ingest/outbox/broker/status", get(outbox_broker_status))
        .route("/api/v1/ingest/outbox/broker/flush", post(outbox_broker_flush))
        .route("/api/v1/ingest/outbox/broker/health", get(outbox_broker_health))
        // S5-E4A-01: Connector SDK runtime load
        .route("/api/v1/connectors", get(connector_list))
        .route("/api/v1/connectors/register", post(connector_register))
        .route("/api/v1/connectors/deregister", post(connector_deregister))
        // S5-E4A-01: Connector get by ID
        .route("/api/v1/connectors/get", get(connector_get))
        // S5-E4A-01: Connector update (version / signed flag)
        .route("/api/v1/connectors/update", post(connector_update))
        // S11-WS1-12: Connector health check
        .route("/api/v1/connectors/health", get(connectors_health))
        .route("/api/v1/ai/policy/update", post(ai_policy_update))
        .route("/api/v1/ai/policy/stats", get(ai_policy_stats))
        // S9-WS8-02: AI policy counter reset
        .route("/api/v1/ai/policy/reset", post(ai_policy_reset))
        // S9-WS8-02: AI governance audit
        .route("/api/v1/ai/governance/audit", get(ai_governance_audit))
        .route("/api/v1/ai/request", post(ai_rate_check))
        // WS4 Ingest endpoints
        .route("/api/v1/ingest/csv", post(ingest_csv))
        .route("/api/v1/ingest/json", post(ingest_json))
        .route("/api/v1/ingest/parquet", post(ingest_parquet))
        .route("/api/v1/ingest/excel", post(ingest_excel))
        .route("/api/v1/ingest/chunked", post(ingest_chunked))
        .route("/api/v1/ingest/status", get(ingest_status))
        // S5-WS4-03: ingest schema registry
        .route("/api/v1/ingest/schema", get(ingest_schema_registry))
        // S5-WS4-03: ingest schema list (format-filtered)
        .route("/api/v1/ingest/schema/list", get(ingest_schema_list))
        // S11-WS1-13: Ingest schema field details
        .route("/api/v1/ingest/schema/fields", get(ingest_schema_fields))
        // S5-WS4-03: ingest format auto-detection
        .route("/api/v1/ingest/format/detect", post(ingest_format_detect))
        // S5-WS4-04: ingest connector configuration validation
        .route("/api/v1/ingest/connector/validate", post(ingest_connector_validate))
        .route("/api/v1/ingest/outbox/status", get(ingest_outbox_status))
        .route("/api/v1/ingest/outbox/replay", post(ingest_outbox_replay))
        // REQ-02 DDL catalog endpoint
        .route("/api/v1/catalog/schemas", get(catalog_schemas))
        // REQ-02 DDL catalog table metadata endpoint (columns + indexes)
        .route("/api/v1/catalog/tables/:table_name/columns", get(catalog_table_columns))
        // REQ-23 ACID transaction introspection
        .route("/api/v1/sql/transactions/active", get(sql_transactions_active))
        // S2-WS2-05: isolation stats per active transaction
        .route("/api/v1/sql/transactions/isolation", get(sql_transactions_isolation))
        // REQ-10/19: benchmark endpoints
        .route("/api/v1/benchmark/ingest", post(benchmark_ingest))
        .route("/api/v1/benchmark/query", post(benchmark_query))
        // S6-001: object-scoped history
        .route("/api/v1/history/object", get(history_object))
        // S6-002: dump structure and data
        .route("/api/v1/export/dump", get(export_dump))
        // S6-003: import SQL execution pipeline
        .route("/api/v1/import/sql", post(import_sql))
        // S6-004: server status for IDE panel
        .route("/api/v1/admin/server-status", get(admin_server_status))
        // S6-005: full-text search (feature-gated by VNG_FTS_ENABLED)
        .route("/api/v1/search/fulltext", post(search_fulltext))
        // MCP: Model Context Protocol tool invocation endpoints
        .route("/api/v1/mcp/capabilities", get(mcp_capabilities))
        .route("/api/v1/mcp/invoke", post(mcp_invoke))
        .with_state(state.clone());

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
}

/// Driver-compatible JSON wire shape (`native-protocol-v1` / Rust driver codec).
fn internal_native_frame_to_driver_wire_json(
    frame: &NativeFrame,
    protocol_version: &str,
) -> serde_json::Value {
    let ft = native_frame_type_wire_name(frame.frame_type);
    json!({
        "frame_type": ft,
        "protocol_version": protocol_version,
        "request_id": frame.request_id,
        "session_id": frame.session_id,
        "payload": frame.payload_json.clone().unwrap_or(json!({})),
    })
}

fn native_frame_type_wire_name(t: NativeFrameType) -> &'static str {
    match t {
        NativeFrameType::Hello => "Hello",
        NativeFrameType::HelloAck => "HelloAck",
        NativeFrameType::Auth => "Auth",
        NativeFrameType::AuthAck => "AuthAck",
        NativeFrameType::Command => "Command",
        NativeFrameType::Result => "Result",
        NativeFrameType::Error => "Error",
        NativeFrameType::Ping => "Ping",
        NativeFrameType::Pong => "Pong",
        NativeFrameType::StreamChunk => "StreamChunk",
        NativeFrameType::StreamEnd => "StreamEnd",
        NativeFrameType::Cancel => "Cancel",
        NativeFrameType::Goodbye => "Goodbye",
    }
}

fn wire_protocol_error_frame(request_id: &str, message: &str) -> serde_json::Value {
    json!({
        "frame_type": "Error",
        "protocol_version": "v1",
        "request_id": request_id,
        "session_id": null,
        "payload": { "kind": "protocol", "message": message }
    })
}

fn strip_command_field(payload: &serde_json::Value) -> serde_json::Value {
    let mut obj = payload
        .as_object()
        .cloned()
        .unwrap_or_else(|| serde_json::Map::new());
    obj.remove("command");
    serde_json::Value::Object(obj)
}

fn wire_json_to_native_dispatch_frame(body: &serde_json::Value) -> Result<NativeFrame, String> {
    let frame_type = body
        .get("frame_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing frame_type".to_string())?;
    if frame_type != "Command" {
        return Err(format!("expected Command frame for dispatch, got {frame_type}"));
    }
    let request_id = body
        .get("request_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let session_id = body
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let payload = body.get("payload").cloned().unwrap_or(json!({}));
    let cmd_str = payload
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing payload.command".to_string())?;
    let command = match cmd_str {
        "health" => NativeCommandKind::Health,
        "sql.analyze" => NativeCommandKind::SqlAnalyze,
        "sql.route" => NativeCommandKind::SqlRoute,
        "sql.execute" => NativeCommandKind::SqlExecute,
        "sql.transaction" => NativeCommandKind::SqlTransaction,
        "ingest.schema.registry" => NativeCommandKind::IngestSchemaRegistry,
        _ => NativeCommandKind::Unknown,
    };
    let payload_json = match command {
        NativeCommandKind::Health | NativeCommandKind::IngestSchemaRegistry => None,
        _ => Some(strip_command_field(&payload)),
    };
    Ok(NativeFrame {
        frame_type: NativeFrameType::Command,
        request_id,
        session_id,
        command: Some(command),
        payload_json,
    })
}

fn native_wire_hello_ack(
    request_id: &str,
    session_from_hello: Option<&serde_json::Value>,
) -> serde_json::Value {
    json!({
        "frame_type": "HelloAck",
        "protocol_version": "v1",
        "request_id": request_id,
        "session_id": session_from_hello.cloned(),
        "payload": {
            "accepted": true,
            "version": "v1",
        }
    })
}

fn native_wire_auth_ack(request_id: &str, session_id: Option<&str>, accepted: bool) -> serde_json::Value {
    json!({
        "frame_type": "AuthAck",
        "protocol_version": "v1",
        "request_id": request_id,
        "session_id": session_id,
        "payload": { "accepted": accepted }
    })
}

/// NT-S6-001: check an Auth frame payload against configured admin_api_key and/or bearer_token.
///
/// Auth succeeds when no credentials are configured (open listener), or when at least one of the
/// supplied fields matches a configured credential.  Both fields are optional in the frame.
fn native_auth_payload_matches_runtime(
    state: &AppState,
    config: &NativeListenerConfig,
    auth_payload: &serde_json::Value,
) -> bool {
    let has_admin_key_cfg = state.admin_api_key.is_some();
    let has_bearer_token_cfg = config.bearer_token.is_some();

    // No credentials configured → open listener, always accept.
    if !has_admin_key_cfg && !has_bearer_token_cfg {
        return true;
    }

    // Check admin_api_key field
    if let Some(expected) = &state.admin_api_key {
        if let Some(key) = auth_payload.get("admin_api_key").and_then(|v| v.as_str()) {
            if key == expected.as_str() {
                return true;
            }
        }
    }

    // NT-S6-001: check bearer_token field
    if let Some(expected_token) = &config.bearer_token {
        if let Some(token) = auth_payload.get("bearer_token").and_then(|v| v.as_str()) {
            if token == expected_token.as_str() {
                return true;
            }
        }
    }

    false
}

/// One JSON object per line on stderr (`component` = `vng_native_listener`) for log aggregation.
fn vng_native_listener_log(event: &str, detail: serde_json::Value) {
    let mut m = serde_json::Map::new();
    m.insert("component".to_string(), json!("vng_native_listener"));
    m.insert("event".to_string(), json!(event));
    if let Some(delta) = detail.as_object() {
        for (k, v) in delta {
            m.insert(k.clone(), v.clone());
        }
    }
    eprintln!("{}", serde_json::Value::Object(m));
}

async fn native_read_framed<S: AsyncRead + Unpin>(
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

async fn run_native_connection<S: AsyncRead + AsyncWrite + Send + Unpin>(
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
pub(crate) fn try_handle_call_insert_rows_demo(
    state: &AppState,
    _headers: &HeaderMap,
    principal: &RuntimeAccessPrincipal,
    connection_id: &str,
    req: &SqlExecuteRequest,
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
    let existing_max: usize = match state.row_store.lock() {
        Ok(rs) => {
            let snap = rs.current_xid();
            rs.scan_at_snapshot(snap)
                .iter()
                .filter(|(k, _)| k.starts_with(&format!("{}:", table_name)))
                .filter_map(|(k, _)| k.splitn(2, ':').nth(1).and_then(|v| v.parse::<usize>().ok()))
                .max()
                .unwrap_or(0)
        }
        Err(_) => return Some(Ok(svc_unavailable_sql_response("row_store mutex poisoned"))),
    };
    let ddl_cols = match state.ddl_catalog.lock() {
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
        let key = format!("{}:{}", table_name, row_id);
        let mut row_data: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        row_data.insert("__table".to_string(), table_name.clone());
        for col_idx in 0..col_count {
            let col_name = ddl_cols.get(col_idx).cloned().unwrap_or_else(|| format!("col_{col_idx}"));
            let val = synthesize_demo_value(&col_name, &table_name, row_id);
            row_data.insert(col_name, val);
        }
        match state.row_store.lock() {
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
        status: "ok",
        route_path: "oltp".to_string(),
        reason: format!("inserted {inserted} demo rows into {table_name}"),
        transaction: Some(SqlTransactionResponse {
            status: "committed",
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
    }))))
}

/// Heuristic value generator for the insert_rows demo.
/// Pure function — no state, easy to unit-test.
fn synthesize_demo_value(col_name: &str, table_name: &str, row_id: usize) -> String {
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

/// Build a 503 SqlExecuteResponse for graceful degradation when an internal
/// mutex is poisoned (which happens after a panic in a critical section).
/// Returning 503 instead of expect()-panicking keeps the rest of the service alive.
fn svc_unavailable_sql_response(reason: &str) -> (StatusCode, Json<SqlExecuteResponse>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(SqlExecuteResponse {
            status: "error",
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

pub(crate) fn extract_delete_key_from_sql(sql: &str) -> Option<String> {
    use voltnuerongrid_sql::tokenizer::{semantic_tokens, Token};
    let tokens = semantic_tokens(sql);
    let upper = sql.trim_start().to_ascii_uppercase();
    if !upper.starts_with("DELETE") {
        return None;
    }
    let mut after_where = false;
    let mut past_eq = false;
    for tok in &tokens {
        match tok {
            Token::Keyword(k) if k.eq_ignore_ascii_case("WHERE") => after_where = true,
            Token::Symbol(s) if s == "=" && after_where => past_eq = true,
            Token::StringLiteral(s) if past_eq => return Some(s.clone()),
            Token::Number(n) if past_eq => return Some(n.clone()),
            _ => {}
        }
    }
    None
}

/// Parse a SQL UPDATE statement and return (row_key, row_data) for MVCC insert (new version).
/// Pattern: UPDATE <table> SET col=val [WHERE col='key']
pub(crate) fn extract_update_row_from_sql(
    sql: &str,
) -> Option<(String, std::collections::HashMap<String, String>)> {
    use voltnuerongrid_sql::ast::{parse_one, Statement};
    use voltnuerongrid_sql::tokenizer::{semantic_tokens, Token};
    let stmt = parse_one(sql).ok()?;
    let Statement::Update(upd) = stmt else {
        return None;
    };
    // Prefer the WHERE clause value as key; fall back to table name
    let tokens = semantic_tokens(sql);
    let mut key = upd.table.clone();
    let mut after_where = false;
    let mut past_eq = false;
    for tok in &tokens {
        match tok {
            Token::Keyword(k) if k.eq_ignore_ascii_case("WHERE") => after_where = true,
            Token::Symbol(s) if s == "=" && after_where => past_eq = true,
            Token::StringLiteral(s) if past_eq => {
                key = s.clone();
                break;
            }
            Token::Number(n) if past_eq => {
                key = n.clone();
                break;
            }
            _ => {}
        }
    }
    let row_key = format!("{}:{}", upd.table, key);
    let mut data = std::collections::HashMap::new();
    data.insert("__table".to_string(), upd.table.clone());
    for (col, val) in &upd.assignments {
        data.insert(col.clone(), val.clone());
    }
    Some((row_key, data))
}

/// Extract ordered column names from a CREATE TABLE DDL statement.
/// Returns `vec!["id", "name", ...]` or an empty Vec if parsing fails.
pub(crate) fn extract_column_names_from_ddl(ddl: &str) -> Vec<String> {
    // Find the column list between the first '(' and last ')'
    let open = ddl.find('(');
    let close = ddl.rfind(')');
    let (open, close) = match (open, close) {
        (Some(o), Some(c)) if c > o => (o, c),
        _ => return Vec::new(),
    };
    let inner = &ddl[open + 1..close];
    // Split on commas at depth 0 (ignore nested parens like DECIMAL(10,2))
    let mut cols = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in inner.chars() {
        match ch {
            '(' => { depth += 1; current.push(ch); }
            ')' => { if depth > 0 { depth -= 1; } current.push(ch); }
            ',' if depth == 0 => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() { cols.push(trimmed); }
                current = String::new();
            }
            _ => { current.push(ch); }
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() { cols.push(trimmed); }

    // Extract the first token (column name) from each clause, skip table constraints
    let constraint_kws = ["PRIMARY", "FOREIGN", "UNIQUE", "CHECK", "CONSTRAINT", "INDEX"];
    cols.into_iter()
        .filter_map(|clause| {
            let first = clause.split_whitespace().next()?.to_ascii_lowercase();
            // Skip constraint lines
            if constraint_kws.iter().any(|kw| first.eq_ignore_ascii_case(kw)) {
                return None;
            }
            Some(first)
        })
        .collect()
}

/// Parse a SQL INSERT statement using the AST parser and return a (row_key, row_data) pair
/// suitable for writing into PagedRowStore. Returns None for non-INSERT or unparseable input.
/// Stores column-value pairs so SELECT can return structured data.
/// The "__table" meta-key identifies which table the row belongs to.
/// `ddl_col_names` provides ordered real column names from the CREATE TABLE DDL; used as
/// fallback when the INSERT has no explicit column list.
pub(crate) fn extract_insert_row_from_sql(
    sql: &str,
) -> Option<(String, std::collections::HashMap<String, String>)> {
    extract_insert_row_from_sql_with_cols(sql, &[])
}

/// Extract ALL rows from a (possibly multi-row) INSERT statement.
/// Returns one `(row_key, RowData, single_row_sql)` per VALUES tuple.
/// Strips `schema.table` qualifiers so the internal SQL parser can handle them.
pub(crate) fn extract_all_insert_rows(
    sql: &str,
) -> Vec<(String, std::collections::HashMap<String, String>, String)> {
    use voltnuerongrid_sql::{parse_one, Statement};
    // Strip schema qualifier: "INSERT INTO oltp.customers" → "INSERT INTO customers"
    let normalized = strip_schema_qualifier_from_insert(sql);
    let ins = match parse_one(&normalized) {
        Ok(Statement::Insert(i)) => i,
        _ => return Vec::new(),
    };
    // Preserve original (schema-qualified) table name for WAL
    let orig_table = {
        let upper = sql.to_ascii_uppercase();
        if let Some(into_pos) = upper.find("INTO") {
            let after = sql[into_pos + 4..].trim_start();
            let end = after.find(|c: char| c == ' ' || c == '\n' || c == '\t' || c == '(').unwrap_or(after.len());
            after[..end].to_string()
        } else {
            ins.table.clone()
        }
    };
    let unqualified_table = orig_table.rsplit('.').next().unwrap_or(&orig_table).to_string();
    let mut results = Vec::new();
    for row_vals in &ins.values {
        if row_vals.is_empty() {
            continue;
        }
        let mut data = std::collections::HashMap::new();
        data.insert("__table".to_string(), unqualified_table.clone());
        for (i, val) in row_vals.iter().enumerate() {
            let col = if !ins.columns.is_empty() {
                ins.columns.get(i).map(|c| c.to_ascii_lowercase()).unwrap_or_else(|| format!("col_{i}"))
            } else {
                format!("col_{i}")
            };
            data.insert(col.clone(), val.clone());
        }
        let first_val = &row_vals[0];
        let row_key = format!("{unqualified_table}:{first_val}");
        // Build a canonical single-row INSERT for WAL replay (uses original table name)
        let col_list = if !ins.columns.is_empty() {
            format!(" ({})", ins.columns.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", "))
        } else {
            String::new()
        };
        let val_list = row_vals.iter()
            .map(|v| {
                let trimmed = v.trim();
                if trimmed.parse::<f64>().is_ok() { trimmed.to_string() } else { format!("'{}'", trimmed.replace('\'', "''")) }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let single_sql = format!("INSERT INTO {orig_table}{col_list} VALUES ({val_list});");
        results.push((row_key, data, single_sql));
    }
    results
}

/// Remove `schema.` prefix from table name in INSERT statement so the parser
/// (which only handles unqualified names) can parse the statement correctly.
fn strip_schema_qualifier_from_insert(sql: &str) -> String {
    if !sql.contains('.') {
        return sql.to_string();
    }
    let sql_upper = sql.to_ascii_uppercase();
    if let Some(into_pos) = sql_upper.find("INTO") {
        let after_into = into_pos + 4;
        let ws_len = sql[after_into..].len() - sql[after_into..].trim_start().len();
        let table_start = after_into + ws_len;
        let table_text = &sql[table_start..];
        let table_end = table_text.find(|c: char| c == ' ' || c == '\n' || c == '\t' || c == '(').unwrap_or(table_text.len());
        let table_name = &table_text[..table_end];
        if let Some(dot) = table_name.find('.') {
            let unqualified_start = table_start + dot + 1;
            let after_table = table_start + table_end;
            return format!("{}{}{}", &sql[..table_start], &sql[unqualified_start..after_table], &sql[after_table..]);
        }
    }
    sql.to_string()
}

fn extract_insert_row_from_sql_with_cols(
    sql: &str,
    ddl_col_names: &[String],
) -> Option<(String, std::collections::HashMap<String, String>)> {
    use voltnuerongrid_sql::{parse_one, Statement};
    let ins = match parse_one(sql) {
        Ok(Statement::Insert(i)) => i,
        _ => return None,
    };
    // Strip schema qualifier (public.foo → foo)
    let table = ins.table.rsplit('.').next().unwrap_or(&ins.table).to_string();

    // Use the first row of values (single-row INSERT)
    let row_vals = ins.values.first()?;
    if row_vals.is_empty() {
        return None;
    }

    let mut data = std::collections::HashMap::new();
    // Store the table name under the meta key __table (used for table-scoped SELECT scans)
    data.insert("__table".to_string(), table.clone());

    for (i, val) in row_vals.iter().enumerate() {
        let col = if !ins.columns.is_empty() {
            // Explicit column list in INSERT statement — always preferred
            ins.columns
                .get(i)
                .map(|c| c.to_ascii_lowercase())
                .unwrap_or_else(|| format!("col_{i}"))
        } else if let Some(name) = ddl_col_names.get(i) {
            // Fall back to DDL-derived column names (CREATE TABLE definition order)
            name.clone()
        } else {
            format!("col_{i}")
        };
        data.insert(col, val.clone());
    }

    // Row key = table:first_value for uniqueness within the store
    let first_val = row_vals[0].as_str();
    let row_key = format!("{table}:{first_val}");
    Some((row_key, data))
}

pub(crate) fn route_path_name(path: QueryPath) -> &'static str {
    match path {
        QueryPath::Oltp => "oltp",
        QueryPath::Olap => "olap",
        QueryPath::Hybrid => "hybrid",
        QueryPath::Unknown => "unknown",
    }
}

pub(crate) fn execute_transaction_statements(statements: Vec<String>) -> (StatusCode, SqlTransactionResponse) {
    if statements.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            SqlTransactionResponse {
                status: "error",
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
                status: "error",
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
            status: "committed",
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
/// S3-WS1-05: parse a WHERE clause string into `VectorizedFilter` predicates.
/// Handles simple `col op val` expressions joined by ` AND `.
pub(crate) fn parse_where_predicates(
    where_clause: &str,
) -> Option<Vec<voltnuerongrid_store::columnar::VectorizedFilter>> {
    use voltnuerongrid_store::columnar::{FilterOp, VectorizedFilter};
    let preds: Vec<VectorizedFilter> = where_clause
        .split(" AND ")
        .filter_map(|clause| {
            let clause = clause.trim();
            let ops: &[(&str, FilterOp)] = &[
                (">=", FilterOp::Gte),
                ("<=", FilterOp::Lte),
                ("!=", FilterOp::Ne),
                (">",  FilterOp::Gt),
                ("<",  FilterOp::Lt),
                ("=",  FilterOp::Eq),
            ];
            for (sym, op) in ops {
                if let Some(pos) = clause.find(sym) {
                    let col = clause[..pos].trim().to_string();
                    let val = clause[pos + sym.len()..].trim()
                        .trim_matches('\'').trim_matches('"').to_string();
                    if !col.is_empty() {
                        return Some(VectorizedFilter { column: col, op: op.clone(), value: val });
                    }
                }
            }
            None
        })
        .collect();
    if preds.is_empty() { None } else { Some(preds) }
}

// ─── S7-WS6-04: Chaos/game-day injection handlers ────────────────────────────

pub(crate) fn now_epoch_ms_chaos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}


// ─── S10-WS15-02: CDC stream from WAL ─────────────────────────────────────────


fn evaluate_deadlock_scan_outcome(
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

fn cleanup_wait_edges_for_resource(
    wait_graph: &mut HashMap<String, String>,
    resource_key: &str,
) {
    wait_graph.retain(|_, waiting_resource| waiting_resource != resource_key);
}

pub(crate) fn execute_olap_query(query: String, max_rows: Option<usize>) -> OlapQueryResponse {
    let started = Instant::now();
    let elapsed = started.elapsed().as_millis();
    let resolved_max_rows = max_rows.unwrap_or(1000);
    OlapQueryResponse {
        status: "ok",
        query_signature: query.chars().take(64).collect(),
        elapsed_ms: elapsed,
        rows: resolved_max_rows.min(10_000),
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
    use voltnuerongrid_exec_datafusion::datafusion::execute_select_from_rows;
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
                execute_select_from_rows(stmt_str, table_rows, remaining)
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
/// Uses `block_in_place` when a multi-thread handle is available (service path)
/// and falls back to a fresh single-threaded runtime for test contexts.
fn run_async_in_executor<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => tokio::runtime::Runtime::new()
            .expect("tokio runtime for DataFusion")
            .block_on(fut),
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

pub(crate) fn execute_udf_runtime_scaffold(sql_batch: &str) -> Result<Vec<UdfExecutionResult>, String> {
    enforce_udf_guardrails(sql_batch)?;
    let mut results = Vec::new();
    for statement in SqlAnalyzer::parse_batch(sql_batch) {
        let normalized = statement.raw.to_ascii_lowercase();
        if normalized.contains("udf_rust(") {
            let input = extract_udf_input(&statement.raw).unwrap_or_else(|| "sample".to_string());
            results.push(UdfExecutionResult {
                language: "rust",
                function: "udf_rust",
                output: input.to_ascii_uppercase(),
                input,
            });
        }
        if normalized.contains("udf_js(") {
            let input = extract_udf_input(&statement.raw).unwrap_or_else(|| "sample".to_string());
            let output: String = input.chars().rev().collect();
            results.push(UdfExecutionResult {
                language: "javascript",
                function: "udf_js",
                output,
                input,
            });
        }
        if normalized.contains("udf_python(") {
            let input = extract_udf_input(&statement.raw).unwrap_or_else(|| "sample".to_string());
            results.push(UdfExecutionResult {
                language: "python",
                function: "udf_python",
                output: input.len().to_string(),
                input,
            });
        }
    }
    Ok(results)
}

pub(crate) fn udf_function_catalog_contract() -> Vec<UdfFunctionCatalogEntry> {
    vec![
        UdfFunctionCatalogEntry {
            name: "udf_rust",
            language: "rust",
            deterministic: true,
            status: "enabled",
        },
        UdfFunctionCatalogEntry {
            name: "udf_js",
            language: "javascript",
            deterministic: false,
            status: "enabled",
        },
        UdfFunctionCatalogEntry {
            name: "udf_python",
            language: "python",
            deterministic: false,
            status: "enabled",
        },
    ]
}

pub(crate) fn udf_guard_policy_contract() -> Vec<UdfLanguageGuardPolicy> {
    vec![
        UdfLanguageGuardPolicy {
            language: "rust",
            blocked_tokens: vec!["unsafe", "std::process", "process::"],
            max_input_bytes: 256,
        },
        UdfLanguageGuardPolicy {
            language: "javascript",
            blocked_tokens: vec!["eval(", "function(", "child_process"],
            max_input_bytes: 256,
        },
        UdfLanguageGuardPolicy {
            language: "python",
            blocked_tokens: vec!["import os", "subprocess", "exec("],
            max_input_bytes: 256,
        },
    ]
}

pub(crate) fn build_udf_execution_plan(sql_batch: &str) -> Vec<UdfExecutionPlanStep> {
    let mut plan = Vec::new();
    for statement in SqlAnalyzer::parse_batch(sql_batch) {
        let mut invocations = Vec::new();
        let normalized = statement.raw.to_ascii_lowercase();
        if normalized.contains("udf_rust(") {
            invocations.push(UdfInvocationPlan {
                function: "udf_rust",
                language: "rust",
                guard_policy: "rust_default",
            });
        }
        if normalized.contains("udf_js(") {
            invocations.push(UdfInvocationPlan {
                function: "udf_js",
                language: "javascript",
                guard_policy: "javascript_default",
            });
        }
        if normalized.contains("udf_python(") {
            invocations.push(UdfInvocationPlan {
                function: "udf_python",
                language: "python",
                guard_policy: "python_default",
            });
        }
        let analysis = SqlAnalyzer::analyze_statement(&statement.raw);
        let route_path = if analysis.kind == SqlStatementKind::Select {
            "olap"
        } else {
            "oltp"
        };
        plan.push(UdfExecutionPlanStep {
            statement: statement.raw,
            route_path: route_path.to_string(),
            udf_invocations: invocations,
        });
    }
    plan
}

fn enforce_udf_guardrails(sql_batch: &str) -> Result<(), String> {
    let lowered = sql_batch.to_ascii_lowercase();
    let has_rust_udf = lowered.contains("udf_rust(");
    let has_js_udf = lowered.contains("udf_js(");
    let has_python_udf = lowered.contains("udf_python(");

    if has_rust_udf && ["unsafe", "std::process", "process::"].iter().any(|t| lowered.contains(t)) {
        return Err("udf_guardrail_blocked_rust_payload".to_string());
    }
    if has_js_udf && ["eval(", "function(", "child_process"].iter().any(|t| lowered.contains(t)) {
        return Err("udf_guardrail_blocked_javascript_payload".to_string());
    }
    if has_python_udf && ["import os", "subprocess", "exec("].iter().any(|t| lowered.contains(t)) {
        return Err("udf_guardrail_blocked_python_payload".to_string());
    }
    Ok(())
}

fn extract_udf_input(statement: &str) -> Option<String> {
    let first = statement.find('\'')?;
    let remaining = &statement[first + 1..];
    let end = remaining.find('\'')?;
    Some(remaining[..end].to_string())
}

pub(crate) fn failure_budget_snapshot(consumed_percent: f64) -> FailureBudgetSnapshot {
    let bounded_consumed = consumed_percent.clamp(0.0, 100.0);
    let remaining = (100.0 - bounded_consumed).max(0.0);
    FailureBudgetSnapshot {
        window_minutes: 60,
        error_budget_percent: 1.0,
        consumed_percent: bounded_consumed,
        remaining_percent: remaining,
        burn_rate: (bounded_consumed / 10.0).max(0.1),
    }
}

pub(crate) fn rate_limit_policy_snapshot(current_minute_count: u32) -> RateLimitPolicySnapshot {
    let (allowed, _, _) = evaluate_rate_limit(current_minute_count, 1, 600, 50);
    RateLimitPolicySnapshot {
        requests_per_minute: 600,
        burst_limit: 50,
        current_minute_count,
        allowed,
    }
}

pub(crate) fn evaluate_failure_budget_alert(
    consumed_percent: f64,
    burn_rate: f64,
) -> FailureBudgetAlertResponse {
    if consumed_percent >= 80.0 || burn_rate >= 3.0 {
        return FailureBudgetAlertResponse {
            status: "ok",
            alert_state: "triggered",
            severity: "critical",
            threshold_percent: 80.0,
            consumed_percent,
            burn_rate,
            recommended_action: "start_automated_dr_failover_drill",
        };
    }
    if consumed_percent >= 50.0 || burn_rate >= 1.5 {
        return FailureBudgetAlertResponse {
            status: "ok",
            alert_state: "warning",
            severity: "high",
            threshold_percent: 50.0,
            consumed_percent,
            burn_rate,
            recommended_action: "increase_error_budget_sampling_and_throttle_low_priority_jobs",
        };
    }
    FailureBudgetAlertResponse {
        status: "ok",
        alert_state: "nominal",
        severity: "info",
        threshold_percent: 50.0,
        consumed_percent,
        burn_rate,
        recommended_action: "continue_monitoring",
    }
}

fn default_dr_hook_policy_config() -> DrHookPolicyConfig {
    DrHookPolicyConfig {
        min_mode: AutonomousMode::Supervised,
        cooldown_seconds: 30,
        max_retries: 3,
        base_backoff_ms: 500,
        max_backoff_ms: 10_000,
        allowed_hooks: vec![
            "failover_drill".to_string(),
            "replay_checkpoint_verify".to_string(),
        ],
    }
}

pub(crate) fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub(crate) fn now_unix_ms_u64() -> u64 {
    now_unix_ms().min(u128::from(u64::MAX)) as u64
}

pub(crate) fn pool_stats_response(stats: &voltnuerongrid_driver_rust::PoolStats) -> PoolStatsResponse {
    PoolStatsResponse {
        total_connections: stats.total_connections,
        idle_connections: stats.idle_connections,
        active_connections: stats.active_connections,
        failed_connections: stats.failed_connections,
        circuit_breaker_state: stats.circuit_breaker_state.clone(),
        storm_active: stats.storm_active,
        current_rps: stats.current_rps,
        total_acquired: stats.total_acquired,
        total_released: stats.total_released,
        total_rejected: stats.total_rejected,
        total_circuit_opens: stats.total_circuit_opens,
    }
}

pub(crate) fn pool_acquire_error_state(error: &PoolAcquireError) -> &'static str {
    match error {
        PoolAcquireError::PoolExhausted { .. } => "pool_exhausted",
        PoolAcquireError::CircuitOpen { .. } => "circuit_open",
        PoolAcquireError::StormRejection { .. } => "storm_rejected",
        PoolAcquireError::AcquireTimeout { .. } => "acquire_timeout",
    }
}


pub(crate) fn acquire_sql_data_plane_connection(
    state: &AppState,
    headers: &HeaderMap,
    principal: &RuntimeAccessPrincipal,
    route_scope: &str,
) -> Result<String, (StatusCode, Json<AuthErrorResponse>)> {
    let now_ms = now_unix_ms_u64();
    let acquire_result = state
        .driver_pool
        .lock()
        .expect("driver pool lock")
        .acquire(now_ms);
    match acquire_result {
        Ok(connection_id) => Ok(connection_id),
        Err(error) => {
            append_runtime_audit_event(
                state,
                AuditEventKind::Sql,
                principal,
                "sql_data_plane_pool_acquire",
                "rejected",
                json!({
                    "route_scope": route_scope,
                    "reason": error.to_string(),
                }),
            );
            let locale = locale_from_headers(headers);
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AuthErrorResponse {
                    status: "unavailable",
                    reason: "driver_pool_unavailable".to_string(),
                    locale: locale.as_str().to_string(),
                    localized_message: "Service temporarily unavailable".to_string(),
                }),
            ))
        }
    }
}

pub(crate) fn release_sql_data_plane_connection(state: &AppState, connection_id: &str) {
    let now_ms = now_unix_ms_u64();
    let _ = state
        .driver_pool
        .lock()
        .expect("driver pool lock")
        .release(connection_id, now_ms);
}

fn compute_retry_backoff_ms(attempt: u32, base_backoff_ms: u64, max_backoff_ms: u64) -> u64 {
    let exponent = attempt.saturating_sub(1).min(8);
    let factor = 1u64 << exponent;
    base_backoff_ms.saturating_mul(factor).min(max_backoff_ms)
}

pub(crate) fn build_retry_plan(policy: &DrHookPolicyConfig, attempts: u32) -> Vec<DrHookRetryPlanStep> {
    (1..=attempts)
        .map(|attempt| {
            let backoff = compute_retry_backoff_ms(
                attempt,
                policy.base_backoff_ms,
                policy.max_backoff_ms,
            );
            // Deterministic jitter contract scaffold: 20% envelope for callers.
            let jitter = (backoff / 5).max(50);
            DrHookRetryPlanStep {
                attempt,
                recommended_backoff_ms: backoff,
                jitter_range_ms: jitter,
            }
        })
        .collect()
}

fn dr_hook_policy_backup_path(path: &str) -> String {
    format!("{path}.bak")
}

fn compute_dr_hook_policy_checksum(snapshot: &DrHookPolicyStateSnapshot) -> String {
    // Canonicalize hook ordering before hashing so checksum is stable.
    let ordered_hooks: std::collections::BTreeMap<String, DrHookRuntimeState> = snapshot
        .hooks
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let encoded = serde_json::to_vec(&ordered_hooks).unwrap_or_default();
    // FNV-1a 64-bit checksum: lightweight corruption guard for persisted state.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in encoded {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn decode_dr_hook_policy_state(contents: &str) -> Option<DrHookPolicyState> {
    if let Ok(envelope) = serde_json::from_str::<DrHookPolicyStateEnvelope>(contents) {
        if envelope.schema_version == 1 {
            let expected = compute_dr_hook_policy_checksum(&envelope.snapshot);
            if envelope.checksum_hex.eq_ignore_ascii_case(&expected) {
                return Some(DrHookPolicyState {
                    hooks: envelope.snapshot.hooks,
                });
            }
            return None;
        }
    }

    // Backward compatibility: support pre-envelope snapshot files.
    serde_json::from_str::<DrHookPolicyStateSnapshot>(contents)
        .map(|snapshot| DrHookPolicyState {
            hooks: snapshot.hooks,
        })
        .ok()
}

fn load_dr_hook_policy_state(path: Option<&str>) -> DrHookPolicyState {
    let Some(path_value) = path else {
        return DrHookPolicyState::default();
    };

    if let Ok(contents) = fs::read_to_string(path_value) {
        if let Some(state) = decode_dr_hook_policy_state(&contents) {
            return state;
        }
    }

    let backup_path = dr_hook_policy_backup_path(path_value);
    if let Ok(contents) = fs::read_to_string(backup_path) {
        if let Some(state) = decode_dr_hook_policy_state(&contents) {
            return state;
        }
    }

    DrHookPolicyState::default()
}

fn persist_dr_hook_policy_state(state: &AppState) {
    let Some(path_value) = state.dr_hook_state_path.as_deref() else {
        return;
    };
    let snapshot = state.dr_hook_policy_state.lock().ok().map(|guard| DrHookPolicyStateSnapshot {
        hooks: guard.hooks.clone(),
    });
    let Some(snapshot) = snapshot else {
        return;
    };
    if let Some(parent) = std::path::Path::new(path_value).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    let envelope = DrHookPolicyStateEnvelope {
        schema_version: 1,
        written_unix_ms: now_unix_ms(),
        checksum_hex: compute_dr_hook_policy_checksum(&snapshot),
        snapshot,
    };

    if let Ok(encoded) = serde_json::to_string_pretty(&envelope) {
        let path = std::path::Path::new(path_value);
        if path.exists() {
            let backup_path = dr_hook_policy_backup_path(path_value);
            let _ = fs::copy(path_value, backup_path);
        }

        let temp_path = format!("{path_value}.tmp");
        if fs::write(&temp_path, encoded).is_ok() {
            let _ = fs::remove_file(path_value);
            let _ = fs::rename(&temp_path, path_value);
            let _ = fs::remove_file(&temp_path);
        }
    }
}

pub(crate) fn enqueue_dr_hook_task(
    state: &AppState,
    hook: &str,
    scope: Option<&str>,
    dry_run: bool,
    requested_by: &str,
    reason: &str,
) -> DrHookScheduledTask {
    let task = DrHookScheduledTask {
        task_id: format!("task-{}", DR_HOOK_COUNTER.fetch_add(1, Ordering::Relaxed)),
        hook: hook.trim().to_ascii_lowercase(),
        scope: scope.unwrap_or("cluster").trim().to_string(),
        dry_run,
        requested_by: requested_by.trim().to_string(),
        reason: reason.trim().to_string(),
        enqueued_unix_ms: now_unix_ms(),
    };
    if let Ok(mut queue) = state.dr_hook_queue.lock() {
        queue.push_back(task.clone());
    }
    record_transport_mutation(
        state,
        &state.node_id,
        "*",
        "scheduler_queue",
        "dr_hook_queue",
        &task.task_id,
        MutationOp::Insert,
        json!({
            "hook": task.hook,
            "scope": task.scope,
            "dry_run": task.dry_run,
            "requested_by": task.requested_by,
            "reason": task.reason,
            "transport": "scheduler_queue"
        }),
    );
    task
}

fn dequeue_dr_hook_task(state: &AppState) -> Option<DrHookScheduledTask> {
    state
        .dr_hook_queue
        .lock()
        .ok()
        .and_then(|mut queue| queue.pop_front())
}

fn build_sre_gate_evaluation(state: &AppState) -> SreGateEvaluationResponse {
    let failure_budget = failure_budget_snapshot(12.5);
    let queue_depth = state.dr_hook_queue.lock().map(|q| q.len()).unwrap_or(usize::MAX);
    let unresolved_critical_signals = state
        .cluster_failure_signals
        .lock()
        .map(|signals| {
            signals
                .iter()
                .filter(|s| s.severity.eq_ignore_ascii_case("critical") && !s.resolved)
                .count()
        })
        .unwrap_or(usize::MAX);
    let persistence_configured = state
        .dr_hook_state_path
        .as_ref()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);

    let criteria = vec![
        SreGateCriterion {
            name: "failure_budget_below_warning".to_string(),
            passed: failure_budget.consumed_percent < 50.0,
            detail: format!("consumed_percent={}", failure_budget.consumed_percent),
        },
        SreGateCriterion {
            name: "dr_queue_depth_below_threshold".to_string(),
            passed: queue_depth < 100,
            detail: format!("queue_depth={queue_depth} threshold=100"),
        },
        SreGateCriterion {
            name: "no_unresolved_critical_signals".to_string(),
            passed: unresolved_critical_signals == 0,
            detail: format!("unresolved_critical_signals={unresolved_critical_signals}"),
        },
        SreGateCriterion {
            name: "dr_state_persistence_configured".to_string(),
            passed: persistence_configured,
            detail: format!("state_path={:?}", state.dr_hook_state_path),
        },
    ];
    let failed: Vec<&SreGateCriterion> = criteria.iter().filter(|c| !c.passed).collect();
    let gate_result = if failed.is_empty() {
        "pass"
    } else if failed.len() == 1 {
        "warn"
    } else {
        "fail"
    };
    let recommended_actions = failed
        .iter()
        .map(|criterion| format!("resolve_{}", criterion.name))
        .collect::<Vec<_>>();

    SreGateEvaluationResponse {
        status: "ok",
        gate_result,
        criteria,
        recommended_actions,
    }
}

fn export_gate_report(path: &str, evaluation: &SreGateEvaluationResponse) {
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    if let Ok(encoded) = serde_json::to_string_pretty(evaluation) {
        let _ = fs::write(path, encoded);
    }
}

pub(crate) fn execute_dr_hook(
    state: &AppState,
    hook: &str,
    scope: Option<&str>,
    dry_run: bool,
) -> DrHookExecutionRecord {
    let execution_id = format!("drh-{}", DR_HOOK_COUNTER.fetch_add(1, Ordering::Relaxed));
    let now_ms = now_unix_ms();
    let policy = state.dr_hook_policy_config.as_ref();
    let normalized_scope = scope.unwrap_or("cluster").trim();
    let normalized_scope = if normalized_scope.is_empty() {
        "cluster"
    } else {
        normalized_scope
    };
    let normalized_hook = hook.trim().to_ascii_lowercase();
    let mut policy_decision = "allow";
    let mut cooldown_remaining_ms = 0u64;
    let mut retry_attempt = 1u32;
    let mut retry_backoff_ms = policy.base_backoff_ms;
    let mut status: &'static str;
    let mut details: String;

    if state.autonomous_mode.rank() < policy.min_mode.rank() {
        policy_decision = "deny_mode";
        status = "rejected";
        details = format!(
            "autonomous_mode {:?} below required {:?}",
            state.autonomous_mode, policy.min_mode
        );
    } else if !policy
        .allowed_hooks
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(&normalized_hook))
    {
        policy_decision = "deny_unsupported_hook";
        status = "rejected";
        details = format!("unsupported_dr_hook={normalized_hook}");
    } else if let Ok(mut guard) = state.dr_hook_policy_state.lock() {
        let runtime = guard
            .hooks
            .entry(normalized_hook.clone())
            .or_insert_with(DrHookRuntimeState::default);
        let cooldown_window_ms = u128::from(policy.cooldown_seconds) * 1_000;

        if runtime.last_attempt_unix_ms > 0
            && now_ms.saturating_sub(runtime.last_attempt_unix_ms) < cooldown_window_ms
        {
            policy_decision = "deny_cooldown";
            status = "cooldown";
            let elapsed = now_ms.saturating_sub(runtime.last_attempt_unix_ms);
            cooldown_remaining_ms = cooldown_window_ms.saturating_sub(elapsed) as u64;
            retry_attempt = runtime.consecutive_failures.saturating_add(1);
            retry_backoff_ms = compute_retry_backoff_ms(
                retry_attempt,
                policy.base_backoff_ms,
                policy.max_backoff_ms,
            );
            details = format!(
                "cooldown_active hook={normalized_hook} remaining_ms={cooldown_remaining_ms}"
            );
        } else {
            retry_attempt = runtime.consecutive_failures.saturating_add(1);
            retry_backoff_ms = compute_retry_backoff_ms(
                retry_attempt,
                policy.base_backoff_ms,
                policy.max_backoff_ms,
            );

            let (resolved_status, resolved_details) = match normalized_hook.as_str() {
                "failover_drill" => {
                    if dry_run {
                        (
                            "simulated",
                            format!("dry_run prepared failover drill for scope={normalized_scope}"),
                        )
                    } else {
                        let (previous, current) =
                            rotate_leader(&state.leader_node_id, "node-dr-failover", &state.node_id);
                        record_transport_mutation(
                            state,
                            &previous,
                            &current,
                            "dr_hook_failover",
                            "cluster_failover",
                            &format!("{}->{}:prepare", previous, current),
                            MutationOp::Insert,
                            json!({
                                "event": "leader_handoff_prepare",
                                "source_node_id": previous,
                                "target_node_id": current,
                                "requested_by": "auto_sre",
                                "hook": "failover_drill",
                                "transport": "dr_hook"
                            }),
                        );
                        record_transport_mutation(
                            state,
                            &previous,
                            &current,
                            "dr_hook_failover",
                            "cluster_failover",
                            &format!("{}->{}:commit", previous, current),
                            MutationOp::Update,
                            json!({
                                "event": "leader_handoff_commit",
                                "source_node_id": previous,
                                "target_node_id": current,
                                "requested_by": "auto_sre",
                                "hook": "failover_drill",
                                "transport": "dr_hook"
                            }),
                        );
                        (
                            "executed",
                            format!(
                                "leader rotated from {previous} to {current} for scope={normalized_scope}"
                            ),
                        )
                    }
                }
                "replay_checkpoint_verify" => (
                    if dry_run { "simulated" } else { "executed" },
                    format!("checkpoint replay verification started for scope={normalized_scope}"),
                ),
                _ => ("rejected", format!("unsupported_dr_hook={normalized_hook}")),
            };

            status = resolved_status;
            details = resolved_details;
            runtime.last_attempt_unix_ms = now_ms;
            if status == "executed" || status == "simulated" {
                runtime.consecutive_failures = 0;
            } else {
                runtime.consecutive_failures = runtime.consecutive_failures.saturating_add(1);
            }
            if runtime.consecutive_failures > policy.max_retries {
                policy_decision = "deny_retry_budget";
                status = "rejected";
                details = format!(
                    "retry_budget_exceeded hook={normalized_hook} failures={} max_retries={}",
                    runtime.consecutive_failures, policy.max_retries
                );
            }
            runtime.last_status = status.to_string();
        }
    } else {
        policy_decision = "deny_policy_state_lock_error";
        status = "rejected";
        details = "policy_state_lock_error".to_string();
    }

    let record = DrHookExecutionRecord {
        execution_id,
        hook: normalized_hook,
        scope: normalized_scope.to_string(),
        status,
        dry_run,
        policy_decision,
        cooldown_remaining_ms,
        retry_backoff_ms,
        retry_attempt,
        details,
    };
    record_transport_mutation(
        state,
        &state.node_id,
        "*",
        "scheduler_execute",
        "dr_hook_execution",
        &record.execution_id,
        if record.status == "executed" || record.status == "simulated" {
            MutationOp::Update
        } else {
            MutationOp::Insert
        },
        json!({
            "hook": record.hook,
            "scope": record.scope,
            "status": record.status,
            "dry_run": record.dry_run,
            "policy_decision": record.policy_decision,
            "retry_attempt": record.retry_attempt,
            "transport": "scheduler_execute"
        }),
    );
    append_dr_hook_record(state, record.clone());
    persist_dr_hook_policy_state(state);
    record
}

pub(crate) fn latest_dr_hook_records(state: &AppState, max_items: usize) -> Vec<DrHookExecutionRecord> {
    match state.dr_hook_records.lock() {
        Ok(records) => {
            let len = records.len();
            let start = len.saturating_sub(max_items);
            records[start..].to_vec()
        }
        Err(_) => Vec::new(),
    }
}

fn append_dr_hook_record(state: &AppState, record: DrHookExecutionRecord) {
    if let Ok(mut records) = state.dr_hook_records.lock() {
        records.push(record);
    }
}

pub(crate) fn evaluate_rate_limit(
    current_minute_count: u32,
    requested_units: u32,
    requests_per_minute: u32,
    burst_limit: u32,
) -> (bool, u32, &'static str) {
    let hard_limit = requests_per_minute.saturating_add(burst_limit);
    let projected = current_minute_count.saturating_add(requested_units);
    if projected > hard_limit {
        return (false, 0, "hard_limit_exceeded");
    }
    let remaining_units = hard_limit.saturating_sub(projected);
    let reason = if projected > requests_per_minute {
        "burst_allowance"
    } else {
        "within_budget"
    };
    (true, remaining_units, reason)
}

fn rotate_leader(
    leader_state: &Arc<Mutex<String>>,
    requested_leader: &str,
    fallback_leader: &str,
) -> (String, String) {
    let requested = requested_leader.trim();
    let next = if requested.is_empty() {
        fallback_leader.to_string()
    } else {
        requested.to_string()
    };

    match leader_state.lock() {
        Ok(mut guard) => {
            let previous = guard.clone();
            *guard = next.clone();
            (previous, next)
        }
        Err(_) => (fallback_leader.to_string(), fallback_leader.to_string()),
    }
}

pub(crate) fn record_transport_mutation(
    state: &AppState,
    source_node_id: &str,
    target_node_id: &str,
    transport: &str,
    table: &str,
    primary_key: &str,
    op: MutationOp,
    payload: serde_json::Value,
) -> Option<u64> {
    let Ok(mut transport_state) = state.replication_transport.lock() else {
        return None;
    };
    let encoded = serde_json::to_string(&payload).ok()?;
    Some(
        transport_state
            .publish(
                source_node_id,
                target_node_id,
                transport,
                table,
                primary_key,
                &encoded,
                op,
            )
            .mutation
            .sequence,
    )
}

fn build_failover_handoff_report(
    state: &AppState,
    source_node_id: &str,
    target_node_id: &str,
) -> FailoverHandoffReportResponse {
    let mut replicas = match state.replica_replay_states.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return FailoverHandoffReportResponse {
                handoff_state: "replica_state_lock_error",
                source_node_id: source_node_id.to_string(),
                target_node_id: target_node_id.to_string(),
                last_applied_sequence_before: 0,
                last_applied_sequence_after: 0,
                replay_batch_size: 0,
                applied_count: 0,
                gap_count: 0,
                gaps: Vec::new(),
            }
        }
    };

    let replica = replicas
        .entry(target_node_id.to_string())
        .or_insert_with(|| ReplicaReplayState::new(target_node_id));
    let last_applied_sequence_before = replica.last_applied_sequence;
    let batch = match state.replication_transport.lock() {
        Ok(transport) => transport.export_for_target_since(
            target_node_id,
            last_applied_sequence_before,
            64,
        ),
        Err(_) => {
            return FailoverHandoffReportResponse {
                handoff_state: "replication_transport_lock_error",
                source_node_id: source_node_id.to_string(),
                target_node_id: target_node_id.to_string(),
                last_applied_sequence_before,
                last_applied_sequence_after: last_applied_sequence_before,
                replay_batch_size: 0,
                applied_count: 0,
                gap_count: 0,
                gaps: Vec::new(),
            }
        }
    };
    let batch = if batch.is_empty() {
        match state.sync_origin.lock() {
            Ok(origin) => replica.build_failover_handoff_batch(&origin, 64),
            Err(_) => Vec::new(),
        }
    } else {
        batch
    };
    if batch.is_empty() {
        return FailoverHandoffReportResponse {
            handoff_state: "no_transport_updates",
            source_node_id: source_node_id.to_string(),
            target_node_id: target_node_id.to_string(),
            last_applied_sequence_before,
            last_applied_sequence_after: last_applied_sequence_before,
            replay_batch_size: 0,
            applied_count: 0,
            gap_count: 0,
            gaps: Vec::new(),
        };
    };
    let replay_batch_size = batch.len();
    let report = replica.apply_batch(&batch);

    FailoverHandoffReportResponse {
        handoff_state: if report.gaps.is_empty() { "handoff_applied" } else { "handoff_gap_detected" },
        source_node_id: source_node_id.to_string(),
        target_node_id: target_node_id.to_string(),
        last_applied_sequence_before,
        last_applied_sequence_after: report.last_applied_sequence,
        replay_batch_size,
        applied_count: report.applied_count,
        gap_count: report.gaps.len(),
        gaps: report
            .gaps
            .into_iter()
            .map(|gap| FailoverHandoffGapResponse {
                expected: gap.expected,
                actual: gap.actual,
            })
            .collect(),
    }
}

pub(crate) fn default_node_cpu_cores() -> u32 {
    std::thread::available_parallelism()
        .map(|value| value.get() as u32)
        .unwrap_or(4)
}

pub(crate) fn default_node_ram_mb() -> u64 {
    env::var("VNG_NODE_TOTAL_RAM_MB")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(16_384)
}

fn initial_cluster_nodes(node_id: &str) -> HashMap<String, ClusterNodeRuntime> {
    let mut nodes = HashMap::new();
    nodes.insert(
        node_id.to_string(),
        ClusterNodeRuntime {
            node_id: node_id.to_string(),
            role: "leader".to_string(),
            status: "active".to_string(),
            total_cpu_cores: env::var("VNG_NODE_TOTAL_CPU_CORES")
                .ok()
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or_else(default_node_cpu_cores),
            total_ram_mb: default_node_ram_mb(),
            draining: false,
            last_heartbeat_ms: now_unix_ms_u64(),
        },
    );
    nodes
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
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn operator_headers(admin_key: &str, operator_id: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-vng-admin-key", HeaderValue::from_str(admin_key).expect("admin key"));
        headers.insert(
            "x-vng-operator-id",
            HeaderValue::from_str(operator_id).expect("operator id"),
        );
        headers
    }

    fn admin_headers(admin_key: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-vng-admin-key", HeaderValue::from_str(admin_key).expect("admin key"));
        headers
    }

    fn tenant_user_headers(user_id: &str, tenant_id: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-vng-user-id", HeaderValue::from_str(user_id).expect("user id"));
        headers.insert(
            "x-vng-tenant-id",
            HeaderValue::from_str(tenant_id).expect("tenant id"),
        );
        headers
    }

    fn state_with_key(key: Option<&str>) -> AppState {
        let allowed_operator_roles = Arc::new(default_allowed_operator_roles());
        let security_config = Arc::new(load_runtime_security_config(&allowed_operator_roles));
        AppState {
            node_id: "node-1".to_string(),
            cluster_mode: "single".to_string(),
            admin_api_key: key.map(|v| v.to_string()),
            security_config: security_config.clone(),
            allowed_operator_roles: allowed_operator_roles.clone(),
            operator_role_bindings: Arc::new(default_operator_role_bindings()),
            tenant_user_bindings: Arc::new(default_tenant_user_bindings()),
            rbac_privilege_matrix: Arc::new(default_rbac_privilege_matrix()),
            kms_runtime: Arc::new(Mutex::new(load_kms_runtime_state(&security_config))),
            leader_node_id: Arc::new(Mutex::new("node-1".to_string())),
            cluster_nodes: Arc::new(Mutex::new(initial_cluster_nodes("node-1"))),
            audit_sink: Arc::new(Mutex::new(AppendOnlyAuditSink::new())),
            action_records: Arc::new(Mutex::new(Vec::new())),
            dr_hook_records: Arc::new(Mutex::new(Vec::new())),
            dr_hook_policy_state: Arc::new(Mutex::new(DrHookPolicyState::default())),
            dr_hook_policy_config: Arc::new(default_dr_hook_policy_config()),
            dr_hook_state_path: None,
            dr_hook_queue: Arc::new(Mutex::new(VecDeque::new())),
            cluster_failure_signals: Arc::new(Mutex::new(Vec::new())),
            sync_origin: Arc::new(Mutex::new(RowStoreSyncOrigin::new())),
            replication_transport: Arc::new(Mutex::new(InMemoryReplicationTransport::new())),
            replica_replay_states: Arc::new(Mutex::new(HashMap::new())),
            pessimistic_locks: Arc::new(Mutex::new(HashMap::new())),
            pessimistic_lock_waits: Arc::new(Mutex::new(HashMap::new())),
            pessimistic_lock_metrics: PessimisticLockContentionMetrics::new(),
            index_manager: Arc::new(Mutex::new(IndexManager::new())),
            constraint_manager: Arc::new(Mutex::new(ConstraintManager::new())),
            ingest_csv_records: Arc::new(Mutex::new(HashMap::new())),
            ingest_json_records: Arc::new(Mutex::new(HashMap::new())),
            ingest_parquet_records: Arc::new(Mutex::new(HashMap::new())),
            ingest_excel_records: Arc::new(Mutex::new(HashMap::new())),
            ingest_outbox_streams: Arc::new(Mutex::new(HashMap::new())),
            ingest_event_bus: Arc::new(Mutex::new(ManagedEventBusTransport::in_memory())),
            ingest_outbox_cursors: Arc::new(Mutex::new(ManagedReplayCursorStore::in_memory())),
            distributed_cache: Arc::new(Mutex::new(DistributedCacheManager::with_default_policy())),
            driver_pool: Arc::new(Mutex::new(ConnectionPoolManager::with_default_policy())),
            plugin_lifecycle: Arc::new(Mutex::new(PluginLifecycleManager::new(256))),
            autonomous_mode: AutonomousMode::Supervised,
            emergency_stop: Arc::new(AtomicEmergencyStop::new(false)),
            guardrails: Arc::new(default_guardrail_rules()),
            ddl_catalog: Arc::new(Mutex::new(DdlCatalog::new())),
            acid_transactions: Arc::new(Mutex::new(AcidTransactionRegistry::default())),
            row_store: Arc::new(Mutex::new(PagedRowStore::default())),
            model_gateway_policy: Arc::new(Mutex::new(ModelGatewayPolicy::default())),
            wal_engine: Arc::new(Mutex::new(BoxedDurabilityEngine::in_memory(DurabilityConfig::default()))),
            chaos_state: Arc::new(Mutex::new(ChaosState::default())),
            olap_store: Arc::new(Mutex::new(HashMap::new())),
            audit_log_path: None,
            raft_state: Arc::new(Mutex::new(RaftNode::new("node-1"))),
            ai_request_counters: Arc::new(Mutex::new(HashMap::new())),
            driver_sessions: Arc::new(Mutex::new(HashMap::new())),
            broker_flush_counts: Arc::new(Mutex::new(HashMap::new())),
            ai_rate_window_starts: Arc::new(Mutex::new(HashMap::new())),
            connector_registry: Arc::new(Mutex::new(Vec::new())),
            tde_override: Arc::new(Mutex::new(None)),
            cdc_cursors: Arc::new(Mutex::new(HashMap::new())),
            // Phase 1.3 — DatabaseCatalog (test default: empty).
            database_catalog: Arc::new(Mutex::new(voltnuerongrid_meta::DatabaseCatalog::new())),
            // Phase 0 — runtime config (test default).
            runtime_config: Arc::new(voltnuerongrid_config::RuntimeConfig::default()),
        }
    }

    fn kms_test_config() -> SecurityConfigContract {
        SecurityConfigContract {
            admin_api_key_env: "VNG_ADMIN_API_KEY".to_string(),
            admin_header_name: "x-vng-admin-key".to_string(),
            tls_required: false,
            mtls_required: false,
            encryption_at_rest_required: true,
            kms_key_ref_env: "VNG_KMS_KEY_URI".to_string(),
            kms_failover_key_ref_envs: vec![
                "VNG_KMS_KEY_URI_REGION_B".to_string(),
                "VNG_KMS_KEY_URI_REGION_C".to_string(),
            ],
            allowed_operator_roles: vec!["dba".to_string(), "security".to_string(), "sre".to_string()],
            token_ttl_seconds: 300,
        }
    }

    #[test]
    fn operator_auth_rejects_request_when_admin_key_not_configured() {
        let state = state_with_key(None);
        let headers = operator_headers("secret", "platform-admin");
        let auth = require_operator_auth(&headers, &state).expect_err("missing configured admin key");
        assert_eq!(auth.0, StatusCode::UNAUTHORIZED);
        assert_eq!(auth.1.reason, "missing_or_invalid_admin_key");
    }

    #[test]
    fn operator_auth_rejects_request_with_missing_key_when_configured() {
        let state = state_with_key(Some("secret"));
        let headers = HeaderMap::new();
        let auth = require_operator_auth(&headers, &state);
        assert!(auth.is_err());
    }

    #[test]
    fn operator_auth_accepts_request_with_matching_admin_key() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "platform-admin");
        assert!(require_operator_auth(&headers, &state).is_ok());
    }

    #[test]
    fn operator_auth_rejects_request_without_operator_identity_when_key_matches() {
        let state = state_with_key(Some("secret"));
        let mut headers = HeaderMap::new();
        headers.insert("x-vng-admin-key", HeaderValue::from_static("secret"));
        let auth = require_operator_auth(&headers, &state).expect_err("missing operator");
        assert_eq!(auth.0, StatusCode::UNAUTHORIZED);
        assert_eq!(auth.1.reason, "missing_or_invalid_operator_identity");
    }

    #[test]
    fn operator_auth_rejects_unknown_operator_identity() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "rogue-operator");
        let auth = require_operator_auth(&headers, &state).expect_err("unknown operator");
        assert_eq!(auth.0, StatusCode::UNAUTHORIZED);
        assert_eq!(auth.1.reason, "missing_or_invalid_operator_identity");
    }

    #[test]
    fn operator_auth_denies_security_role_from_failover_execution() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "security-bot");
        let auth = require_operator_auth(&headers, &state);
        assert!(auth.is_ok());
        let privilege = require_operator_privilege(
            &headers,
            &state,
            "cluster.failover",
            "cluster",
            PrivilegeAction::Execute,
        )
        .expect_err("security role should not execute failover");
        assert_eq!(privilege.0, StatusCode::FORBIDDEN);
        assert_eq!(privilege.1.reason, "insufficient_privilege");
    }

    #[test]
    fn operator_auth_allows_ai_operator_for_autonomous_actions() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "autopilot");
        let identity = require_operator_privilege(
            &headers,
            &state,
            "autonomous.actions",
            "autonomous/actions",
            PrivilegeAction::Execute,
        )
        .expect("ai operator should be allowed");
        assert_eq!(identity.role, OperatorRole::AiOperator);
    }

    #[test]
    fn operator_auth_allows_dba_for_storage_catalog_management() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "platform-admin");
        let identity = require_operator_privilege(
            &headers,
            &state,
            "storage.catalog",
            "store/indexes",
            PrivilegeAction::Manage,
        )
        .expect("dba should manage storage catalog");
        assert_eq!(identity.role, OperatorRole::Dba);
    }

    #[test]
    fn operator_auth_denies_ai_operator_from_storage_catalog_management() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "autopilot");
        let privilege = require_operator_privilege(
            &headers,
            &state,
            "storage.catalog",
            "store/indexes",
            PrivilegeAction::Manage,
        )
        .expect_err("ai operator should not manage store catalog");
        assert_eq!(privilege.0, StatusCode::FORBIDDEN);
        assert_eq!(privilege.1.reason, "insufficient_privilege");
    }

    #[test]
    fn operator_auth_allows_dba_for_ingest_write() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "platform-admin");
        let identity = require_operator_privilege(
            &headers,
            &state,
            "ingest.connectors",
            "ingest/csv",
            PrivilegeAction::Write,
        )
        .expect("dba should write ingest connectors");
        assert_eq!(identity.role, OperatorRole::Dba);
    }

    #[test]
    fn sql_runtime_allows_tenant_analyst_for_analyze() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        assert!(require_sql_runtime_principal(
            &headers,
            &state,
            PrivilegeAction::Read,
            "sql/analyze",
        )
        .is_ok());
    }

    #[test]
    fn sql_runtime_denies_cross_tenant_user_scope() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "globex");
        let auth = require_sql_runtime_principal(
            &headers,
            &state,
            PrivilegeAction::Read,
            "sql/analyze",
        )
        .expect_err("cross-tenant user should be rejected");
        assert_eq!(auth.0, StatusCode::UNAUTHORIZED);
        assert_eq!(auth.1.reason, "missing_or_invalid_user_identity");
    }

    #[test]
    fn sql_runtime_allows_operator_dba_for_execute() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "platform-admin");
        assert!(require_sql_runtime_principal(
            &headers,
            &state,
            PrivilegeAction::Execute,
            "sql/execute",
        )
        .is_ok());
    }

    #[test]
    fn store_create_index_appends_tenant_storage_audit_event() {
        let state = state_with_key(None);
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(store_create_index(
                State(state.clone()),
                tenant_user_headers("admin-acme", "acme"),
                Json(CreateIndexRequest {
                    name: "idx_audit_acme".to_string(),
                    table: "tenant/acme/orders".to_string(),
                    column: "customer_id".to_string(),
                    unique: Some(false),
                }),
            ))
            .expect("tenant admin should create audited index");

        assert_eq!(response.0, StatusCode::CREATED);
        let events = state.audit_sink.lock().expect("audit lock").latest(1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AuditEventKind::Storage);
        assert_eq!(events[0].actor, "admin-acme");
        assert!(events[0].details_json.contains("\"tenant_id\":\"acme\""));
        assert!(events[0].details_json.contains("store/indexes/create"));
    }

    #[test]
    fn ingest_csv_appends_tenant_ingest_audit_event() {
        let state = state_with_key(None);
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(ingest_csv(
                State(state.clone()),
                tenant_user_headers("admin-acme", "acme"),
                Json(IngestCsvRequest {
                    connector_id: "orders-csv".to_string(),
                    csv_data: "id,amount\n1,42\n".to_string(),
                }),
            ))
            .expect("tenant admin should ingest csv");

        assert_eq!(response.0, StatusCode::OK);
        let events = state.audit_sink.lock().expect("audit lock").latest(1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AuditEventKind::Ingest);
        assert_eq!(events[0].actor, "admin-acme");
        assert!(events[0].details_json.contains("\"tenant_id\":\"acme\""));
        assert!(events[0].details_json.contains("orders-csv"));
    }

    #[test]
    fn sql_execute_accepts_tenant_analyst_headers() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_execute(
                State(state),
                headers,
                Json(SqlExecuteRequest {
                    sql_batch: "SELECT udf_rust('hello');".to_string(),
                    max_rows: Some(10),
                }),
            ))
            .expect("sql execute response");

        assert_eq!(response.0, StatusCode::OK);
        assert_eq!(response.1.status, "ok");
    }

    #[test]
    fn sql_route_accepts_tenant_analyst_headers() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_route(
                State(state),
                headers,
                Json(SqlRouteRequest {
                    sql_batch: "SELECT 1".to_string(),
                }),
            ))
            .expect("sql route response");

        assert_eq!(response.status, "ok");
    }

    #[test]
    fn sql_transaction_accepts_tenant_analyst_headers() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_transaction(
                State(state),
                headers,
                Json(SqlTransactionRequest {
                    statements: vec!["BEGIN".to_string(), "COMMIT".to_string()],
                    isolation_level: None,
                }),
            ))
            .expect("sql transaction response");

        assert_eq!(response.0, StatusCode::OK);
        assert_eq!(response.1.status, "committed");
    }

    #[test]
    fn h07_sql_data_plane_pool_acquire_release_on_sql_handlers() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let _ = runtime
            .block_on(sql_route(
                State(state.clone()),
                headers.clone(),
                Json(SqlRouteRequest {
                    sql_batch: "SELECT 1".to_string(),
                }),
            ))
            .expect("sql route response");

        let _ = runtime
            .block_on(sql_transaction(
                State(state.clone()),
                headers.clone(),
                Json(SqlTransactionRequest {
                    statements: vec!["BEGIN".to_string(), "COMMIT".to_string()],
                    isolation_level: None,
                }),
            ))
            .expect("sql transaction response");

        let _ = runtime
            .block_on(sql_execute(
                State(state.clone()),
                headers,
                Json(SqlExecuteRequest {
                    sql_batch: "SELECT udf_rust('hello');".to_string(),
                    max_rows: Some(10),
                }),
            ))
            .expect("sql execute response");

        let stats = state
            .driver_pool
            .lock()
            .expect("driver pool lock")
            .pool_stats(now_unix_ms_u64());
        assert!(stats.total_acquired >= 3);
        assert!(stats.total_released >= 3);
        assert_eq!(stats.total_rejected, 0);
    }

    #[test]
    fn h07_sql_data_plane_pool_rejects_when_pool_exhausted() {
        let state = state_with_key(None);
        {
            let mut pool = state.driver_pool.lock().expect("driver pool lock");
            for _ in 0..50 {
                let _ = pool.acquire(1_000).expect("pre-acquire should succeed");
            }
        }

        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let result = runtime.block_on(sql_execute(
            State(state),
            headers,
            Json(SqlExecuteRequest {
                sql_batch: "SELECT 1".to_string(),
                max_rows: Some(10),
            }),
        ));

        match result {
            Ok(_) => panic!("expected pool exhaustion rejection"),
            Err(error) => {
                assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);
                assert_eq!(error.1.reason, "driver_pool_unavailable");
            }
        }
    }

    #[test]
    fn ingest_runtime_allows_tenant_user_write_and_status_scope() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let write = require_ingest_runtime_privilege(
            &headers,
            &state,
            PrivilegeAction::Write,
            "ingest/connectors/orders-csv/csv",
        )
        .expect("tenant user should write ingest");
        let read = require_ingest_runtime_privilege(
            &headers,
            &state,
            PrivilegeAction::Read,
            ingest_status_scope(),
        )
        .expect("tenant user should read ingest status");
        assert!(matches!(write, RuntimeAccessPrincipal::TenantUser(_)));
        assert!(matches!(read, RuntimeAccessPrincipal::TenantUser(_)));
    }

    #[test]
    fn ingest_runtime_denies_tenant_role_without_grant() {
        let mut bindings = default_tenant_user_bindings();
        bindings.insert(
            "viewer-acme".to_string(),
            TenantUserBinding {
                tenant_id: "acme".to_string(),
                role: "tenant_viewer".to_string(),
            },
        );
        let state = AppState {
            tenant_user_bindings: Arc::new(bindings),
            ..state_with_key(None)
        };
        let headers = tenant_user_headers("viewer-acme", "acme");

        let auth = require_ingest_runtime_privilege(
            &headers,
            &state,
            PrivilegeAction::Write,
            "ingest/connectors/orders-csv/csv",
        )
        .expect_err("tenant_viewer should not write ingest");

        assert_eq!(auth.0, StatusCode::FORBIDDEN);
        assert_eq!(auth.1.reason, "insufficient_privilege");
    }

    #[test]
    fn audit_runtime_allows_tenant_analyst_read_scope() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");

        let principal = require_audit_runtime_principal(
            &headers,
            &state,
            PrivilegeAction::Read,
            "audit/events",
        )
        .expect("tenant analyst should read tenant audit scope");

        assert!(matches!(principal, RuntimeAccessPrincipal::TenantUser(_)));
    }

    #[test]
    fn audit_events_filters_to_tenant_scope() {
        let state = state_with_key(None);
        append_runtime_audit_event(
            &state,
            AuditEventKind::Sql,
            &RuntimeAccessPrincipal::TenantUser(TenantUserIdentity {
                user_id: "analyst-acme".to_string(),
                tenant_id: "acme".to_string(),
                role: "tenant_analyst".to_string(),
            }),
            "sql_route",
            "ok",
            json!({ "route_scope": "sql/route" }),
        );
        append_runtime_audit_event(
            &state,
            AuditEventKind::Sql,
            &RuntimeAccessPrincipal::TenantUser(TenantUserIdentity {
                user_id: "analyst-globex".to_string(),
                tenant_id: "globex".to_string(),
                role: "tenant_analyst".to_string(),
            }),
            "sql_route",
            "ok",
            json!({ "route_scope": "sql/route" }),
        );

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let response = runtime
            .block_on(audit_events(
                State(state),
                tenant_user_headers("analyst-acme", "acme"),
                Query(AuditEventsQuery { max_items: Some(10) }),
            ))
            .expect("tenant audit response");

        assert_eq!(response.status, "ok");
        assert_eq!(response.total_events, 1);
        assert_eq!(response.events[0].actor, "analyst-acme");
        assert!(response.events[0].details_json.contains("\"tenant_id\":\"acme\""));
    }

    #[test]
    fn store_list_indexes_filters_to_tenant_namespace() {
        use voltnuerongrid_store::index::{IndexDescriptor, IndexKind};

        let state = state_with_key(None);
        {
            let mut mgr = state.index_manager.lock().expect("index lock");
            mgr.create_index(IndexDescriptor {
                name: "idx_acme_orders".to_string(),
                table: "tenant/acme/orders".to_string(),
                column: "customer_id".to_string(),
                kind: IndexKind::BTree,
                unique: false,
            })
            .expect("create acme index");
            mgr.create_index(IndexDescriptor {
                name: "idx_globex_orders".to_string(),
                table: "tenant/globex/orders".to_string(),
                column: "customer_id".to_string(),
                kind: IndexKind::BTree,
                unique: false,
            })
            .expect("create globex index");
        }

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let response = runtime
            .block_on(store_list_indexes(
                State(state),
                tenant_user_headers("analyst-acme", "acme"),
            ))
            .expect("tenant store list response");

        assert_eq!(response.status, "ok");
        assert_eq!(response.indexes.len(), 1);
        assert_eq!(response.indexes[0].name, "idx_acme_orders");
    }

    #[test]
    fn store_index_lookup_denies_cross_tenant_index_lookup() {
        use voltnuerongrid_store::index::{IndexDescriptor, IndexKind};

        let state = state_with_key(None);
        {
            let mut mgr = state.index_manager.lock().expect("index lock");
            mgr.create_index(IndexDescriptor {
                name: "idx_globex_orders".to_string(),
                table: "tenant/globex/orders".to_string(),
                column: "customer_id".to_string(),
                kind: IndexKind::BTree,
                unique: false,
            })
            .expect("create globex index");
            mgr.get_mut("idx_globex_orders")
                .expect("lookup mutable index")
                .insert("C100", "row-1")
                .expect("seed index row");
        }

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let auth = runtime
            .block_on(store_index_lookup(
                State(state),
                tenant_user_headers("analyst-acme", "acme"),
                Json(IndexLookupRequest {
                    index_name: "idx_globex_orders".to_string(),
                    value: "C100".to_string(),
                }),
            ))
            .expect_err("cross-tenant index lookup should be rejected");

        assert_eq!(auth.0, StatusCode::FORBIDDEN);
        assert_eq!(auth.1.reason, "insufficient_privilege");
    }

    #[test]
    fn store_validate_constraint_accepts_tenant_scoped_table() {
        use voltnuerongrid_store::constraints::{ConstraintDescriptor, ConstraintKind};

        let state = state_with_key(None);
        state
            .constraint_manager
            .lock()
            .expect("constraint lock")
            .add_constraint(ConstraintDescriptor {
                name: "tenant_acme_pk".to_string(),
                table: "tenant/acme/orders".to_string(),
                column: "id".to_string(),
                kind: ConstraintKind::PrimaryKey,
            })
            .expect("add tenant constraint");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let response = runtime
            .block_on(store_validate_constraint(
                State(state),
                tenant_user_headers("analyst-acme", "acme"),
                Json(ValidateConstraintRequest {
                    table: "tenant/acme/orders".to_string(),
                    column: "id".to_string(),
                    value: Some("ord-1".to_string()),
                }),
            ))
            .expect("tenant constraint validate response");

        assert_eq!(response.status, "ok");
        assert!(response.valid);
        assert!(response.violation.is_none());
    }

    #[test]
    fn store_create_index_accepts_tenant_admin_for_tenant_table() {
        let state = state_with_key(None);
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(store_create_index(
                State(state.clone()),
                tenant_user_headers("admin-acme", "acme"),
                Json(CreateIndexRequest {
                    name: "idx_acme_orders_admin".to_string(),
                    table: "tenant/acme/orders".to_string(),
                    column: "customer_id".to_string(),
                    unique: Some(false),
                }),
            ))
            .expect("tenant admin should create index");

        assert_eq!(response.0, StatusCode::CREATED);
        let mgr = state.index_manager.lock().expect("index lock");
        assert!(mgr.get("idx_acme_orders_admin").is_some());
    }

    #[test]
    fn store_create_index_denies_tenant_admin_for_cross_tenant_table() {
        let state = state_with_key(None);
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let auth = runtime
            .block_on(store_create_index(
                State(state),
                tenant_user_headers("admin-acme", "acme"),
                Json(CreateIndexRequest {
                    name: "idx_globex_orders_admin".to_string(),
                    table: "tenant/globex/orders".to_string(),
                    column: "customer_id".to_string(),
                    unique: Some(false),
                }),
            ))
            .expect_err("tenant admin should not manage cross-tenant table");

        assert_eq!(auth.0, StatusCode::FORBIDDEN);
        assert_eq!(auth.1.reason, "insufficient_privilege");
    }

    #[test]
    fn store_create_index_denies_tenant_analyst_manage_scope() {
        let state = state_with_key(None);
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let auth = runtime
            .block_on(store_create_index(
                State(state),
                tenant_user_headers("analyst-acme", "acme"),
                Json(CreateIndexRequest {
                    name: "idx_acme_orders_analyst".to_string(),
                    table: "tenant/acme/orders".to_string(),
                    column: "customer_id".to_string(),
                    unique: Some(false),
                }),
            ))
            .expect_err("tenant analyst should not manage store catalog");

        assert_eq!(auth.0, StatusCode::FORBIDDEN);
        assert_eq!(auth.1.reason, "insufficient_privilege");
    }

    #[test]
    fn store_add_constraint_accepts_tenant_admin_for_tenant_table() {
        let state = state_with_key(None);
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(store_add_constraint(
                State(state),
                tenant_user_headers("admin-acme", "acme"),
                Json(AddConstraintRequest {
                    name: "tenant_acme_orders_pk".to_string(),
                    table: "tenant/acme/orders".to_string(),
                    column: "id".to_string(),
                    kind: "primary_key".to_string(),
                }),
            ))
            .expect("tenant admin should add constraint");

        assert_eq!(response.0, StatusCode::CREATED);
    }

    #[test]
    fn store_drop_index_accepts_tenant_admin_for_tenant_table() {
        use voltnuerongrid_store::index::{IndexDescriptor, IndexKind};

        let state = state_with_key(None);
        {
            let mut mgr = state.index_manager.lock().expect("index lock");
            mgr.create_index(IndexDescriptor {
                name: "idx_acme_drop".to_string(),
                table: "tenant/acme/orders".to_string(),
                column: "customer_id".to_string(),
                kind: IndexKind::BTree,
                unique: false,
            })
            .expect("seed tenant index");
        }
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(store_drop_index(
                State(state.clone()),
                tenant_user_headers("admin-acme", "acme"),
                Json(DropIndexRequest {
                    name: "idx_acme_drop".to_string(),
                }),
            ))
            .expect("tenant admin should drop own index");

        assert_eq!(response.0, StatusCode::OK);
        let mgr = state.index_manager.lock().expect("index lock");
        assert!(mgr.get("idx_acme_drop").is_none());
    }

    #[test]
    fn ingest_status_scopes_counts_to_tenant_records() {
        let state = state_with_key(None);
        state
            .ingest_csv_records
            .lock()
            .expect("csv lock")
            .insert("tenant/acme/c1".to_string(), vec![]);
        state
            .ingest_csv_records
            .lock()
            .expect("csv lock")
            .insert("tenant/acme/c2".to_string(), vec![voltnuerongrid_ingest::IngestRecord {
                key: "1".to_string(),
                payload: "{\"id\":\"1\"}".to_string(),
            }]);
        state
            .ingest_json_records
            .lock()
            .expect("json lock")
            .insert("tenant/globex/j1".to_string(), vec![voltnuerongrid_ingest::IngestRecord {
                key: "2".to_string(),
                payload: "{\"id\":\"2\"}".to_string(),
            }]);

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let response = runtime
            .block_on(ingest_status(
                State(state),
                tenant_user_headers("analyst-acme", "acme"),
            ))
            .expect("ingest status response");

        assert_eq!(response.status, "ok");
        assert_eq!(response.csv_connectors, 2);
        assert_eq!(response.json_connectors, 0);
        assert_eq!(response.total_records_loaded, 1);
    }

    #[test]
    fn failover_rotate_leader_updates_state() {
        let leader = Arc::new(Mutex::new("node-1".to_string()));
        let (previous, current) = rotate_leader(&leader, "node-2", "node-1");
        assert_eq!(previous, "node-1");
        assert_eq!(current, "node-2");
        assert_eq!(leader.lock().expect("lock").as_str(), "node-2");
    }

    #[test]
    fn failover_rotate_leader_uses_fallback_for_blank_request() {
        let leader = Arc::new(Mutex::new("node-1".to_string()));
        let (_, current) = rotate_leader(&leader, "   ", "node-1");
        assert_eq!(current, "node-1");
    }

    #[test]
    fn failover_status_reports_healthy_without_critical_signals() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "platform-admin");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(failover_status(State(state), headers))
            .expect("authorized failover status response");

        assert_eq!(response.status, "healthy");
        assert_eq!(response.unresolved_critical_count, 0);
    }

    #[test]
    fn failover_status_reports_degraded_with_unresolved_critical_signal() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "platform-admin");
        if let Ok(mut signals) = state.cluster_failure_signals.lock() {
            signals.push(ClusterFailureSignal {
                signal_id: "sig-status-critical".to_string(),
                node_id: "node-2".to_string(),
                transport: "raft".to_string(),
                failure_type: "leader_heartbeat_timeout".to_string(),
                severity: "critical".to_string(),
                message: "control-plane heartbeat timeout".to_string(),
                observed_unix_ms: now_unix_ms(),
                resolved: false,
                resolved_by: None,
                resolved_unix_ms: None,
                resolution_note: None,
            });
        }
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(failover_status(State(state), headers))
            .expect("authorized failover status response");

        assert_eq!(response.status, "degraded");
        assert_eq!(response.unresolved_critical_count, 1);
    }

    #[test]
    fn h03_control_plane_chaos_cycle_recovers_after_failover_and_reconcile() {
        let state = state_with_key(Some("secret"));
        if let Ok(mut signals) = state.cluster_failure_signals.lock() {
            signals.push(ClusterFailureSignal {
                signal_id: "sig-h03-chaos".to_string(),
                node_id: "node-2".to_string(),
                transport: "raft".to_string(),
                failure_type: "leader_heartbeat_timeout".to_string(),
                severity: "critical".to_string(),
                message: "control-plane heartbeat timeout".to_string(),
                observed_unix_ms: now_unix_ms(),
                resolved: false,
                resolved_by: None,
                resolved_unix_ms: None,
                resolution_note: None,
            });
        }
        let headers = operator_headers("secret", "platform-admin");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let degraded = runtime
            .block_on(failover_status(State(state.clone()), headers.clone()))
            .expect("authorized degraded status response");
        assert_eq!(degraded.status, "degraded");
        assert_eq!(degraded.unresolved_critical_count, 1);

        let failover_response = runtime
            .block_on(failover_simulate(
                State(state.clone()),
                headers.clone(),
                Json(FailoverSimulateRequest {
                    new_leader_node_id: "node-2".to_string(),
                    reason: Some("h03_control_plane_chaos".to_string()),
                    requested_by: Some("ignored-body-operator".to_string()),
                }),
            ))
            .expect("failover response");

        assert_eq!(failover_response.0.new_leader_node_id, "node-2");
        assert_eq!(failover_response.0.handoff_report.handoff_state, "handoff_applied");

        let reconcile_response = runtime
            .block_on(sre_failure_reconcile(
                State(state.clone()),
                headers,
                Json(FailureReconcileRequest {
                    signal_ids: None,
                    resolve_all_critical: Some(true),
                    note: Some("h03_control_plane_chaos_reconcile".to_string()),
                }),
            ))
            .expect("reconcile response");

        assert_eq!(reconcile_response.0.resolved_count, 1);
        assert_eq!(reconcile_response.0.unresolved_critical_count, 0);

        let recovered = runtime
            .block_on(failover_status(State(state), operator_headers("secret", "platform-admin")))
            .expect("authorized recovered status response");
        assert_eq!(recovered.status, "healthy");
        assert_eq!(recovered.leader_node_id, "node-2");
        assert_eq!(recovered.unresolved_critical_count, 0);
    }

    #[test]
    fn failover_status_requires_operator_auth() {
        let state = state_with_key(Some("secret"));
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let result = runtime.block_on(failover_status(State(state), HeaderMap::new()));
        let err = match result {
            Ok(_) => panic!("unauthenticated failover status must be rejected"),
            Err(err) => err,
        };

        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn failover_status_denies_security_role_without_failover_privilege() {
        let state = state_with_key(Some("secret"));
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let result = runtime.block_on(failover_status(
            State(state),
            operator_headers("secret", "security-bot"),
        ));
        let err = match result {
            Ok(_) => panic!("security role must not read failover status"),
            Err(err) => err,
        };

        assert_eq!(err.0, StatusCode::FORBIDDEN);
        assert_eq!(err.1.reason, "insufficient_privilege");
    }

    #[test]
    fn failover_simulate_requires_operator_auth() {
        let state = state_with_key(Some("secret"));
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let result = runtime.block_on(failover_simulate(
            State(state),
            HeaderMap::new(),
            Json(FailoverSimulateRequest {
                new_leader_node_id: "node-2".to_string(),
                reason: Some("auth-negative".to_string()),
                requested_by: None,
            }),
        ));
        let err = match result {
            Ok(_) => panic!("unauthenticated failover_simulate must be rejected"),
            Err(err) => err,
        };

        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn failover_simulate_denies_security_role_without_execute_privilege() {
        let state = state_with_key(Some("secret"));
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let result = runtime.block_on(failover_simulate(
            State(state),
            operator_headers("secret", "security-bot"),
            Json(FailoverSimulateRequest {
                new_leader_node_id: "node-2".to_string(),
                reason: Some("auth-negative".to_string()),
                requested_by: None,
            }),
        ));
        let err = match result {
            Ok(_) => panic!("security role must not execute failover_simulate"),
            Err(err) => err,
        };

        assert_eq!(err.0, StatusCode::FORBIDDEN);
        assert_eq!(err.1.reason, "insufficient_privilege");
    }

    #[test]
    fn h03_multi_node_cluster_runtime_chaos_replays_targeted_handoffs_across_rotations() {
        let state = state_with_key(None);

        let (_, leader_after_first_rotation) =
            rotate_leader(&state.leader_node_id, "node-2", &state.node_id);
        assert_eq!(leader_after_first_rotation, "node-2");

        record_transport_mutation(
            &state,
            "node-1",
            "node-2",
            "raft",
            "cluster_runtime_outbox",
            "node-2-targeted-prepare",
            MutationOp::Insert,
            json!({ "event": "targeted_prepare", "target": "node-2" }),
        );
        record_transport_mutation(
            &state,
            "node-1",
            "*",
            "raft",
            "cluster_runtime_outbox",
            "broadcast-cluster-state",
            MutationOp::Update,
            json!({ "event": "broadcast_cluster_state", "epoch": 1 }),
        );
        record_transport_mutation(
            &state,
            "node-1",
            "node-3",
            "raft",
            "cluster_runtime_outbox",
            "node-3-targeted-prepare",
            MutationOp::Insert,
            json!({ "event": "targeted_prepare", "target": "node-3" }),
        );

        let node_2_handoff = build_failover_handoff_report(&state, "node-1", "node-2");
        assert_eq!(node_2_handoff.handoff_state, "handoff_applied");
        assert_eq!(node_2_handoff.replay_batch_size, 2);
        assert_eq!(node_2_handoff.applied_count, 2);
        assert_eq!(node_2_handoff.last_applied_sequence_after, 2);

        let (_, leader_after_second_rotation) =
            rotate_leader(&state.leader_node_id, "node-3", &state.node_id);
        assert_eq!(leader_after_second_rotation, "node-3");

        let node_3_handoff = build_failover_handoff_report(&state, "node-2", "node-3");
        assert_eq!(node_3_handoff.handoff_state, "handoff_gap_detected");
        assert_eq!(node_3_handoff.replay_batch_size, 2);
        assert_eq!(node_3_handoff.applied_count, 0);
        assert_eq!(node_3_handoff.last_applied_sequence_after, 0);
        assert_eq!(node_3_handoff.gap_count, 1);
        assert_eq!(node_3_handoff.gaps[0].expected, 1);
        assert_eq!(node_3_handoff.gaps[0].actual, 2);

        record_transport_mutation(
            &state,
            "node-3",
            "*",
            "raft",
            "cluster_runtime_outbox",
            "broadcast-cluster-state-2",
            MutationOp::Update,
            json!({ "event": "broadcast_cluster_state", "epoch": 2 }),
        );
        record_transport_mutation(
            &state,
            "node-3",
            "node-2",
            "raft",
            "cluster_runtime_outbox",
            "node-2-targeted-rejoin",
            MutationOp::Update,
            json!({ "event": "targeted_rejoin", "target": "node-2" }),
        );

        let (_, leader_after_third_rotation) =
            rotate_leader(&state.leader_node_id, "node-2", &state.node_id);
        assert_eq!(leader_after_third_rotation, "node-2");

        let node_2_rejoin = build_failover_handoff_report(&state, "node-3", "node-2");
        assert_eq!(node_2_rejoin.handoff_state, "handoff_gap_detected");
        assert_eq!(node_2_rejoin.last_applied_sequence_before, 2);
        assert_eq!(node_2_rejoin.replay_batch_size, 2);
        assert_eq!(node_2_rejoin.applied_count, 0);
        assert_eq!(node_2_rejoin.last_applied_sequence_after, 2);
        assert_eq!(node_2_rejoin.gap_count, 1);
        assert_eq!(node_2_rejoin.gaps[0].expected, 3);
        assert_eq!(node_2_rejoin.gaps[0].actual, 4);

        let replicas = state.replica_replay_states.lock().expect("replica lock");
        let node_2_replica = replicas.get("node-2").expect("node-2 replica");
        let node_2_sequences: Vec<u64> = node_2_replica
            .applied
            .iter()
            .map(|mutation| mutation.sequence)
            .collect();
        assert_eq!(node_2_sequences, vec![1, 2]);

        let node_3_replica = replicas.get("node-3").expect("node-3 replica");
        assert!(node_3_replica.applied.is_empty());
        assert_eq!(node_3_replica.last_applied_sequence, 0);
    }

    #[test]
    fn failover_handoff_report_replays_only_unapplied_sequences_for_new_leader() {
        let state = state_with_key(None);
        {
            let mut origin = state.sync_origin.lock().expect("origin lock");
            origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
            origin.append("orders", "2", "{\"amount\":80}", MutationOp::Insert);
            origin.append("orders", "3", "{\"amount\":90}", MutationOp::Insert);
            origin.append("orders", "4", "{\"amount\":110}", MutationOp::Update);
        }
        {
            let origin = state.sync_origin.lock().expect("origin lock");
            let mut replicas = state.replica_replay_states.lock().expect("replica lock");
            let replica = replicas
                .entry("node-2".to_string())
                .or_insert_with(|| ReplicaReplayState::new("node-2"));
            let initial = origin.export_since(0, 2);
            let report = replica.apply_batch(&initial);
            assert_eq!(report.applied_count, 2);
        }

        let handoff = build_failover_handoff_report(&state, "node-1", "node-2");
        assert_eq!(handoff.handoff_state, "handoff_applied");
        assert_eq!(handoff.last_applied_sequence_before, 2);
        assert_eq!(handoff.last_applied_sequence_after, 4);
        assert_eq!(handoff.replay_batch_size, 2);
        assert_eq!(handoff.applied_count, 2);
        assert_eq!(handoff.gap_count, 0);
    }

    #[test]
    fn failover_handoff_report_returns_empty_when_no_transport_state_exists() {
        let state = state_with_key(None);
        let handoff = build_failover_handoff_report(&state, "node-1", "node-2");
        assert_eq!(handoff.handoff_state, "no_transport_updates");
        assert_eq!(handoff.replay_batch_size, 0);
        assert_eq!(handoff.applied_count, 0);
    }

    #[test]
    fn failover_transport_mutations_feed_runtime_handoff_report() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "automation");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(failover_simulate(
                State(state.clone()),
                headers,
                Json(FailoverSimulateRequest {
                    new_leader_node_id: "node-2".to_string(),
                    reason: Some("unit_test_failover".to_string()),
                    requested_by: Some("ignored-body-operator".to_string()),
                }),
            ))
            .expect("failover response");

        assert_eq!(response.0.handoff_report.handoff_state, "handoff_applied");
        assert_eq!(response.0.handoff_report.replay_batch_size, 2);
        assert_eq!(response.0.handoff_report.applied_count, 2);
    }

    #[test]
    fn failover_handoff_report_detects_gap_for_target_leader() {
        let state = state_with_key(None);
        {
            let mut origin = state.sync_origin.lock().expect("origin lock");
            origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
            origin.append("orders", "2", "{\"amount\":80}", MutationOp::Insert);
            origin.append("orders", "3", "{\"amount\":90}", MutationOp::Insert);
            origin.append("orders", "4", "{\"amount\":110}", MutationOp::Update);
            origin.remove_sequence_for_fault_injection(3);
        }
        {
            let origin = state.sync_origin.lock().expect("origin lock");
            let mut replicas = state.replica_replay_states.lock().expect("replica lock");
            let replica = replicas
                .entry("node-2".to_string())
                .or_insert_with(|| ReplicaReplayState::new("node-2"));
            let initial = origin.export_since(0, 2);
            let report = replica.apply_batch(&initial);
            assert_eq!(report.applied_count, 2);
        }

        let handoff = build_failover_handoff_report(&state, "node-1", "node-2");
        assert_eq!(handoff.handoff_state, "handoff_gap_detected");
        assert_eq!(handoff.last_applied_sequence_before, 2);
        assert_eq!(handoff.last_applied_sequence_after, 2);
        assert_eq!(handoff.replay_batch_size, 1);
        assert_eq!(handoff.applied_count, 0);
        assert_eq!(handoff.gap_count, 1);
        assert_eq!(handoff.gaps[0].expected, 3);
        assert_eq!(handoff.gaps[0].actual, 4);
    }

    #[test]
    fn audit_append_event_writes_to_sink() {
        let state = state_with_key(None);
        append_audit_event(
            &state,
            AuditEventKind::Security,
            "operator",
            "autonomous_emergency_stop",
            "ok",
            "{\"enabled\":true}",
        );
        let count = state
            .audit_sink
            .lock()
            .expect("sink lock")
            .len();
        assert_eq!(count, 1);
    }

    #[test]
    fn action_trace_id_is_generated() {
        let first = next_action_trace_id();
        assert!(first.starts_with("atrace-"));
    }

    #[test]
    fn append_action_record_writes_to_history() {
        let state = state_with_key(None);
        let record = AutonomousActionExecutionRecord::new(
            "atrace-test".to_string(),
            "performance_tune",
            "session",
            "operator",
            AutonomousActionDecision::Allow,
            "ok",
        );
        append_action_record(&state, record);
        let records = latest_action_records(&state, 10);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].trace_id, "atrace-test");
    }

    #[test]
    fn autonomous_records_runtime_allows_tenant_analyst_read_scope() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");

        let principal = require_autonomous_records_runtime_principal(
            &headers,
            &state,
            PrivilegeAction::Read,
            "autonomous/records",
        )
        .expect("tenant analyst should read tenant autonomous records scope");

        assert!(matches!(principal, RuntimeAccessPrincipal::TenantUser(_)));
    }

    #[test]
    fn autonomous_action_records_filter_to_tenant_scope() {
        let state = state_with_key(None);
        append_action_record(
            &state,
            AutonomousActionExecutionRecord::new(
                "atrace-acme".to_string(),
                "rebalance_cache",
                "tenants/acme/autonomous/records",
                "platform-admin",
                AutonomousActionDecision::Allow,
                "tenant scoped",
            )
            .with_tenant_id(Some("acme")),
        );
        append_action_record(
            &state,
            AutonomousActionExecutionRecord::new(
                "atrace-globex".to_string(),
                "rebalance_cache",
                "tenants/globex/autonomous/records",
                "platform-admin",
                AutonomousActionDecision::Allow,
                "tenant scoped",
            )
            .with_tenant_id(Some("globex")),
        );

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let response = runtime
            .block_on(autonomous_action_records(
                State(state),
                tenant_user_headers("analyst-acme", "acme"),
                Query(AutonomousActionRecordsQuery { max_items: Some(10) }),
            ))
            .expect("tenant autonomous records response");

        assert_eq!(response.status, "ok");
        assert_eq!(response.total_records, 1);
        assert_eq!(response.records[0].trace_id, "atrace-acme");
        assert_eq!(response.records[0].tenant_id.as_deref(), Some("acme"));
    }

    #[test]
    fn authorize_action_response_tags_tenant_scope_record_and_audit() {
        let state = state_with_key(None);

        let response = build_authorize_action_response(
            &state,
            StatusCode::OK,
            "rebalance_cache",
            "tenants/acme/autonomous/records",
            "allow",
            "tenant scope allowed".to_string(),
            "atrace-tenant",
            "platform-admin",
            AutonomousActionDecision::Allow,
        );

        assert_eq!(response.0, StatusCode::OK);
        let records = latest_action_records(&state, 10);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tenant_id.as_deref(), Some("acme"));

        let events = state.audit_sink.lock().expect("audit lock").latest(1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AuditEventKind::Autonomous);
        assert!(events[0].details_json.contains("\"tenant_id\":\"acme\""));
    }

    #[test]
    fn ws1_udf_runtime_scaffold_executes_polyglot_functions() {
        let sql = "SELECT udf_rust('hello'); SELECT udf_js('abc'); SELECT udf_python('delta');";
        let results = execute_udf_runtime_scaffold(sql).expect("udf scaffold should execute");
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].language, "rust");
        assert_eq!(results[0].output, "HELLO");
        assert_eq!(results[1].language, "javascript");
        assert_eq!(results[1].output, "cba");
        assert_eq!(results[2].language, "python");
        assert_eq!(results[2].output, "5");
    }

    #[test]
    fn ws1_udf_runtime_scaffold_blocks_unsafe_payload() {
        let sql = "SELECT udf_python('x'); import os";
        let err = execute_udf_runtime_scaffold(sql).expect_err("unsafe payload should be blocked");
        assert_eq!(err, "udf_guardrail_blocked_python_payload");
    }

    #[test]
    fn ws1_udf_execution_plan_contains_route_and_invocations() {
        let sql = "SELECT udf_rust('hello'); UPDATE t SET v = udf_python('xy')";
        let plan = build_udf_execution_plan(sql);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].route_path, "olap");
        assert_eq!(plan[0].udf_invocations.len(), 1);
        assert_eq!(plan[0].udf_invocations[0].language, "rust");
        assert_eq!(plan[1].route_path, "oltp");
        assert_eq!(plan[1].udf_invocations[0].language, "python");
    }

    #[test]
    fn ws1_udf_catalog_and_policy_contracts_cover_polyglot_set() {
        let catalog = udf_function_catalog_contract();
        assert_eq!(catalog.len(), 3);
        assert!(catalog.iter().any(|f| f.language == "rust"));
        assert!(catalog.iter().any(|f| f.language == "javascript"));
        assert!(catalog.iter().any(|f| f.language == "python"));

        let policies = udf_guard_policy_contract();
        assert_eq!(policies.len(), 3);
        assert!(policies.iter().all(|p| p.max_input_bytes == 256));
    }

    #[test]
    fn ws22_pessimistic_lock_blocks_conflicting_transaction() {
        let mut lock_table = HashMap::new();
        let mut wait_graph = HashMap::new();
        let (first_status, first) = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-1",
            "table:orders:row:42",
            "test-owner",
            30_000,
            0,
            10_000,
        );
        assert_eq!(first_status, StatusCode::OK);
        assert_eq!(first.lock_state, "acquired");

        let (conflict_status, conflict) = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-2",
            "table:orders:row:42",
            "test-owner",
            30_000,
            0,
            10_010,
        );
        assert_eq!(conflict_status, StatusCode::CONFLICT);
        assert_eq!(conflict.lock_state, "held_by_other_transaction");
        assert_eq!(
            conflict.lock.expect("conflict lock").transaction_id,
            "tx-1".to_string()
        );
    }

    #[test]
    fn ws22_pessimistic_lock_release_requires_owner_transaction() {
        let mut lock_table = HashMap::new();
        let mut wait_graph = HashMap::new();
        let _ = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-1",
            "table:inventory:sku:100",
            "test-owner",
            30_000,
            0,
            11_000,
        );

        let (release_conflict_status, release_conflict) =
            release_pessimistic_lock(&mut lock_table, &mut wait_graph, "tx-2", "table:inventory:sku:100");
        assert_eq!(release_conflict_status, StatusCode::CONFLICT);
        assert_eq!(release_conflict.lock_state, "ownership_mismatch");

        let (release_ok_status, release_ok) =
            release_pessimistic_lock(&mut lock_table, &mut wait_graph, "tx-1", "table:inventory:sku:100");
        assert_eq!(release_ok_status, StatusCode::OK);
        assert_eq!(release_ok.lock_state, "released");
    }

    #[test]
    fn ws22_pessimistic_lock_wait_timeout_returns_request_timeout() {
        let mut lock_table = HashMap::new();
        let mut wait_graph = HashMap::new();
        let _ = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-1",
            "table:payments:row:7",
            "test-owner",
            30_000,
            0,
            12_000,
        );

        let (timeout_status, timeout) = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-2",
            "table:payments:row:7",
            "test-owner",
            30_000,
            2_000,
            12_050,
        );
        assert_eq!(timeout_status, StatusCode::REQUEST_TIMEOUT);
        assert_eq!(timeout.lock_state, "wait_timeout");
        assert_eq!(timeout.reason, "pessimistic_lock_wait_timeout");
    }

    #[test]
    fn ws22_pessimistic_lock_detects_deadlock_risk_cycle() {
        let mut lock_table = HashMap::new();
        let mut wait_graph = HashMap::new();
        let _ = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-a",
            "table:orders:row:1",
            "test-owner",
            30_000,
            0,
            13_000,
        );
        let _ = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-b",
            "table:orders:row:2",
            "test-owner",
            30_000,
            0,
            13_010,
        );

        let (first_wait_status, first_wait) = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-a",
            "table:orders:row:2",
            "test-owner",
            30_000,
            2_000,
            13_020,
        );
        assert_eq!(first_wait_status, StatusCode::REQUEST_TIMEOUT);
        assert_eq!(first_wait.lock_state, "wait_timeout");

        let (deadlock_status, deadlock) = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-b",
            "table:orders:row:1",
            "test-owner",
            30_000,
            2_000,
            13_030,
        );
        assert_eq!(deadlock_status, StatusCode::CONFLICT);
        assert_eq!(deadlock.lock_state, "deadlock_risk");
        assert_eq!(deadlock.reason, "pessimistic_lock_deadlock_risk");
    }

    #[test]
    fn ws22_pessimistic_lock_detects_deadlock_risk_multi_hop_cycle() {
        let mut lock_table = HashMap::new();
        let mut wait_graph = HashMap::new();

        let _ = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-a",
            "table:orders:row:11",
            "test-owner",
            30_000,
            0,
            14_000,
        );
        let _ = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-b",
            "table:orders:row:12",
            "test-owner",
            30_000,
            0,
            14_010,
        );
        let _ = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-c",
            "table:orders:row:13",
            "test-owner",
            30_000,
            0,
            14_020,
        );

        let (a_wait_status, a_wait) = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-a",
            "table:orders:row:12",
            "test-owner",
            30_000,
            2_000,
            14_030,
        );
        assert_eq!(a_wait_status, StatusCode::REQUEST_TIMEOUT);
        assert_eq!(a_wait.lock_state, "wait_timeout");

        let (b_wait_status, b_wait) = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-b",
            "table:orders:row:13",
            "test-owner",
            30_000,
            2_000,
            14_040,
        );
        assert_eq!(b_wait_status, StatusCode::REQUEST_TIMEOUT);
        assert_eq!(b_wait.lock_state, "wait_timeout");

        let (deadlock_status, deadlock) = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-c",
            "table:orders:row:11",
            "test-owner",
            30_000,
            2_000,
            14_050,
        );
        assert_eq!(deadlock_status, StatusCode::CONFLICT);
        assert_eq!(deadlock.lock_state, "deadlock_risk");
        assert_eq!(deadlock.reason, "pessimistic_lock_deadlock_risk");
    }

    #[test]
    fn ws22_pessimistic_lock_scan_cap_returns_timeout_diagnostic() {
        let mut lock_table = HashMap::new();
        let mut wait_graph = HashMap::new();
        let resources: Vec<String> = (0..=DEADLOCK_SCAN_MAX_HOPS)
            .map(|idx| format!("table:orders:row:{}", 100 + idx))
            .collect();
        let tx_ids: Vec<String> = (0..=DEADLOCK_SCAN_MAX_HOPS)
            .map(|idx| format!("tx-chain-{idx}"))
            .collect();

        for idx in 0..tx_ids.len() {
            let _ = acquire_pessimistic_lock(
                &mut lock_table,
                &mut wait_graph,
                &tx_ids[idx],
                &resources[idx],
                "test-owner",
                30_000,
                0,
                15_000 + (idx as u128),
            );
        }

        for idx in 0..(tx_ids.len() - 1) {
            let _ = acquire_pessimistic_lock(
                &mut lock_table,
                &mut wait_graph,
                &tx_ids[idx],
                &resources[idx + 1],
                "test-owner",
                30_000,
                2_000,
                15_100 + (idx as u128),
            );
        }

        let (status, response) = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-requester",
            &resources[0],
            "test-owner",
            30_000,
            2_000,
            15_500,
        );
        assert_eq!(status, StatusCode::REQUEST_TIMEOUT);
        assert_eq!(response.lock_state, "wait_timeout");
        assert_eq!(
            response.reason,
            "pessimistic_lock_wait_timeout_scan_cap_reached"
        );
    }

    #[test]
    fn ws22_pessimistic_lock_release_cleans_wait_edges_for_resource() {
        let mut lock_table = HashMap::new();
        let mut wait_graph = HashMap::new();
        let _ = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-holder",
            "table:orders:row:301",
            "test-owner",
            30_000,
            0,
            16_000,
        );

        let _ = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-waiter",
            "table:orders:row:301",
            "test-owner",
            30_000,
            2_000,
            16_010,
        );
        assert!(wait_graph.contains_key("tx-waiter"));

        let (release_status, _) = release_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-holder",
            "table:orders:row:301",
        );
        assert_eq!(release_status, StatusCode::OK);
        assert!(!wait_graph.contains_key("tx-waiter"));
    }

    #[test]
    fn ws22_pessimistic_lock_expiry_cleans_wait_edges_for_resource() {
        let mut lock_table = HashMap::new();
        let mut wait_graph = HashMap::new();
        let _ = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-holder",
            "table:orders:row:401",
            "test-owner",
            1_000,
            0,
            17_000,
        );
        let _ = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-waiter",
            "table:orders:row:401",
            "test-owner",
            30_000,
            2_000,
            17_100,
        );
        assert!(wait_graph.contains_key("tx-waiter"));

        let (acquire_status, acquire_result) = acquire_pessimistic_lock(
            &mut lock_table,
            &mut wait_graph,
            "tx-new-holder",
            "table:orders:row:401",
            "test-owner",
            30_000,
            0,
            18_200,
        );
        assert_eq!(acquire_status, StatusCode::OK);
        assert_eq!(acquire_result.lock_state, "acquired");
        assert!(!wait_graph.contains_key("tx-waiter"));
    }

    #[test]
    fn ws22_pessimistic_lock_contention_metrics_counts_outcomes() {
        let metrics = PessimisticLockContentionMetrics::new();
        let mut lock_table = HashMap::new();
        let mut wait_graph = HashMap::new();

        // Grant a lock -> lock_grants++
        let (s1, r1) = acquire_pessimistic_lock(
            &mut lock_table, &mut wait_graph, "tx-1", "res:a", "owner", 30_000, 0, 20_000,
        );
        assert_eq!(s1, StatusCode::OK);
        assert!(r1.lock_state == "acquired" || r1.lock_state == "renewed");
        metrics.lock_grants.fetch_add(1, Ordering::Relaxed);

        // Conflict (no wait_timeout) -> lock_conflicts++
        let (s2, r2) = acquire_pessimistic_lock(
            &mut lock_table, &mut wait_graph, "tx-2", "res:a", "owner", 30_000, 0, 20_010,
        );
        assert_eq!(s2, StatusCode::CONFLICT);
        assert_eq!(r2.lock_state, "held_by_other_transaction");
        metrics.lock_conflicts.fetch_add(1, Ordering::Relaxed);

        // Wait timeout -> wait_timeouts++
        let (s3, r3) = acquire_pessimistic_lock(
            &mut lock_table, &mut wait_graph, "tx-3", "res:a", "owner", 30_000, 2_000, 20_020,
        );
        assert_eq!(s3, StatusCode::REQUEST_TIMEOUT);
        assert_eq!(r3.lock_state, "wait_timeout");
        assert_eq!(r3.reason, "pessimistic_lock_wait_timeout");
        metrics.wait_timeouts.fetch_add(1, Ordering::Relaxed);

        // Deadlock detection -> deadlock_detections++
        let _ = acquire_pessimistic_lock(
            &mut lock_table, &mut wait_graph, "tx-d1", "res:d1", "owner", 30_000, 0, 20_100,
        );
        metrics.lock_grants.fetch_add(1, Ordering::Relaxed);
        let _ = acquire_pessimistic_lock(
            &mut lock_table, &mut wait_graph, "tx-d2", "res:d2", "owner", 30_000, 0, 20_110,
        );
        metrics.lock_grants.fetch_add(1, Ordering::Relaxed);
        let _ = acquire_pessimistic_lock(
            &mut lock_table, &mut wait_graph, "tx-d1", "res:d2", "owner", 30_000, 2_000, 20_120,
        );
        metrics.wait_timeouts.fetch_add(1, Ordering::Relaxed);
        let (s4, r4) = acquire_pessimistic_lock(
            &mut lock_table, &mut wait_graph, "tx-d2", "res:d1", "owner", 30_000, 2_000, 20_130,
        );
        assert_eq!(s4, StatusCode::CONFLICT);
        assert_eq!(r4.lock_state, "deadlock_risk");
        metrics.deadlock_detections.fetch_add(1, Ordering::Relaxed);

        // Release -> lock_releases++
        let (s5, r5) = release_pessimistic_lock(&mut lock_table, &mut wait_graph, "tx-1", "res:a");
        assert_eq!(s5, StatusCode::OK);
        assert_eq!(r5.lock_state, "released");
        metrics.lock_releases.fetch_add(1, Ordering::Relaxed);

        // Verify metric counts
        assert_eq!(metrics.lock_grants.load(Ordering::Relaxed), 3);
        assert_eq!(metrics.lock_conflicts.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.wait_timeouts.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.deadlock_detections.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.lock_releases.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.scan_cap_timeouts.load(Ordering::Relaxed), 0);

        // Verify contention ratio: (1 deadlock + 0 scan_cap + 2 wait_timeout + 1 conflict) / (1+0+2+3+1) = 4/7
        let total = 1 + 0 + 2 + 3 + 1;
        let contention = 1 + 0 + 2 + 1;
        let expected_ratio = contention as f64 / total as f64;
        let actual_ratio = {
            let d = metrics.deadlock_detections.load(Ordering::Relaxed);
            let sc = metrics.scan_cap_timeouts.load(Ordering::Relaxed);
            let wt = metrics.wait_timeouts.load(Ordering::Relaxed);
            let g = metrics.lock_grants.load(Ordering::Relaxed);
            let c = metrics.lock_conflicts.load(Ordering::Relaxed);
            let total = d + sc + wt + g + c;
            if total > 0 { (d + sc + wt + c) as f64 / total as f64 } else { 0.0 }
        };
        assert!((actual_ratio - expected_ratio).abs() < 0.001);
    }

    /// Runs last among `ws22_*` tests when the harness uses `--test-threads=1` (alphabetically after `ws22_…`).
    /// Emits one stderr line consumed by `run-ws22-pessimistic-lock-smoke.ps1` for gate / trend summaries.
    #[test]
    fn zzz_ws22_gate_lock_contention_metrics_emit() {
        let d = WS22_GATE_DEADLOCK_DETECTIONS.load(Ordering::Relaxed);
        let s = WS22_GATE_SCAN_CAP_TIMEOUTS.load(Ordering::Relaxed);
        eprintln!(
            "WS22_GATE_LOCK_METRICS_JSON:{}",
            json!({
                "deadlock_detections": d,
                "scan_cap_timeouts": s,
            })
        );
    }

    #[test]
    fn h06_cache_runtime_endpoints_and_metrics() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "platform-admin");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let set_response = runtime
            .block_on(sre_cache_set(
                State(state.clone()),
                headers.clone(),
                Json(CacheSetRequest {
                    partition_id: "tenant-acme".to_string(),
                    key: "customer:42".to_string(),
                    value: json!({"tier":"gold"}),
                    ttl_ms: Some(60_000),
                }),
            ))
            .expect("cache set should succeed");
        assert_eq!(set_response.status, "ok");

        let get_response = runtime
            .block_on(sre_cache_get(
                State(state.clone()),
                headers.clone(),
                Query(CacheGetQuery {
                    partition_id: "tenant-acme".to_string(),
                    key: "customer:42".to_string(),
                }),
            ))
            .expect("cache get should succeed");
        assert_eq!(get_response.status, "ok");
        assert!(get_response.hit);
        assert_eq!(get_response.value, Some(json!({"tier":"gold"})));

        let metrics = runtime
            .block_on(sre_cache_metrics(
                State(state.clone()),
                headers.clone(),
            ))
            .expect("cache metrics should succeed");
        assert_eq!(metrics.status, "ok");
        assert!(metrics.partition_count >= 1);
        assert!(metrics.total_entries >= 1);

        let invalidate = runtime
            .block_on(sre_cache_invalidate(
                State(state.clone()),
                headers.clone(),
                Json(CacheInvalidateRequest {
                    partition_id: "tenant-acme".to_string(),
                    key: "customer:42".to_string(),
                }),
            ))
            .expect("cache invalidate should succeed");
        assert_eq!(invalidate.status, "ok");
        assert!(invalidate.removed);

        let rebalance = runtime
            .block_on(sre_cache_rebalance(State(state), headers))
            .expect("cache rebalance should succeed");
        assert_eq!(rebalance.status, "ok");
        assert!(rebalance.rebalanced_partitions >= 1);
    }

    #[test]
    fn h07_driver_pool_runtime_hooks() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "platform-admin");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let acquire = runtime
            .block_on(sre_driver_pool_acquire(
                State(state.clone()),
                headers.clone(),
                Json(PoolAcquireRequest { now_ms: Some(1_000) }),
            ))
            .expect("pool acquire should succeed");
        assert_eq!(acquire.status, "ok");
        assert_eq!(acquire.acquire_state, "acquired");
        let connection_id = acquire
            .connection_id
            .as_ref()
            .cloned()
            .expect("connection id");

        let failure = runtime
            .block_on(sre_driver_pool_failure(
                State(state.clone()),
                headers.clone(),
                Json(PoolFailureRequest {
                    connection_id: connection_id.clone(),
                    error: Some("simulated-burst-failure".to_string()),
                    now_ms: Some(1_100),
                }),
            ))
            .expect("pool failure hook should succeed");
        assert_eq!(failure.status, "ok");
        assert!(failure.marked_failed);

        let release = runtime
            .block_on(sre_driver_pool_release(
                State(state.clone()),
                headers.clone(),
                Json(PoolReleaseRequest {
                    connection_id,
                    now_ms: Some(1_200),
                }),
            ))
            .expect("pool release should succeed");
        assert_eq!(release.status, "ok");

        let recover = runtime
            .block_on(sre_driver_pool_recover(
                State(state.clone()),
                headers.clone(),
                Json(PoolRecoverRequest {
                    now_ms: Some(35_000),
                    prune_unhealthy: Some(true),
                }),
            ))
            .expect("pool recover should succeed");
        assert_eq!(recover.status, "ok");

        let stats = runtime
            .block_on(sre_driver_pool_stats(State(state), headers))
            .expect("pool stats should succeed");
        assert!(stats.total_connections >= 1);
    }

    #[test]
    fn h08_signed_provenance_enforcement_endpoint_path() {
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "security-bot");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let rejected = runtime
            .block_on(security_plugins_provenance_register(
                State(state.clone()),
                headers.clone(),
                Json(SignedProvenanceRegistrationRequest {
                    plugin_id: "connector.kafka".to_string(),
                    plugin_version: "1.0.0".to_string(),
                    checksum_sha256: "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd".to_string(),
                    display_name: None,
                    owner: Some("team-ingest".to_string()),
                    license: Some("Apache-2.0".to_string()),
                    capabilities: Some(vec!["ingest.read".to_string()]),
                    schema_version: Some("v1".to_string()),
                    signature_algorithm: "ed25519".to_string(),
                    signature_key_id: "ws7-signer-1".to_string(),
                    signature_base64: "dGVzdC1zaWduYXR1cmUtcGF5bG9hZA==".to_string(),
                    revoked_key_ids: Some(Vec::new()),
                    attestations: vec![
                        SignedProvenanceAttestationRequest {
                            attester_id: "ci-1".to_string(),
                            attested_at_ms: Some(1_700_000_000_100),
                            attestation_type: "checksum_verification".to_string(),
                            payload_digest_sha256: "digest-1".to_string(),
                            signature_base64: "sig-1".to_string(),
                            passed: true,
                        },
                    ],
                    sbom_entries: Some(vec![SignedProvenanceSbomEntryRequest {
                        component_name: "serde".to_string(),
                        component_version: "1.0".to_string(),
                        license: "Apache-2.0".to_string(),
                        checksum_sha256: "sum-1".to_string(),
                        source_url: None,
                    }]),
                }),
            ))
            .expect("endpoint should return rejection payload");
        assert_eq!(rejected.status, "error");
        assert_eq!(rejected.registration_state, "rejected");
        assert!(!rejected.chain_complete);

        let accepted = runtime
            .block_on(security_plugins_provenance_register(
                State(state),
                headers,
                Json(SignedProvenanceRegistrationRequest {
                    plugin_id: "connector.kafka".to_string(),
                    plugin_version: "1.0.1".to_string(),
                    checksum_sha256: "bbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddee".to_string(),
                    display_name: Some("Kafka Connector".to_string()),
                    owner: Some("team-ingest".to_string()),
                    license: Some("Apache-2.0".to_string()),
                    capabilities: Some(vec!["ingest.read".to_string()]),
                    schema_version: Some("v1".to_string()),
                    signature_algorithm: "ed25519".to_string(),
                    signature_key_id: "ws7-signer-1".to_string(),
                    signature_base64: "dGVzdC1zaWduYXR1cmUtcGF5bG9hZA==".to_string(),
                    revoked_key_ids: Some(Vec::new()),
                    attestations: vec![
                        SignedProvenanceAttestationRequest {
                            attester_id: "ci-1".to_string(),
                            attested_at_ms: Some(1_700_000_000_100),
                            attestation_type: "checksum_verification".to_string(),
                            payload_digest_sha256: "digest-1".to_string(),
                            signature_base64: "sig-1".to_string(),
                            passed: true,
                        },
                        SignedProvenanceAttestationRequest {
                            attester_id: "ci-2".to_string(),
                            attested_at_ms: Some(1_700_000_000_101),
                            attestation_type: "signature_verification".to_string(),
                            payload_digest_sha256: "digest-2".to_string(),
                            signature_base64: "sig-2".to_string(),
                            passed: true,
                        },
                        SignedProvenanceAttestationRequest {
                            attester_id: "review-1".to_string(),
                            attested_at_ms: Some(1_700_000_000_102),
                            attestation_type: "review_approval".to_string(),
                            payload_digest_sha256: "digest-3".to_string(),
                            signature_base64: "sig-3".to_string(),
                            passed: true,
                        },
                    ],
                    sbom_entries: Some(vec![SignedProvenanceSbomEntryRequest {
                        component_name: "serde".to_string(),
                        component_version: "1.0".to_string(),
                        license: "Apache-2.0".to_string(),
                        checksum_sha256: "sum-1".to_string(),
                        source_url: None,
                    }]),
                }),
            ))
            .expect("endpoint should accept complete provenance");
        assert_eq!(accepted.status, "ok");
        assert_eq!(accepted.registration_state, "registered");
        assert!(accepted.chain_complete);
        assert!(accepted.audit_records_total >= 1);
    }

    #[test]
    fn ws11_parses_locale_from_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "accept-language",
            HeaderValue::from_static("es-ES,es;q=0.9"),
        );
        let locale = locale_from_headers(&headers);
        assert_eq!(locale, SupportedLocale::EsEs);
    }

    #[test]
    fn ws11_locale_header_falls_back_to_en_us_for_unknown_locale() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "accept-language",
            HeaderValue::from_static("de-DE,de;q=0.8"),
        );
        let locale = locale_from_headers(&headers);
        assert_eq!(locale, SupportedLocale::EnUs);
    }

    #[test]
    fn ws12_evaluate_rate_limit_denies_when_hard_limit_exceeded() {
        let (allowed, remaining, reason) = evaluate_rate_limit(650, 1, 600, 50);
        assert!(!allowed);
        assert_eq!(remaining, 0);
        assert_eq!(reason, "hard_limit_exceeded");
    }

    #[test]
    fn ws12_evaluate_rate_limit_allows_with_burst_allowance() {
        let (allowed, remaining, reason) = evaluate_rate_limit(620, 5, 600, 50);
        assert!(allowed);
        assert_eq!(remaining, 25);
        assert_eq!(reason, "burst_allowance");
    }

    #[test]
    fn ws12_failure_budget_snapshot_computes_remaining() {
        let snapshot = failure_budget_snapshot(12.5);
        assert_eq!(snapshot.window_minutes, 60);
        assert_eq!(snapshot.error_budget_percent, 1.0);
        assert_eq!(snapshot.consumed_percent, 12.5);
        assert_eq!(snapshot.remaining_percent, 87.5);
        assert!(snapshot.burn_rate > 0.0);
    }

    #[test]
    fn ws12_failure_budget_alert_escalates_to_critical() {
        let alert = evaluate_failure_budget_alert(82.0, 1.2);
        assert_eq!(alert.alert_state, "triggered");
        assert_eq!(alert.severity, "critical");
    }

    #[test]
    fn ws12_dr_hook_executes_failover_when_not_dry_run() {
        let state = state_with_key(None);
        let execution = execute_dr_hook(&state, "failover_drill", Some("cluster"), false);
        assert_eq!(execution.status, "executed");
        assert!(execution.details.contains("leader rotated"));
        assert_eq!(latest_dr_hook_records(&state, 10).len(), 1);
    }

    #[test]
    fn ws12_dr_hook_rejects_unsupported_hook() {
        let state = state_with_key(None);
        let execution = execute_dr_hook(&state, "unknown_hook", None, true);
        assert_eq!(execution.status, "rejected");
        assert!(execution.details.contains("unsupported_dr_hook"));
    }

    #[test]
    fn ws12_dr_hook_applies_cooldown_window() {
        let state = state_with_key(None);
        let first = execute_dr_hook(&state, "replay_checkpoint_verify", Some("cluster"), false);
        assert_eq!(first.status, "executed");
        let second = execute_dr_hook(&state, "replay_checkpoint_verify", Some("cluster"), false);
        assert_eq!(second.status, "cooldown");
        assert_eq!(second.policy_decision, "deny_cooldown");
        assert!(second.cooldown_remaining_ms > 0);
    }

    #[test]
    fn ws12_retry_backoff_growth_is_capped() {
        assert_eq!(compute_retry_backoff_ms(1, 500, 10_000), 500);
        assert_eq!(compute_retry_backoff_ms(2, 500, 10_000), 1_000);
        assert_eq!(compute_retry_backoff_ms(3, 500, 10_000), 2_000);
        assert_eq!(compute_retry_backoff_ms(8, 500, 10_000), 10_000);
    }

    #[test]
    fn ws12_dr_hook_denies_when_mode_below_policy() {
        let mut state = state_with_key(None);
        state.autonomous_mode = AutonomousMode::Advisory;
        let execution = execute_dr_hook(&state, "failover_drill", Some("cluster"), false);
        assert_eq!(execution.status, "rejected");
        assert_eq!(execution.policy_decision, "deny_mode");
    }

    #[test]
    fn ws12_retry_plan_builds_monotonic_backoff() {
        let policy = default_dr_hook_policy_config();
        let plan = build_retry_plan(&policy, 5);
        assert_eq!(plan.len(), 5);
        assert_eq!(plan[0].recommended_backoff_ms, 500);
        assert!(plan[1].recommended_backoff_ms >= plan[0].recommended_backoff_ms);
        assert!(plan[4].recommended_backoff_ms >= plan[3].recommended_backoff_ms);
    }

    #[test]
    fn ws12_persistent_policy_state_roundtrip() {
        let temp = std::env::temp_dir().join(format!("vng-ws12-{}.json", now_unix_ms()));
        let state = AppState {
            dr_hook_state_path: Some(temp.to_string_lossy().to_string()),
            ..state_with_key(None)
        };
        let _ = execute_dr_hook(&state, "failover_drill", Some("cluster"), true);
        let loaded = load_dr_hook_policy_state(state.dr_hook_state_path.as_deref());
        assert!(loaded.hooks.contains_key("failover_drill"));
        let persisted = fs::read_to_string(&temp).expect("state file readable");
        assert!(persisted.contains("\"schema_version\": 1"));
        assert!(persisted.contains("\"checksum_hex\""));
        let _ = fs::remove_file(temp);
    }

    #[test]
    fn ws12_policy_state_falls_back_to_backup_when_primary_corrupted() {
        let temp = std::env::temp_dir().join(format!("vng-ws12-corrupt-{}.json", now_unix_ms()));
        let temp_str = temp.to_string_lossy().to_string();
        let backup = format!("{temp_str}.bak");

        let state = AppState {
            dr_hook_state_path: Some(temp_str.clone()),
            ..state_with_key(None)
        };

        let _ = execute_dr_hook(&state, "failover_drill", Some("cluster"), true);
        // Trigger a second persist so backup file is created.
        let _ = execute_dr_hook(&state, "replay_checkpoint_verify", Some("cluster"), true);

        fs::write(&temp, "{not valid json").expect("corrupt primary");
        let loaded = load_dr_hook_policy_state(Some(&temp_str));
        assert!(loaded.hooks.contains_key("failover_drill"));

        let _ = fs::remove_file(temp);
        let _ = fs::remove_file(backup);
    }

    #[test]
    fn ws12_policy_state_loads_legacy_snapshot_format() {
        let temp = std::env::temp_dir().join(format!("vng-ws12-legacy-{}.json", now_unix_ms()));
        let mut hooks = HashMap::new();
        hooks.insert(
            "failover_drill".to_string(),
            DrHookRuntimeState {
                last_attempt_unix_ms: 123,
                consecutive_failures: 1,
                last_status: "success".to_string(),
            },
        );
        let legacy = DrHookPolicyStateSnapshot { hooks };
        let encoded = serde_json::to_string_pretty(&legacy).expect("encode legacy");
        fs::write(&temp, encoded).expect("write legacy");

        let loaded = load_dr_hook_policy_state(Some(temp.to_string_lossy().as_ref()));
        assert!(loaded.hooks.contains_key("failover_drill"));

        let _ = fs::remove_file(temp);
    }

    #[test]
    fn ws12_scheduler_queue_enqueues_tasks() {
        let state = state_with_key(None);
        let task = enqueue_dr_hook_task(
            &state,
            "failover_drill",
            Some("cluster"),
            true,
            "tester",
            "unit_test",
        );
        assert_eq!(task.hook, "failover_drill");
        let depth = state.dr_hook_queue.lock().expect("queue lock").len();
        assert_eq!(depth, 1);
    }

    #[test]
    fn ws12_failure_signal_queues_auto_remediation() {
        let state = state_with_key(None);
        if let Ok(mut signals) = state.cluster_failure_signals.lock() {
            signals.push(ClusterFailureSignal {
                signal_id: "sig-1".to_string(),
                node_id: "node-2".to_string(),
                transport: "gossip".to_string(),
                failure_type: "node_unreachable".to_string(),
                severity: "critical".to_string(),
                message: "heartbeat timeout".to_string(),
                observed_unix_ms: now_unix_ms(),
                resolved: false,
                resolved_by: None,
                resolved_unix_ms: None,
                resolution_note: None,
            });
        }
        let task = enqueue_dr_hook_task(
            &state,
            "failover_drill",
            Some("cluster"),
            false,
            "auto_sre",
            "critical_node_unreachable_signal",
        );
        assert_eq!(task.reason, "critical_node_unreachable_signal");
    }

    #[test]
    fn ws12_gate_criteria_detects_critical_signal() {
        let state = AppState {
            dr_hook_state_path: Some("state/test.json".to_string()),
            ..state_with_key(None)
        };
        if let Ok(mut signals) = state.cluster_failure_signals.lock() {
            signals.push(ClusterFailureSignal {
                signal_id: "sig-critical".to_string(),
                node_id: "node-3".to_string(),
                transport: "raft".to_string(),
                failure_type: "replication_lag".to_string(),
                severity: "critical".to_string(),
                message: "lag over threshold".to_string(),
                observed_unix_ms: now_unix_ms(),
                resolved: false,
                resolved_by: None,
                resolved_unix_ms: None,
                resolution_note: None,
            });
        }
        let evaluation = build_sre_gate_evaluation(&state);
        assert_eq!(evaluation.gate_result, "warn");
    }

    #[test]
    fn ws12_reconcile_marks_critical_resolved() {
        let state = state_with_key(None);
        if let Ok(mut signals) = state.cluster_failure_signals.lock() {
            signals.push(ClusterFailureSignal {
                signal_id: "sig-reconcile".to_string(),
                node_id: "node-4".to_string(),
                transport: "gossip".to_string(),
                failure_type: "node_unreachable".to_string(),
                severity: "critical".to_string(),
                message: "heartbeat timeout".to_string(),
                observed_unix_ms: now_unix_ms(),
                resolved: false,
                resolved_by: None,
                resolved_unix_ms: None,
                resolution_note: None,
            });
        }
        if let Ok(mut signals) = state.cluster_failure_signals.lock() {
            for signal in signals.iter_mut() {
                if signal.signal_id == "sig-reconcile" {
                    signal.resolved = true;
                    signal.resolved_by = Some("tester".to_string());
                    signal.resolved_unix_ms = Some(now_unix_ms());
                }
            }
        }
        let unresolved = state
            .cluster_failure_signals
            .lock()
            .expect("signal lock")
            .iter()
            .filter(|s| s.severity == "critical" && !s.resolved)
            .count();
        assert_eq!(unresolved, 0);
    }

    #[test]
    fn ws12_gate_export_writes_artifact() {
        let state = state_with_key(None);
        let evaluation = build_sre_gate_evaluation(&state);
        let output = std::env::temp_dir().join(format!("vng-gate-{}.json", now_unix_ms()));
        export_gate_report(output.to_string_lossy().as_ref(), &evaluation);
        let exists = output.exists();
        let _ = fs::remove_file(output);
        assert!(exists);
    }

    // â”€â”€ WS2 Index + Constraint tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[test]
    fn ws2_index_create_lookup_drop_lifecycle() {
        use voltnuerongrid_store::index::{IndexDescriptor, IndexKind};

        let state = state_with_key(None);
        {
            let mut mgr = state.index_manager.lock().expect("lock");
            mgr.create_index(IndexDescriptor {
                name: "idx_orders_customer".to_string(),
                table: "orders".to_string(),
                column: "customer_id".to_string(),
                kind: IndexKind::BTree,
                unique: false,
            })
            .expect("create index");

            let idx = mgr.get_mut("idx_orders_customer").expect("get idx");
            idx.insert("C100", "row-1").expect("insert");
            idx.insert("C100", "row-2").expect("insert");
            idx.insert("C200", "row-3").expect("insert");
        }
        {
            let mgr = state.index_manager.lock().expect("lock");
            let idx = mgr.get("idx_orders_customer").expect("get idx");
            assert_eq!(idx.lookup("C100").len(), 2);
            assert_eq!(idx.lookup("C200").len(), 1);
            assert!(idx.lookup("C999").is_empty());
            assert_eq!(mgr.index_count(), 1);
        }
        {
            let mut mgr = state.index_manager.lock().expect("lock");
            let dropped = mgr.drop_index("idx_orders_customer").expect("drop");
            assert_eq!(dropped.name, "idx_orders_customer");
            assert_eq!(mgr.index_count(), 0);
        }
    }

    #[test]
    fn ws2_unique_index_rejects_duplicate_via_appstate() {
        use voltnuerongrid_store::index::{IndexDescriptor, IndexKind, IndexError};

        let state = state_with_key(None);
        let mut mgr = state.index_manager.lock().expect("lock");
        mgr.create_index(IndexDescriptor {
            name: "idx_pk".to_string(),
            table: "users".to_string(),
            column: "id".to_string(),
            kind: IndexKind::BTree,
            unique: true,
        })
        .expect("create");
        let idx = mgr.get_mut("idx_pk").expect("get");
        idx.insert("1", "row-1").expect("first insert ok");
        let err = idx.insert("1", "row-2").unwrap_err();
        assert!(matches!(err, IndexError::UniqueViolation { .. }));
    }

    #[test]
    fn ws2_constraint_pk_not_null_via_appstate() {
        use voltnuerongrid_store::constraints::{ConstraintDescriptor, ConstraintKind};

        let state = state_with_key(None);
        let mut mgr = state.constraint_manager.lock().expect("lock");
        mgr.add_constraint(ConstraintDescriptor {
            name: "pk_users".to_string(),
            table: "users".to_string(),
            column: "id".to_string(),
            kind: ConstraintKind::PrimaryKey,
        })
        .expect("add pk");
        mgr.add_constraint(ConstraintDescriptor {
            name: "nn_name".to_string(),
            table: "users".to_string(),
            column: "name".to_string(),
            kind: ConstraintKind::NotNull,
        })
        .expect("add nn");

        // Valid insert
        mgr.validate("users", "id", Some("1")).expect("pk valid");
        mgr.record_committed_value("users", "id", "1");

        // PK duplicate rejected
        assert!(mgr.validate("users", "id", Some("1")).is_err());

        // PK null rejected
        assert!(mgr.validate("users", "id", None).is_err());

        // NOT NULL rejected
        assert!(mgr.validate("users", "name", None).is_err());

        // NOT NULL accepted
        mgr.validate("users", "name", Some("Alice")).expect("nn valid");
    }

    // â”€â”€ WS4 Ingest tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[test]
    fn ws4_csv_ingest_via_appstate() {
        use voltnuerongrid_ingest::csv::CsvConnector;

        let state = state_with_key(None);
        let csv = "id,name,region\n1,Alice,us-east\n2,Bob,eu-west\n";
        let mut conn = CsvConnector::new("csv-orders", "CSV Orders");
        let count = conn.load_csv(csv);
        assert_eq!(count, 2);

        let records = conn.read_batch(usize::MAX);
        state
            .ingest_csv_records
            .lock()
            .expect("lock")
            .insert("csv-orders".to_string(), records);

        let map = state.ingest_csv_records.lock().expect("lock");
        assert_eq!(map.get("csv-orders").expect("get").len(), 2);
    }

    #[test]
    fn ws4_json_ingest_via_appstate() {
        use voltnuerongrid_ingest::json::JsonConnector;

        let state = state_with_key(None);
        let ndjson = "{\"id\":\"1\",\"name\":\"Alice\"}\n{\"id\":\"2\",\"name\":\"Bob\"}\n";
        let mut conn = JsonConnector::new("json-users", "JSON Users", "id");
        let count = conn.load_ndjson(ndjson);
        assert_eq!(count, 2);

        let records = conn.read_batch(usize::MAX);
        state
            .ingest_json_records
            .lock()
            .expect("lock")
            .insert("json-users".to_string(), records);

        let map = state.ingest_json_records.lock().expect("lock");
        assert_eq!(map.get("json-users").expect("get").len(), 2);
    }

    #[test]
    fn ws4_parquet_ingest_via_appstate() {
        use arrow_array::{Int32Array, RecordBatch, StringArray};
        use arrow_schema::{DataType, Field, Schema};
        use parquet::arrow::ArrowWriter;
        use std::sync::Arc;
        use voltnuerongrid_ingest::parquet::ParquetConnector;

        let id = StringArray::from(vec!["k1", "k2"]);
        let amt = Int32Array::from(vec![7, 8]);
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("amount", DataType::Int32, false),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(id) as arrow_array::ArrayRef, Arc::new(amt)],
        )
        .expect("batch");
        let mut buffer = Vec::new();
        {
            let mut writer = ArrowWriter::try_new(&mut buffer, schema, None).expect("writer");
            writer.write(&batch).expect("write");
            writer.close().expect("close");
        }

        let state = state_with_key(None);
        let mut conn = ParquetConnector::new("pq-orders", "Parquet Orders");
        let count = conn.load_parquet_bytes(&buffer).expect("parquet load");
        assert_eq!(count, 2);
        let records = conn.read_batch(usize::MAX);
        state
            .ingest_parquet_records
            .lock()
            .expect("lock")
            .insert("pq-orders".to_string(), records);

        let map = state.ingest_parquet_records.lock().expect("lock");
        assert_eq!(map.get("pq-orders").expect("get").len(), 2);
    }

    #[test]
    fn ws4_excel_ingest_via_appstate() {
        use rust_xlsxwriter::{Format, Workbook};
        use voltnuerongrid_ingest::excel::ExcelConnector;

        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        let header = Format::new().set_bold();
        sheet.write_string_with_format(0, 0, "id", &header).unwrap();
        sheet.write_string_with_format(0, 1, "sku", &header).unwrap();
        sheet.write_number(1, 0, 100).unwrap();
        sheet.write_string(1, 1, "A1").unwrap();
        let buffer = workbook.save_to_buffer().expect("buffer");

        let state = state_with_key(None);
        let mut conn = ExcelConnector::new("xlsx-stock", "Excel Stock");
        let count = conn.load_xlsx_bytes(&buffer).expect("excel load");
        assert_eq!(count, 1);
        let records = conn.read_batch(usize::MAX);
        state
            .ingest_excel_records
            .lock()
            .expect("lock")
            .insert("xlsx-stock".to_string(), records);

        let map = state.ingest_excel_records.lock().expect("lock");
        assert_eq!(map.get("xlsx-stock").expect("get").len(), 1);
        assert_eq!(map.get("xlsx-stock").expect("get")[0].key, "100");
    }

    #[test]
    fn ws4_ingest_status_counts_loaded_records() {
        use voltnuerongrid_ingest::csv::CsvConnector;
        use voltnuerongrid_ingest::json::JsonConnector;

        let state = state_with_key(None);

        let mut csv_conn = CsvConnector::new("c1", "C1");
        csv_conn.load_csv("id,v\n1,a\n2,b\n");
        state
            .ingest_csv_records
            .lock()
            .expect("lock")
            .insert("c1".to_string(), csv_conn.read_batch(usize::MAX));

        let mut json_conn = JsonConnector::new("j1", "J1", "id");
        json_conn.load_ndjson("{\"id\":\"x\"}\n");
        state
            .ingest_json_records
            .lock()
            .expect("lock")
            .insert("j1".to_string(), json_conn.read_batch(usize::MAX));

        let csv_map = state.ingest_csv_records.lock().expect("lock");
        let json_map = state.ingest_json_records.lock().expect("lock");
        let csv_total: usize = csv_map.values().map(|v| v.len()).sum();
        let json_total: usize = json_map.values().map(|v| v.len()).sum();
        assert_eq!(csv_total + json_total, 3);
    }

    #[test]
    fn h05_security_kms_status_prefers_primary_env() {
        let mut state = state_with_key(Some("secret"));
        state.security_config = Arc::new(kms_test_config());
        state.kms_runtime = Arc::new(Mutex::new(KmsRuntimeState {
            providers: vec![{
                let mut provider = ConfiguredKmsProviderAdapter::from_key_ref("kms://region-a/key-primary");
                provider.register_key_ref("VNG_KMS_KEY_URI", "kms://region-a/key-primary");
                provider.register_key_ref("VNG_KMS_KEY_URI_REGION_B", "kms://region-b/key-secondary");
                provider
            }],
            ..KmsRuntimeState::default()
        }));

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let response = runtime
            .block_on(security_kms_status(
                State(state),
                operator_headers("secret", "security-bot"),
            ))
            .expect("kms status")
            .0;

        assert_eq!(response.status, "ok");
        assert_eq!(response.resolution_state, "primary_active");
        assert_eq!(response.selected_env.as_deref(), Some("VNG_KMS_KEY_URI"));
        assert!(!response.failover_used);
    }

    #[test]
    fn h05_security_kms_outage_simulation_fails_over_and_recovers() {
        let mut state = state_with_key(Some("secret"));
        state.security_config = Arc::new(kms_test_config());
        state.kms_runtime = Arc::new(Mutex::new(KmsRuntimeState {
            providers: vec![{
                let mut provider = ConfiguredKmsProviderAdapter::from_key_ref("kms://region-a/key-primary");
                provider.register_key_ref("VNG_KMS_KEY_URI", "kms://region-a/key-primary");
                provider.register_key_ref("VNG_KMS_KEY_URI_REGION_B", "kms://region-b/key-secondary");
                provider.register_key_ref("VNG_KMS_KEY_URI_REGION_C", "kms://region-c/key-tertiary");
                provider
            }],
            ..KmsRuntimeState::default()
        }));

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let outage = runtime
            .block_on(security_kms_outage_simulate(
                State(state.clone()),
                operator_headers("secret", "security-bot"),
                Json(SecurityKmsOutageSimulateRequest {
                    unavailable_envs: vec!["VNG_KMS_KEY_URI".to_string()],
                    note: Some("primary_down".to_string()),
                }),
            ))
            .expect("outage simulate")
            .0;
        assert_eq!(outage.status, "degraded");
        assert_eq!(outage.resolution_state, "failover_active");
        assert_eq!(outage.selected_env.as_deref(), Some("VNG_KMS_KEY_URI_REGION_B"));
        assert!(outage.failover_used);

        let recovered = runtime
            .block_on(security_kms_outage_reconcile(
                State(state),
                operator_headers("secret", "security-bot"),
                Json(SecurityKmsOutageReconcileRequest {
                    note: Some("region_restored".to_string()),
                }),
            ))
            .expect("outage reconcile")
            .0;
        assert_eq!(recovered.status, "ok");
        assert_eq!(recovered.selected_env.as_deref(), Some("VNG_KMS_KEY_URI"));
        assert!(!recovered.failover_used);
    }

    #[test]
    fn h05_security_kms_status_reports_unresolved_when_all_regions_out() {
        let mut state = state_with_key(Some("secret"));
        state.security_config = Arc::new(kms_test_config());
        state.kms_runtime = Arc::new(Mutex::new(KmsRuntimeState {
            providers: vec![{
                let mut provider = ConfiguredKmsProviderAdapter::from_key_ref("kms://region-a/key-primary");
                provider.register_key_ref("VNG_KMS_KEY_URI", "kms://region-a/key-primary");
                provider.register_key_ref("VNG_KMS_KEY_URI_REGION_B", "kms://region-b/key-secondary");
                provider
            }],
            unavailable_envs: HashSet::from([
                "VNG_KMS_KEY_URI".to_string(),
                "VNG_KMS_KEY_URI_REGION_B".to_string(),
                "VNG_KMS_KEY_URI_REGION_C".to_string(),
            ]),
            ..KmsRuntimeState::default()
        }));

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let response = runtime
            .block_on(security_kms_status(
                State(state),
                operator_headers("secret", "security-bot"),
            ))
            .expect("kms status")
            .0;

        assert_eq!(response.status, "degraded");
        assert_eq!(response.resolution_state, "unresolved");
        assert!(response.selected_env.is_none());
        assert!(response.last_error.is_some());
    }

    #[test]
    fn h04_ingest_outbox_replay_acknowledges_exactly_once_per_consumer() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let _ = runtime
            .block_on(ingest_csv(
                State(state.clone()),
                headers.clone(),
                Json(IngestCsvRequest {
                    connector_id: "orders".to_string(),
                    csv_data: "id,value\n1,a\n2,b\n".to_string(),
                }),
            ))
            .expect("ingest csv");

        let status = runtime
            .block_on(ingest_outbox_status(State(state.clone()), headers.clone()))
            .expect("outbox status")
            .0;
        assert_eq!(status.total_events, 2);
        assert_eq!(status.stream_count, 1);

        let first_replay = runtime
            .block_on(ingest_outbox_replay(
                State(state.clone()),
                headers.clone(),
                Json(IngestOutboxReplayRequest {
                    connector_id: "orders".to_string(),
                    consumer_id: Some("projection-a".to_string()),
                    max_items: Some(10),
                    acknowledge: Some(true),
                }),
            ))
            .expect("first replay")
            .0;
        assert_eq!(first_replay.delivered_count, 2);
        assert_eq!(first_replay.delivery_state, "delivered_and_acked");
        assert_eq!(first_replay.cursor_after_ack, Some(2));

        let second_replay = runtime
            .block_on(ingest_outbox_replay(
                State(state.clone()),
                headers.clone(),
                Json(IngestOutboxReplayRequest {
                    connector_id: "orders".to_string(),
                    consumer_id: Some("projection-a".to_string()),
                    max_items: Some(10),
                    acknowledge: Some(true),
                }),
            ))
            .expect("second replay")
            .0;
        assert_eq!(second_replay.delivered_count, 0);
        assert_eq!(second_replay.delivery_state, "already_acknowledged");

        let independent_consumer = runtime
            .block_on(ingest_outbox_replay(
                State(state),
                headers,
                Json(IngestOutboxReplayRequest {
                    connector_id: "orders".to_string(),
                    consumer_id: Some("projection-b".to_string()),
                    max_items: Some(10),
                    acknowledge: Some(true),
                }),
            ))
            .expect("independent replay")
            .0;
        assert_eq!(independent_consumer.delivered_count, 2);
    }

    // â”€â”€ WS3 HTAP Routing Policy Tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    #[test]
    fn ws3_sql_route_identifies_point_select_oltp_path() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_route(
                State(state),
                headers,
                Json(SqlRouteRequest {
                    sql_batch: "SELECT * FROM orders WHERE amount > 1000;".to_string(),
                }),
            ))
            .expect("sql route response");

        assert_eq!(response.status, "ok");
        assert_eq!(response.route_path, "oltp");
        assert!(response.reason.contains("point-select") || response.reason.contains("transactional"));
    }

    #[test]
    fn ws3_sql_route_identifies_analytical_select_olap_path() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_route(
                State(state),
                headers,
                Json(SqlRouteRequest {
                    sql_batch: "SELECT region, SUM(amount) FROM orders GROUP BY region;".to_string(),
                }),
            ))
            .expect("sql route response");

        assert_eq!(response.status, "ok");
        assert_eq!(response.route_path, "olap");
        assert!(response.reason.contains("analytical") || response.reason.contains("workload"));
    }

    #[test]
    fn ws3_sql_route_identifies_write_oltp_path() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_route(
                State(state),
                headers,
                Json(SqlRouteRequest {
                    sql_batch: "INSERT INTO orders VALUES (1, 'acme', 999.99);".to_string(),
                }),
            ))
            .expect("sql route response");

        assert_eq!(response.status, "ok");
        assert_eq!(response.route_path, "oltp");
        assert!(response.reason.contains("transactional"));
    }

    #[test]
    fn ws3_sql_route_identifies_mixed_batch_hybrid_path() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_route(
                State(state),
                headers,
                Json(SqlRouteRequest {
                    sql_batch: "BEGIN; INSERT INTO logs VALUES (1); SELECT COUNT(*) FROM orders; COMMIT;".to_string(),
                }),
            ))
            .expect("sql route response");

        assert_eq!(response.status, "ok");
        assert_eq!(response.route_path, "hybrid");
        assert!(response.reason.contains("mixed") || response.reason.len() > 0);
    }

    #[test]
    fn ws3_sql_route_routes_multiple_point_selects_as_oltp() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_route(
                State(state),
                headers,
                Json(SqlRouteRequest {
                    sql_batch: "SELECT * FROM orders; SELECT * FROM products; SELECT * FROM customers;".to_string(),
                }),
            ))
            .expect("sql route response");

        assert_eq!(response.status, "ok");
        assert_eq!(response.route_path, "oltp");
        assert_eq!(response.statements.len(), 3);
        for statement in &response.statements {
            assert_eq!(statement.path, "oltp");
        }
    }

    #[test]
    fn ws3_sql_execute_routes_and_executes_olap_query() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_execute(
                State(state.clone()),
                headers,
                Json(SqlExecuteRequest {
                    sql_batch: "SELECT COUNT(*) FROM orders;".to_string(),
                    max_rows: Some(100),
                }),
            ))
            .expect("sql execute response");

        assert_eq!(response.0, StatusCode::OK);
        assert_eq!(response.1.status, "ok");
        assert_eq!(response.1.route_path, "olap");
        assert!(response.1.olap.is_some());
        assert_eq!(response.1.transaction, None);

        let audit_events = state.audit_sink.lock().expect("audit lock").latest(1);
        assert_eq!(audit_events[0].kind, AuditEventKind::Sql);
        assert!(audit_events[0].details_json.contains("sql/execute"));
    }

    #[test]
    fn ws3_sql_execute_routes_and_executes_oltp_transaction() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("admin-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_execute(
                State(state.clone()),
                headers,
                Json(SqlExecuteRequest {
                    sql_batch: "BEGIN; UPDATE orders SET amount = 1500 WHERE id = 1; COMMIT;".to_string(),
                    max_rows: Some(10),
                }),
            ))
            .expect("sql execute response");

        assert_eq!(response.0, StatusCode::OK);
        assert_eq!(response.1.status, "ok");
        assert_eq!(response.1.route_path, "oltp");
        assert!(response.1.transaction.is_some());
        assert!(response.1.transaction.as_ref().unwrap().status.contains("commit"));
    }

    #[test]
    fn ws3_sql_route_rejects_unknown_or_invalid_statements() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_route(
                State(state.clone()),
                headers,
                Json(SqlRouteRequest {
                    sql_batch: "INVALID SYNTAX HERE;".to_string(),
                }),
            ))
            .expect("sql route response");

        assert_eq!(response.status, "ok");
        assert_eq!(response.route_path, "unknown");
    }

    #[test]
    fn ws3_routing_policy_enforces_max_rows_limit() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_execute(
                State(state),
                headers,
                Json(SqlExecuteRequest {
                    sql_batch: "SELECT COUNT(*) FROM orders;".to_string(),
                    max_rows: Some(50),
                }),
            ))
            .expect("sql execute response");

        assert_eq!(response.0, StatusCode::OK);
        if let Some(olap) = response.1.olap.as_ref() {
            assert!(olap.rows <= 10_000.min(50));
        }
    }

    #[test]
    fn ws3_sql_analyze_classifies_statement_kinds_for_routing() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_analyze(
                State(state),
                headers,
                Json(SqlAnalyzeRequest {
                    sql_batch: "SELECT 1; INSERT INTO t VALUES (1); UPDATE t SET x = 2; DELETE FROM t;".to_string(),
                }),
            ))
            .expect("sql analyze response");

        assert_eq!(response.status, "ok");
        assert_eq!(response.total_statements, 4);
        assert_eq!(response.rejected_statements, 0);
        
        let analyzed = &response.statements;
        assert_eq!(analyzed[0].kind, "Select");
        assert!(!analyzed[0].requires_transaction);
        assert_eq!(analyzed[1].kind, "Insert");
        assert!(analyzed[1].requires_transaction);
        assert_eq!(analyzed[2].kind, "Update");
        assert!(analyzed[2].requires_transaction);
        assert_eq!(analyzed[3].kind, "Delete");
        assert!(analyzed[3].requires_transaction);
    }

    #[test]
    fn nt_s2_003_sql_analyze_gateway_wrapper_preserves_http_payload() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let req = SqlAnalyzeRequest {
            sql_batch: "SELECT 1; UPDATE t SET x = 2;".to_string(),
        };

        let handler_response = runtime
            .block_on(sql_analyze(State(state.clone()), headers.clone(), Json(req.clone())))
            .expect("sql analyze response");

        let dispatcher = CommandDispatcher::new();
        let envelope = build_http_envelope(
            &headers,
            CanonicalCommandName::SqlAnalyze,
            req,
            "http-sql-analyze-test",
        );
        let canonical = dispatcher.dispatch_sql_analyze(&envelope);

        assert_eq!(canonical.payload.status, handler_response.status);
        assert_eq!(canonical.payload.total_statements, handler_response.total_statements);
        assert_eq!(
            canonical.payload.rejected_statements,
            handler_response.rejected_statements
        );
        assert_eq!(
            canonical.payload.statements.len(),
            handler_response.statements.len()
        );
    }

    #[test]
    fn ws3_routing_policy_distributes_concurrent_queries() {
        let state = state_with_key(None);
        let headers1 = tenant_user_headers("analyst-acme", "acme");
        let headers2 = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let handle1 = {
            let state_clone = state.clone();
            let headers_clone = headers1.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("runtime");
                rt.block_on(sql_route(
                    State(state_clone),
                    headers_clone,
                    Json(SqlRouteRequest {
                        sql_batch: "SELECT * FROM orders;".to_string(),
                    }),
                ))
            })
        };

        let response = runtime
            .block_on(sql_route(
                State(state.clone()),
                headers2,
                Json(SqlRouteRequest {
                    sql_batch: "SELECT * FROM products;".to_string(),
                }),
            ))
            .expect("sql route response");

        assert_eq!(response.status, "ok");
        assert_eq!(response.route_path, "oltp");

        let result = handle1.join().expect("thread join").expect("thread route call");
        assert_eq!(result.status, "ok");
        assert_eq!(result.route_path, "oltp");
    }

    #[test]
    fn nt_s2_003_sql_route_gateway_wrapper_preserves_http_payload() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let req = SqlRouteRequest {
            sql_batch: "SELECT * FROM orders;".to_string(),
        };

        let handler_response = runtime
            .block_on(sql_route(State(state.clone()), headers.clone(), Json(req.clone())))
            .expect("sql route response");

        let dispatcher = CommandDispatcher::new();
        let envelope = build_http_envelope(
            &headers,
            CanonicalCommandName::SqlRoute,
            req,
            "http-sql-route-test",
        );
        let canonical = dispatcher.dispatch_sql_route(&envelope);

        assert_eq!(canonical.payload.status, handler_response.status);
        assert_eq!(canonical.payload.route_path, handler_response.route_path);
        assert_eq!(canonical.payload.reason, handler_response.reason);
        assert_eq!(
            canonical.payload.statements.len(),
            handler_response.statements.len()
        );
    }

    #[test]
    fn nt_s2_003_sql_execute_route_decision_wrapper_preserves_routing_result() {
        let headers = tenant_user_headers("analyst-acme", "acme");
        let req = SqlExecuteRequest {
            sql_batch: "SELECT * FROM orders WHERE id = '1';".to_string(),
            max_rows: Some(25),
        };

        let envelope = build_http_envelope(
            &headers,
            CanonicalCommandName::SqlExecute,
            req.clone(),
            "http-sql-execute-test",
        );
        let dispatcher = CommandDispatcher::new();
        let wrapped_decision = dispatcher.dispatch_sql_execute_route_decision(&envelope);
        let direct_decision = HtapQueryRouter::route_batch(&req.sql_batch);

        assert_eq!(wrapped_decision.payload.path, direct_decision.path);
        assert_eq!(wrapped_decision.payload.reason, direct_decision.reason);
        assert_eq!(
            wrapped_decision.payload.statements.len(),
            direct_decision.statements.len()
        );
    }

    #[test]
    fn nt_s2_003_sql_transaction_context_wrapper_preserves_payload() {
        let headers = tenant_user_headers("analyst-acme", "acme");
        let req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO orders VALUES (1)".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: Some("serializable".to_string()),
        };

        let envelope = build_http_envelope(
            &headers,
            CanonicalCommandName::SqlTransaction,
            req.clone(),
            "http-sql-transaction-test",
        );
        let dispatcher = CommandDispatcher::new();
        let wrapped_context = dispatcher.dispatch_sql_transaction_context(&envelope);

        assert_eq!(wrapped_context.payload.statements, req.statements);
        assert_eq!(
            wrapped_context.payload.isolation_level.as_deref(),
            Some("serializable")
        );
        assert_eq!(wrapped_context.request_id, "http-sql-transaction-test");
    }

    #[test]
    fn nt_s2_003_native_adapter_maps_command_frame_to_canonical_envelope() {
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-req-1".to_string(),
            session_id: Some("sess-native-1".to_string()),
            command: Some(NativeCommandKind::SqlAnalyze),
            payload_json: None,
        };
        let payload = SqlAnalyzeRequest {
            sql_batch: "SELECT 1;".to_string(),
        };

        let canonical =
            NativeAdapter::from_command_frame(&frame, CanonicalCommandName::SqlAnalyze, payload)
                .expect("native command frame should map to canonical envelope");

        assert_eq!(canonical.request_id, "native-req-1");
        assert_eq!(canonical.transport, TransportKind::Native);
        assert_eq!(canonical.command, CanonicalCommandName::SqlAnalyze);
        assert_eq!(canonical.session_context.as_deref(), Some("sess-native-1"));
        assert_eq!(
            canonical
                .transport_metadata
                .get("protocol")
                .map(String::as_str),
            Some("native")
        );
    }

    #[test]
    fn nt_s2_003_native_adapter_maps_canonical_error_to_error_frame() {
        let error = CanonicalError {
            request_id: "native-req-err-1".to_string(),
            transport: TransportKind::Native,
            kind: "protocol",
            message: "bad frame".to_string(),
        };

        let frame = NativeAdapter::error_to_error_frame(&error);
        assert_eq!(frame.frame_type, NativeFrameType::Error);
        assert_eq!(frame.request_id, "native-req-err-1");
        let payload = frame.payload_json.expect("error payload expected");
        assert_eq!(payload.get("kind").and_then(|v| v.as_str()), Some("protocol"));
        assert_eq!(payload.get("message").and_then(|v| v.as_str()), Some("bad frame"));
    }

    #[test]
    fn nt_s2_003_native_health_dispatch_roundtrip_produces_result_frame() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-health-1".to_string(),
            session_id: Some("native-session-1".to_string()),
            command: Some(NativeCommandKind::Health),
            payload_json: None,
        };

        let result_frame = NativeAdapter::dispatch_health_frame(&frame, &state, &dispatcher)
            .expect("native health dispatch should succeed");

        assert_eq!(result_frame.frame_type, NativeFrameType::Result);
        assert_eq!(result_frame.request_id, "native-health-1");
        let payload = result_frame.payload_json.expect("result payload expected");
        assert_eq!(payload.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert_eq!(
            payload.get("node_id").and_then(|v| v.as_str()),
            Some(state.node_id.as_str())
        );
    }

    #[test]
    fn nt_s2_003_native_sql_analyze_dispatch_roundtrip_produces_result_frame() {
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-analyze-1".to_string(),
            session_id: Some("native-session-2".to_string()),
            command: Some(NativeCommandKind::SqlAnalyze),
            payload_json: Some(json!({
                "sql_batch": "SELECT 1; UPDATE t SET x = 2;"
            })),
        };

        let result_frame =
            NativeAdapter::dispatch_sql_analyze_frame(&frame, &dispatcher)
                .expect("native sql.analyze dispatch should succeed");

        assert_eq!(result_frame.frame_type, NativeFrameType::Result);
        assert_eq!(result_frame.request_id, "native-analyze-1");
        let payload = result_frame.payload_json.expect("result payload expected");
        assert_eq!(payload.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert_eq!(
            payload
                .get("total_statements")
                .and_then(|v| v.as_u64()),
            Some(2)
        );
    }

    #[test]
    fn nt_s2_003_native_sql_analyze_dispatch_rejects_missing_payload() {
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-analyze-err-1".to_string(),
            session_id: Some("native-session-err".to_string()),
            command: Some(NativeCommandKind::SqlAnalyze),
            payload_json: None,
        };

        let err = NativeAdapter::dispatch_sql_analyze_frame(&frame, &dispatcher)
            .expect_err("missing payload should error");
        assert_eq!(err.kind, "protocol");
        assert!(err.message.contains("missing payload"));
    }

    #[test]
    fn nt_s2_003_native_sql_route_dispatch_roundtrip_produces_result_frame() {
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-route-1".to_string(),
            session_id: Some("native-session-route".to_string()),
            command: Some(NativeCommandKind::SqlRoute),
            payload_json: Some(json!({
                "sql_batch": "SELECT * FROM orders;"
            })),
        };

        let result_frame = NativeAdapter::dispatch_sql_route_frame(&frame, &dispatcher)
            .expect("native sql.route dispatch should succeed");

        assert_eq!(result_frame.frame_type, NativeFrameType::Result);
        assert_eq!(result_frame.request_id, "native-route-1");
        let payload = result_frame.payload_json.expect("result payload expected");
        assert_eq!(payload.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert!(payload.get("route_path").and_then(|v| v.as_str()).is_some());
    }

    #[test]
    fn nt_s2_003_native_sql_route_dispatch_rejects_invalid_payload() {
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-route-err-1".to_string(),
            session_id: Some("native-session-route-err".to_string()),
            command: Some(NativeCommandKind::SqlRoute),
            payload_json: Some(json!({
                "unexpected": "shape"
            })),
        };

        let err = NativeAdapter::dispatch_sql_route_frame(&frame, &dispatcher)
            .expect_err("invalid payload should error");
        assert_eq!(err.kind, "serialization");
        assert!(err.message.contains("invalid sql.route payload"));
    }

    #[test]
    fn nt_s2_003_native_sql_execute_route_decision_dispatch_roundtrip_produces_result_frame() {
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-execute-1".to_string(),
            session_id: Some("native-session-execute".to_string()),
            command: Some(NativeCommandKind::SqlExecute),
            payload_json: Some(json!({
                "sql_batch": "SELECT * FROM orders WHERE id = '1';",
                "max_rows": 50
            })),
        };

        let result_frame =
            NativeAdapter::dispatch_sql_execute_route_decision_frame(&frame, &dispatcher)
                .expect("native sql.execute route decision dispatch should succeed");

        assert_eq!(result_frame.frame_type, NativeFrameType::Result);
        assert_eq!(result_frame.request_id, "native-execute-1");
        let payload = result_frame.payload_json.expect("result payload expected");
        assert!(payload.get("path").is_some());
        assert!(payload.get("reason").is_some());
        assert!(payload.get("statements").is_some());
    }

    #[test]
    fn nt_s2_003_native_sql_execute_route_decision_dispatch_rejects_invalid_payload() {
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-execute-err-1".to_string(),
            session_id: Some("native-session-execute-err".to_string()),
            command: Some(NativeCommandKind::SqlExecute),
            payload_json: Some(json!({
                "invalid": "shape"
            })),
        };

        let err =
            NativeAdapter::dispatch_sql_execute_route_decision_frame(&frame, &dispatcher)
                .expect_err("invalid payload should error");
        assert_eq!(err.kind, "serialization");
        assert!(err.message.contains("invalid sql.execute payload"));
    }

    #[test]
    fn nt_s2_003_native_sql_transaction_context_dispatch_roundtrip_produces_result_frame() {
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-tx-1".to_string(),
            session_id: Some("native-session-tx".to_string()),
            command: Some(NativeCommandKind::SqlTransaction),
            payload_json: Some(json!({
                "statements": ["BEGIN", "UPDATE t SET x = 1", "COMMIT"],
                "isolation_level": "serializable"
            })),
        };

        let result_frame =
            NativeAdapter::dispatch_sql_transaction_context_frame(&frame, &dispatcher)
                .expect("native sql.transaction context dispatch should succeed");

        assert_eq!(result_frame.frame_type, NativeFrameType::Result);
        assert_eq!(result_frame.request_id, "native-tx-1");
        let payload = result_frame.payload_json.expect("result payload expected");
        assert_eq!(
            payload
                .get("statement_count")
                .and_then(|v| v.as_u64()),
            Some(3)
        );
        assert_eq!(
            payload
                .get("isolation_level")
                .and_then(|v| v.as_str()),
            Some("serializable")
        );
    }

    #[test]
    fn nt_s2_003_native_sql_transaction_context_dispatch_rejects_invalid_payload() {
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-tx-err-1".to_string(),
            session_id: Some("native-session-tx-err".to_string()),
            command: Some(NativeCommandKind::SqlTransaction),
            payload_json: Some(json!({
                "invalid": "shape"
            })),
        };

        let err =
            NativeAdapter::dispatch_sql_transaction_context_frame(&frame, &dispatcher)
                .expect_err("invalid payload should error");
        assert_eq!(err.kind, "serialization");
        assert!(err.message.contains("invalid sql.transaction payload"));
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_rejects_missing_command_with_error_frame() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-missing-command-1".to_string(),
            session_id: Some("native-session-missing-command".to_string()),
            command: None,
            payload_json: Some(json!({ "sql_batch": "SELECT 1;" })),
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);

        assert_eq!(result.frame_type, NativeFrameType::Error);
        assert_eq!(result.request_id, "native-missing-command-1");
        let payload = result.payload_json.expect("error payload expected");
        assert_eq!(payload.get("kind").and_then(|v| v.as_str()), Some("protocol"));
        assert!(
            payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .contains("missing command")
        );
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_rejects_unknown_command_with_error_frame() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-unknown-command-1".to_string(),
            session_id: Some("native-session-unknown-command".to_string()),
            command: Some(NativeCommandKind::Unknown),
            payload_json: Some(json!({ "noop": true })),
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);

        assert_eq!(result.frame_type, NativeFrameType::Error);
        assert_eq!(result.request_id, "native-unknown-command-1");
        let payload = result.payload_json.expect("error payload expected");
        assert_eq!(payload.get("kind").and_then(|v| v.as_str()), Some("protocol"));
        assert_eq!(
            payload.get("message").and_then(|v| v.as_str()),
            Some("unsupported native command: unknown")
        );
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_rejects_non_command_frame_with_error_frame() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Ping,
            request_id: "native-non-command-1".to_string(),
            session_id: Some("native-session-non-command".to_string()),
            command: Some(NativeCommandKind::Health),
            payload_json: None,
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);

        assert_eq!(result.frame_type, NativeFrameType::Error);
        assert_eq!(result.request_id, "native-non-command-1");
        let payload = result.payload_json.expect("error payload expected");
        assert_eq!(payload.get("kind").and_then(|v| v.as_str()), Some("protocol"));
        assert_eq!(
            payload.get("message").and_then(|v| v.as_str()),
            Some("expected COMMAND frame for native dispatch")
        );
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_routes_health_to_result_frame() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-dispatch-health-1".to_string(),
            session_id: Some("native-session-dispatch-health".to_string()),
            command: Some(NativeCommandKind::Health),
            payload_json: None,
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);

        assert_eq!(result.frame_type, NativeFrameType::Result);
        assert_eq!(result.request_id, "native-dispatch-health-1");
        let payload = result.payload_json.expect("result payload expected");
        assert_eq!(payload.get("status").and_then(|v| v.as_str()), Some("ok"));
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_routes_ingest_schema_registry_to_result_frame() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-ingest-schema-1".to_string(),
            session_id: Some("native-session-ingest-schema".to_string()),
            command: Some(NativeCommandKind::IngestSchemaRegistry),
            payload_json: None,
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);

        assert_eq!(result.frame_type, NativeFrameType::Result);
        assert_eq!(result.request_id, "native-ingest-schema-1");
        let payload = result.payload_json.expect("result payload expected");
        assert_eq!(payload.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert!(payload.get("connector_count").is_some());
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_routes_sql_analyze_to_result_frame() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-dispatch-analyze-1".to_string(),
            session_id: Some("native-session-dispatch-analyze".to_string()),
            command: Some(NativeCommandKind::SqlAnalyze),
            payload_json: Some(json!({
                "sql_batch": "SELECT 1; SELECT 2;"
            })),
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);

        assert_eq!(result.frame_type, NativeFrameType::Result);
        assert_eq!(result.request_id, "native-dispatch-analyze-1");
        let payload = result.payload_json.expect("result payload expected");
        assert_eq!(payload.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert_eq!(
            payload.get("total_statements").and_then(|v| v.as_u64()),
            Some(2)
        );
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_normalizes_handler_serialization_error() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-dispatch-serialization-1".to_string(),
            session_id: Some("native-session-dispatch-serialization".to_string()),
            command: Some(NativeCommandKind::SqlAnalyze),
            payload_json: Some(json!({
                "invalid": "shape"
            })),
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);

        assert_eq!(result.frame_type, NativeFrameType::Error);
        assert_eq!(result.request_id, "native-dispatch-serialization-1");
        let payload = result.payload_json.expect("error payload expected");
        assert_eq!(payload.get("kind").and_then(|v| v.as_str()), Some("serialization"));
        assert!(
            payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .contains("invalid sql.analyze payload")
        );
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_routes_sql_route_to_result_frame() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-dispatch-route-1".to_string(),
            session_id: Some("native-session-dispatch-route".to_string()),
            command: Some(NativeCommandKind::SqlRoute),
            payload_json: Some(json!({
                "sql_batch": "SELECT * FROM t;"
            })),
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);
        assert_eq!(result.frame_type, NativeFrameType::Result);
        assert_eq!(result.request_id, "native-dispatch-route-1");
        let payload = result.payload_json.expect("result payload expected");
        assert_eq!(payload.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert!(payload.get("route_path").is_some());
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_routes_sql_execute_to_result_frame() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-dispatch-execute-1".to_string(),
            session_id: Some("native-session-dispatch-execute".to_string()),
            command: Some(NativeCommandKind::SqlExecute),
            payload_json: Some(json!({
                "sql_batch": "SELECT 1;",
                "max_rows": 10
            })),
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);
        assert_eq!(result.frame_type, NativeFrameType::Result);
        assert_eq!(result.request_id, "native-dispatch-execute-1");
        let payload = result.payload_json.expect("result payload expected");
        assert!(payload.get("path").is_some());
        assert!(payload.get("reason").is_some());
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_routes_sql_transaction_to_result_frame() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-dispatch-tx-1".to_string(),
            session_id: Some("native-session-dispatch-tx".to_string()),
            command: Some(NativeCommandKind::SqlTransaction),
            payload_json: Some(json!({
                "statements": ["BEGIN", "SELECT 1", "COMMIT"],
                "isolation_level": "read_committed"
            })),
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);
        assert_eq!(result.frame_type, NativeFrameType::Result);
        assert_eq!(result.request_id, "native-dispatch-tx-1");
        let payload = result.payload_json.expect("result payload expected");
        assert_eq!(
            payload.get("statement_count").and_then(|v| v.as_u64()),
            Some(3)
        );
        assert_eq!(
            payload.get("isolation_level").and_then(|v| v.as_str()),
            Some("read_committed")
        );
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_normalizes_sql_route_protocol_error() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-dispatch-route-protocol-1".to_string(),
            session_id: Some("native-session-dispatch-route-protocol".to_string()),
            command: Some(NativeCommandKind::SqlRoute),
            payload_json: None,
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);
        assert_eq!(result.frame_type, NativeFrameType::Error);
        assert_eq!(result.request_id, "native-dispatch-route-protocol-1");
        let payload = result.payload_json.expect("error payload expected");
        assert_eq!(payload.get("kind").and_then(|v| v.as_str()), Some("protocol"));
        assert!(
            payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .contains("missing payload for sql.route frame")
        );
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_normalizes_sql_execute_serialization_error() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-dispatch-execute-serialization-1".to_string(),
            session_id: Some("native-session-dispatch-execute-serialization".to_string()),
            command: Some(NativeCommandKind::SqlExecute),
            payload_json: Some(json!({
                "invalid": true
            })),
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);
        assert_eq!(result.frame_type, NativeFrameType::Error);
        assert_eq!(result.request_id, "native-dispatch-execute-serialization-1");
        let payload = result.payload_json.expect("error payload expected");
        assert_eq!(payload.get("kind").and_then(|v| v.as_str()), Some("serialization"));
        assert!(
            payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .contains("invalid sql.execute payload")
        );
    }

    #[test]
    fn nt_s2_003_native_dispatch_frame_normalizes_sql_transaction_protocol_error() {
        let state = state_with_key(None);
        let dispatcher = CommandDispatcher::new();
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            request_id: "native-dispatch-tx-protocol-1".to_string(),
            session_id: Some("native-session-dispatch-tx-protocol".to_string()),
            command: Some(NativeCommandKind::SqlTransaction),
            payload_json: None,
        };

        let result = NativeAdapter::dispatch_frame(&frame, &state, &dispatcher);
        assert_eq!(result.frame_type, NativeFrameType::Error);
        assert_eq!(result.request_id, "native-dispatch-tx-protocol-1");
        let payload = result.payload_json.expect("error payload expected");
        assert_eq!(payload.get("kind").and_then(|v| v.as_str()), Some("protocol"));
        assert!(
            payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .contains("missing payload for sql.transaction frame")
        );
    }

    // â”€â”€ REQ-07: parallel / chunked ingest loading KPI tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[test]
    fn ws4_chunked_load_produces_correct_chunk_count() {
        use voltnuerongrid_ingest::chunked_loader::load_records_chunked;
        use voltnuerongrid_ingest::batch_config::IngestParallelConfig;
        use voltnuerongrid_ingest::IngestRecord;

        let records: Vec<IngestRecord> = (0..35)
            .map(|i| IngestRecord { key: format!("k{i}"), payload: format!("v{i}") })
            .collect();

        let cfg = IngestParallelConfig { max_in_flight_tasks: 4, chunk_target_rows: 10 };
        let stats = load_records_chunked(&records, &cfg);

        // 35 / 10 = 4 chunks (10+10+10+5)
        assert_eq!(stats.total_records, 35);
        assert_eq!(stats.chunk_count, 4);
        assert_eq!(stats.outcomes.len(), 4);
        assert_eq!(stats.outcomes[3].records_in_chunk, 5);
    }

    #[test]
    fn ws4_chunked_load_tasks_dispatched_honours_in_flight_cap() {
        use voltnuerongrid_ingest::chunked_loader::load_records_chunked;
        use voltnuerongrid_ingest::batch_config::IngestParallelConfig;
        use voltnuerongrid_ingest::IngestRecord;

        let records: Vec<IngestRecord> = (0..100)
            .map(|i| IngestRecord { key: format!("k{i}"), payload: format!("v{i}") })
            .collect();

        // 100 records / 10 per chunk = 10 chunks; only 3 in-flight at a time
        let cfg = IngestParallelConfig { max_in_flight_tasks: 3, chunk_target_rows: 10 };
        let stats = load_records_chunked(&records, &cfg);

        assert_eq!(stats.chunk_count, 10);
        assert_eq!(stats.tasks_dispatched, 3); // capped at max_in_flight_tasks
        assert_eq!(stats.total_records, 100);
        // All chunks still appear in outcomes even across multiple waves
        assert_eq!(stats.outcomes.len(), 10);
    }

    #[test]
    fn ws4_chunked_load_empty_payload_is_safe() {
        use voltnuerongrid_ingest::chunked_loader::load_records_chunked;
        use voltnuerongrid_ingest::batch_config::IngestParallelConfig;

        let cfg = IngestParallelConfig::default();
        let stats = load_records_chunked(&[], &cfg);

        assert_eq!(stats.total_records, 0);
        assert_eq!(stats.chunk_count, 0);
        assert_eq!(stats.tasks_dispatched, 0);
        assert!(stats.outcomes.is_empty());
    }

    #[test]
    fn ws4_chunked_load_single_chunk_within_target() {
        use voltnuerongrid_ingest::chunked_loader::load_records_chunked;
        use voltnuerongrid_ingest::batch_config::IngestParallelConfig;
        use voltnuerongrid_ingest::IngestRecord;

        let records: Vec<IngestRecord> = (0..7)
            .map(|i| IngestRecord { key: format!("k{i}"), payload: format!("v{i}") })
            .collect();

        let cfg = IngestParallelConfig { max_in_flight_tasks: 4, chunk_target_rows: 10 };
        let stats = load_records_chunked(&records, &cfg);

        assert_eq!(stats.chunk_count, 1);
        assert_eq!(stats.tasks_dispatched, 1);
        assert_eq!(stats.outcomes[0].records_in_chunk, 7);
    }

    // REQ-07: chunked HTTP endpoint integration tests
    #[test]
    fn ws4_chunked_http_endpoint_stores_records() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let req = IngestChunkedRequest {
            connector_id: "chunked-connector-1".to_string(),
            records: vec![
                r#"{"id":1,"val":"alpha"}"#.to_string(),
                r#"{"id":2,"val":"beta"}"#.to_string(),
                r#"{"id":3,"val":"gamma"}"#.to_string(),
            ],
            chunk_target_rows: Some(2),
            max_in_flight_tasks: Some(2),
        };
        let response = rt
            .block_on(ingest_chunked(State(state.clone()), headers, Json(req)))
            .expect("chunked ingest should succeed");
        assert_eq!(response.0, StatusCode::OK);
        assert_eq!(response.1.status, "ok");
        assert_eq!(response.1.total_records, 3);
        // Verify records were persisted in json store
        let json_map = state.ingest_json_records.lock().unwrap();
        let stored = json_map.values().next().expect("should have stored records");
        assert_eq!(stored.len(), 3, "all 3 records should be in the store");
    }

    #[test]
    fn ws4_chunked_http_endpoint_empty_records_is_safe() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let req = IngestChunkedRequest {
            connector_id: "chunked-empty".to_string(),
            records: vec![],
            chunk_target_rows: None,
            max_in_flight_tasks: None,
        };
        let response = rt
            .block_on(ingest_chunked(State(state.clone()), headers, Json(req)))
            .expect("empty chunked ingest should be safe");
        assert_eq!(response.0, StatusCode::OK);
        assert_eq!(response.1.total_records, 0);
    }

    // â”€â”€ REQ-12: legacy aggregate routing through sql_execute â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[test]
    fn ws3_legacy_agg_sum_routed_through_sql_execute_olap_path() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let (status, Json(body)) = runtime
            .block_on(sql_execute(
                State(state),
                headers,
                Json(SqlExecuteRequest {
                    sql_batch: "SELECT SUM(amount) FROM orders;".to_string(),
                    max_rows: Some(100),
                }),
            ))
            .expect("sql execute should succeed");

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.route_path, "olap");

        let agg_results = body.legacy_agg_results
            .expect("SUM should produce legacy_agg_results");
        assert!(!agg_results.is_empty());
        assert_eq!(agg_results[0].aggregation, "SUM");
        assert!(agg_results[0].result.is_some());
        assert_eq!(agg_results[0].source, "legacy_agg_olap_path");
    }

    #[test]
    fn ws3_legacy_agg_count_and_avg_detected_together() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let (status, Json(body)) = runtime
            .block_on(sql_execute(
                State(state),
                headers,
                Json(SqlExecuteRequest {
                    sql_batch: "SELECT COUNT(id), AVG(price) FROM products;".to_string(),
                    max_rows: None,
                }),
            ))
            .expect("sql execute should succeed");

        assert_eq!(status, StatusCode::OK);
        let agg_results = body.legacy_agg_results
            .expect("COUNT + AVG should produce legacy_agg_results");
        assert!(agg_results.iter().any(|r| r.aggregation == "COUNT"));
        assert!(agg_results.iter().any(|r| r.aggregation == "AVG"));
    }

    #[test]
    fn ws3_legacy_agg_none_when_no_aggregate_in_select() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_execute(
                State(state),
                headers,
                Json(SqlExecuteRequest {
                    sql_batch: "SELECT id, name FROM orders;".to_string(),
                    max_rows: Some(50),
                }),
            ))
            .expect("sql execute should succeed");

        assert_eq!(response.0, StatusCode::OK);
        assert!(
            response.1.legacy_agg_results.is_none(),
            "plain SELECT should not produce legacy_agg_results"
        );
    }

    #[test]
    fn ws3_legacy_agg_not_emitted_for_oltp_paths() {
        let state = state_with_key(None);
        let headers = tenant_user_headers("admin-acme", "acme");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime
            .block_on(sql_execute(
                State(state),
                headers,
                Json(SqlExecuteRequest {
                    sql_batch: "INSERT INTO orders (id, amount) VALUES (99, 500);".to_string(),
                    max_rows: None,
                }),
            ))
            .expect("sql execute should succeed");

        assert_eq!(response.0, StatusCode::OK);
        // INSERT goes to OLTP path; no OLAP SELECT â†’ no legacy_agg_results
        assert!(response.1.legacy_agg_results.is_none());
    }

    // â”€â”€ REQ-02: DDL catalog tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    #[test]
    fn ws2_ddl_catalog_create_table_wires_through_sql_execute() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let req = SqlExecuteRequest {
            sql_batch: "CREATE TABLE orders (id INT, amount FLOAT)".to_string(),
            max_rows: None,
        };
        let response = rt
            .block_on(sql_execute(
                State(state.clone()),
                headers.clone(),
                Json(req),
            ))
            .expect("sql execute should succeed");
        assert_eq!(response.0, StatusCode::OK);
        // The catalog should now have the entry (touches_catalog = true for CREATE TABLE)
        let catalog = state.ddl_catalog.lock().unwrap();
        assert_eq!(catalog.active_count(), 1);
        let entries = catalog.active_entries();
        assert_eq!(entries[0].object_name, "orders");
        assert_eq!(entries[0].object_kind, "table");
    }

    #[test]
    fn ws2_ddl_catalog_drop_table_removes_active_entry() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        // Create then drop via sql_execute
        let create_req = SqlExecuteRequest {
            sql_batch: "CREATE TABLE temp_data (x INT)".to_string(),
            max_rows: None,
        };
        rt.block_on(sql_execute(State(state.clone()), headers.clone(), Json(create_req)))
            .expect("create should succeed");
        {
            let catalog = state.ddl_catalog.lock().unwrap();
            assert_eq!(catalog.active_count(), 1, "table should be active after create");
        }
        let drop_req = SqlExecuteRequest {
            sql_batch: "DROP TABLE temp_data".to_string(),
            max_rows: None,
        };
        rt.block_on(sql_execute(State(state.clone()), headers.clone(), Json(drop_req)))
            .expect("drop should succeed");
        let catalog = state.ddl_catalog.lock().unwrap();
        assert_eq!(catalog.active_count(), 0, "table should be gone after drop");
        assert_eq!(catalog.total_count(), 1, "total should include dropped entry");
    }

    #[test]
    fn ws2_catalog_table_columns_returns_columns_for_created_table() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let tenant_headers = tenant_user_headers("admin-acme", "acme");

        let create_req = SqlExecuteRequest {
            sql_batch: "CREATE TABLE orders (id INT, amount FLOAT)".to_string(),
            max_rows: None,
        };
        let _ = rt.block_on(sql_execute(State(state.clone()), tenant_headers.clone(), Json(create_req)))
            .expect("create should succeed");

        let response = rt
            .block_on(catalog_table_columns(
                State(state.clone()),
                Path("orders".to_string()),
                tenant_headers,
            ))
            .expect("catalog_table_columns should succeed");

        assert_eq!(response.0, StatusCode::OK);
        let body = response.1.0;
        assert_eq!(body.status, "ok");
        assert_eq!(body.table_name.to_ascii_lowercase(), "orders");
        assert_eq!(body.columns.len(), 2);
        assert_eq!(body.columns[0].name.to_ascii_lowercase(), "id");
        assert_eq!(body.columns[1].name.to_ascii_lowercase(), "amount");
    }

    #[test]
    fn ws2_catalog_table_columns_requires_auth() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = HeaderMap::new();

        let result = rt.block_on(catalog_table_columns(
            State(state),
            Path("orders".to_string()),
            headers,
        ));

        match result {
            Ok(_) => panic!("expected auth error"),
            Err((status, _)) => assert_eq!(status, StatusCode::UNAUTHORIZED),
        }
    }

    #[test]
    fn ws2_admin_schema_tree_returns_views_functions_triggers_and_events() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let tenant_headers = tenant_user_headers("admin-acme", "acme");

        for sql in [
            "CREATE VIEW order_summary AS SELECT * FROM orders",
            "CREATE FUNCTION compute_tax(x FLOAT) RETURNS FLOAT LANGUAGE sql AS $$ SELECT x $$",
            "CREATE TRIGGER orders_audit AFTER INSERT ON orders FOR EACH ROW EXECUTE FUNCTION audit_orders()",
            "CREATE EVENT refresh_cache ON SCHEDULE EVERY 1 HOUR DO CALL warm_cache()",
        ] {
            let req = SqlExecuteRequest {
                sql_batch: sql.to_string(),
                max_rows: None,
            };
            let response = rt
                .block_on(sql_execute(State(state.clone()), tenant_headers.clone(), Json(req)))
                .expect("sql execute should succeed");
            assert_eq!(response.0, StatusCode::OK);
        }

        let response = rt
            .block_on(admin_schema_tree(State(state.clone()), admin_headers("secret")))
            .expect("admin schema tree should succeed");

        assert_eq!(response.0, StatusCode::OK);
        let body = response.1.0;
        let schema = &body.databases[0].schemas[0];
        assert!(schema.views.iter().any(|view| view.name == "order_summary"));
        assert!(schema.functions.iter().any(|func| func.name == "compute_tax"));
        assert!(schema.triggers.iter().any(|trigger| trigger.name == "orders_audit" && trigger.table == "orders"));
        assert!(schema.events.iter().any(|event| event.name == "refresh_cache" && event.schedule == "EVERY 1 HOUR"));
    }

    // â”€â”€ REQ-23: ACID transaction tracking tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    #[test]
    fn ws23_acid_tx_begin_commit_tracked_in_registry() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO accounts VALUES (1, 'alice', 500.0)".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        let response = rt
            .block_on(sql_transaction(State(state.clone()), headers, Json(req)))
            .expect("transaction should succeed");
        assert_eq!(response.0, StatusCode::OK);
        let acid = state.acid_transactions.lock().unwrap();
        assert_eq!(acid.all_transactions().len(), 1, "should have 1 tracked transaction");
        let tx = acid.all_transactions()[0];
        assert!(matches!(tx.state, AcidTxState::Committed), "state should be Committed");
        assert_eq!(tx.statement_count, 3, "all 3 statements recorded");
    }

    #[test]
    fn ws23_acid_tx_rollback_tracked_in_registry() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "DELETE FROM staging WHERE id = 99".to_string(),
                "ROLLBACK".to_string(),
            ],
            isolation_level: None,
        };
        rt.block_on(sql_transaction(State(state.clone()), headers, Json(req)))
            .expect("transaction should succeed");
        let acid = state.acid_transactions.lock().unwrap();
        let tx = acid.all_transactions()[0];
        assert!(matches!(tx.state, AcidTxState::RolledBack), "state should be RolledBack");
    }

    #[test]
    fn ws23_acid_savepoint_create_and_release() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO orders VALUES (1, 'pending')".to_string(),
                "SAVEPOINT sp1".to_string(),
                "UPDATE orders SET status = 'shipped' WHERE id = 1".to_string(),
                "RELEASE SAVEPOINT sp1".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        rt.block_on(sql_transaction(State(state.clone()), headers, Json(req)))
            .expect("transaction should succeed");
        let acid = state.acid_transactions.lock().unwrap();
        let tx = acid.all_transactions()[0];
        assert!(matches!(tx.state, AcidTxState::Committed), "state should be Committed after COMMIT");
        // RELEASE SAVEPOINT removes sp1 from the list
        assert!(!tx.savepoints.contains(&"sp1".to_string()), "sp1 should be released (removed)");
    }

    #[test]
    fn ws23_acid_rollback_to_savepoint_records_marker() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO events VALUES (1, 'start')".to_string(),
                "SAVEPOINT before_risky".to_string(),
                "DELETE FROM events WHERE id = 1".to_string(),
                "ROLLBACK TO before_risky".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        rt.block_on(sql_transaction(State(state.clone()), headers, Json(req)))
            .expect("transaction should succeed");
        let acid = state.acid_transactions.lock().unwrap();
        let tx = acid.all_transactions()[0];
        assert!(matches!(tx.state, AcidTxState::Committed), "should commit after ROLLBACK TO + COMMIT");
        let has_marker = tx.savepoints.iter().any(|s| s.contains("rolled_back_to:before_risky"));
        assert!(has_marker, "rollback-to marker should be recorded in savepoints list");
    }

    // â”€â”€ REQ-23: isolation level enforcement tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    #[test]
    fn ws23_acid_isolation_level_from_request_field() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO orders VALUES (1, 'ok')".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: Some("serializable".to_string()),
        };
        rt.block_on(sql_transaction(State(state.clone()), headers, Json(req)))
            .expect("serializable transaction should succeed");
        let acid = state.acid_transactions.lock().unwrap();
        let tx = acid.all_transactions()[0];
        assert_eq!(
            tx.isolation_level, "serializable",
            "isolation_level should be stored from request"
        );
    }

    #[test]
    fn ws23_acid_serializable_conflict_returns_409() {
        // Pre-seed a concurrent serializable transaction that has already written to "inventory"
        let state = state_with_key(None);
        {
            let mut acid = state.acid_transactions.lock().unwrap();
            acid.begin("tx-concurrent", "node-1", "serializable", 1_000_u128);
            acid.record_statement("tx-concurrent", Some("inventory".to_string()));
        }
        // Now attempt a second serializable transaction writing to the same table
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let headers = tenant_user_headers("analyst-acme", "acme");
        let req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "UPDATE inventory SET qty = 0 WHERE id = 1".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: Some("serializable".to_string()),
        };
        let result = rt.block_on(sql_transaction(State(state.clone()), headers, Json(req)));
        match result {
            Err((status, _body)) => {
                assert_eq!(status, StatusCode::CONFLICT, "should return 409 on serializable conflict");
            }
            Ok((status, _body)) => {
                panic!("Expected Err 409 CONFLICT, got Ok({status:?})");
            }
        }
    }

    // â”€â”€ REQ-12: real ingest data in legacy agg â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    #[test]
    fn ws3_legacy_agg_uses_real_ingest_data_when_available() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        // Pre-populate ingest JSON store with numeric payload
        {
            let mut guard = state.ingest_json_records.lock().unwrap();
            guard.insert(
                "connector-metrics".to_string(),
                vec![
                    voltnuerongrid_ingest::IngestRecord {
                        key: "r1".to_string(),
                        payload: r#"{"value":10.0,"score":20.0}"#.to_string(),
                    },
                    voltnuerongrid_ingest::IngestRecord {
                        key: "r2".to_string(),
                        payload: r#"{"value":30.0,"score":40.0}"#.to_string(),
                    },
                ],
            );
        }
        let headers = tenant_user_headers("analyst-acme", "acme");
        let req = SqlExecuteRequest {
            sql_batch: "SELECT SUM(value) FROM metrics".to_string(),
            max_rows: None,
        };
        let response = rt
            .block_on(sql_execute(State(state), headers, Json(req)))
            .expect("sql execute should succeed");
        assert_eq!(response.0, StatusCode::OK);
        let agg_results = response.1.legacy_agg_results.as_ref().expect("should have agg results");
        let sum_entry = agg_results.iter().find(|r| r.aggregation == "SUM").expect("SUM result");
        // Real data: [10.0, 20.0, 30.0, 40.0] â†’ SUM = 100.0
        let sum_val = sum_entry.result.expect("SUM should have numeric result");
        assert!((sum_val - 100.0).abs() < 1e-9, "SUM should be 100.0, got {sum_val}");
    }

    // ------------------------------------------------------------------
    // REQ-21: Concurrency stress tests
    // ------------------------------------------------------------------

    #[test]
    fn ws21_concurrent_sql_execute_tenant_isolation() {
        // Spawn 8 threads each issuing sql_execute as the same registered tenant.
        // Verify all calls succeed without panicking or data races on shared state.
        use std::sync::Arc;

        let state = Arc::new(state_with_key(None));
        let handles: Vec<_> = (0u8..8)
            .map(|i| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().expect("runtime");
                    // Use the registered tenant; vary the SQL to avoid contention on metrics
                    let headers = tenant_user_headers("analyst-acme", "acme");
                    let req = SqlExecuteRequest {
                        sql_batch: format!("SELECT COUNT(*) FROM metrics_thread_{i}"),
                        max_rows: None,
                    };
                    let result = rt.block_on(sql_execute(
                        State((*state).clone()),
                        headers,
                        Json(req),
                    ));
                    (i, result.is_ok())
                })
            })
            .collect();

        for handle in handles {
            let (i, ok) = handle.join().expect("thread panicked");
            assert!(ok, "Thread {i} sql_execute failed");
        }
    }

    #[test]
    fn ws21_concurrent_ingest_no_data_corruption() {
        // 4 threads each insert 10 records directly into distinct ingest partitions.
        // After all threads complete, each connector must have exactly 10 records.
        use std::sync::Arc;

        let state = Arc::new(state_with_key(None));
        let handles: Vec<_> = (0u8..4)
            .map(|i| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let connector_id = format!("connector-ws21-{i}");
                    let records: Vec<voltnuerongrid_ingest::IngestRecord> = (0u8..10)
                        .map(|j| voltnuerongrid_ingest::IngestRecord {
                            key: format!("k-{i}-{j}"),
                            payload: format!(r#"{{"id":{},"thread":{}}}"#, j, i),
                        })
                        .collect();
                    state
                        .ingest_json_records
                        .lock()
                        .expect("ingest lock")
                        .insert(connector_id.clone(), records);
                    connector_id
                })
            })
            .collect();

        for handle in handles {
            let connector_id = handle.join().expect("thread panicked");
            let guard = state.ingest_json_records.lock().unwrap();
            let records = guard.get(&connector_id);
            assert!(
                records.is_some(),
                "Connector {connector_id} missing after concurrent ingest"
            );
            assert_eq!(
                records.unwrap().len(),
                10,
                "Connector {connector_id} should have 10 records"
            );
        }
    }

    #[test]
    fn ws21_concurrent_cache_set_get_no_cross_partition_leak() {
        // 4 threads each SET a key in their own cache partition, then GET it.
        // No thread should see another thread's partition data on GET.
        use std::sync::Arc;

        let state = Arc::new(state_with_key(None));
        let handles: Vec<_> = (0u8..4)
            .map(|i| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let partition_id = format!("ws21-part-{i}");
                    let key = "sensor-reading".to_string();
                    let value = serde_json::json!(i as u64 * 100);
                    let now_ms = now_unix_ms_u64();
                    {
                        let mut guard = state.distributed_cache.lock().unwrap();
                        guard
                            .set(&partition_id, key.clone(), value.clone(), None, now_ms)
                            .expect("cache set should succeed");
                    }
                    let retrieved = {
                        let mut guard = state.distributed_cache.lock().unwrap();
                        guard.get(&partition_id, &key, now_ms).unwrap()
                    };
                    assert_eq!(
                        retrieved,
                        Some(value),
                        "Partition {partition_id} should return its own value"
                    );
                    i
                })
            })
            .collect();

        let completed: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().expect("thread panicked"))
            .collect();
        assert_eq!(completed.len(), 4);
    }

    // REQ-21: concurrent ACID transactions â€” no state race on shared registry
    #[test]
    fn ws21_concurrent_acid_transactions_no_state_race() {
        // 4 threads each run a complete BEGIN/INSERT/COMMIT through distinct transactions.
        // All should succeed without panicking on the shared Mutex<AcidTransactionRegistry>.
        use std::sync::Arc;

        let state = Arc::new(state_with_key(None));
        let handles: Vec<_> = (0u8..4)
            .map(|i| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().expect("runtime");
                    let headers = tenant_user_headers("analyst-acme", "acme");
                    let req = SqlTransactionRequest {
                        statements: vec![
                            "BEGIN".to_string(),
                            format!("INSERT INTO tbl_{i} VALUES ({i}, 'data')"),
                            "COMMIT".to_string(),
                        ],
                        isolation_level: None,
                    };
                    let result = rt.block_on(sql_transaction(
                        State((*state).clone()),
                        headers,
                        Json(req),
                    ));
                    (i, result.is_ok())
                })
            })
            .collect();

        for handle in handles {
            let (i, ok) = handle.join().expect("thread panicked");
            assert!(ok, "Thread {i} acid transaction unexpectedly failed");
        }
    }

    // REQ-21: high-cardinality tenant concurrency â€” 16 concurrent sql_execute calls
    #[test]
    fn ws21_high_cardinality_tenant_sql_execute() {
        use std::sync::Arc;

        let state = Arc::new(state_with_key(None));
        let handles: Vec<_> = (0u16..16)
            .map(|i| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().expect("runtime");
                    // Use the registered tenant; differentiate by SQL query content
                    let headers = tenant_user_headers("analyst-acme", "acme");
                    let req = SqlExecuteRequest {
                        sql_batch: format!("SELECT * FROM metrics WHERE shard = {i}"),
                        max_rows: None,
                    };
                    let result = rt.block_on(sql_execute(
                        State((*state).clone()),
                        headers,
                        Json(req),
                    ));
                    (i, result.is_ok())
                })
            })
            .collect();

        for handle in handles {
            let (i, ok) = handle.join().expect("thread panicked");
            assert!(ok, "Thread {i} sql_execute unexpectedly failed");
        }
    }

    // ------------------------------------------------------------------
    // REQ-27: Redis-compat cache command endpoint tests
    // ------------------------------------------------------------------

    #[test]
    fn ws27_redis_compat_ping_returns_pong() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "automation");
        let req = RedisCacheCommandRequest {
            cmd: "PING".to_string(),
            partition_id: None,
            key: None,
            value: None,
            ttl_ms: None,
            delta: None,
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        let result = rt.block_on(cache_redis_command(State(state), headers, Json(req)));
        let response = result.expect("PING should succeed").0;
        assert_eq!(response.status, "ok");
        assert_eq!(response.value, Some(serde_json::json!("PONG")));
    }

    #[test]
    fn ws27_redis_compat_set_get_del_lifecycle() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));

        // SET
        let set_req = RedisCacheCommandRequest {
            cmd: "SET".to_string(),
            partition_id: Some("ws27-test".to_string()),
            key: Some("sensor-key".to_string()),
            value: Some(serde_json::json!(42)),
            ttl_ms: None,
            delta: None,
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        let set_result = rt
            .block_on(cache_redis_command(
                State(state.clone()),
                operator_headers("secret", "automation"),
                Json(set_req),
            ))
            .expect("SET should succeed");
        assert_eq!(set_result.0.status, "ok");

        // GET â€” should hit
        let get_req = RedisCacheCommandRequest {
            cmd: "GET".to_string(),
            partition_id: Some("ws27-test".to_string()),
            key: Some("sensor-key".to_string()),
            value: None,
            ttl_ms: None,
            delta: None,
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        let get_result = rt
            .block_on(cache_redis_command(
                State(state.clone()),
                operator_headers("secret", "automation"),
                Json(get_req),
            ))
            .expect("GET should succeed");
        assert_eq!(get_result.0.value, Some(serde_json::json!(42)));

        // EXISTS â€” should be true
        let exists_req = RedisCacheCommandRequest {
            cmd: "EXISTS".to_string(),
            partition_id: Some("ws27-test".to_string()),
            key: Some("sensor-key".to_string()),
            value: None,
            ttl_ms: None,
            delta: None,
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        let exists_result = rt
            .block_on(cache_redis_command(
                State(state.clone()),
                operator_headers("secret", "automation"),
                Json(exists_req),
            ))
            .expect("EXISTS should succeed");
        assert_eq!(exists_result.0.exists, Some(true));

        // KEYS â€” should contain sensor-key
        let keys_req = RedisCacheCommandRequest {
            cmd: "KEYS".to_string(),
            partition_id: Some("ws27-test".to_string()),
            key: None,
            value: None,
            ttl_ms: None,
            delta: None,
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        let keys_result = rt
            .block_on(cache_redis_command(
                State(state.clone()),
                operator_headers("secret", "automation"),
                Json(keys_req),
            ))
            .expect("KEYS should succeed");
        let keys = keys_result.0.keys.unwrap_or_default();
        assert!(
            keys.contains(&"sensor-key".to_string()),
            "sensor-key should appear in KEYS result"
        );

        // DEL â€” remove it
        let del_req = RedisCacheCommandRequest {
            cmd: "DEL".to_string(),
            partition_id: Some("ws27-test".to_string()),
            key: Some("sensor-key".to_string()),
            value: None,
            ttl_ms: None,
            delta: None,
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        let del_result = rt
            .block_on(cache_redis_command(
                State(state.clone()),
                operator_headers("secret", "automation"),
                Json(del_req),
            ))
            .expect("DEL should succeed");
        assert_eq!(del_result.0.removed, Some(true));

        // GET after DEL â€” should be None
        let get_after_del = RedisCacheCommandRequest {
            cmd: "GET".to_string(),
            partition_id: Some("ws27-test".to_string()),
            key: Some("sensor-key".to_string()),
            value: None,
            ttl_ms: None,
            delta: None,
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        let get_after_result = rt
            .block_on(cache_redis_command(
                State(state.clone()),
                operator_headers("secret", "automation"),
                Json(get_after_del),
            ))
            .expect("GET after DEL should succeed");
        assert_eq!(get_after_result.0.value, None);
    }

    #[test]
    fn ws27_redis_compat_flush_clears_partition() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));

        // Populate 3 keys
        for i in 0..3u8 {
            let set_req = RedisCacheCommandRequest {
                cmd: "SET".to_string(),
                partition_id: Some("ws27-flush".to_string()),
                key: Some(format!("key-{i}")),
                value: Some(serde_json::json!(i)),
                ttl_ms: None,
                delta: None,
                expire_ms: None,
            keys: None,
            start: None,
            stop: None,
            field: None,
            };
            rt.block_on(cache_redis_command(
                State(state.clone()),
                operator_headers("secret", "automation"),
                Json(set_req),
            ))
            .expect("SET should succeed");
        }

        // FLUSH
        let flush_req = RedisCacheCommandRequest {
            cmd: "FLUSH".to_string(),
            partition_id: Some("ws27-flush".to_string()),
            key: None,
            value: None,
            ttl_ms: None,
            delta: None,
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        let flush_result = rt
            .block_on(cache_redis_command(
                State(state.clone()),
                operator_headers("secret", "automation"),
                Json(flush_req),
            ))
            .expect("FLUSH should succeed");
        assert_eq!(flush_result.0.flushed_count, Some(3));
    }

    #[test]
    fn ws27_redis_compat_unsupported_command_returns_error() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let req = RedisCacheCommandRequest {
            cmd: "ZADD".to_string(),
            partition_id: None,
            key: None,
            value: None,
            ttl_ms: None,
            delta: None,
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        let result = rt.block_on(cache_redis_command(
            State(state),
            operator_headers("secret", "automation"),
            Json(req),
        ));
        let response = result.expect("handler returns Ok even for unsupported cmd").0;
        assert_eq!(response.status, "error");
        assert!(response.error.unwrap_or_default().contains("ZADD"));
    }

    // REQ-27: INCR / DECR / EXPIRE lifecycle tests
    #[test]
    fn ws27_redis_compat_incr_decr_lifecycle() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "automation");

        // INCR on non-existent key: should start at 0 â†’ becomes 1
        let incr = RedisCacheCommandRequest {
            cmd: "INCR".to_string(),
            partition_id: Some("metrics".to_string()),
            key: Some("counter".to_string()),
            value: None,
            ttl_ms: None,
            delta: None,
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        let r = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(incr)))
            .expect("incr ok").0;
        assert_eq!(r.status, "ok");
        assert_eq!(r.value.as_ref().and_then(|v| v.as_f64()), Some(1.0), "first INCR â†’ 1");

        // INCRBY 9 â†’ total should be 10
        let incrby = RedisCacheCommandRequest {
            cmd: "INCRBY".to_string(),
            partition_id: Some("metrics".to_string()),
            key: Some("counter".to_string()),
            value: None,
            ttl_ms: None,
            delta: Some(9.0),
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        let r2 = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(incrby)))
            .expect("incrby ok").0;
        assert_eq!(r2.value.as_ref().and_then(|v| v.as_f64()), Some(10.0), "after INCRBY 9 â†’ 10");

        // DECR â†’ 9
        let decr = RedisCacheCommandRequest {
            cmd: "DECR".to_string(),
            partition_id: Some("metrics".to_string()),
            key: Some("counter".to_string()),
            value: None,
            ttl_ms: None,
            delta: None,
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        let r3 = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(decr)))
            .expect("decr ok").0;
        assert_eq!(r3.value.as_ref().and_then(|v| v.as_f64()), Some(9.0), "after DECR â†’ 9");
    }

    #[test]
    fn ws27_redis_compat_expire_updates_ttl() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "automation");

        // SET a value with no TTL
        let set_req = RedisCacheCommandRequest {
            cmd: "SET".to_string(),
            partition_id: Some("sess".to_string()),
            key: Some("session_token".to_string()),
            value: Some(serde_json::json!("abc123")),
            ttl_ms: None,
            delta: None,
            expire_ms: None,
        keys: None,
        start: None,
        stop: None,
        field: None,
        };
        rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(set_req)))
            .expect("set ok");

        // EXPIRE with 5 minutes TTL
        let expire_req = RedisCacheCommandRequest {
            cmd: "EXPIRE".to_string(),
            partition_id: Some("sess".to_string()),
            key: Some("session_token".to_string()),
            value: None,
            ttl_ms: None,
            delta: None,
            expire_ms: Some(300_000),
            keys: None, start: None, stop: None, field: None,
        };
        let r = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(expire_req)))
            .expect("expire ok").0;
        assert_eq!(r.status, "ok");
        assert_eq!(r.exists, Some(true), "EXPIRE on existing key returns true");

        // EXPIRE on non-existent key returns false
        let expire_miss = RedisCacheCommandRequest {
            cmd: "EXPIRE".to_string(),
            partition_id: Some("sess".to_string()),
            key: Some("no_such_key".to_string()),
            value: None,
            ttl_ms: None,
            delta: None,
            expire_ms: Some(10_000),
            keys: None, start: None, stop: None, field: None,
        };
        let r2 = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(expire_miss)))
            .expect("expire miss ok").0;
        assert_eq!(r2.exists, Some(false), "EXPIRE on missing key returns false");
    }

    // â”€â”€ REQ-27: MGET / MSET / GETSET tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    #[test]
    fn ws27_redis_compat_mget_mset_lifecycle() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "automation");
        let part = Some("kv".to_string());

        // MSET: set a, b, c in one command via JSON object value
        let mset_req = RedisCacheCommandRequest {
            cmd: "MSET".to_string(),
            partition_id: part.clone(),
            key: None,
            value: Some(serde_json::json!({"a": 1, "b": 2, "c": 3})),
            ttl_ms: None, delta: None, expire_ms: None, keys: None, start: None, stop: None, field: None,
        };
        let r = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(mset_req)))
            .expect("mset ok").0;
        assert_eq!(r.status, "ok");
        assert_eq!(r.value, Some(serde_json::json!(3)), "MSET returns count of keys set");

        // MGET: retrieve a, b, c and a missing key
        let mget_req = RedisCacheCommandRequest {
            cmd: "MGET".to_string(),
            partition_id: part.clone(),
            key: None,
            value: None,
            ttl_ms: None, delta: None, expire_ms: None,
            keys: Some(vec!["a".to_string(), "b".to_string(), "c".to_string(), "x".to_string()]),
            start: None, stop: None, field: None,
        };
        let r2 = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(mget_req)))
            .expect("mget ok").0;
        assert_eq!(r2.status, "ok");
        let arr = r2.value.unwrap();
        let items = arr.as_array().expect("array");
        assert_eq!(items[0], serde_json::json!(1), "a = 1");
        assert_eq!(items[1], serde_json::json!(2), "b = 2");
        assert_eq!(items[2], serde_json::json!(3), "c = 3");
        assert_eq!(items[3], serde_json::Value::Null, "x = null (missing)");
    }

    #[test]
    fn ws27_redis_compat_getset_returns_old_sets_new() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "automation");
        let part = Some("gs".to_string());

        // Pre-set a value
        let set_req = RedisCacheCommandRequest {
            cmd: "SET".to_string(), partition_id: part.clone(), key: Some("counter".to_string()),
            value: Some(serde_json::json!(42)), ttl_ms: None, delta: None, expire_ms: None,
            keys: None, start: None, stop: None, field: None,
        };
        rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(set_req)))
            .expect("set ok");

        // GETSET â€” should return 42 and store 99
        let gs_req = RedisCacheCommandRequest {
            cmd: "GETSET".to_string(), partition_id: part.clone(), key: Some("counter".to_string()),
            value: Some(serde_json::json!(99)), ttl_ms: None, delta: None, expire_ms: None,
            keys: None, start: None, stop: None, field: None,
        };
        let r = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(gs_req)))
            .expect("getset ok").0;
        assert_eq!(r.value, Some(serde_json::json!(42)), "GETSET returns old value");

        // Now GET should return 99
        let get_req = RedisCacheCommandRequest {
            cmd: "GET".to_string(), partition_id: part.clone(), key: Some("counter".to_string()),
            value: None, ttl_ms: None, delta: None, expire_ms: None, keys: None, start: None, stop: None, field: None,
        };
        let r2 = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(get_req)))
            .expect("get ok").0;
        assert_eq!(r2.value, Some(serde_json::json!(99)), "GET returns new value after GETSET");
    }

    // â”€â”€ REQ-27: LPUSH / RPUSH / LLEN / LRANGE tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    #[test]
    fn ws27_redis_compat_list_lpush_rpush_lrange_llen() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "automation");
        let part = Some("lists".to_string());
        let key = Some("queue".to_string());

        // RPUSH three items
        for v in [10, 20, 30] {
            let req = RedisCacheCommandRequest {
                cmd: "RPUSH".to_string(), partition_id: part.clone(), key: key.clone(),
                value: Some(serde_json::json!(v)), ttl_ms: None, delta: None, expire_ms: None,
                keys: None, start: None, stop: None, field: None,
            };
            rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(req)))
                .expect("rpush ok");
        }

        // LLEN should be 3
        let llen_req = RedisCacheCommandRequest {
            cmd: "LLEN".to_string(), partition_id: part.clone(), key: key.clone(),
            value: None, ttl_ms: None, delta: None, expire_ms: None, keys: None, start: None, stop: None, field: None,
        };
        let r = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(llen_req)))
            .expect("llen ok").0;
        assert_eq!(r.value, Some(serde_json::json!(3)), "LLEN = 3 after 3 RPUSHes");

        // LPUSH prepends 0 â†’ list becomes [0, 10, 20, 30]
        let lpush_req = RedisCacheCommandRequest {
            cmd: "LPUSH".to_string(), partition_id: part.clone(), key: key.clone(),
            value: Some(serde_json::json!(0)), ttl_ms: None, delta: None, expire_ms: None,
            keys: None, start: None, stop: None, field: None,
        };
        rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(lpush_req)))
            .expect("lpush ok");

        // LRANGE 0 -1 returns full list [0, 10, 20, 30]
        let lrange_req = RedisCacheCommandRequest {
            cmd: "LRANGE".to_string(), partition_id: part.clone(), key: key.clone(),
            value: None, ttl_ms: None, delta: None, expire_ms: None, keys: None,
            start: Some(0), stop: Some(-1), field: None,
        };
        let r2 = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(lrange_req)))
            .expect("lrange ok").0;
        assert_eq!(
            r2.value,
            Some(serde_json::json!([0, 10, 20, 30])),
            "LRANGE 0 -1 returns full list"
        );

        // LRANGE 1 2 returns middle slice [10, 20]
        let lrange2_req = RedisCacheCommandRequest {
            cmd: "LRANGE".to_string(), partition_id: part.clone(), key: key.clone(),
            value: None, ttl_ms: None, delta: None, expire_ms: None, keys: None,
            start: Some(1), stop: Some(2), field: None,
        };
        let r3 = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(), Json(lrange2_req)))
            .expect("lrange2 ok").0;
        assert_eq!(r3.value, Some(serde_json::json!([10, 20])), "LRANGE 1 2 returns [10, 20]");
    }

    // â”€â”€ REQ-23: repeatable-read snapshot timestamp â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    #[test]
    fn ws23_acid_repeatable_read_records_snapshot_timestamp() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");

        // repeatable_read transaction â†’ should record snapshot timestamp
        let req_rr = SqlTransactionRequest {
            statements: vec!["BEGIN".to_string(), "SELECT 1".to_string(), "COMMIT".to_string()],
            isolation_level: Some("repeatable_read".to_string()),
        };
        rt.block_on(sql_transaction(State(state.clone()), headers.clone(), Json(req_rr)))
            .expect("repeatable_read tx should succeed");

        let acid = state.acid_transactions.lock().unwrap();
        let txs = acid.all_transactions();
        let rr_tx = txs.iter().find(|t| t.isolation_level == "repeatable_read")
            .expect("repeatable_read tx should be in registry");
        assert!(
            rr_tx.read_snapshot_at_ms.is_some(),
            "repeatable_read tx must record read_snapshot_at_ms"
        );
        assert_eq!(
            rr_tx.read_snapshot_at_ms, Some(rr_tx.started_at_unix_ms),
            "snapshot timestamp equals begin timestamp"
        );
        drop(acid);

        // read_committed transaction â†’ no snapshot
        let req_rc = SqlTransactionRequest {
            statements: vec!["BEGIN".to_string(), "COMMIT".to_string()],
            isolation_level: Some("read_committed".to_string()),
        };
        rt.block_on(sql_transaction(State(state.clone()), headers, Json(req_rc)))
            .expect("read_committed tx should succeed");

        let acid2 = state.acid_transactions.lock().unwrap();
        let rc_tx = acid2.all_transactions().into_iter()
            .find(|t| t.isolation_level == "read_committed")
            .expect("read_committed tx should be in registry");
        assert!(
            rc_tx.read_snapshot_at_ms.is_none(),
            "read_committed tx must NOT record read_snapshot_at_ms"
        );
    }

    // ── REQ-23: WAL durability ────────────────────────────────────────────────
    #[test]
    fn ws23_acid_wal_records_write_sequence() {
        // Each statement recorded during an active transaction must be appended to wal_log.
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");

        let req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO orders VALUES (1)".to_string(),
                "UPDATE orders SET status='done' WHERE id=1".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: Some("read_committed".to_string()),
        };
        rt.block_on(sql_transaction(State(state.clone()), headers.clone(), Json(req)))
            .expect("transaction should succeed");

        let acid = state.acid_transactions.lock().unwrap();
        let txs = acid.all_transactions();
        // The most recently completed transaction
        let tx = txs.iter().max_by_key(|t| t.started_at_unix_ms)
            .expect("at least one transaction");
        // WAL is cleared on commit, so wal_log must be empty after commit
        assert!(
            tx.wal_log.is_empty(),
            "WAL log must be cleared after commit"
        );
        // statement_count should reflect the non-control statements recorded
        assert!(tx.statement_count >= 1, "at least 1 DML statement recorded");
    }

    #[test]
    fn ws23_acid_wal_accumulates_during_active_tx() {
        // Verify that wal_log accumulates entries for each recorded statement while
        // the transaction is still active (before commit/rollback).
        let state = state_with_key(None);
        let tx_id = "wal-test-tx-001";
        let now_ms = 1_000_000_u128;

        {
            let mut acid = state.acid_transactions.lock().unwrap();
            acid.begin(tx_id, "node-1", "read_committed", now_ms);
            acid.record_statement(tx_id, Some("orders".to_string()));
            acid.record_statement(tx_id, Some("inventory".to_string()));
            acid.record_statement(tx_id, Some("orders".to_string())); // same table again

            let entry = acid.all_transactions().into_iter()
                .find(|t| t.transaction_id == tx_id)
                .expect("tx must exist");

            assert_eq!(entry.wal_log.len(), 3, "3 statements → 3 WAL entries");
            assert_eq!(entry.wal_log[0].1, "orders", "first WAL entry table = orders");
            assert_eq!(entry.wal_log[1].1, "inventory", "second WAL entry table = inventory");
            assert_eq!(entry.wal_log[2].1, "orders", "third WAL entry table = orders");
        }

        // After rollback, wal_log must be cleared
        {
            let mut acid = state.acid_transactions.lock().unwrap();
            let rolled = acid.rollback(tx_id, now_ms + 1);
            assert!(rolled, "rollback must succeed");

            let entry = acid.all_transactions().into_iter()
                .find(|t| t.transaction_id == tx_id)
                .expect("tx must still exist in registry");
            assert!(entry.wal_log.is_empty(), "WAL log must be cleared after rollback");
        }
    }

    // â”€â”€ REQ-21: mixed concurrent operations â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    #[test]
    fn ws21_mixed_ops_concurrent_ingest_sql_cache() {
        // Three concurrent threads: sql_execute + ingest_chunked + cache SET/GET.
        // All run on the same AppState. No panics or data corruption expected.
        use std::sync::Arc;
        let state = Arc::new(state_with_key(Some("secret")));

        // Thread 1: sql_execute
        let s1 = Arc::clone(&state);
        let t1 = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("rt");
            let headers = tenant_user_headers("analyst-acme", "acme");
            let req = SqlExecuteRequest {
                sql_batch: "SELECT COUNT(*) FROM events".to_string(),
                max_rows: None,
            };
            rt.block_on(sql_execute(State((*s1).clone()), headers, Json(req))).is_ok()
        });

        // Thread 2: ingest_chunked (uses tenant write privilege)
        let s2 = Arc::clone(&state);
        let t2 = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("rt");
            let headers = tenant_user_headers("analyst-acme", "acme");
            let req = IngestChunkedRequest {
                connector_id: "mixed-ops-conn".to_string(),
                records: vec![r#"{"id":1}"#.to_string(), r#"{"id":2}"#.to_string()],
                chunk_target_rows: Some(1),
                max_in_flight_tasks: Some(2),
            };
            rt.block_on(ingest_chunked(State((*s2).clone()), headers, Json(req))).is_ok()
        });

        // Thread 3: cache SET + GET
        let s3 = Arc::clone(&state);
        let t3 = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("rt");
            let headers = operator_headers("secret", "automation");
            let set_req = RedisCacheCommandRequest {
                cmd: "SET".to_string(),
                partition_id: Some("ws21".to_string()),
                key: Some("k1".to_string()),
                value: Some(serde_json::json!("hello")),
                ttl_ms: None, delta: None, expire_ms: None, keys: None, start: None, stop: None, field: None,
            };
            rt.block_on(cache_redis_command(State((*s3).clone()), headers, Json(set_req))).is_ok()
        });

        assert!(t1.join().expect("t1 panicked"), "sql_execute failed");
        assert!(t2.join().expect("t2 panicked"), "ingest_chunked failed");
        assert!(t3.join().expect("t3 panicked"), "cache SET failed");
    }

    // â”€â”€ REQ-07: async fan-out dispatches chunks in parallel â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    #[test]
    fn ws4_chunked_async_fanout_dispatches_in_parallel() {
        // Verify the async fan-out path in ingest_chunked: chunks are dispatched
        // via spawn_blocking and results are collected correctly.
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");

        // 12 records with chunk_target_rows=4 â†’ 3 chunks, max_in_flight=2 â†’ 2 waves
        let records: Vec<String> = (0..12).map(|i| format!(r#"{{"id":{i}}}"#)).collect();
        let req = IngestChunkedRequest {
            connector_id: "async-fanout-test".to_string(),
            records,
            chunk_target_rows: Some(4),
            max_in_flight_tasks: Some(2),
        };

        let resp = rt.block_on(ingest_chunked(State(state.clone()), headers, Json(req)))
            .expect("ingest_chunked async fanout should succeed");

        assert_eq!(resp.0, StatusCode::OK);
        assert_eq!(resp.1.total_records, 12, "all 12 records counted");
        assert_eq!(resp.1.chunk_count, 3, "3 chunks of 4");
        assert_eq!(resp.1.tasks_dispatched, 2, "max in-flight=2 â†’ dispatched=2");
        assert_eq!(resp.1.chunks_succeeded, 3, "all 3 chunks succeeded");
        assert_eq!(resp.1.chunks_failed, 0, "no chunks failed");
    }

    // ── REQ-27: Hash commands (HSET / HGET / HDEL / HGETALL) ─────────────────
    #[test]
    fn ws27_redis_compat_hash_hset_hget_hdel_hgetall() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "automation");
        let part = Some("ws27-hash".to_string());
        let key = Some("user:42".to_string());

        // Helper to build request with field
        let make = |cmd: &str, field: Option<&str>, val: Option<serde_json::Value>| RedisCacheCommandRequest {
            cmd: cmd.to_string(),
            partition_id: part.clone(),
            key: key.clone(),
            value: val,
            ttl_ms: None, delta: None, expire_ms: None, keys: None, start: None, stop: None,
            field: field.map(str::to_string),
        };

        // HSET name = "Alice"
        rt.block_on(cache_redis_command(State(state.clone()), headers.clone(),
            Json(make("HSET", Some("name"), Some(serde_json::json!("Alice"))))))
            .expect("HSET name ok");

        // HSET age = 30
        rt.block_on(cache_redis_command(State(state.clone()), headers.clone(),
            Json(make("HSET", Some("age"), Some(serde_json::json!(30))))))
            .expect("HSET age ok");

        // HGET name → "Alice"
        let r = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(),
            Json(make("HGET", Some("name"), None))))
            .expect("HGET ok").0;
        assert_eq!(r.value, Some(serde_json::json!("Alice")), "HGET name = Alice");

        // HGETALL → object with both fields
        let r2 = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(),
            Json(make("HGETALL", None, None))))
            .expect("HGETALL ok").0;
        let obj = r2.value.as_ref().and_then(|v| v.as_object()).expect("HGETALL returns object");
        assert_eq!(obj.get("name"), Some(&serde_json::json!("Alice")), "HGETALL name");
        assert_eq!(obj.get("age"), Some(&serde_json::json!(30)), "HGETALL age");

        // HDEL name → removed=true
        let r3 = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(),
            Json(make("HDEL", Some("name"), None))))
            .expect("HDEL ok").0;
        assert_eq!(r3.removed, Some(true), "HDEL removed name");

        // HGET missing field → None
        let r4 = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(),
            Json(make("HGET", Some("name"), None))))
            .expect("HGET missing ok").0;
        assert_eq!(r4.value, None, "HGET after HDEL returns None");
    }

    // ── REQ-27: Set commands (SADD / SMEMBERS / SCARD) ───────────────────────
    #[test]
    fn ws27_redis_compat_set_sadd_smembers_scard() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "automation");
        let part = Some("ws27-sets".to_string());
        let key = Some("tags:post:1".to_string());

        let make_sadd = |v: serde_json::Value| RedisCacheCommandRequest {
            cmd: "SADD".to_string(),
            partition_id: part.clone(),
            key: key.clone(),
            value: Some(v),
            ttl_ms: None, delta: None, expire_ms: None, keys: None, start: None, stop: None, field: None,
        };

        // SADD 3 distinct members
        for tag in ["rust", "database", "htap"] {
            let r = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(),
                Json(make_sadd(serde_json::json!(tag)))))
                .expect("SADD ok").0;
            assert_eq!(r.value, Some(serde_json::json!(1)), "new member added = 1");
        }

        // SADD duplicate → 0 (already exists)
        let r_dup = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(),
            Json(make_sadd(serde_json::json!("rust")))))
            .expect("SADD dup ok").0;
        assert_eq!(r_dup.value, Some(serde_json::json!(0)), "duplicate add = 0");

        // SCARD → 3
        let scard_req = RedisCacheCommandRequest {
            cmd: "SCARD".to_string(),
            partition_id: part.clone(),
            key: key.clone(),
            value: None, ttl_ms: None, delta: None, expire_ms: None, keys: None, start: None, stop: None, field: None,
        };
        let r_card = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(),
            Json(scard_req)))
            .expect("SCARD ok").0;
        assert_eq!(r_card.value, Some(serde_json::json!(3)), "SCARD = 3");

        // SMEMBERS contains all three tags
        let smembers_req = RedisCacheCommandRequest {
            cmd: "SMEMBERS".to_string(),
            partition_id: part.clone(),
            key: key.clone(),
            value: None, ttl_ms: None, delta: None, expire_ms: None, keys: None, start: None, stop: None, field: None,
        };
        let r_mb = rt.block_on(cache_redis_command(State(state.clone()), headers.clone(),
            Json(smembers_req)))
            .expect("SMEMBERS ok").0;
        let members = r_mb.value.as_ref().and_then(|v| v.as_array()).expect("array");
        for tag in ["rust", "database", "htap"] {
            assert!(members.contains(&serde_json::json!(tag)), "contains {tag}");
        }
    }

    // ── WS0: Workspace / CI / governance foundation tests ─────────────────────
    /// Resolve the workspace root from the crate manifest directory.
    /// Cargo sets the CWD to the crate directory during tests, so we navigate
    /// two levels up (services/voltnuerongridd → services → workspace root).
    fn ws0_workspace_root() -> std::path::PathBuf {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest.parent().and_then(|p| p.parent()).unwrap_or(manifest).to_path_buf()
    }

    #[test]
    fn ws0_ci_workflow_file_exists() {
        // Governance: the CI workflow definition must be present in the repository.
        let path = ws0_workspace_root().join(".github/workflows/ci.yml");
        assert!(path.exists(), "CI workflow file .github/workflows/ci.yml must exist at {:?}", path);
    }

    #[test]
    fn ws0_kpi_scripts_scaffold_exists() {
        // Governance: the KPI gate-script directory must be present.
        let path = ws0_workspace_root().join("tests/kpi/scripts");
        assert!(path.exists(), "tests/kpi/scripts directory must exist at {:?}", path);
    }

    #[test]
    fn ws0_kpi_results_scaffold_exists() {
        // Governance: the KPI results artifact directory must be present.
        let path = ws0_workspace_root().join("tests/kpi/results");
        assert!(path.exists(), "tests/kpi/results directory must exist at {:?}", path);
    }

    #[test]
    fn ws0_cargo_workspace_manifest_exists() {
        // Governance: the top-level Cargo workspace manifest must be present.
        let path = ws0_workspace_root().join("Cargo.toml");
        assert!(path.exists(), "Cargo.toml workspace manifest must exist at {:?}", path);
    }

    #[test]
    fn ws0_deploy_local_scaffold_exists() {
        // Governance: the local deploy scaffold directory must exist.
        let path = ws0_workspace_root().join("deploy/local");
        assert!(path.exists(), "deploy/local directory must exist at {:?}", path);
    }

    // ── WS2A: Transactional row store / HTAP sync-origin tests ───────────────
    #[test]
    fn ws2a_row_store_sync_origin_registers_mutations() {
        // Validate that RowStoreSyncOrigin accumulates mutation events in sequence
        // order and that the recorded sequence IDs are strictly monotonic.
        use voltnuerongrid_store::htap_sync::{MutationOp, RowStoreSyncOrigin};
        let mut origin = RowStoreSyncOrigin::new();
        let m1 = origin.append("orders", "k1", r#"{"v":1}"#, MutationOp::Insert);
        let m2 = origin.append("orders", "k2", r#"{"v":2}"#, MutationOp::Update);
        let m3 = origin.append("orders", "k3", r#"{"v":3}"#, MutationOp::Delete);
        assert!(m1.sequence < m2.sequence, "sequence must increase");
        assert!(m2.sequence < m3.sequence, "sequence must increase");
        assert_eq!(origin.pending_len(), 3, "all three mutations still pending");
    }

    #[test]
    fn ws2a_htap_sync_origin_detects_sequence_gaps() {
        // Validate the gap-detection utility correctly identifies missing
        // sequence IDs in a synthetic batch with a deliberate gap.
        use voltnuerongrid_store::htap_sync::{MutationOp, RowMutation, RowStoreSyncOrigin};
        let batch: Vec<RowMutation> = vec![
            RowMutation { sequence: 1, table: "t".into(), primary_key: "k1".into(), payload_json: "{}".into(), op: MutationOp::Insert },
            RowMutation { sequence: 2, table: "t".into(), primary_key: "k2".into(), payload_json: "{}".into(), op: MutationOp::Update },
            // sequence 3 intentionally absent — gap here
            RowMutation { sequence: 4, table: "t".into(), primary_key: "k4".into(), payload_json: "{}".into(), op: MutationOp::Delete },
        ];
        let gaps = RowStoreSyncOrigin::detect_sequence_gaps(&batch);
        assert_eq!(gaps.len(), 1, "exactly one gap expected, got {:?}", gaps);
        assert_eq!(gaps[0].expected, 3, "gap should be at sequence 3");
    }

    #[test]
    fn ws2a_htap_sync_origin_snapshot_restore_is_idempotent() {
        // Snapshot a populated origin, restore it, then verify the restored
        // origin reaches the identical state (same next_sequence).
        use voltnuerongrid_store::htap_sync::{MutationOp, RowStoreSyncOrigin};
        let mut origin = RowStoreSyncOrigin::new();
        origin.append("orders", "k1", r#"{"a":1}"#, MutationOp::Insert);
        origin.append("orders", "k2", r#"{"a":2}"#, MutationOp::Update);
        let snap = origin.snapshot();
        let next_before = snap.next_sequence;

        let restored = RowStoreSyncOrigin::restore(snap);
        let snap2 = restored.snapshot();
        assert_eq!(snap2.next_sequence, next_before, "restored next_sequence must match original");
        assert_eq!(restored.pending_len(), 2, "restored pending must contain same mutations");
    }

    // ── REQ-21: sustained load ────────────────────────────────────────────────
    #[test]
    fn ws21_sustained_load_sql_execute() {
        // Run 50 sequential sql_execute calls on the same AppState and verify
        // all succeed without panics (models sustained single-tenant load).
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let start = std::time::Instant::now();

        for i in 0..50u32 {
            let req = SqlExecuteRequest {
                sql_batch: format!("SELECT {i} AS seq"),
                max_rows: None,
            };
            rt.block_on(sql_execute(State(state.clone()), headers.clone(), Json(req)))
                .unwrap_or_else(|_| panic!("sql_execute failed at iteration {i}"));
        }

        let elapsed_ms = start.elapsed().as_millis();
        // All 50 calls should complete in under 5 seconds on any dev machine
        assert!(elapsed_ms < 5_000, "50 sequential calls took {elapsed_ms}ms, expected < 5000ms");
    }

    #[test]
    fn ws21_benchmark_ingest_reports_positive_rps() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "automation");
        let req = BenchmarkIngestRequest {
            record_count: Some(200),
            chunk_target_rows: Some(64),
        };
        let (status, Json(body)) = rt
            .block_on(benchmark_ingest(State(state), headers, Json(req)))
            .expect("benchmark ingest");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.record_count, 200);
        assert!(body.chunk_count > 0, "chunk_count must be positive");
        assert!(body.records_per_second.is_finite() && body.records_per_second > 0.0);
    }

    #[test]
    fn ws21_benchmark_query_reports_positive_ops_per_sec() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let headers = operator_headers("secret", "platform-admin");
        let req = BenchmarkQueryRequest {
            op_count: Some(800),
        };
        let (status, Json(body)) = rt
            .block_on(benchmark_query(State(state), headers, Json(req)))
            .expect("benchmark query");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.op_count, 800);
        assert!(body.ops_per_second.is_finite() && body.ops_per_second > 0.0);
    }

    #[test]
    fn ws21_benchmark_endpoints_require_operator_auth() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let empty = HeaderMap::new();
        let ingest_req = BenchmarkIngestRequest {
            record_count: Some(10),
            chunk_target_rows: Some(32),
        };
        let ingest_res = rt.block_on(benchmark_ingest(
            State(state.clone()),
            empty.clone(),
            Json(ingest_req),
        ));
        let err = match ingest_res {
            Err(e) => e,
            Ok(_) => panic!("benchmark ingest must reject unauthenticated callers"),
        };
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);

        let query_req = BenchmarkQueryRequest { op_count: Some(10) };
        let query_res = rt.block_on(benchmark_query(State(state), empty, Json(query_req)));
        let err2 = match query_res {
            Err(e) => e,
            Ok(_) => panic!("benchmark query must reject unauthenticated callers"),
        };
        assert_eq!(err2.0, StatusCode::UNAUTHORIZED);
    }

    // ── REQ-23: snapshot read path enforcement ────────────────────────────────
    #[test]
    fn ws23_acid_read_uncommitted_does_not_record_snapshot() {
        // read_uncommitted must NOT set read_snapshot_at_ms — it sees all in-progress writes
        let state = state_with_key(None);
        let tx_id = "test-ru-no-snapshot";
        let now_ms = 2_000_000_u128;
        {
            let mut acid = state.acid_transactions.lock().unwrap();
            acid.begin(tx_id, "node-1", "read_uncommitted", now_ms);
            let entry = acid.all_transactions().into_iter()
                .find(|t| t.transaction_id == tx_id)
                .expect("tx must exist in registry");
            assert!(
                entry.read_snapshot_at_ms.is_none(),
                "read_uncommitted must NOT record read_snapshot_at_ms"
            );
        }
    }

    #[test]
    fn ws23_acid_serializable_uses_write_lock_not_snapshot() {
        // serializable uses write-lock conflict detection rather than MVCC snapshot timestamps.
        // It must NOT set read_snapshot_at_ms; conflict detection is done via table write tracking.
        let state = state_with_key(None);
        let tx_id = "test-serializable-no-snapshot";
        let now_ms = 3_000_000_u128;
        {
            let mut acid = state.acid_transactions.lock().unwrap();
            acid.begin(tx_id, "node-1", "serializable", now_ms);
            let entry = acid.all_transactions().into_iter()
                .find(|t| t.transaction_id == tx_id)
                .expect("tx must exist in registry");
            assert_eq!(
                entry.isolation_level, "serializable",
                "isolation level should be recorded"
            );
            // serializable conflict detection is via concurrent-write tracking, not snapshot timestamps
            assert!(
                entry.read_snapshot_at_ms.is_none(),
                "serializable uses write-lock detection — read_snapshot_at_ms must not be set"
            );
        }
    }

    // ── REQ-27: Redis-compat extended coverage ────────────────────────────────
    #[test]
    fn ws27_redis_compat_set_with_ttl_returns_ok() {
        // SET with a ttl_ms should succeed — key is stored with an expiry deadline
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let req = RedisCacheCommandRequest {
            cmd: "SET".to_string(),
            partition_id: Some("ttl-part".to_string()),
            key: Some("temp-key".to_string()),
            value: Some(serde_json::json!("ephemeral")),
            ttl_ms: Some(60_000),
            delta: None,
            expire_ms: None,
            keys: None, start: None, stop: None, field: None,
        };
        let result = rt.block_on(cache_redis_command(
            State(state),
            operator_headers("secret", "automation"),
            Json(req),
        )).expect("SET with TTL should succeed").0;
        assert_eq!(result.status, "ok");
    }

    #[test]
    fn ws27_redis_compat_getset_on_missing_key_returns_null_old_value() {
        // GETSET on a non-existent key returns null for the old value, stores the new value
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let state = state_with_key(Some("secret"));
        let req = RedisCacheCommandRequest {
            cmd: "GETSET".to_string(),
            partition_id: Some("gs-new".to_string()),
            key: Some("brand-new-key".to_string()),
            value: Some(serde_json::json!("first-write")),
            ttl_ms: None, delta: None, expire_ms: None,
            keys: None, start: None, stop: None, field: None,
        };
        let result = rt.block_on(cache_redis_command(
            State(state.clone()),
            operator_headers("secret", "automation"),
            Json(req),
        )).expect("GETSET on new key should succeed").0;
        assert_eq!(result.value, None, "GETSET on missing key must return null old value");

        // Subsequent GET should return the newly written value
        let get_req = RedisCacheCommandRequest {
            cmd: "GET".to_string(),
            partition_id: Some("gs-new".to_string()),
            key: Some("brand-new-key".to_string()),
            value: None, ttl_ms: None, delta: None, expire_ms: None,
            keys: None, start: None, stop: None, field: None,
        };
        let get_result = rt.block_on(cache_redis_command(
            State(state),
            operator_headers("secret", "automation"),
            Json(get_req),
        )).expect("GET should succeed").0;
        assert_eq!(get_result.value, Some(serde_json::json!("first-write")));
    }

    // ------------------------------------------------------------------
    // REQ-31 / WS3: Additional HTAP routing coverage
    // ------------------------------------------------------------------

    #[test]
    fn ws3_htap_router_window_function_routed_as_olap() {
        // OVER( signals a window function — should be classified as OLAP.
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let rt = tokio::runtime::Runtime::new().expect("runtime");

        let r = rt.block_on(sql_route(
            State(state),
            headers,
            Json(SqlRouteRequest {
                sql_batch: "SELECT id, SUM(amount) OVER(PARTITION BY region) FROM orders;".to_string(),
            }),
        )).expect("sql_route window function");

        assert_eq!(r.status, "ok");
        assert_eq!(r.route_path, "olap", "window function (OVER) must route to olap");
    }

    #[test]
    fn ws3_htap_router_having_clause_routed_as_olap() {
        // HAVING is an aggregation filter — definitively OLAP.
        let state = state_with_key(None);
        let headers = tenant_user_headers("analyst-acme", "acme");
        let rt = tokio::runtime::Runtime::new().expect("runtime");

        let r = rt.block_on(sql_route(
            State(state),
            headers,
            Json(SqlRouteRequest {
                sql_batch: "SELECT region, COUNT(*) FROM orders GROUP BY region HAVING COUNT(*) > 5;".to_string(),
            }),
        )).expect("sql_route having clause");

        assert_eq!(r.status, "ok");
        assert_eq!(r.route_path, "olap", "HAVING clause must route to olap");
    }

    // ------------------------------------------------------------------
    // REQ-21: Additional concurrency stress tests
    // ------------------------------------------------------------------

    #[test]
    fn ws21_multi_tenant_ddl_catalog_isolation() {
        // 4 threads each issue distinct CREATE TABLE DDL via sql_execute concurrently.
        // All must succeed without corrupting the shared ddl_catalog mutex.
        use std::sync::Arc;

        let state = Arc::new(state_with_key(None));
        let handles: Vec<_> = (0u8..4)
            .map(|i| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().expect("runtime");
                    // Use the registered admin-acme user for DDL operations
                    let headers = tenant_user_headers("admin-acme", "acme");
                    let req = SqlExecuteRequest {
                        sql_batch: format!(
                            "CREATE TABLE concurrent_table_{i} (id INT PRIMARY KEY, val FLOAT);"
                        ),
                        max_rows: None,
                    };
                    let result = rt.block_on(sql_execute(State((*state).clone()), headers, Json(req)));
                    (i, result.is_ok())
                })
            })
            .collect();

        for handle in handles {
            let (i, ok): (u8, bool) = handle.join().expect("thread panicked");
            assert!(ok, "Thread {i} DDL execute failed");
        }
    }

    #[test]
    fn ws21_concurrent_pessimistic_lock_acquire_distinct_resources() {
        // 4 threads each acquire a pessimistic lock on distinct resources simultaneously.
        // All must succeed without deadlock or race.
        use std::sync::Arc;

        let state = Arc::new(state_with_key(None));
        let handles: Vec<_> = (0u8..4)
            .map(|i| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().expect("runtime");
                    let req = PessimisticLockAcquireRequest {
                        transaction_id: format!("tx-concurrent-{i}"),
                        resource: format!("resource-{i}"),
                        owner: Some(format!("owner-{i}")),
                        ttl_ms: None,
                        wait_timeout_ms: Some(500),
                    };
                    let (status, _) = rt.block_on(sql_pessimistic_lock_acquire(
                        State((*state).clone()),
                        Json(req),
                    ));
                    (i, status == StatusCode::OK)
                })
            })
            .collect();

        for handle in handles {
            let (i, ok): (u8, bool) = handle.join().expect("thread panicked");
            assert!(ok, "Thread {i} lock acquire failed");
        }
    }

    // ------------------------------------------------------------------
    // S3-WS1-04: SQL Tokenizer integration tests
    // ------------------------------------------------------------------

    #[test]
    fn s3_ws1_tokenizer_counts_keywords_in_olap_query() {
        // Verify the new tokenizer correctly identifies ANSI SQL keywords
        // in a typical OLAP query — a real parser step beyond heuristics.
        use voltnuerongrid_sql::tokenizer::{semantic_tokens, Token};
        let sql = "SELECT region, SUM(amount) OVER(PARTITION BY region) \
                   FROM orders GROUP BY region HAVING SUM(amount) > 100;";
        let tokens = semantic_tokens(sql);
        let keywords: Vec<_> = tokens.iter()
            .filter_map(|t| if let Token::Keyword(k) = t { Some(k.as_str()) } else { None })
            .collect();
        assert!(keywords.contains(&"SELECT"));
        assert!(keywords.contains(&"SUM"));
        assert!(keywords.contains(&"OVER"));
        assert!(keywords.contains(&"PARTITION"));
        assert!(keywords.contains(&"GROUP"));
        assert!(keywords.contains(&"HAVING"));
        // Must not count whitespace or punctuation as keywords
        assert!(!keywords.contains(&"("));
        assert!(!keywords.contains(&")"));
    }

    #[test]
    fn s3_ws1_tokenizer_parses_transaction_block() {
        use voltnuerongrid_sql::tokenizer::{semantic_tokens, Token};
        let sql = "BEGIN; INSERT INTO orders VALUES (1, 'acme', 99.99); COMMIT;";
        let tokens = semantic_tokens(sql);
        let keywords: Vec<_> = tokens.iter()
            .filter_map(|t| if let Token::Keyword(k) = t { Some(k.as_str()) } else { None })
            .collect();
        assert!(keywords.contains(&"BEGIN"));
        assert!(keywords.contains(&"INSERT"));
        assert!(keywords.contains(&"INTO"));
        assert!(keywords.contains(&"VALUES"));
        assert!(keywords.contains(&"COMMIT"));
        // String literal extracted correctly
        let lits: Vec<_> = tokens.iter()
            .filter_map(|t| if let Token::StringLiteral(s) = t { Some(s.as_str()) } else { None })
            .collect();
        assert!(lits.contains(&"acme"));
    }

    // ------------------------------------------------------------------
    // S2-WS2-04: MVCC PagedRowStore integration tests
    // ------------------------------------------------------------------

    #[test]
    fn s2_ws2_mvcc_row_store_insert_and_snapshot_read() {
        use voltnuerongrid_store::mvcc::PagedRowStore;
        use std::collections::HashMap;

        let mut store = PagedRowStore::new(64);
        let xid1 = store.begin_xid();
        let mut data = HashMap::new();
        data.insert("tenant_id".to_string(), "acme".to_string());
        data.insert("amount".to_string(), "500".to_string());
        store.insert(xid1, "order:acme:1", data.clone());

        let snap = store.current_xid();

        // Future write must not pollute snapshot
        let xid2 = store.begin_xid();
        let mut data2 = HashMap::new();
        data2.insert("tenant_id".to_string(), "acme".to_string());
        data2.insert("amount".to_string(), "9999".to_string());
        store.insert(xid2, "order:acme:1", data2);

        let visible = store.read_at_snapshot("order:acme:1", snap)
            .expect("row must be visible at snapshot");
        assert_eq!(visible["amount"], "500", "snapshot must see the pre-update value");

        let latest = store.read_latest("order:acme:1")
            .expect("latest row must exist");
        assert_eq!(latest["amount"], "9999", "latest read must see updated value");
    }

    #[test]
    fn s2_ws2_mvcc_row_store_delete_creates_tombstone() {
        use voltnuerongrid_store::mvcc::PagedRowStore;
        use std::collections::HashMap;

        let mut store = PagedRowStore::new(64);
        let xid = store.begin_xid();
        let mut data = HashMap::new();
        data.insert("status".to_string(), "active".to_string());
        store.insert(xid, "session:xyz", data);

        let snap_before = store.current_xid();
        let xid2 = store.begin_xid();
        assert!(store.delete(xid2, "session:xyz"), "delete must return true for existing row");

        // Pre-delete snapshot still sees the row
        assert!(store.read_at_snapshot("session:xyz", snap_before).is_some());
        // Post-delete latest read returns None
        assert!(store.read_latest("session:xyz").is_none());
    }

    #[test]
    fn s2_ws2_mvcc_row_store_wired_in_appstate() {
        // Verify the row_store field is accessible via AppState and can be used.
        let state = state_with_key(None);
        let mut store = state.row_store.lock().unwrap();
        let xid = store.begin_xid();
        let mut data = std::collections::HashMap::new();
        data.insert("key".to_string(), "value".to_string());
        store.insert(xid, "test:1", data);
        let result = store.read_latest("test:1").expect("row must exist");
        assert_eq!(result["key"], "value");
    }

    // ── S5-WS4-03 + S2-WS2-05 integration tests ─────────────────────────────

    #[test]
    fn s5_ws4_extract_insert_parses_simple_values() {
        let result =
            extract_insert_row_from_sql("INSERT INTO orders VALUES ('ord-1', 500)");
        assert!(result.is_some());
        let (key, data) = result.unwrap();
        assert!(key.starts_with("orders:"), "unexpected key: {key}");
        // __table meta key holds the table name
        assert_eq!(data.get("__table").map(String::as_str), Some("orders"));
        // Values stored as positional column names (no explicit column list)
        assert_eq!(data.get("col_0").map(String::as_str), Some("ord-1"),
            "first positional value should be under col_0");
        assert_eq!(data.get("col_1").map(String::as_str), Some("500"),
            "second positional value should be under col_1");
    }

    #[test]
    fn s5_ws4_extract_insert_ignores_non_insert() {
        assert!(extract_insert_row_from_sql("SELECT * FROM orders").is_none());
        assert!(extract_insert_row_from_sql("UPDATE orders SET x=1").is_none());
        assert!(extract_insert_row_from_sql("COMMIT").is_none());
        assert!(extract_insert_row_from_sql("").is_none());
    }

    #[test]
    fn s5_ws4_extract_insert_parses_named_columns() {
        let result = extract_insert_row_from_sql(
            "INSERT INTO users (id, name, age) VALUES ('u1', 'Alice', 30)"
        );
        assert!(result.is_some());
        let (key, data) = result.unwrap();
        assert!(key.starts_with("users:"), "key: {key}");
        assert_eq!(data.get("__table").map(String::as_str), Some("users"));
        assert_eq!(data.get("id").map(String::as_str), Some("u1"));
        assert_eq!(data.get("name").map(String::as_str), Some("Alice"));
        assert_eq!(data.get("age").map(String::as_str), Some("30"));
    }

    #[test]
    fn s2_ws2_commit_flush_writes_inserts_to_row_store() {
        let state = state_with_key(None);
        let stmts = vec![
            "INSERT INTO products VALUES ('prod-1', 99)".to_string(),
            "INSERT INTO products VALUES ('prod-2', 149)".to_string(),
        ];
        {
            let mut rs = state.row_store.lock().expect("row_store lock");
            let xid = rs.begin_xid();
            for stmt in &stmts {
                if let Some((k, d)) = extract_insert_row_from_sql(stmt) {
                    rs.insert(xid, &k, d);
                }
            }
        }
        let rs = state.row_store.lock().expect("row_store lock");
        let snap = rs.scan_at_snapshot(rs.current_xid());
        assert_eq!(snap.len(), 2, "both inserted rows should be visible");
        let tables: Vec<&str> = snap
            .iter()
            .filter_map(|(_, d)| d.get("__table").map(String::as_str))
            .collect();
        assert!(
            tables.iter().all(|t| *t == "products"),
            "all rows should be in the products table"
        );
    }

    #[tokio::test]
    async fn s5_ws4_row_store_receives_ingest_style_writes() {
        use voltnuerongrid_store::mvcc::PagedRowStore;
        let mut rs = PagedRowStore::default();
        let xid = rs.begin_xid();
        // Simulate what ingest_csv/ingest_json handler now does
        for (key, payload, source) in &[
            ("rec:1", "alice,30", "csv:conn-a"),
            ("rec:2", "bob,25", "csv:conn-a"),
            ("rec:3", r#"{\"id\":\"u3\"}"#, "json:conn-b"),
        ] {
            let mut data = std::collections::HashMap::new();
            data.insert("payload".to_string(), payload.to_string());
            data.insert("source".to_string(), source.to_string());
            rs.insert(xid, key, data);
        }
        let visible = rs.scan_at_snapshot(xid);
        assert_eq!(visible.len(), 3);
        assert!(visible
            .iter()
            .any(|(_, d)| d.get("source").map(String::as_str) == Some("json:conn-b")));
    }

    #[test]
    fn s3_ws1_ast_parser_select_round_trip() {
        use voltnuerongrid_sql::{parse_one, Statement};
        let stmt = parse_one("SELECT id, name FROM users WHERE active = 1").unwrap();
        let Statement::Select(sel) = stmt else { panic!("expected Select") };
        assert_eq!(sel.table.as_deref(), Some("users"));
        assert!(sel.where_clause.is_some());
    }

    #[test]
    fn s3_ws1_ast_parser_insert_round_trip() {
        use voltnuerongrid_sql::{parse_one, Statement};
        let stmt =
            parse_one("INSERT INTO events (id, name) VALUES ('e1', 'launch')").unwrap();
        let Statement::Insert(ins) = stmt else { panic!("expected Insert") };
        assert_eq!(ins.table, "events");
        assert_eq!(ins.columns, vec!["id", "name"]);
        assert_eq!(ins.values[0], vec!["e1", "launch"]);
    }

    // ── S2-WS2-05: COMMIT flush handles DELETE statements ───────────────────
    #[test]
    fn s2_ws2_commit_flush_handles_delete_statement() {
        // extract_delete_key_from_sql returns the WHERE-clause value
        let key = extract_delete_key_from_sql("DELETE FROM orders WHERE id = 'o99'");
        assert_eq!(key, Some("o99".to_string()));
        // Non-DELETE returns None
        assert!(extract_delete_key_from_sql("SELECT * FROM orders").is_none());
        // Missing WHERE returns None
        assert!(extract_delete_key_from_sql("DELETE FROM orders").is_none());
    }

    // ── S2-WS2-05: COMMIT flush handles UPDATE statements ───────────────────
    #[test]
    fn s2_ws2_commit_flush_handles_update_statement() {
        let result = extract_update_row_from_sql(
            "UPDATE products SET price='42' WHERE id='p1'",
        );
        let (key, data) = result.expect("should parse UPDATE");
        assert_eq!(key, "products:p1");
        assert_eq!(data.get("price"), Some(&"42".to_string()));
        assert_eq!(data.get("__table"), Some(&"products".to_string()));
    }

    // ── S3-WS1-05: planner routes aggregate query to OLAP ───────────────────
    #[test]
    fn s3_ws1_planner_routes_aggregate_to_olap() {
        use voltnuerongrid_exec::{LogicalPlan, QueryPlanner};
        use voltnuerongrid_sql::parse_one;
        let stmt = parse_one("SELECT region, SUM(revenue) FROM sales GROUP BY region").unwrap();
        let plan = QueryPlanner::plan(&stmt);
        assert!(plan.has_aggregation());
        let est = QueryPlanner::estimate_cost(&plan);
        assert_eq!(
            est.recommended_path,
            voltnuerongrid_exec::QueryPath::Olap,
            "aggregate queries should route to OLAP"
        );
    }

    // ── S3-WS1-05: planner routes filtered SELECT to OLTP ───────────────────
    #[test]
    fn s3_ws1_planner_select_with_filter_routes_oltp() {
        use voltnuerongrid_exec::{LogicalPlan, QueryPlanner};
        use voltnuerongrid_sql::parse_one;
        let stmt = parse_one("SELECT id FROM users WHERE id = 'u1'").unwrap();
        let plan = QueryPlanner::plan(&stmt);
        assert!(!plan.has_aggregation());
        let est = QueryPlanner::estimate_cost(&plan);
        assert_eq!(
            est.recommended_path,
            voltnuerongrid_exec::QueryPath::Oltp,
            "filtered point selects should route to OLTP"
        );
    }

    // ── S3-WS1-05: sql_route response includes planner cost hints ────────────
    #[test]
    fn s3_ws1_sql_route_response_includes_planner_fields() {
        let state = state_with_key(Some("test-key"));
        let req = SqlRouteRequest {
            sql_batch: "SELECT region, SUM(revenue) FROM sales GROUP BY region".to_string(),
        };
        let headers = operator_headers("test-key", "admin");
        let resp = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(sql_route(State(state), headers, Json(req)))
            .unwrap();
        assert_eq!(resp.0.status, "ok");
        assert!(!resp.0.statements.is_empty(), "should have at least one routed statement");
        let stmt = &resp.0.statements[0];
        // Aggregate query should get planner_path == "olap"
        assert_eq!(stmt.planner_path, "olap", "aggregate should map to olap planner path");
        assert!(stmt.estimated_rows > 0, "estimated_rows should be positive");
        assert!(stmt.relative_cost > 0.0, "relative_cost should be positive");
        assert!(resp.0.batch_estimated_rows > 0);
        assert!(resp.0.batch_relative_cost > 0.0);
    }

    #[test]
    fn s3_ws1_sql_route_point_select_gets_oltp_planner_path() {
        let state = state_with_key(Some("test-key"));
        let req = SqlRouteRequest {
            sql_batch: "SELECT id FROM orders WHERE id = 'o1'".to_string(),
        };
        let headers = operator_headers("test-key", "admin");
        let resp = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(sql_route(State(state), headers, Json(req)))
            .unwrap();
        let stmt = &resp.0.statements[0];
        assert_eq!(stmt.planner_path, "oltp", "filtered select should be oltp");
    }

    // ── S3-WS1-05: sql_execute response includes planner_path ───────────────
    #[test]
    fn s3_ws1_sql_execute_planner_path_populated_for_aggregate() {
        let state = state_with_key(Some("test-key"));
        let req = SqlExecuteRequest {
            sql_batch: "SELECT region, SUM(revenue) FROM sales GROUP BY region".to_string(),
            max_rows: None,
        };
        let headers = operator_headers("test-key", "admin");
        let resp = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(sql_execute(State(state), headers, Json(req)))
            .unwrap();
        let response = resp.1.0;
        assert!(
            response.planner_path.is_some(),
            "planner_path must be set for parseable SQL"
        );
        assert_eq!(
            response.planner_path.as_deref(),
            Some("olap"),
            "aggregate batch should generate olap planner_path"
        );
    }

    // ── S5-WS4-03 / S2-WS2-04: store/rows/scan returns committed rows ────────
    #[test]
    fn s5_ws4_store_rows_scan_returns_committed_rows() {
        let state = state_with_key(Some("test-key"));
        // Write two rows into the row store directly
        {
            let mut rs = state.row_store.lock().expect("row_store lock");
            let xid = rs.begin_xid();
            let mut d1 = std::collections::HashMap::new();
            d1.insert("source".to_string(), "test".to_string());
            d1.insert("payload".to_string(), "row-one".to_string());
            rs.insert(xid, "scan-test:row1", d1);
            let xid2 = rs.begin_xid();
            let mut d2 = std::collections::HashMap::new();
            d2.insert("source".to_string(), "test".to_string());
            d2.insert("payload".to_string(), "row-two".to_string());
            rs.insert(xid2, "scan-test:row2", d2);
        }
        let req = StoreRowsScanRequest {
            snapshot_xid: None,
            key_prefix: Some("scan-test:".to_string()),
            limit: None,
        };
        let headers = operator_headers("test-key", "admin");
        let resp = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(store_rows_scan(State(state), headers, Json(req)))
            .unwrap();
        let scan = resp.1.0;
        assert_eq!(scan.status, "ok");
        assert_eq!(scan.row_count, 2, "should return the two inserted rows");
        assert!(scan.rows.iter().any(|r| r.key == "scan-test:row1"));
        assert!(scan.rows.iter().any(|r| r.key == "scan-test:row2"));
    }

    #[test]
    fn s5_ws4_store_rows_scan_key_prefix_filters_rows() {
        let state = state_with_key(Some("test-key"));
        {
            let mut rs = state.row_store.lock().expect("row_store lock");
            let xid = rs.begin_xid();
            let mut d = std::collections::HashMap::new();
            d.insert("x".to_string(), "1".to_string());
            rs.insert(xid, "prefix-a:row", d.clone());
            let xid2 = rs.begin_xid();
            rs.insert(xid2, "prefix-b:row", d.clone());
        }
        let req = StoreRowsScanRequest {
            snapshot_xid: None,
            key_prefix: Some("prefix-a:".to_string()),
            limit: None,
        };
        let headers = operator_headers("test-key", "admin");
        let resp = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(store_rows_scan(State(state), headers, Json(req)))
            .unwrap();
        let scan = resp.1.0;
        assert_eq!(scan.row_count, 1, "prefix filter should exclude prefix-b row");
        assert_eq!(scan.rows[0].key, "prefix-a:row");
    }

    #[test]
    fn s5_ws4_store_rows_scan_respects_limit() {
        let state = state_with_key(Some("test-key"));
        {
            let mut rs = state.row_store.lock().expect("row_store lock");
            for i in 0..10 {
                let xid = rs.begin_xid();
                let mut d = std::collections::HashMap::new();
                d.insert("i".to_string(), i.to_string());
                rs.insert(xid, &format!("limit-test:{i}"), d);
            }
        }
        let req = StoreRowsScanRequest {
            snapshot_xid: None,
            key_prefix: Some("limit-test:".to_string()),
            limit: Some(3),
        };
        let headers = operator_headers("test-key", "admin");
        let resp = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(store_rows_scan(State(state), headers, Json(req)))
            .unwrap();
        assert_eq!(resp.1.0.row_count, 3, "limit of 3 should cap the scan");
    }

    // ── S4-WS3-02: OLTP physical executor dispatch ───────────────────────────

    #[tokio::test]
    async fn s4_ws3_sql_execute_oltp_path_returns_rows_from_row_store() {
        // Insert two rows via PagedRowStore directly
        let state = state_with_key(Some("test-key"));
        {
            let mut rs = state.row_store.lock().unwrap();
            let xid = rs.begin_xid();
            let mut d = std::collections::HashMap::new();
            d.insert("value".to_string(), "42".to_string());
            rs.insert(xid, "oltp-key-1", d.clone());
            let xid2 = rs.begin_xid();
            d.insert("value".to_string(), "99".to_string());
            rs.insert(xid2, "oltp-key-2", d);
        }
        // Point SELECT with WHERE targeting oltp-key-1 → planner routes as oltp
        let req = SqlExecuteRequest {
            sql_batch: "SELECT value FROM rows WHERE id = 'oltp-key-1'".to_string(),
            max_rows: Some(10),
        };
        let headers = operator_headers("test-key", "admin");
        let resp = sql_execute(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(resp.1.0.status, "ok");
        // Planner should have routed as oltp
        assert_eq!(resp.1.0.planner_path.as_deref(), Some("oltp"));
        // OLTP rows should be populated and contain the matching key
        let rows = resp.1.0.oltp_rows.expect("expected oltp_rows for oltp path");
        assert!(!rows.is_empty(), "should return at least one oltp row");
        assert!(rows.iter().any(|r| r.key.contains("oltp-key-1")));
    }

    #[tokio::test]
    async fn s4_ws3_sql_execute_olap_aggregate_has_no_oltp_rows() {
        let state = state_with_key(Some("test-key"));
        let req = SqlExecuteRequest {
            sql_batch: "SELECT SUM(amount) FROM orders GROUP BY region".to_string(),
            max_rows: None,
        };
        let headers = operator_headers("test-key", "admin");
        let resp = sql_execute(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(resp.1.0.status, "ok");
        // Aggregate → olap path: oltp_rows should be None
        assert!(resp.1.0.oltp_rows.is_none(), "aggregate query should not populate oltp_rows");
        assert_eq!(resp.1.0.planner_path.as_deref(), Some("olap"));
    }

    // ── S4-WS3-04: HTAP sync publishes mutations on COMMIT ───────────────────

    #[tokio::test]
    async fn s4_ws3_04_commit_publishes_insert_to_sync_origin() {
        let state = state_with_key(Some("test-key"));
        let req = crate::SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO events VALUES ('evt-sync-1', 'login')".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        let headers = operator_headers("test-key", "admin");
        let resp = sql_transaction(State(state.clone()), headers, Json(req)).await.unwrap();
        assert_eq!(resp.1.0.status, "committed");
        // Sync origin should have at least one pending mutation
        let origin = state.sync_origin.lock().unwrap();
        assert!(origin.pending_len() >= 1, "commit should have published at least one mutation");
    }

    #[tokio::test]
    async fn s4_ws3_04_htap_export_returns_mutations_after_commit() {
        let state = state_with_key(Some("test-key"));
        // Commit an INSERT
        let tx_req = crate::SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO metrics VALUES ('m-htap-1', 'cpu', 80)".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        let headers = operator_headers("test-key", "admin");
        sql_transaction(State(state.clone()), headers.clone(), Json(tx_req)).await.unwrap();
        // Export since sequence 0
        let export_req = StoreHtapExportRequest { since_sequence: Some(0), max_items: Some(50) };
        let resp = store_htap_export(State(state), headers, Json(export_req)).await.unwrap();
        assert_eq!(resp.1.0.status, "ok");
        assert!(resp.1.0.mutation_count >= 1, "at least one mutation should be exported");
        assert!(resp.1.0.mutations.iter().any(|m| m.op == "insert"));
    }

    // ── S9-WS8A-02: tamper-evident audit chain ───────────────────────────────

    #[tokio::test]
    async fn s9_ws8a_02_audit_chain_verify_clean_chain_is_valid() {
        let state = state_with_key(Some("test-key"));
        // Generate some audit events by running a SQL execute
        let req = SqlExecuteRequest {
            sql_batch: "SELECT 1".to_string(),
            max_rows: None,
        };
        let headers = operator_headers("test-key", "admin");
        sql_execute(State(state.clone()), headers.clone(), Json(req)).await.unwrap();
        // Verify chain
        let resp = audit_chain_verify(State(state), headers).await.unwrap();
        assert_eq!(resp.0.status, "ok");
        assert!(resp.0.chain_valid, "chain should be valid for unmodified audit log");
    }

    #[tokio::test]
    async fn s9_ws8a_02_audit_chain_events_have_non_empty_hashes() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        // Trigger an audit event
        let req = SqlExecuteRequest { sql_batch: "SELECT now()".to_string(), max_rows: None };
        sql_execute(State(state.clone()), headers.clone(), Json(req)).await.unwrap();
        // Retrieve events and check chain_hash populated
        let sink = state.audit_sink.lock().unwrap();
        let events = sink.all().to_vec();
        drop(sink);
        assert!(!events.is_empty());
        for e in &events {
            assert!(!e.chain_hash.is_empty(), "every event must have a chain_hash");
            assert_ne!(e.chain_hash, "0000000000000000");
        }
    }

    // ─── S4-WS3-03: vectorized columnar scan ─────────────────────────────────

    #[tokio::test]
    async fn s4_ws3_03_columnar_scan_returns_typed_columns_for_committed_rows() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        // Insert rows directly into PagedRowStore so they are visible
        {
            let mut rs = state.row_store.lock().unwrap();
            let xid = rs.begin_xid();
            rs.insert(xid, "user-1", [("age".to_string(), "30".to_string()), ("name".to_string(), "alice".to_string())].into_iter().collect());
            rs.insert(xid, "user-2", [("age".to_string(), "25".to_string()), ("name".to_string(), "bob".to_string())].into_iter().collect());
        }
        let resp = store_columnar_scan(State(state), headers).await.unwrap();
        let body = resp.1.0;
        assert_eq!(body.status, "ok");
        assert_eq!(body.rows_scanned, 2);
        assert!(body.columns_materialized >= 2, "expected at least 2 columns");
        let age_col = body.columns.iter().find(|c| c.name == "age");
        assert!(age_col.is_some(), "age column must be materialized");
        let col = age_col.unwrap();
        assert_eq!(col.type_hint, "int64", "age should be inferred as int64");
    }

    #[tokio::test]
    async fn s4_ws3_03_columnar_scan_empty_store_returns_zero_rows() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let resp = store_columnar_scan(State(state), headers).await.unwrap();
        let body = resp.1.0;
        assert_eq!(body.status, "ok");
        assert_eq!(body.rows_scanned, 0);
        assert_eq!(body.columns_materialized, 0);
    }

    // ─── S6-WS5-03: TLS status ───────────────────────────────────────────────

    #[tokio::test]
    async fn s6_ws5_03_tls_status_returns_contract_flags() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let resp = security_tls_status(State(state), headers).await.unwrap();
        let body = resp.0;
        assert_eq!(body.status, "ok");
        // Default dev config has tls_required = false, mtls_required = false
        assert!(!body.tls_required);
        assert!(!body.mtls_required);
        assert!(!body.cert_rotation_supported); // scaffold only
        assert_eq!(body.cert_source, "not_configured");
        assert_eq!(body.key_source, "not_configured");
        assert!(!body.cert_present);
        assert!(!body.key_present);
        assert!(!body.cert_pair_configured);
    }

    // ─── S6-WS5-04: TDE status ───────────────────────────────────────────────

    #[tokio::test]
    async fn s6_ws5_04_tde_status_reports_encryption_at_rest_required() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let resp = security_tde_status(State(state), headers).await.unwrap();
        let body = resp.0;
        assert_eq!(body.status, "ok");
        // Default config has encryption_at_rest_required = true
        assert!(body.encryption_at_rest_required);
        // KMS key env var not set in test env, so tde_active should be false
        assert!(!body.tde_active);
        assert!(!body.key_env_var.is_empty(), "key_env_var must be non-empty");
    }

    // ─── S9-WS8-02: AI model gateway policy ─────────────────────────────────

    #[tokio::test]
    async fn s9_ws8_02_ai_policy_read_returns_default_isolation_enabled() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let resp = ai_policy(State(state), headers).await.unwrap();
        let body = resp.0;
        assert_eq!(body.status, "ok");
        assert!(body.policy.isolation_enabled, "isolation should be enabled by default");
        assert_eq!(body.policy.max_tokens_per_request, 4096);
        assert_eq!(body.policy.rate_limit_rpm, 60);
    }

    #[tokio::test]
    async fn s9_ws8_02_ai_policy_update_persists_new_values() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let update_req = AiPolicyUpdateRequest {
            isolation_enabled: Some(false),
            allowed_models: Some(vec!["gpt-4o".to_string(), "claude-3".to_string()]),
            max_tokens_per_request: Some(8192),
            rate_limit_rpm: Some(120),
        };
        let resp = ai_policy_update(State(state.clone()), headers.clone(), Json(update_req)).await.unwrap();
        let body = resp.0;
        assert_eq!(body.status, "ok");
        assert!(!body.policy.isolation_enabled);
        assert_eq!(body.policy.allowed_models, vec!["gpt-4o", "claude-3"]);
        assert_eq!(body.policy.max_tokens_per_request, 8192);
        assert_eq!(body.policy.rate_limit_rpm, 120);
        // Read back to confirm persistence
        let read_resp = ai_policy(State(state), headers).await.unwrap();
        assert!(!read_resp.0.policy.isolation_enabled);
        assert_eq!(read_resp.0.policy.max_tokens_per_request, 8192);
    }

    // ─── S4-WS3-04: HTAP OLAP consumer apply ─────────────────────────────────

    #[tokio::test]
    async fn s4_ws3_04_htap_apply_inserts_and_scan_returns_rows() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let payload = serde_json::json!({"name": "alice", "score": "98"}).to_string();
        let req = StoreHtapApplyRequest {
            mutations: vec![
                OlapApplyMutation {
                    sequence: 1,
                    primary_key: "user:1".to_string(),
                    payload_json: payload.clone(),
                    op: "insert".to_string(),
                },
                OlapApplyMutation {
                    sequence: 2,
                    primary_key: "user:2".to_string(),
                    payload_json: serde_json::json!({"name": "bob", "score": "75"}).to_string(),
                    op: "insert".to_string(),
                },
            ],
        };
        let resp = store_htap_apply(State(state.clone()), headers.clone(), Json(req)).await.unwrap();
        assert_eq!(resp.1.0.applied_count, 2);
        assert_eq!(resp.1.0.last_applied_sequence, 2);
        // Scan should return 2 rows
        let scan_resp = store_htap_olap_scan(State(state.clone()), headers.clone()).await.unwrap();
        assert_eq!(scan_resp.1.0.row_count, 2);
    }

    #[tokio::test]
    async fn s4_ws3_04_htap_apply_delete_removes_row() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        // Insert a row first
        let insert_req = StoreHtapApplyRequest {
            mutations: vec![OlapApplyMutation {
                sequence: 1,
                primary_key: "item:99".to_string(),
                payload_json: r#"{"sku":"ABC"}"#.to_string(),
                op: "insert".to_string(),
            }],
        };
        store_htap_apply(State(state.clone()), headers.clone(), Json(insert_req)).await.unwrap();
        // Delete the row
        let delete_req = StoreHtapApplyRequest {
            mutations: vec![OlapApplyMutation {
                sequence: 2,
                primary_key: "item:99".to_string(),
                payload_json: "{}".to_string(),
                op: "delete".to_string(),
            }],
        };
        store_htap_apply(State(state.clone()), headers.clone(), Json(delete_req)).await.unwrap();
        // Scan should return 0 rows
        let scan_resp = store_htap_olap_scan(State(state), headers).await.unwrap();
        assert_eq!(scan_resp.1.0.row_count, 0);
    }

    // ─── S9-WS8A-02: Audit export ─────────────────────────────────────────────

    #[tokio::test]
    async fn s9_ws8a_02_audit_export_returns_buffered_events() {
        let state = state_with_key(Some("test-key"));
        // Emit a few audit events first by calling any handler.
        let headers = operator_headers("test-key", "admin");
        // Call a handler that emits an audit event (health just needs the route)
        // We can directly append via the sink for isolation.
        {
            let mut sink = state.audit_sink.lock().unwrap();
            sink.append(
                voltnuerongrid_audit::AuditEventKind::Sql,
                "test-actor",
                "test-action",
                "ok",
                "{}",
            );
            sink.append(
                voltnuerongrid_audit::AuditEventKind::Security,
                "test-actor",
                "test-security-action",
                "ok",
                "{}",
            );
        }
        let resp = audit_export(State(state.clone()), headers, Query(AuditExportQuery::default())).await.unwrap();
        // At least the 2 events we manually appended
        assert!(resp.1.0.event_count >= 2);
        assert!(!resp.1.0.file_backed); // no VNG_AUDIT_LOG_PATH set in test
        assert!(resp.1.0.audit_log_path.is_none());
    }


    // ─── S2-WS2-02: WAL durability + recovery integration tests ──────────────

    #[tokio::test]
    async fn s2_ws2_02_wal_status_returns_zero_on_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_status(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.wal_len, 0);
        assert_eq!(body.latest_sequence, 0);
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_status_requires_operator_auth() {
        let state = state_with_key(Some("test-key"));
        let err = match wal_status(State(state), HeaderMap::new()).await {
            Ok(_) => panic!("wal_status should reject unauthenticated calls"),
            Err(err) => err,
        };
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s2_ws2_02_commit_writes_wal_records() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let tx_req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO items (id, name) VALUES ('item:1', 'alpha')".to_string(),
                "INSERT INTO items (id, name) VALUES ('item:2', 'beta')".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        sql_transaction(State(state.clone()), headers, Json(tx_req)).await.ok();
        let (status, Json(body)) = wal_status(
            State(state),
            operator_headers("test-key", "admin"),
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.wal_len >= 2, "WAL should have at least 2 records after COMMIT");
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_recover_dry_run_does_not_change_row_store() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let tx_req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO orders (id, total) VALUES ('ord:1', '99')".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        sql_transaction(State(state.clone()), headers, Json(tx_req)).await.ok();
        let rows_before = { let rs = state.row_store.lock().unwrap(); rs.visible_row_count(rs.current_xid()) };
        let recover_req = WalRecoverRequest { dry_run: Some(true) };
        let (_, Json(body)) = wal_recover(
            State(state.clone()),
            axum::extract::Json(recover_req),
        ).await;
        assert!(body.dry_run);
        assert!(body.records_replayed >= 1);
        let rows_after = { let rs = state.row_store.lock().unwrap(); rs.visible_row_count(rs.current_xid()) };
        assert_eq!(rows_before, rows_after, "dry_run must not modify row store");
    }

    // ─── S7-WS6-04: Chaos injection integration tests ────────────────────────

    #[tokio::test]
    async fn s7_ws6_04_chaos_status_returns_empty_initially() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (_, Json(body)) = chaos_status(State(state), headers).await.unwrap();
        assert_eq!(body.active_fault_count, 0);
        assert_eq!(body.total_injected, 0);
    }

    #[tokio::test]
    async fn s7_ws6_04_chaos_status_requires_operator_auth() {
        let state = state_with_key(Some("test-key"));
        let err = match chaos_status(State(state), HeaderMap::new()).await {
            Ok(_) => panic!("chaos status should reject unauthenticated calls"),
            Err(err) => err,
        };
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s7_ws6_04_chaos_inject_records_active_fault() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let body = ChaosInjectRequest {
            fault_type: "network_partition".to_string(),
            target_node: Some("node-2".to_string()),
            parameters: [("loss_pct".to_string(), "50".to_string())].into_iter().collect(),
        };
        let (ok_status, _) = chaos_inject(State(state.clone()), headers.clone(), axum::extract::Json(body))
            .await
            .unwrap();
        assert_eq!(ok_status, StatusCode::OK);
        let (_, Json(status)) = chaos_status(State(state), headers).await.unwrap();
        assert_eq!(status.active_fault_count, 1);
        assert_eq!(status.total_injected, 1);
        assert_eq!(status.active_faults[0].fault_type, "network_partition");
    }

    #[tokio::test]
    async fn s7_ws6_04_chaos_clear_removes_active_faults() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        for fault in ["node_crash", "packet_loss"] {
            let body = ChaosInjectRequest {
                fault_type: fault.to_string(),
                target_node: None,
                parameters: HashMap::new(),
            };
            let _ = chaos_inject(State(state.clone()), headers.clone(), axum::extract::Json(body))
                .await
                .unwrap();
        }
        let (_, Json(before)) = chaos_status(State(state.clone()), headers.clone()).await.unwrap();
        assert_eq!(before.active_fault_count, 2);
        let _ = chaos_clear(State(state.clone()), headers.clone()).await.unwrap();
        let (_, Json(after)) = chaos_status(State(state), headers).await.unwrap();
        assert_eq!(after.active_fault_count, 0, "active faults should be cleared");
        assert_eq!(after.total_injected, 2, "history should be preserved");
    }

    // ─── S3-WS1-05 + S4-WS3-03: planner filter pushdown integration tests ────

    #[tokio::test]
    async fn s3_ws1_05_olap_filter_pushdown_reduces_batch() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let tx_req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO products (id, category) VALUES ('p:1', 'electronics')".to_string(),
                "INSERT INTO products (id, category) VALUES ('p:2', 'books')".to_string(),
                "INSERT INTO products (id, category) VALUES ('p:3', 'electronics')".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        sql_transaction(State(state.clone()), headers.clone(), Json(tx_req)).await.ok();
        let exec_req = SqlExecuteRequest {
            sql_batch: "SELECT COUNT(*) FROM products GROUP BY category".to_string(),
            max_rows: None,
        };
        let resp = sql_execute(State(state), headers, Json(exec_req)).await.unwrap();
        assert_eq!(resp.1.0.planner_path.as_deref(), Some("olap"));
        assert!(resp.1.0.olap_agg_results.is_some());
    }
    // ─── S7-WS6-02: Raft consensus ────────────────────────────────────────────

    #[tokio::test]
    async fn s7_ws6_02_raft_status_returns_follower_at_term_0() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let resp = raft_status(State(state), headers).await.unwrap();
        assert_eq!(resp.0.status, "ok");
        assert_eq!(resp.0.raft.current_term, 0);
        assert!(matches!(resp.0.raft.role, raft::RaftRole::Follower));
        assert_eq!(resp.0.raft.log_length, 0);
    }

    #[tokio::test]
    async fn s7_ws6_02_raft_vote_grants_to_higher_term_candidate() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = RaftVoteRequest {
            term: 5,
            candidate_id: "node-2".to_string(),
            last_log_index: 0,
            last_log_term: 0,
        };
        let resp = raft_vote(State(state), headers, Json(req)).await.unwrap();
        assert!(resp.0.vote_granted);
        assert_eq!(resp.0.term, 5);
    }

    #[tokio::test]
    async fn s7_ws6_02_raft_append_adds_entries_to_log() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let entries = vec![
            raft::RaftLogEntry { index: 1, term: 1, command: "INSERT INTO t VALUES (1)".to_string() },
        ];
        let req = RaftAppendRequest {
            term: 1,
            leader_id: "node-2".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries,
            leader_commit: 1,
        };
        let resp = raft_append(State(state.clone()), headers.clone(), Json(req)).await.unwrap();
        assert!(resp.0.success);
        assert_eq!(resp.0.match_index, 1);
        // Verify log grew
        let status_resp = raft_status(State(state), headers).await.unwrap();
        assert_eq!(status_resp.0.raft.log_length, 1);
        assert_eq!(status_resp.0.raft.commit_index, 1);
    }

    // ── S2-WS2-05: Transaction isolation stats endpoint tests ─────────────────

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
            acid.begin("tx-iso-1", "node-1", "serializable", 0u128);
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = sql_transactions_isolation(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.active_count, 1);
        assert_eq!(body.transactions[0].transaction_id, "tx-iso-1");
        assert_eq!(body.transactions[0].isolation_level, "serializable");
    }

    // ─── S2-WS2-05: Write-write conflict detection ────────────────────────────

    #[tokio::test]
    async fn s2_ws2_05_second_commit_on_same_key_returns_conflict() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        // First transaction: insert user:conflict into row_store
        let tx_req1 = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO users (id, name) VALUES ('user:conflict', 'alice')".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        let resp1 = sql_transaction(State(state.clone()), headers.clone(), Json(tx_req1)).await;
        assert!(resp1.is_ok(), "first tx should commit without error");
        // Second transaction targeting same key — was_modified_after should be true
        // because the first tx advanced the xid without our snapshot capturing it.
        // We simulate this by using snapshot_xid = 0 (the test starts at xid=0).
        // The conflict detection checks was_modified_after(key, snapshot_xid_at_start=0).
        let tx_req2 = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO users (id, name) VALUES ('user:conflict', 'bob')".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        let resp2 = sql_transaction(State(state.clone()), headers.clone(), Json(tx_req2)).await;
        // The second tx should detect a write-write conflict (409) because user:conflict
        // was already committed by tx1, so was_modified_after returns true.
        assert!(
            resp2.is_err(),
            "second commit on same key should return a write-write conflict (409)"
        );
        let err = resp2.unwrap_err();
        assert_eq!(err.0, StatusCode::CONFLICT);
        assert!(err.1.0.reason.contains("write_write_conflict"));
    }

    #[tokio::test]
    async fn s2_ws2_05_different_keys_do_not_conflict() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let tx_req1 = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO orders (id, amount) VALUES ('order:A', '100')".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        let resp1 = sql_transaction(State(state.clone()), headers.clone(), Json(tx_req1)).await;
        assert!(resp1.is_ok());
        let tx_req2 = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO orders (id, amount) VALUES ('order:B', '200')".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        let resp2 = sql_transaction(State(state.clone()), headers.clone(), Json(tx_req2)).await;
        assert!(resp2.is_ok(), "different keys should not conflict: {:?}", resp2);
    }

    // ─── S7-WS6-03: Raft election timeout endpoint ───────────────────────────

    #[tokio::test]
    async fn s7_ws6_03_raft_tick_increments_counter() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let resp = raft_tick(State(state), headers).await.unwrap();
        assert_eq!(resp.0.status, "ok");
        assert_eq!(resp.0.ticks_since_heartbeat, 1);
        assert!(!resp.0.election_triggered);
    }

    #[tokio::test]
    async fn s7_ws6_03_raft_tick_triggers_election_after_timeout() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        // Default timeout is 10 ticks; fire 9 ticks without triggering.
        for _ in 0..9 {
            raft_tick(State(state.clone()), headers.clone()).await.unwrap();
        }
        let snap = raft_status(State(state.clone()), headers.clone()).await.unwrap();
        assert_eq!(snap.0.raft.role, raft::RaftRole::Follower);
        // 10th tick triggers election.
        let resp = raft_tick(State(state.clone()), headers.clone()).await.unwrap();
        assert!(resp.0.election_triggered, "10th tick must trigger election");
        assert_eq!(resp.0.role, raft::RaftRole::Candidate);
        assert_eq!(resp.0.current_term, 1);
    }

    // ─── S4-WS3-02: OLAP vectorized executor ─────────────────────────────────

    #[tokio::test]
    async fn s4_ws3_02_olap_agg_results_populated_for_aggregate_query() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        // Seed some rows via COMMIT so the OLAP executor has data.
        let tx_req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO metrics (id, value) VALUES ('m:1', '10')".to_string(),
                "INSERT INTO metrics (id, value) VALUES ('m:2', '20')".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        sql_transaction(State(state.clone()), headers.clone(), Json(tx_req)).await.ok();
        // Aggregate query → planner_path = "olap"
        let exec_req = SqlExecuteRequest {
            sql_batch: "SELECT COUNT(*) FROM metrics GROUP BY value".to_string(),
            max_rows: None,
        };
        let resp = sql_execute(State(state.clone()), headers.clone(), Json(exec_req)).await.unwrap();
        assert_eq!(resp.1.0.planner_path.as_deref(), Some("olap"));
        assert!(
            resp.1.0.olap_agg_results.is_some(),
            "OLAP aggregate query should populate olap_agg_results"
        );
        let agg = resp.1.0.olap_agg_results.unwrap();
        assert!(!agg.is_empty(), "agg results should have at least one column");
    }

    // ─── S9-WS8-02: Rate limiter ─────────────────────────────────────────────

    #[tokio::test]
    async fn s9_ws8_02_ai_request_rate_check_allows_within_limit() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let body = AiRequestBody { model_id: "gpt-4o".to_string(), tokens: Some(100) };
        let resp = ai_rate_check(State(state), headers, Json(body)).await.unwrap();
        assert_eq!(resp.0, StatusCode::OK);
        assert_eq!(resp.1.0.status, "ok");
        assert_eq!(resp.1.0.request_count, 1);
        assert!(resp.1.0.tokens_checked);
    }

    #[tokio::test]
    async fn s9_ws8_02_ai_request_rate_check_rejects_over_token_limit() {
        let state = state_with_key(Some("test-key"));
        // Set a tight token limit.
        {
            let mut p = state.model_gateway_policy.lock().unwrap();
            p.max_tokens_per_request = 50;
        }
        let headers = operator_headers("test-key", "admin");
        let body = AiRequestBody { model_id: "gpt-4o".to_string(), tokens: Some(100) };
        let resp = ai_rate_check(State(state), headers, Json(body)).await;
        assert!(resp.is_err());
        let err = resp.unwrap_err();
        assert_eq!(err.0, StatusCode::TOO_MANY_REQUESTS);
        assert!(err.1.0.reason.contains("token_limit_exceeded"));
    }

    // ─── S8-WS10-02: Driver wire protocol integration tests ──────────────────

    #[tokio::test]
    async fn s8_ws10_02_protocol_info_returns_version() {
        let (status, Json(body)) = driver_protocol_info().await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.protocol_version, "1.0");
        assert_eq!(body.encoding, "json");
        assert!(body.max_batch_size >= 100);
        assert!(body.auth_modes.contains(&"admin_key".to_string()));
        assert!(body.supported_statements.contains(&"SELECT".to_string()));
    }

    #[tokio::test]
    async fn s8_ws10_02_driver_connect_issues_session_token() {
        let state = state_with_key(Some("test-key"));
        let req = DriverConnectRequest {
            driver_name: "rust-driver".to_string(),
            driver_version: "0.1.0".to_string(),
            requested_capabilities: Some(vec![
                "batch_execute".to_string(),
                "unknown_cap".to_string(),
            ]),
        };
        let (status, Json(body)) = driver_connect(State(state.clone()), Json(req)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "connected");
        assert!(body.session_token.starts_with("drv-sess-"), "token should have drv-sess- prefix");
        // unknown_cap should be filtered out; only batch_execute negotiated
        assert_eq!(body.negotiated_capabilities, vec!["batch_execute".to_string()]);
        // Session should be stored
        let sessions = state.driver_sessions.lock().unwrap();
        assert!(sessions.contains_key(&body.session_token));
    }

    #[tokio::test]
    async fn s8_ws10_02_driver_connect_acquires_pool_connection() {
        let state = state_with_key(Some("test-key"));
        let req = DriverConnectRequest {
            driver_name: "pool-aware-driver".to_string(),
            driver_version: "0.2.0".to_string(),
            requested_capabilities: None,
        };

        let (_, Json(body)) = driver_connect(State(state.clone()), Json(req)).await;
        let sessions = state.driver_sessions.lock().unwrap();
        let session = sessions
            .get(&body.session_token)
            .expect("connected session must exist");
        assert!(
            session.pooled_connection_id.is_some(),
            "driver session should own a pooled connection id"
        );
    }

    // ─── S10-WS15-02: CDC stream integration tests ───────────────────────────

    #[tokio::test]
    async fn s10_ws15_02_cdc_stream_returns_empty_on_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let (status, Json(body)) = cdc_stream(State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.event_count, 0);
        assert!(body.events.is_empty());
    }

    #[tokio::test]
    async fn s10_ws15_02_cdc_stream_returns_events_after_commit() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let tx_req = SqlTransactionRequest {
            statements: vec![
                "BEGIN".to_string(),
                "INSERT INTO cdc_test (id, val) VALUES ('cdc:1', 'alpha')".to_string(),
                "INSERT INTO cdc_test (id, val) VALUES ('cdc:2', 'beta')".to_string(),
                "COMMIT".to_string(),
            ],
            isolation_level: None,
        };
        sql_transaction(State(state.clone()), headers, Json(tx_req)).await.ok();
        let (status, Json(body)) = cdc_stream(State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.event_count >= 2, "CDC stream should have at least 2 events after COMMIT");
        assert!(body.events.iter().all(|e| !e.key.is_empty()));
    }

    // ── S5-WS4-03: Ingest schema registry endpoint tests ─────────────────────

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

    // ─── S5-WS4-03: Ingest schema list endpoint tests ────────────────────────

    #[tokio::test]
    async fn s5_ws4_03_ingest_schema_list_no_filter_returns_all_formats() {
        use voltnuerongrid_ingest::IngestRecord;
        let state = state_with_key(Some("test-key"));
        {
            let mut csv = state.ingest_csv_records.lock().unwrap();
            csv.insert("csv-orders".to_string(), vec![
                IngestRecord { key: "r1".to_string(), payload: "id=1".to_string() },
            ]);
            let mut json = state.ingest_json_records.lock().unwrap();
            json.insert("json-events".to_string(), vec![
                IngestRecord { key: "e1".to_string(), payload: r#"{"id":1}"#.to_string() },
            ]);
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = ingest_schema_list(
            State(state), headers, Query(IngestSchemaListQuery { format: None }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.connector_count, 2, "no filter returns both csv and json entries");
        assert!(body.format_filter.is_none());
    }

    #[tokio::test]
    async fn s5_ws4_03_ingest_schema_list_csv_filter_excludes_json() {
        use voltnuerongrid_ingest::IngestRecord;
        let state = state_with_key(Some("test-key"));
        {
            let mut csv = state.ingest_csv_records.lock().unwrap();
            csv.insert("csv-orders".to_string(), vec![
                IngestRecord { key: "r1".to_string(), payload: "id=1".to_string() },
            ]);
            let mut json = state.ingest_json_records.lock().unwrap();
            json.insert("json-events".to_string(), vec![
                IngestRecord { key: "e1".to_string(), payload: r#"{"id":1}"#.to_string() },
            ]);
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = ingest_schema_list(
            State(state), headers, Query(IngestSchemaListQuery { format: Some("csv".to_string()) }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.connector_count, 1, "csv filter must return only csv entries");
        assert_eq!(body.entries[0].format, "csv");
        assert_eq!(body.format_filter.as_deref(), Some("csv"));
    }

    // ─── S5-WS4-03: Ingest format detect endpoint tests ──────────────────────

    #[tokio::test]
    async fn s5_ws4_03_ingest_format_detect_csv_sample() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = IngestFormatDetectRequest {
            sample_data: "id,name,email
1,Alice,a@x.com
".to_string(),
        };
        let (status, Json(body)) = ingest_format_detect(
            State(state), headers, Json(req),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.detected_format, "csv");
        assert_eq!(body.field_count, 3);
        assert!(body.confidence >= 0.8, "csv confidence must be >= 0.8");
    }

    #[tokio::test]
    async fn s5_ws4_03_ingest_format_detect_json_sample() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = IngestFormatDetectRequest {
            sample_data: r#"{"id": 1, "name": "Bob", "score": 42}"#.to_string(),
        };
        let (status, Json(body)) = ingest_format_detect(
            State(state), headers, Json(req),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.detected_format, "json");
        assert_eq!(body.field_count, 3);
        assert!(body.confidence >= 0.9, "json confidence must be >= 0.9");
    }

    // ─── S5-WS4-04: Connector validation tests ──────────────────────────────

    #[tokio::test]
    async fn s5_ws4_04_ingest_connector_validate_json_format() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = IngestConnectorValidateRequest {
            connector_id: "conn-1".to_string(),
            format: "json".to_string(),
            config_json: r#"{"batch_size": 100}"#.to_string(),
        };
        let (status, Json(body)) = ingest_connector_validate(
            State(state), headers, Json(req),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.valid, "valid JSON config with known format must pass");
        assert!(body.issues.is_empty(), "no issues for a valid request");
    }

    #[tokio::test]
    async fn s5_ws4_04_ingest_connector_validate_unknown_format_is_invalid() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = IngestConnectorValidateRequest {
            connector_id: "conn-2".to_string(),
            format: "xml".to_string(),
            config_json: r#"{"tag": "row"}"#.to_string(),
        };
        let (status, Json(body)) = ingest_connector_validate(
            State(state), headers, Json(req),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.valid, "unknown format must fail validation");
        assert!(!body.issues.is_empty(), "issues must describe the format error");
    }

    // ─── S5-WS4A-02: Broker adapter integration tests ────────────────────────

    #[tokio::test]
    async fn s5_ws4a_02_broker_status_lists_adapters() {
        let state = state_with_key(Some("test-key"));
        let (status, Json(body)) = outbox_broker_status(State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.adapters.len(), 3);
        let types: Vec<&str> = body.adapters.iter().map(|a| a.broker_type.as_str()).collect();
        assert!(types.contains(&"kafka"));
        assert!(types.contains(&"nats"));
        assert!(types.contains(&"event_hubs"));
        // All disabled in scaffold
        assert!(body.adapters.iter().all(|a| !a.enabled));
        // All flush counts zero on fresh state
        assert!(body.adapters.iter().all(|a| a.flush_count == 0));
    }

    #[tokio::test]
    async fn s5_ws4a_02_broker_flush_increments_count() {
        let state = state_with_key(Some("test-key"));
        // Flush kafka twice
        for _ in 0..2 {
            let req = BrokerFlushRequest { broker_type: "kafka".to_string(), max_events: Some(10) };
            let (status, Json(body)) = outbox_broker_flush(State(state.clone()), Json(req)).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body.status, "ok");
            assert_eq!(body.broker_type, "kafka");
        }
        // Status should now show flush_count == 2 for kafka
        let (_, Json(status_body)) = outbox_broker_status(State(state)).await;
        let kafka = status_body.adapters.iter().find(|a| a.broker_type == "kafka").unwrap();
        assert_eq!(kafka.flush_count, 2);
    }


    // ─── S5-E4A-01: Connector SDK runtime load tests ────────────────────────────

    #[tokio::test]
    async fn s5_e4a_01_register_connector_ok() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = ConnectorRegisterRequest {
            connector_id: "my-kafka-src".to_string(),
            connector_type: "kafka-source".to_string(),
            version: "1.0.0".to_string(),
            signed: Some(true),
        };
        let (status, Json(body)) = connector_register(State(state.clone()), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.connector_id, "my-kafka-src");
        assert!(body.registered_at_ms > 0);
    }

    #[tokio::test]
    async fn s5_e4a_01_list_connectors_includes_registered() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        for (id, ctype) in &[("conn-1", "csv-source"), ("conn-2", "nats-sink")] {
            let req = ConnectorRegisterRequest {
                connector_id: id.to_string(),
                connector_type: ctype.to_string(),
                version: "0.1.0".to_string(),
                signed: None,
            };
            connector_register(State(state.clone()), headers.clone(), Json(req)).await.unwrap();
        }
        let (status, Json(body)) = connector_list(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.connector_count, 2);
        let ids: Vec<&str> = body.connectors.iter().map(|c| c.connector_id.as_str()).collect();
        assert!(ids.contains(&"conn-1"));
        assert!(ids.contains(&"conn-2"));
    }

    // ── S7-WS6-02: Raft commit progress endpoint tests ──────────────────────

    #[tokio::test]
    async fn s7_ws6_02_raft_commit_progress_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_commit_progress(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.commit_index, 0);
        assert_eq!(body.log_length, 0);
        assert_eq!(body.uncommitted, 0);
    }

    #[tokio::test]
    async fn s7_ws6_02_raft_commit_progress_after_log_append() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        {
            let mut node = state.raft_state.lock().unwrap();
            node.log.push(raft::RaftLogEntry { index: 1, term: 1, command: "SET x=1".to_string() });
        }
        let (status, Json(body)) = raft_commit_progress(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.log_length, 1);
        assert_eq!(body.uncommitted, 1, "log has 1 entry, commit_index=0 => uncommitted=1");
    }

    // ── S7-WS6-02: Raft snapshot endpoint tests ───────────────────────────

    #[tokio::test]
    async fn s7_ws6_02_raft_snapshot_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_snapshot(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.term, 0, "fresh node has term 0");
        assert_eq!(body.commit_index, 0);
        assert_eq!(body.log_length, 0);
        assert_eq!(body.fencing_token, 0);
    }

    #[tokio::test]
    async fn s7_ws6_02_raft_snapshot_reflects_term_update() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        {
            let mut node = state.raft_state.lock().unwrap();
            node.current_term = 5;
            node.commit_index = 3;
        }
        let (status, Json(body)) = raft_snapshot(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.term, 5, "snapshot must reflect updated term");
        assert_eq!(body.commit_index, 3);
    }

    // ─── S7-WS6-03: Raft leader endpoint tests ───────────────────────────
    #[tokio::test]
    async fn s7_ws6_03_raft_leader_fresh_state_is_follower() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_leader(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert!(!body.is_leader, "fresh node starts as Follower, not leader");
        assert_eq!(body.current_term, 0);
    }

    #[tokio::test]
    async fn s7_ws6_03_raft_leader_reflects_term_after_vote() {
        let state = state_with_key(Some("test-key"));
        {
            let mut node = state.raft_state.lock().unwrap();
            node.current_term = 5;
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_leader(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.current_term, 5, "leader response must reflect updated term");
    }

    // ─── S7-WS6-01: Raft vote statistics tests ───────────────────────────────

    #[tokio::test]
    async fn s7_ws6_01_raft_vote_stats_fresh_state_shows_zero_counts() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_vote_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.total_votes_granted, 0, "fresh node must have zero votes granted");
        assert_eq!(body.total_votes_rejected, 0, "fresh node must have zero votes rejected");
    }

    #[tokio::test]
    async fn s7_ws6_01_raft_vote_stats_reflects_current_term() {
        let state = state_with_key(Some("test-key"));
        {
            let mut node = state.raft_state.lock().unwrap();
            node.current_term = 7;
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_vote_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.current_term, 7, "vote stats must reflect current raft term");
    }

    // ─── S7-WS6-03: Raft fencing token tests ─────────────────────────────

    #[tokio::test]
    async fn s7_ws6_03_fencing_token_zero_on_follower() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_fence(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.fencing_token, 0, "fresh follower fencing token must be 0");
        assert_eq!(body.current_term, 0);
    }

    #[tokio::test]
    async fn s7_ws6_03_fencing_token_advances_on_election() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        {
            let mut raft = state.raft_state.lock().unwrap();
            raft.become_candidate();
            raft.become_leader();
        }
        let (status, Json(body)) = raft_fence(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.fencing_token, 1, "fencing token must advance after becoming leader");
    }

    // ─── S9-WS8A-02: Audit export pagination tests ─────────────────────────

    #[tokio::test]
    async fn s9_ws8a_02_audit_export_pagination_limit_respected() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        {
            let mut sink = state.audit_sink.lock().unwrap();
            for i in 0..5 {
                sink.append(
                    voltnuerongrid_audit::AuditEventKind::Sql,
                    "test-actor",
                    &format!("action-{i}"),
                    "ok",
                    "{}",
                );
            }
        }
        let params = AuditExportQuery { cursor: Some(0), limit: Some(2) };
        let resp = audit_export(State(state), headers, Query(params)).await.unwrap();
        assert_eq!(resp.1.0.event_count, 2, "limit=2 should return exactly 2 events");
        assert_eq!(resp.1.0.total_event_count, 5, "total should still be 5");
        assert_eq!(resp.1.0.limit, 2);
        assert_eq!(resp.1.0.cursor, 0);
    }

    #[tokio::test]
    async fn s9_ws8a_02_audit_export_pagination_cursor_advances() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        {
            let mut sink = state.audit_sink.lock().unwrap();
            for i in 0..4 {
                sink.append(
                    voltnuerongrid_audit::AuditEventKind::Security,
                    "actor",
                    &format!("op-{i}"),
                    "ok",
                    "{}",
                );
            }
        }
        let params = AuditExportQuery { cursor: Some(2), limit: Some(10) };
        let resp = audit_export(State(state), headers, Query(params)).await.unwrap();
        assert_eq!(resp.1.0.event_count, 2, "cursor=2 leaves 2 remaining events");
        assert_eq!(resp.1.0.cursor, 2);
        assert_eq!(resp.1.0.total_event_count, 4);
    }

    // ─── S6-WS5-04: TDE toggle tests ───────────────────────────────────────────

    #[tokio::test]
    async fn s6_ws5_04_tde_toggle_enables_tde() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = TdeToggleRequest { enable: true };
        let (status, Json(body)) = security_tde_toggle(State(state.clone()), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.tde_active);
        assert!(body.override_applied);
        let stored = *state.tde_override.lock().unwrap();
        assert_eq!(stored, Some(true));
    }

    #[tokio::test]
    async fn s6_ws5_04_tde_toggle_disables_tde() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = TdeToggleRequest { enable: false };
        let (status, Json(body)) = security_tde_toggle(State(state.clone()), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.tde_active);
        assert!(body.override_applied);
        let stored = *state.tde_override.lock().unwrap();
        assert_eq!(stored, Some(false));
    }

    // ─── S6-WS5-04: TDE override-status endpoint ─────────────────────────────

    #[tokio::test]
    async fn s6_ws5_04_tde_override_status_no_override_set() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = security_tde_override_status(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.override_set, "override not set on fresh state");
        assert_eq!(body.override_value, None);
        // encryption_at_rest_required defaults to true in state_with_key
        assert!(body.effective_tde_active, "effective = config default when no override");
    }

    #[tokio::test]
    async fn s6_ws5_04_tde_override_status_after_toggle_reflects_override() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        // Disable TDE via toggle first.
        let toggle_req = TdeToggleRequest { enable: false };
        security_tde_toggle(State(state.clone()), headers.clone(), Json(toggle_req)).await.unwrap();
        // Now check override status.
        let (status, Json(body)) = security_tde_override_status(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.override_set, "override must be set after toggle");
        assert_eq!(body.override_value, Some(false));
        assert!(!body.effective_tde_active, "effective must be false after disable toggle");
    }

    // ─── S9-WS8-02: Sliding window rate limiter test ─────────────────────────

    #[tokio::test]
    async fn s9_ws8_02_rate_window_counter_increments_within_window() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        // Make two requests for the same model — counter should increment.
        for i in 1u64..=2 {
            let body = AiRequestBody { model_id: "test-model".to_string(), tokens: Some(10) };
            let resp = ai_rate_check(State(state.clone()), headers.clone(), Json(body))
                .await
                .unwrap();
            assert_eq!(resp.0, StatusCode::OK);
            assert_eq!(resp.1.0.request_count, i,
                "request_count should be {i} after {i} call(s)");
        }
    }

    // ─── S9-WS8-02: Model allowlist enforcement tests ────────────────────────

    #[tokio::test]
    async fn s9_ws8_02_ai_request_allowlist_rejects_unlisted_model() {
        let state = state_with_key(Some("test-key"));
        {
            let mut p = state.model_gateway_policy.lock().unwrap();
            p.allowed_models = vec!["gpt-4o".to_string()];
        }
        let headers = operator_headers("test-key", "admin");
        let body = AiRequestBody { model_id: "claude-3-opus".to_string(), tokens: Some(10) };
        let resp = ai_rate_check(State(state), headers, Json(body)).await;
        assert!(resp.is_err(), "unlisted model must be rejected");
        assert_eq!(resp.unwrap_err().0, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn s9_ws8_02_ai_request_allowlist_permits_listed_model() {
        let state = state_with_key(Some("test-key"));
        {
            let mut p = state.model_gateway_policy.lock().unwrap();
            p.allowed_models = vec!["gpt-4o".to_string(), "claude-3-opus".to_string()];
        }
        let headers = operator_headers("test-key", "admin");
        let body = AiRequestBody { model_id: "gpt-4o".to_string(), tokens: Some(10) };
        let resp = ai_rate_check(State(state), headers, Json(body)).await.unwrap();
        assert_eq!(resp.0, StatusCode::OK);
    }

    // ─── S10-WS15-02: CDC cursor tracking tests ──────────────────────────────

    #[tokio::test]
    async fn s10_ws15_02_cdc_cursor_fresh_state_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let (status, Json(body)) = cdc_cursor_status(
            State(state),
            Query(CdcCursorQuery { table: "orders".to_string() }),
    ).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.table_name, "orders");
        assert_eq!(body.cursor_position, 0, "fresh state must return cursor 0");
    }


    #[tokio::test]
    async fn s10_ws15_02_cdc_cursor_advance_and_read() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        // Advance cursor to position 42
        let req = CdcCursorAdvanceRequest { table_name: "orders".to_string(), position: 42 };
        let (status, Json(body)) = cdc_cursor_advance(
            State(state.clone()), headers, Json(req),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.cursor_position, 42);
        // Read it back
        let (status2, Json(body2)) = cdc_cursor_status(
            State(state),
            Query(CdcCursorQuery { table: "orders".to_string() }),
        ).await;
        assert_eq!(status2, StatusCode::OK);
        assert_eq!(body2.cursor_position, 42, "cursor must persist after advance");
    }

    // ─── S10-WS15-02: CDC cursor rewind tests ────────────────────────────────
    #[tokio::test]
    async fn s10_ws15_02_cdc_cursor_rewind_sets_cursor_to_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let adv = CdcCursorAdvanceRequest { table_name: "events".to_string(), position: 77 };
        cdc_cursor_advance(State(state.clone()), headers.clone(), Json(adv)).await.unwrap();
        let req = CdcCursorRewindRequest { table_name: "events".to_string() };
        let (status, Json(body)) = cdc_cursor_rewind(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.cursor_position, 0, "rewind must reset cursor to 0");
    }

    #[tokio::test]
    async fn s10_ws15_02_cdc_cursor_rewind_unknown_table_creates_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = CdcCursorRewindRequest { table_name: "new_table".to_string() };
        let (status, Json(body)) = cdc_cursor_rewind(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.cursor_position, 0, "rewind on new table must create cursor at 0");
    }

    // ─── S10-WS15-02: CDC metrics tests ──────────────────────────────────────
    #[tokio::test]
    async fn s10_ws15_02_cdc_metrics_empty_state_returns_zero_counts() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = cdc_metrics(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.total_events, 0);
        assert_eq!(body.insert_count, 0);
        assert_eq!(body.delete_count, 0);
        assert_eq!(body.tables_seen, 0);
    }

    #[tokio::test]
    async fn s10_ws15_02_cdc_metrics_after_mutations_counts_inserts() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("orders:1", "val1");
            wal.append_mutation("orders:2", "val2");
            wal.append_mutation("users:1", "val3");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = cdc_metrics(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_events, 3);
        assert_eq!(body.insert_count, 3, "all are inserts (not __deleted__)");
        assert_eq!(body.delete_count, 0);
        assert_eq!(body.tables_seen, 2, "orders and users are 2 distinct table prefixes");
    }

    // ─── S2-WS2-04: Row store snapshot export tests ───────────────────────────

    #[tokio::test]
    async fn s2_ws2_04_row_snapshot_empty_on_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = row_store_snapshot(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.row_count, 0, "empty store must return 0 rows");
        assert!(body.rows.is_empty());
    }

    // ── S2-WS2-04: Row store stats endpoint ─────────────────────────────────
    #[tokio::test]
    async fn s2_ws2_04_row_store_stats_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = row_store_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.total_visible_rows, 0, "fresh store has no visible rows");
        assert!(body.total_pages >= 1, "store always has at least one page");
    }

    #[tokio::test]
    async fn s2_ws2_04_row_store_stats_reflects_inserted_rows() {
        let state = state_with_key(Some("test-key"));
        {
            let mut rs = state.row_store.lock().unwrap();
            let xid = rs.begin_xid();
            let mut d = std::collections::HashMap::new();
            d.insert("col".to_string(), "val".to_string());
            rs.insert(xid, "stats-row-1", d.clone());
            rs.insert(xid, "stats-row-2", d);
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = row_store_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_rows, 2, "two rows inserted");
        assert_eq!(body.total_visible_rows, 2, "both rows visible at head xid");
    }

    // ── S2-WS2-04: Row store prefix count endpoint tests ─────────────────

    #[tokio::test]
    async fn s2_ws2_04_row_count_empty_store_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = row_store_count(
            State(state),
            headers,
            Query(RowCountQuery { key_prefix: None }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.count, 0, "empty store has 0 rows");
        assert!(body.key_prefix.is_none());
    }

    #[tokio::test]
    async fn s2_ws2_04_row_count_with_prefix_filters_correctly() {
        let state = state_with_key(Some("test-key"));
        {
            let mut rs = state.row_store.lock().unwrap();
            let xid = rs.begin_xid();
            rs.insert(xid, "orders:1", std::collections::HashMap::from([("v".to_string(), "a".to_string())]));
            rs.insert(xid, "orders:2", std::collections::HashMap::from([("v".to_string(), "b".to_string())]));
            rs.insert(xid, "products:1", std::collections::HashMap::from([("v".to_string(), "c".to_string())]));
        }
        let headers = operator_headers("test-key", "admin");
        // Count all rows
        let (_, Json(all)) = row_store_count(
            State(state.clone()),
            headers.clone(),
            Query(RowCountQuery { key_prefix: None }),
        ).await.unwrap();
        assert_eq!(all.count, 3, "3 total rows");
        // Count only orders:* prefix
        let (_, Json(filtered)) = row_store_count(
            State(state),
            headers,
            Query(RowCountQuery { key_prefix: Some("orders:".to_string()) }),
        ).await.unwrap();
        assert_eq!(filtered.count, 2, "2 orders rows match the prefix");
        assert_eq!(filtered.key_prefix.as_deref(), Some("orders:"));
    }

    // ── S2-WS2-04: Row store delete-by-key endpoint tests ─────────────────────

    #[tokio::test]
    async fn s2_ws2_04_row_delete_existing_key_returns_deleted_true() {
        let state = state_with_key(Some("test-key"));
        {
            let mut rs = state.row_store.lock().unwrap();
            let xid = rs.begin_xid();
            rs.insert(xid, "orders:99", std::collections::HashMap::from([("v".to_string(), "x".to_string())]));
        }
        let headers = operator_headers("test-key", "admin");
        let req = RowDeleteRequest { key: "orders:99".to_string() };
        let (status, Json(body)) = row_store_delete(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.deleted, "existing key must report deleted = true");
        assert_eq!(body.key, "orders:99");
    }

    #[tokio::test]
    async fn s2_ws2_04_row_delete_missing_key_returns_deleted_false() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = RowDeleteRequest { key: "no-such-key".to_string() };
        let (status, Json(body)) = row_store_delete(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.deleted, "missing key must report deleted = false");
    }

    #[tokio::test]
    async fn s2_ws2_04_row_snapshot_shows_inserted_rows() {
        let state = state_with_key(Some("test-key"));
        // Insert two rows directly into the store.
        {
            let mut store = state.row_store.lock().unwrap();
            let xid = store.begin_xid();
            store.insert(xid, "tenant:1", std::collections::HashMap::from([
                ("name".to_string(), "acme".to_string()),
            ]));
            store.insert(xid, "tenant:2", std::collections::HashMap::from([
                ("name".to_string(), "beta".to_string()),
            ]));
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = row_store_snapshot(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.row_count, 2, "snapshot must include both inserted rows");
        let keys: Vec<&str> = body.rows.iter().map(|r| r.key.as_str()).collect();
        assert!(keys.contains(&"tenant:1"));
        assert!(keys.contains(&"tenant:2"));
    }


    // ── S8-WS10-02: driver disconnect ──────────────────────────────────────

    #[tokio::test]
    async fn s8_ws10_02_driver_disconnect_removes_session() {
        let state = state_with_key(Some("test-key"));
        // First connect to create a session.
        let connect_req = DriverConnectRequest {
            driver_name: "test-driver".to_string(),
            driver_version: "1.0".to_string(),
            requested_capabilities: None,
        };
        let (_, Json(conn_body)) = driver_connect(State(state.clone()), Json(connect_req)).await;
        let token = conn_body.session_token.clone();
        // Verify session exists.
        assert!(state.driver_sessions.lock().unwrap().contains_key(&token));
        // Disconnect.
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = driver_disconnect(
            State(state.clone()),
            headers,
            Json(DriverDisconnectRequest { session_token: token.clone() }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.disconnected);
        assert_eq!(body.session_token, token);
        assert!(!state.driver_sessions.lock().unwrap().contains_key(&token));
    }

    #[tokio::test]
    async fn s8_ws10_02_driver_disconnect_missing_session_returns_false() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = driver_disconnect(
            State(state),
            headers,
            Json(DriverDisconnectRequest { session_token: "nonexistent-token".to_string() }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.disconnected);
    }

    #[tokio::test]
    async fn s8_ws10_02_driver_disconnect_requires_operator_auth() {
        let state = state_with_key(Some("test-key"));
        let req = DriverDisconnectRequest {
            session_token: "missing-auth".to_string(),
        };
        let result = driver_disconnect(State(state), HeaderMap::new(), Json(req)).await;
        let err = match result {
            Ok(_) => panic!("disconnect without operator auth must fail"),
            Err(err) => err,
        };
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s8_ws10_02_driver_disconnect_releases_pool_connection() {
        let state = state_with_key(Some("test-key"));
        let connect_req = DriverConnectRequest {
            driver_name: "pool-aware-driver".to_string(),
            driver_version: "0.2.0".to_string(),
            requested_capabilities: None,
        };
        let (_, Json(conn_body)) = driver_connect(State(state.clone()), Json(connect_req)).await;

        let headers = operator_headers("test-key", "admin");
        let (_, Json(disconnect_body)) = driver_disconnect(
            State(state.clone()),
            headers,
            Json(DriverDisconnectRequest {
                session_token: conn_body.session_token,
            }),
        )
        .await
        .expect("disconnect should succeed");

        assert!(disconnect_body.disconnected);

        let pool_stats = state
            .driver_pool
            .lock()
            .unwrap()
            .pool_stats(now_unix_ms_u64());
        assert_eq!(
            pool_stats.active_connections, 0,
            "disconnect should release the pooled connection back to idle"
        );
    }

    // ── S7-WS6-02: raft log entries endpoint ───────────────────────────────

    #[tokio::test]
    async fn s7_ws6_02_raft_log_fresh_state_empty() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_log(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.log_length, 0);
        assert_eq!(body.commit_index, 0);
        assert!(body.entries.is_empty());
    }

    #[tokio::test]
    async fn s7_ws6_02_raft_log_after_append_has_entries() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        {
            let mut node = state.raft_state.lock().unwrap();
            node.log.push(crate::raft::RaftLogEntry { index: 1, term: 1, command: "INSERT INTO t VALUES (1)".to_string() });
            node.commit_index = 1;
        }
        let (status, Json(body)) = raft_log(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.log_length, 1);
        assert_eq!(body.commit_index, 1);
        assert_eq!(body.entries[0].command, "INSERT INTO t VALUES (1)");
    }

    // ── S2-WS2-02: WAL forced checkpoint endpoint ──────────────────────────

    #[tokio::test]
    async fn s2_ws2_02_wal_force_checkpoint_increments_count() {
        let state = state_with_key(Some("test-key"));
        // Add some WAL records.
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("k1", "v1");
            wal.append_mutation("k2", "v2");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_force_checkpoint(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.wal_len_before, 2);
        assert_eq!(body.wal_len_after, 0);
        assert_eq!(body.checkpoint_count, 1);
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_force_checkpoint_on_empty_wal() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_force_checkpoint(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.wal_len_before, 0);
        assert_eq!(body.wal_len_after, 0);
        assert_eq!(body.checkpoint_count, 1, "checkpoint taken even on empty WAL");
    }

    // ── S2-WS2-02: WAL compact tests ──────────────────────────────────────────
    #[tokio::test]
    async fn s2_ws2_02_wal_compact_empty_wal_returns_compacted_false() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_compact(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.records_before, 0);
        assert_eq!(body.records_after, 0);
        assert!(!body.compacted, "empty WAL has nothing to compact");
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_compact_after_mutations_clears_wal() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("k1", "v1");
            wal.append_mutation("k2", "v2");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_compact(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.records_before, 2, "2 mutations appended before compact");
        assert_eq!(body.records_after, 0, "compact clears WAL via checkpoint");
        assert!(body.compacted, "records were removed so compacted = true");
    }

    // ── S2-WS2-02: WAL bounds tests ───────────────────────────────────────────
    #[tokio::test]
    async fn s2_ws2_02_wal_bounds_empty_state_shows_none_sequences() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_bounds(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.record_count, 0);
        assert_eq!(body.oldest_sequence, None, "no records means no oldest sequence");
        assert_eq!(body.newest_sequence, None, "no records means no newest sequence");
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_bounds_after_mutations_shows_sequences() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("k1", "v1");
            wal.append_mutation("k2", "v2");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_bounds(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.record_count, 2);
        assert!(body.oldest_sequence.is_some(), "oldest sequence must be Some after mutations");
        assert!(body.newest_sequence.is_some(), "newest sequence must be Some after mutations");
    }

    // ── S2-WS2-02: WAL tail ───────────────────────────────────────────────────
    #[tokio::test]
    async fn s2_ws2_02_wal_tail_empty_returns_zero_entries() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_tail(
            State(state),
            headers,
            axum::extract::Query(WalTailQuery::default()),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.record_count, 0);
        assert!(body.entries.is_empty());
        assert_eq!(body.limit_applied, 10);
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_tail_respects_limit() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("k1", "v1");
            wal.append_mutation("k2", "v2");
            wal.append_mutation("k3", "v3");
            wal.append_mutation("k4", "v4");
            wal.append_mutation("k5", "v5");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_tail(
            State(state),
            headers,
            axum::extract::Query(WalTailQuery { limit: Some(3) }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.record_count, 3, "limit=3 means only 3 newest entries");
        assert_eq!(body.limit_applied, 3);
    }

    // ── S2-WS2-03: WAL mutations tests ───────────────────────────────────────

    #[tokio::test]
    async fn s2_ws2_03_wal_mutations_returns_keys_and_values() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("user:101", "alice@example.com");
            wal.append_mutation("user:102", "bob@example.com");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_mutations(
            State(state),
            headers,
            axum::extract::Query(WalMutationsQuery { limit: Some(10) }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.mutation_count, 2);
        assert_eq!(body.mutations[0].key, "user:101");
        assert_eq!(body.mutations[0].value, "alice@example.com");
        assert_eq!(body.mutations[1].key, "user:102");
        assert_eq!(body.mutations[1].value, "bob@example.com");
    }

    #[tokio::test]
    async fn s2_ws2_03_wal_mutations_respects_limit() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            for i in 0..100u64 {
                wal.append_mutation(&format!("k{}", i), &format!("v{}", i));
            }
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_mutations(
            State(state),
            headers,
            axum::extract::Query(WalMutationsQuery { limit: Some(25) }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.mutation_count, 25, "limit=25 means only 25 newest mutations");
        assert_eq!(body.limit_applied, 25);
    }

    // ── S2-WS2-02: WAL segment list ───────────────────────────────────────────
    #[tokio::test]
    async fn s2_ws2_02_wal_segment_list_empty_returns_one_active_segment() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_segment_list(State(state), headers).await.unwrap();
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
        let (status, Json(body)) = wal_segment_list(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.active_record_count, 2, "2 mutations in active segment");
        let active = body.segments.iter().find(|s| s.is_active).unwrap();
        assert_eq!(active.record_count, 2);
        assert!(active.start_sequence.is_some());
        assert!(active.end_sequence.is_some());
    }

    // ─── S2-WS2-02: WAL checkpoint history endpoint tests ────────────────────

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

    // ── S7-WS6-04: Chaos health check ────────────────────────────────────────
    #[tokio::test]
    async fn s7_ws6_04_chaos_health_fresh_state_is_healthy() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = chaos_health(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.cluster_healthy, "fresh state should be healthy");
        assert_eq!(body.active_fault_count, 0);
    }

    // ── S7-WS6-04: Chaos history endpoint ───────────────────────────────────
    #[tokio::test]
    async fn s7_ws6_04_chaos_history_empty_on_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = chaos_history(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.history_len, 0, "no history on fresh state");
        assert!(body.events.is_empty());
    }

    #[tokio::test]
    async fn s7_ws6_04_chaos_history_shows_cleared_events() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        // Inject a fault.
        let req = ChaosInjectRequest {
            fault_type: "node_crash".to_string(),
            target_node: None,
            parameters: HashMap::new(),
        };
        let _ = chaos_inject(State(state.clone()), headers.clone(), axum::extract::Json(req))
            .await
            .unwrap();
        // Clear it (moves to history).
        let _ = chaos_clear(State(state.clone()), headers.clone()).await.unwrap();
        // Now check history.
        let (status, Json(body)) = chaos_history(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.history_len, 1, "one cleared event in history");
        assert_eq!(body.events[0].fault_type, "node_crash");
    }

    #[tokio::test]
    async fn s7_ws6_04_chaos_health_with_faults_is_unhealthy() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        {
            let mut cs = state.chaos_state.lock().unwrap();
            cs.active_faults.push(ChaosEvent {
                fault_type: "node_crash".to_string(),
                target_node: Some("node-1".to_string()),
                parameters: std::collections::HashMap::new(),
                injected_at_ms: 0,
                cleared_at_ms: None,
            });
        }
        let (status, Json(body)) = chaos_health(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.cluster_healthy, "active faults should mark unhealthy");
        assert_eq!(body.active_fault_count, 1);
    }

    // ── S4-WS3-04: HTAP lag ────────────────────────────────────────────────────
    #[tokio::test]
    async fn s4_ws3_04_htap_lag_fresh_state_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let result = htap_lag(State(state), headers).await;
        let (status, Json(body)) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.sync_origin_pending, 0);
        assert_eq!(body.olap_row_count, 0);
        assert_eq!(body.estimated_lag_mutations, 0);
    }

    #[tokio::test]
    async fn s4_ws3_04_htap_lag_after_olap_apply_shows_rows() {
        let state = state_with_key(Some("test-key"));
        {
            let mut olap = state.olap_store.lock().unwrap();
            let mut row = std::collections::HashMap::new();
            row.insert("k".to_string(), "v".to_string());
            olap.insert("row-1".to_string(), row);
        }
        let headers = operator_headers("test-key", "admin");
        let result = htap_lag(State(state), headers).await;
        let (status, Json(body)) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.olap_row_count, 1);
    }

    // ── S4-WS3-04: HTAP force-sync endpoint tests ────────────────────────────

    #[tokio::test]
    async fn s4_ws3_04_htap_force_sync_fresh_state_no_mutations() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = htap_force_sync(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.mutations_applied, 0, "no pending mutations on fresh state");
        assert_eq!(body.olap_row_count_after, 0);
    }

    #[tokio::test]
    async fn s4_ws3_04_htap_force_sync_drains_pending_mutations() {
        let state = state_with_key(Some("test-key"));
        // Seed the sync_origin with one pending insert mutation.
        {
            let mut origin = state.sync_origin.lock().unwrap();
            origin.append(
                "products",
                "prod:1",
                r#"{"name":"widget","price":"9.99"}"#,
                MutationOp::Insert,
            );
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = htap_force_sync(State(state.clone()), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.mutations_applied, 1, "one pending mutation must be applied");
        assert_eq!(body.olap_row_count_after, 1, "olap_store must have 1 row after sync");
        // Verify sync_origin was drained.
        let pending = state.sync_origin.lock().unwrap().pending_len();
        assert_eq!(pending, 0, "sync_origin must be empty after force-sync");
    }

    // ── S5-WS4A-02: Broker health ─────────────────────────────────────────────
    #[tokio::test]
    async fn s5_ws4a_02_broker_health_fresh_state_lists_three_brokers() {
        let state = state_with_key(Some("test-key"));
        let (status, Json(body)) = outbox_broker_health(State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.broker_count, 3, "should list kafka, nats, event_hubs");
        // All healthy on empty WAL (no pending data means no lag)
        assert!(body.brokers.iter().all(|b| b.healthy));
    }

    #[tokio::test]
    async fn s5_ws4a_02_broker_health_after_flush_shows_count() {
        let state = state_with_key(Some("test-key"));
        {
            let mut counts = state.broker_flush_counts.lock().unwrap();
            counts.insert("kafka".to_string(), 3);
        }
        let (status, Json(body)) = outbox_broker_health(State(state)).await;
        assert_eq!(status, StatusCode::OK);
        let kafka = body.brokers.iter().find(|b| b.broker_type == "kafka").unwrap();
        assert_eq!(kafka.flush_count, 3);
        assert!(kafka.healthy);
    }

    // ── S9-WS8-02: AI policy stats ────────────────────────────────────────────
    #[tokio::test]
    async fn s9_ws8_02_ai_policy_stats_fresh_state_no_requests() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let result = ai_policy_stats(State(state), headers).await;
        let (status, Json(body)) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.model_count, 0);
        assert_eq!(body.total_requests, 0);
        assert!(!body.allowed_models_enforced, "default policy has empty allowed_models");
    }

    // ── S9-WS8-02: AI policy reset endpoint ─────────────────────────────────
    #[tokio::test]
    async fn s9_ws8_02_ai_policy_reset_clears_counters() {
        let state = state_with_key(Some("test-key"));
        // Seed a counter directly.
        {
            let mut counters = state.ai_request_counters.lock().unwrap();
            counters.insert("gpt-4".to_string(), 42);
            counters.insert("llama-3".to_string(), 7);
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = ai_policy_reset(State(state.clone()), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.models_cleared, 2, "two models were cleared");
        // Verify counters are actually empty.
        let counters = state.ai_request_counters.lock().unwrap();
        assert!(counters.is_empty(), "counters must be empty after reset");
    }

    #[tokio::test]
    async fn s9_ws8_02_ai_policy_reset_on_empty_state_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = ai_policy_reset(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.models_cleared, 0, "nothing to clear in fresh state");
    }

    // ─── S9-WS8-02: AI governance audit tests ────────────────────────────────
    #[tokio::test]
    async fn s9_ws8_02_ai_governance_audit_empty_state_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = ai_governance_audit(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.total_models, 0);
        assert_eq!(body.total_requests, 0);
        assert!(body.entries.is_empty());
    }

    #[tokio::test]
    async fn s9_ws8_02_ai_governance_audit_reflects_request_counts() {
        let state = state_with_key(Some("test-key"));
        {
            let mut counters = state.ai_request_counters.lock().unwrap();
            counters.insert("model-a".to_string(), 10);
            counters.insert("model-b".to_string(), 5);
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = ai_governance_audit(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_models, 2);
        assert_eq!(body.total_requests, 15);
        assert!(!body.entries.is_empty(), "entries must be populated");
    }

    #[tokio::test]
    async fn s9_ws8_02_ai_policy_stats_after_request_shows_count() {
        let state = state_with_key(Some("test-key"));
        {
            let mut counters = state.ai_request_counters.lock().unwrap();
            counters.insert("gpt-4".to_string(), 5);
            counters.insert("gpt-3.5".to_string(), 2);
        }
        let headers = operator_headers("test-key", "admin");
        let result = ai_policy_stats(State(state), headers).await;
        let (status, Json(body)) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.model_count, 2);
        assert_eq!(body.total_requests, 7);
    }

    // ── S6-WS5-03: TLS cert rotation ─────────────────────────────────────────
    #[tokio::test]
    async fn s6_ws5_03_tls_rotate_requires_operator_auth() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let result = security_tls_rotate(
            State(state),
            headers,
            axum::extract::Json(TlsCertRotateRequest::default()),
        ).await;
        // Should succeed with operator auth (cert_source will be "not_configured" in test env)
        let (status, _) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn s6_ws5_03_tls_rotate_returns_not_configured_without_cert_env() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let result = security_tls_rotate(
            State(state),
            headers,
            axum::extract::Json(TlsCertRotateRequest { reason: Some("test".to_string()) }),
        ).await;
        let (status, axum::extract::Json(body)) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.cert_source, "not_configured");
        assert_eq!(body.key_source, "not_configured");
        assert!(!body.cert_present);
        assert!(!body.key_present);
        assert!(!body.preflight_ok);
        assert!(!body.rotation_initiated, "cert not configured so rotation_initiated=false");
        assert_eq!(body.reason, "test");
    }

    // ── S6-WS5-03: TLS cert info tests ───────────────────────────────────────
    #[tokio::test]
    async fn s6_ws5_03_tls_cert_info_fresh_state_not_configured() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = security_tls_cert_info(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.cert_source, "not_configured");
        assert_eq!(body.key_source, "not_configured");
        assert!(!body.cert_present);
        assert!(!body.key_present);
        assert!(!body.preflight_ok);
        assert!(!body.cert_rotation_supported, "cert rotation is scaffold");
    }

    #[tokio::test]
    async fn s6_ws5_03_tls_cert_info_reflects_security_config() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = security_tls_cert_info(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        // Default dev config has tls_required=false, mtls_required=false
        assert!(!body.tls_required, "default dev config has tls_required=false");
        assert!(!body.mtls_required, "default dev config has mtls_required=false");
    }

    // ── S8-WS10-02: Driver session list ──────────────────────────────────────
    #[tokio::test]
    async fn s8_ws10_02_driver_session_list_fresh_state_empty() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let result = driver_session_list(State(state), headers).await;
        let (status, axum::extract::Json(body)) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.session_count, 0);
        assert!(body.sessions.is_empty());
    }

    #[tokio::test]
    async fn s8_ws10_02_driver_session_list_shows_connected_session() {
        let state = state_with_key(Some("test-key"));
        {
            let mut sessions = state.driver_sessions.lock().unwrap();
            sessions.insert("drv-sess-42".to_string(), DriverSession {
                driver_name: "test-driver".to_string(),
                driver_version: "1.0".to_string(),
                connected_at_ms: 12345,
                assigned_node_id: "node-1".to_string(),
                pooled_connection_id: None,
            });
        }
        let headers = operator_headers("test-key", "admin");
        let result = driver_session_list(State(state), headers).await;
        let (status, axum::extract::Json(body)) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.session_count, 1);
        assert_eq!(body.sessions[0].session_token, "drv-sess-42");
        assert_eq!(body.sessions[0].driver_name, "test-driver");
    }

    // ── S8-WS10-02: Driver health endpoint tests ─────────────────────────────

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
                assigned_node_id: "node-1".to_string(),
                pooled_connection_id: None,
            });
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = driver_health(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.active_sessions, 1);
        assert!(body.healthy);
    }

    // ── S8-WS10-02: Driver query tests ───────────────────────────────────────
    #[tokio::test]
    async fn s8_ws10_02_driver_query_invalid_session_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = DriverQueryRequest {
            session_token: "no-such-session".to_string(),
            sql: "SELECT * FROM orders".to_string(),
        };
        let result = driver_query(State(state), headers, Json(req)).await;
        assert!(result.is_err(), "invalid session token must fail");
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s8_ws10_02_driver_query_valid_session_returns_ok() {
        let state = state_with_key(Some("test-key"));
        {
            let mut sessions = state.driver_sessions.lock().unwrap();
            sessions.insert("drv-sess-99".to_string(), DriverSession {
                driver_name: "test-drv".to_string(),
                driver_version: "2.0.0".to_string(),
                connected_at_ms: 0,
                assigned_node_id: "node-1".to_string(),
                pooled_connection_id: None,
            });
        }
        let headers = operator_headers("test-key", "admin");
        let req = DriverQueryRequest {
            session_token: "drv-sess-99".to_string(),
            sql: "SELECT COUNT(*) FROM events".to_string(),
        };
        let (status, Json(body)) = driver_query(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.session_token, "drv-sess-99");
        assert_eq!(body.sql, "SELECT COUNT(*) FROM events");
        assert_eq!(body.rows_returned, 0);
    }

    // ── S8-WS10-02: Driver ping ───────────────────────────────────────────────
    #[tokio::test]
    async fn s8_ws10_02_driver_ping_invalid_session_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = DriverPingRequest { session_token: "ghost-token".to_string() };
        let result = driver_ping(State(state), headers, Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s8_ws10_02_driver_ping_valid_session_returns_pong() {
        let state = state_with_key(Some("test-key"));
        {
            let mut sessions = state.driver_sessions.lock().unwrap();
            sessions.insert("drv-sess-42".to_string(), DriverSession {
                driver_name: "test-drv".to_string(),
                driver_version: "1.0.0".to_string(),
                connected_at_ms: 0,
                assigned_node_id: "node-1".to_string(),
                pooled_connection_id: None,
            });
        }
        let headers = operator_headers("test-key", "admin");
        let req = DriverPingRequest { session_token: "drv-sess-42".to_string() };
        let (status, Json(body)) = driver_ping(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "pong");
        assert_eq!(body.session_token, "drv-sess-42");
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
        let Err((status, _)) = result else { panic!("expected error") };
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

    // ── S10-WS15-02: CDC stream filter ────────────────────────────────────────
    #[tokio::test]
    async fn s10_ws15_02_cdc_stream_filter_matching_table_returns_events() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("k1", "v1");
            wal.append_mutation("k2", "v2");
        }
        let query = CdcStreamFilterQuery { table: Some("row_store".to_string()) };
        let (status, axum::extract::Json(body)) = cdc_stream_filter(State(state), axum::extract::Query(query)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.event_count, 2, "row_store table should match all WAL events");
        assert_eq!(body.table_filter.as_deref(), Some("row_store"));
    }

    // ── S10-WS15-02: CDC stream latest endpoint ──────────────────────────────
    #[tokio::test]
    async fn s10_ws15_02_cdc_stream_latest_returns_empty_on_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let query = CdcLatestQuery { limit: None };
        let (status, axum::extract::Json(body)) =
            cdc_stream_latest(State(state), axum::extract::Query(query)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.event_count, 0, "no events on fresh state");
        assert_eq!(body.limit_applied, 10, "default limit is 10");
    }

    #[tokio::test]
    async fn s10_ws15_02_cdc_stream_latest_respects_limit() {
        let state = state_with_key(Some("test-key"));
        // Add 5 WAL mutations.
        {
            let mut wal = state.wal_engine.lock().unwrap();
            for i in 0..5 {
                wal.append_mutation(&format!("k{i}"), &format!("v{i}"));
            }
        }
        // Request only latest 3.
        let query = CdcLatestQuery { limit: Some(3) };
        let (status, axum::extract::Json(body)) =
            cdc_stream_latest(State(state), axum::extract::Query(query)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.event_count, 3, "limit=3 returns 3 events");
        assert_eq!(body.limit_applied, 3);
    }

    #[tokio::test]
    async fn s10_ws15_02_cdc_stream_filter_unknown_table_returns_empty() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("k1", "v1");
        }
        let query = CdcStreamFilterQuery { table: Some("nonexistent_table".to_string()) };
        let (status, axum::extract::Json(body)) = cdc_stream_filter(State(state), axum::extract::Query(query)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.event_count, 0, "unknown table filter returns no events");
    }

    // ── S2-WS2-02: WAL stats endpoint ────────────────────────────────────────
    #[tokio::test]
    async fn s2_ws2_02_wal_stats_fresh_state_empty() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.record_count, 0);
        assert_eq!(body.checkpoint_count, 0);
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_stats_reflects_appended_records() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("k1", "v1");
            wal.append_mutation("k2", "v2");
            wal.append_mutation("k3", "v3");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.record_count, 3);
        assert_eq!(body.checkpoint_count, 0, "no checkpoint performed yet");
    }

    // ── S2-WS2-02: WAL replay endpoint tests ─────────────────────────────────
    #[tokio::test]
    async fn s2_ws2_02_wal_replay_empty_on_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_replay(
            State(state),
            headers,
            axum::extract::Query(WalReplayQuery::default()),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_records, 0);
        assert_eq!(body.matched_records, 0);
        assert!(body.entries.is_empty());
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_replay_filters_by_op_type() {
        let state = state_with_key(Some("test-key"));
        {
            let mut wal = state.wal_engine.lock().unwrap();
            wal.append_mutation("k1", "v1");
            wal.append_mutation("k2", "__deleted__");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_replay(
            State(state),
            headers,
            axum::extract::Query(WalReplayQuery { table_filter: None, op_filter: Some("delete".to_string()) }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_records, 2);
        assert_eq!(body.matched_records, 1, "only 1 delete record should match");
        assert_eq!(body.entries[0].value, "__deleted__");
    }

    // ── S7-WS6-02: Raft heartbeat endpoint ───────────────────────────────────
    #[tokio::test]
    async fn s7_ws6_02_raft_heartbeat_resets_tick_counter() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        {
            let mut node = state.raft_state.lock().unwrap();
            node.ticks_since_heartbeat = 5;
        }
        let (status, Json(body)) = raft_heartbeat(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.ticks_reset_to, 0);
        assert!(body.heartbeat_accepted);
    }

    #[tokio::test]
    async fn s7_ws6_02_raft_heartbeat_returns_current_term() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        {
            let mut node = state.raft_state.lock().unwrap();
            node.current_term = 3;
        }
        let (status, Json(body)) = raft_heartbeat(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.term, 3);
    }

    // ─── S7-WS6-02: Raft election status tests ───────────────────────────────
    #[tokio::test]
    async fn s7_ws6_02_raft_election_status_fresh_state_is_follower() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_election_status(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert!(matches!(body.role, RaftRole::Follower), "fresh state must be Follower");
        assert!(!body.is_election_pending, "Follower is not in election");
        assert!(body.election_timeout_ticks > 0);
    }

    #[tokio::test]
    async fn s7_ws6_02_raft_election_status_remaining_ticks_decrements() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        {
            let mut node = state.raft_state.lock().unwrap();
            node.ticks_since_heartbeat = 3;
        }
        let (status, Json(body)) = raft_election_status(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.ticks_since_heartbeat, 3);
        assert_eq!(
            body.remaining_ticks,
            body.election_timeout_ticks.saturating_sub(3),
            "remaining = timeout - ticks_used"
        );
    }

    // ─── S4-WS3-04: HTAP status tests ────────────────────────────────────────
    #[tokio::test]
    async fn s4_ws3_04_htap_status_empty_state_is_synchronized() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = htap_status(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.sync_origin_pending, 0);
        assert!(body.is_synchronized, "no pending mutations means synchronized");
    }

    #[tokio::test]
    async fn s4_ws3_04_htap_status_reflects_olap_row_count() {
        let state = state_with_key(Some("test-key"));
        {
            let mut olap = state.olap_store.lock().unwrap();
            olap.insert("k1".to_string(), std::collections::HashMap::new());
            olap.insert("k2".to_string(), std::collections::HashMap::new());
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = htap_status(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.olap_row_count, 2, "olap_store row count must be visible");
    }

    // ── S9-WS8A-02: Audit integrity snapshot ─────────────────────────────────
    #[tokio::test]
    async fn s9_ws8a_02_audit_snapshot_fresh_state_valid_chain() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = audit_snapshot(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.event_count, 0);
        assert!(body.chain_valid, "empty chain should be valid");
        assert_eq!(body.genesis_hash, "genesis-0000000000000000");
    }

    #[tokio::test]
    async fn s9_ws8a_02_audit_snapshot_reflects_appended_events() {
        let state = state_with_key(Some("test-key"));
        {
            let mut sink = state.audit_sink.lock().unwrap();
            sink.append(voltnuerongrid_audit::AuditEventKind::Sql, "actor", "action", "ok", "{}");
            sink.append(voltnuerongrid_audit::AuditEventKind::Security, "actor", "action2", "ok", "{}");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = audit_snapshot(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.event_count, 2);
        assert!(body.chain_valid, "2-event chain should be valid");
    }

    // ─── S7-WS6-04: Chaos fire drill tests ────────────────────────────────────────
    #[tokio::test]
    async fn s7_ws6_04_chaos_fire_drill_adds_to_history() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = ChaosFireDrillRequest { drill_type: "network-partition".to_string(), target_node: None };
        let (status, Json(body)) = chaos_fire_drill(State(state.clone()), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.faults_injected, 1);
        let cs = state.chaos_state.lock().unwrap();
        assert_eq!(cs.event_history.len(), 1, "fire drill must appear in history");
        assert!(cs.active_faults.is_empty(), "fire drill must not leave active faults");
    }

    #[tokio::test]
    async fn s7_ws6_04_chaos_fire_drill_with_target_node() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = ChaosFireDrillRequest {
            drill_type: "cpu-spike".to_string(),
            target_node: Some("node-2".to_string()),
        };
        let (status, Json(body)) = chaos_fire_drill(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.target_node, "node-2");
        assert_eq!(body.drill_type, "cpu-spike");
    }

    // ─── S9-WS8A-02: Audit purge tests ──────────────────────────────────────
    #[tokio::test]
    async fn s9_ws8a_02_audit_purge_empty_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = AuditPurgeRequest { confirm: true };
        let (status, Json(body)) = audit_purge(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.events_purged, 0, "empty sink purge must report 0 events");
        assert!(body.chain_reset, "chain must be reset after purge");
    }

    #[tokio::test]
    async fn s9_ws8a_02_audit_purge_clears_events() {
        let state = state_with_key(Some("test-key"));
        {
            let mut sink = state.audit_sink.lock().unwrap();
            sink.append(voltnuerongrid_audit::AuditEventKind::Sql, "actor", "q1", "ok", "{}");
            sink.append(voltnuerongrid_audit::AuditEventKind::Sql, "actor", "q2", "ok", "{}");
        }
        let headers = operator_headers("test-key", "admin");
        let req = AuditPurgeRequest { confirm: true };
        let (status, Json(body)) = audit_purge(State(state.clone()), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.events_purged, 2, "must report 2 events purged");
        assert!(body.chain_reset);
        let sink = state.audit_sink.lock().unwrap();
        assert!(sink.is_empty(), "audit sink must be empty after purge");
    }

    // ── S9-WS8A-01: Audit CLI summary endpoint ───────────────────────────────
    #[tokio::test]
    async fn s9_ws8a_01_audit_cli_summary_empty_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = audit_cli_summary(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.total_events, 0, "no events on fresh state");
        assert!(body.chain_valid, "empty chain is valid");
        assert_eq!(body.last_event_kind, "none", "no events means kind = none");
    }

    #[tokio::test]
    async fn s9_ws8a_01_audit_cli_summary_reflects_appended_events() {
        let state = state_with_key(Some("test-key"));
        {
            let mut sink = state.audit_sink.lock().unwrap();
            sink.append(voltnuerongrid_audit::AuditEventKind::Sql, "actor", "q1", "ok", "{}");
            sink.append(voltnuerongrid_audit::AuditEventKind::Security, "actor", "auth", "ok", "{}");
        }
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = audit_cli_summary(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_events, 2, "two appended events");
        assert!(body.chain_valid);
        // last event was Security kind
        assert!(body.last_event_kind.to_lowercase().contains("security"),
            "last event kind must be Security, got: {}", body.last_event_kind);
    }

    // ── S7-WS6-03: Raft member list endpoint ─────────────────────────────────
    #[tokio::test]
    async fn s7_ws6_03_raft_member_list_single_node() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = raft_member_list(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.member_count, 1);
        assert_eq!(body.members.len(), 1);
        assert!(!body.members[0].node_id.is_empty(), "member must have a node_id");
    }

    #[tokio::test]
    async fn s7_ws6_03_raft_member_list_reflects_term() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        {
            let mut node = state.raft_state.lock().unwrap();
            node.current_term = 7;
        }
        let (status, Json(body)) = raft_member_list(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.members[0].term, 7);
    }

    #[tokio::test]
    async fn s7_ws6_02_raft_log_requires_operator_auth() {
        let state = state_with_key(Some("test-key"));

        let err = raft_log(State(state), HeaderMap::new())
            .await
            .expect_err("raft log must reject missing auth");

        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s7_ws6_02_raft_heartbeat_denies_security_role() {
        let state = state_with_key(Some("test-key"));

        let err = raft_heartbeat(State(state), operator_headers("test-key", "security-bot"))
            .await
            .expect_err("security role must not execute raft heartbeat");

        assert_eq!(err.0, StatusCode::FORBIDDEN);
        assert_eq!(err.1.reason, "insufficient_privilege");
    }

    // ── S4-WS3-02: Columnar project endpoint ─────────────────────────────────
    #[tokio::test]
    async fn s4_ws3_02_columnar_project_empty_store_returns_no_columns() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let params = ColumnarProjectQuery { columns: None };
        let (status, Json(body)) = store_columnar_project(State(state), headers, Query(params)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.rows_scanned, 0);
        assert_eq!(body.columns_projected, 0);
    }

    #[tokio::test]
    async fn s4_ws3_02_columnar_project_returns_all_when_no_filter() {
        let state = state_with_key(Some("test-key"));
        // Insert a row so there are columns to materialise.
        {
            let mut rs = state.row_store.lock().unwrap();
            let xid = rs.begin_xid();
            let mut data = std::collections::HashMap::new();
            data.insert("source".to_string(), "test".to_string());
            data.insert("payload".to_string(), "hello".to_string());
            rs.insert(xid, "row-1", data);
        }
        let headers = operator_headers("test-key", "admin");
        let params = ColumnarProjectQuery { columns: None };
        let (status, Json(body)) = store_columnar_project(State(state), headers, Query(params)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.rows_scanned > 0, "should have scanned rows");
        assert!(body.columns_projected > 0, "should project all columns when no filter");
    }

    // ── S4-WS3-03: Columnar aggregate endpoint ──────────────────────────────
    #[tokio::test]
    async fn s4_ws3_03_columnar_aggregate_count_on_empty_store() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let params = ColumnarAggregateQuery { column: None, op: None };
        let (status, Json(body)) =
            store_columnar_aggregate(State(state), headers, Query(params)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.op, "count", "default op must be count");
        assert_eq!(body.rows_scanned, 0, "empty store has no rows");
    }

    #[tokio::test]
    async fn s4_ws3_03_columnar_aggregate_count_reflects_inserted_rows() {
        let state = state_with_key(Some("test-key"));
        {
            let mut rs = state.row_store.lock().unwrap();
            let xid = rs.begin_xid();
            for i in 0..3 {
                let mut d = std::collections::HashMap::new();
                d.insert("payload".to_string(), format!("val-{i}"));
                rs.insert(xid, &format!("agg-row-{i}"), d);
            }
        }
        let headers = operator_headers("test-key", "admin");
        let params = ColumnarAggregateQuery { column: Some("payload".to_string()), op: Some("count".to_string()) };
        let (status, Json(body)) =
            store_columnar_aggregate(State(state), headers, Query(params)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.op, "count");
        assert_eq!(body.column, "payload");
        assert_eq!(body.result, "3", "count of 3 rows should be 3");
        assert_eq!(body.rows_scanned, 3);
    }

    // ── S5-E4A-01: Connector deregister endpoint ──────────────────────────────
    #[tokio::test]
    async fn s5_e4a_01_deregister_known_connector_returns_removed_true() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        // Register first.
        let req = ConnectorRegisterRequest {
            connector_id: "conn-x".to_string(),
            connector_type: "csv-source".to_string(),
            version: "1.0".to_string(),
            signed: Some(true),
        };
        connector_register(State(state.clone()), headers.clone(), Json(req)).await.unwrap();
        // Now deregister.
        let dreq = ConnectorDeregisterRequest { connector_id: "conn-x".to_string() };
        let (status, Json(body)) = connector_deregister(State(state.clone()), headers, Json(dreq)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.removed, "known connector must report removed = true");
        assert_eq!(body.connector_id, "conn-x");
        // Registry should now be empty.
        let reg = state.connector_registry.lock().unwrap();
        assert_eq!(reg.len(), 0);
    }

    #[tokio::test]
    async fn s5_e4a_01_deregister_unknown_connector_returns_removed_false() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let dreq = ConnectorDeregisterRequest { connector_id: "no-such-connector".to_string() };
        let (status, Json(body)) = connector_deregister(State(state), headers, Json(dreq)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.removed, "unknown connector must report removed = false");
    }

    // ─── S5-E4A-01: Connector get-by-id tests ───────────────────────────────
    #[tokio::test]
    async fn s5_e4a_01_connector_get_existing_returns_found() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let reg_req = ConnectorRegisterRequest {
            connector_id: "my-connector".to_string(),
            connector_type: "csv".to_string(),
            version: "1.0.0".to_string(),
            signed: Some(true),
        };
        connector_register(State(state.clone()), headers.clone(), Json(reg_req)).await.unwrap();
        let (status, Json(body)) = connector_get(
            State(state),
            headers,
            Query(ConnectorGetQuery { id: "my-connector".to_string() }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.found, "registered connector must be found");
        assert!(body.connector.is_some(), "connector data must be present");
    }

    #[tokio::test]
    async fn s5_e4a_01_connector_get_unknown_returns_not_found() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = connector_get(
            State(state),
            headers,
            Query(ConnectorGetQuery { id: "no-such-connector".to_string() }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.found, "unknown connector must report found = false");
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

    // ─── S11-WS1-10: Row store keys endpoint tests ────────────────────────────

    #[tokio::test]
    async fn s11_ws1_10_store_rows_keys_empty_on_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = store_rows_keys(
            State(state),
            headers,
            Query(StoreRowsKeysQuery { prefix: None }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_keys, 0, "fresh row store must have no keys");
        assert!(body.keys.is_empty());
    }

    #[tokio::test]
    async fn s11_ws1_10_store_rows_keys_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = store_rows_keys(
            State(state),
            headers,
            Query(StoreRowsKeysQuery { prefix: None }),
        ).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-10: WAL truncate endpoint tests ──────────────────────────────

    #[tokio::test]
    async fn s11_ws1_10_wal_truncate_empty_wal_returns_not_truncated() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let req = WalTruncateRequest { up_to_sequence: 1 };
        let (status, Json(body)) = wal_truncate(State(state), headers, Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.truncated, "empty WAL must return truncated = false");
        assert_eq!(body.records_removed, 0);
    }

    #[tokio::test]
    async fn s11_ws1_10_wal_truncate_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let req = WalTruncateRequest { up_to_sequence: 100 };
        let result = wal_truncate(State(state), headers, Json(req)).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-11: Row store version endpoint tests ─────────────────────────

    #[tokio::test]
    async fn s11_ws1_11_row_store_version_fresh_state_returns_zero_xid() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = row_store_version(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.current_xid, 0, "fresh row store must have xid 0");
        assert_eq!(body.total_rows, 0);
    }

    #[tokio::test]
    async fn s11_ws1_11_row_store_version_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = row_store_version(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-11: HTAP stats endpoint tests ────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_11_htap_stats_empty_olap_store() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = htap_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.table_count, 0, "fresh OLAP store must have no tables");
        assert_eq!(body.total_entries, 0);
    }

    #[tokio::test]
    async fn s11_ws1_11_htap_stats_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = htap_stats(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-12: Connector health endpoint tests ──────────────────────────

    #[tokio::test]
    async fn s11_ws1_12_connectors_health_empty_registry() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = connectors_health(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total, 0, "fresh registry must have no connectors");
        assert_eq!(body.healthy, 0);
    }

    #[tokio::test]
    async fn s11_ws1_12_connectors_health_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = connectors_health(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-12: Row store page stats endpoint tests ──────────────────────

    #[tokio::test]
    async fn s11_ws1_12_rows_page_stats_fresh_state() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_page_stats(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.visible_rows, 0, "fresh row store must have no visible rows");
        assert_eq!(body.current_xid, 0);
    }

    #[tokio::test]
    async fn s11_ws1_12_rows_page_stats_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_page_stats(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-13: Ingest schema fields endpoint tests ──────────────────────

    #[tokio::test]
    async fn s11_ws1_13_ingest_schema_fields_unknown_schema_returns_empty() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = ingest_schema_fields(
            State(state),
            headers,
            Query(IngestSchemaFieldsQuery { schema_id: "no-such-schema".to_string() }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.field_count, 0, "unknown schema must return zero fields");
        assert!(body.fields.is_empty());
    }

    #[tokio::test]
    async fn s11_ws1_13_ingest_schema_fields_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = ingest_schema_fields(
            State(state),
            headers,
            Query(IngestSchemaFieldsQuery { schema_id: "s1".to_string() }),
        ).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-13: WAL seq endpoint tests ───────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_13_wal_seq_fresh_state_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_seq(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.latest_sequence, 0, "fresh WAL must have sequence 0");
        assert_eq!(body.wal_len, 0);
    }

    #[tokio::test]
    async fn s11_ws1_13_wal_seq_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_seq(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-14: WAL head endpoint tests ─────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_14_wal_head_empty_wal_returns_zero_entries() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_head(
            State(state),
            headers,
            Query(WalHeadQuery { limit: None }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.record_count, 0, "empty WAL must return zero entries");
        assert!(body.entries.is_empty());
    }

    #[tokio::test]
    async fn s11_ws1_14_wal_head_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_head(
            State(state),
            headers,
            Query(WalHeadQuery { limit: Some(5) }),
        ).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-14: Rows modified endpoint tests ────────────────────────────

    #[tokio::test]
    async fn s11_ws1_14_rows_modified_fresh_store_returns_empty() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_modified(
            State(state),
            headers,
            Query(RowsModifiedQuery { since_xid: 0 }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.modified_count, 0, "fresh row store must return zero modified rows");
        assert_eq!(body.since_xid, 0);
    }

    #[tokio::test]
    async fn s11_ws1_14_rows_modified_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_modified(
            State(state),
            headers,
            Query(RowsModifiedQuery { since_xid: 1 }),
        ).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-15: WAL range endpoint tests ─────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_15_wal_range_empty_wal_returns_zero_entries() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_range(
            State(state),
            headers,
            Query(WalRangeQuery { from_seq: 0, to_seq: None }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.record_count, 0, "empty WAL must return zero range entries");
        assert!(body.entries.is_empty());
    }

    #[tokio::test]
    async fn s11_ws1_15_wal_range_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_range(
            State(state),
            headers,
            Query(WalRangeQuery { from_seq: 0, to_seq: Some(100) }),
        ).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-15: Rows XID endpoint tests ──────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_15_rows_xid_fresh_state_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_xid(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.current_xid, 0, "fresh row store must have current_xid 0");
        assert_eq!(body.next_xid, 1, "next_xid must be current_xid + 1");
    }

    #[tokio::test]
    async fn s11_ws1_15_rows_xid_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_xid(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-16: WAL size endpoint tests ──────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_16_wal_size_empty_wal_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_size(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.record_count, 0, "empty WAL must report zero records");
        assert_eq!(body.estimated_bytes, 0, "empty WAL must report zero bytes");
    }

    #[tokio::test]
    async fn s11_ws1_16_wal_size_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_size(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-16: Rows visible endpoint tests ──────────────────────────────

    #[tokio::test]
    async fn s11_ws1_16_rows_visible_fresh_store_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_visible(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.visible_row_count, 0, "fresh store must have zero visible rows");
        assert_eq!(body.snapshot_xid, 0, "fresh snapshot must be xid 0");
    }

    #[tokio::test]
    async fn s11_ws1_16_rows_visible_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_visible(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-17: WAL latest endpoint tests ────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_17_wal_latest_empty_wal_has_no_record() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_latest(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(!body.has_record, "empty WAL must return has_record = false");
        assert_eq!(body.sequence, 0, "empty WAL sequence must be 0");
    }

    #[tokio::test]
    async fn s11_ws1_17_wal_latest_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_latest(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-17: Rows total endpoint tests ────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_17_rows_total_fresh_store_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_total(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_row_count, 0, "fresh store must have zero total rows");
    }

    #[tokio::test]
    async fn s11_ws1_17_rows_total_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_total(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-18: WAL by-key endpoint tests ────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_18_wal_by_key_empty_wal_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_by_key(
            State(state),
            headers,
            Query(WalByKeyQuery { key_prefix: "user:".to_string() }),
        ).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.record_count, 0, "empty WAL must return zero records for any prefix");
        assert_eq!(body.key_prefix, "user:");
    }

    #[tokio::test]
    async fn s11_ws1_18_wal_by_key_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_by_key(
            State(state),
            headers,
            Query(WalByKeyQuery { key_prefix: "k".to_string() }),
        ).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-18: Rows keys count endpoint tests ───────────────────────────

    #[tokio::test]
    async fn s11_ws1_18_rows_keys_count_fresh_store_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_keys_count(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.key_count, 0, "fresh store must have zero distinct keys");
    }

    #[tokio::test]
    async fn s11_ws1_18_rows_keys_count_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_keys_count(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-20: WAL delta endpoint tests ─────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_20_wal_delta_fresh_wal_returns_zero_counts() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_delta(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.insert_count, 0, "fresh WAL must have zero inserts");
        assert_eq!(body.delete_count, 0, "fresh WAL must have zero deletes");
    }

    #[tokio::test]
    async fn s11_ws1_20_wal_delta_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_delta(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-20: Rows tombstone count endpoint tests ──────────────────────

    #[tokio::test]
    async fn s11_ws1_20_rows_tombstone_count_fresh_store_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_tombstone_count(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.tombstone_count, 0, "fresh row store must have zero tombstones");
    }

    #[tokio::test]
    async fn s11_ws1_20_rows_tombstone_count_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_tombstone_count(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-19: WAL checkpoint latest endpoint tests ─────────────────────

    #[tokio::test]
    async fn s11_ws1_19_wal_checkpoint_latest_fresh_state_returns_zero_id() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_checkpoint_latest(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.checkpoint_id, 0, "fresh WAL has no checkpoints");
    }

    #[tokio::test]
    async fn s11_ws1_19_wal_checkpoint_latest_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_checkpoint_latest(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-19: Rows scan visible endpoint tests ─────────────────────────

    #[tokio::test]
    async fn s11_ws1_19_rows_scan_visible_fresh_store_returns_empty() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_scan_visible(State(state), headers, Query(RowsScanVisibleQuery { limit: None })).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.row_count, 0, "fresh row store must return empty scan");
    }

    #[tokio::test]
    async fn s11_ws1_19_rows_scan_visible_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_scan_visible(State(state), headers, Query(RowsScanVisibleQuery { limit: None })).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }


    // ─── S11-WS1-21: WAL unique keys endpoint tests ───────────────────────────

    #[tokio::test]
    async fn s11_ws1_21_wal_unique_keys_fresh_wal_returns_zero() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_unique_keys(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.unique_key_count, 0, "fresh WAL must have zero unique keys");
    }

    #[tokio::test]
    async fn s11_ws1_21_wal_unique_keys_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_unique_keys(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ─── S11-WS1-21: Rows XID history endpoint tests ──────────────────────────

    #[tokio::test]
    async fn s11_ws1_21_rows_xid_history_fresh_store_returns_zero_xid() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_xid_history(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.current_xid, 0, "fresh store must have current_xid = 0");
        assert_eq!(body.next_xid, 1, "next_xid must be current_xid + 1");
    }

    #[tokio::test]
    async fn s11_ws1_21_rows_xid_history_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_xid_history(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ── S11-WS1-22: WAL age + rows first key tests ───────────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_22_wal_age_returns_ok_with_span() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_age(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.sequence_span, body.newest_sequence.saturating_sub(body.oldest_sequence));
    }

    #[tokio::test]
    async fn s11_ws1_22_wal_age_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_age(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s11_ws1_22_rows_first_key_returns_ok_empty_store() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_first_key(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert!(!body.has_key, "fresh empty store must have has_key = false");
        assert_eq!(body.first_key, "", "empty store must have empty first_key");
    }

    #[tokio::test]
    async fn s11_ws1_22_rows_first_key_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_first_key(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ── S11-WS1-23: WAL keys list + rows last key tests ──────────────────────────────────────

    #[tokio::test]
    async fn s11_ws1_23_wal_keys_list_returns_ok_empty_wal() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_keys_list(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.key_count, 0, "fresh WAL must have zero keys");
        assert!(body.keys.is_empty(), "keys list must be empty for fresh WAL");
    }

    #[tokio::test]
    async fn s11_ws1_23_wal_keys_list_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_keys_list(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s11_ws1_23_rows_last_key_returns_ok_empty_store() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_last_key(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert!(!body.has_key, "fresh empty store must have has_key = false");
        assert_eq!(body.last_key, "", "empty store must have empty last_key");
    }

    #[tokio::test]
    async fn s11_ws1_23_rows_last_key_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_last_key(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ── S11-WS1-24: Rows count distinct + rows key exists tests ───────────────────────────────

    #[tokio::test]
    async fn s11_ws1_24_rows_count_distinct_returns_ok() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_count_distinct(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.distinct_value_count, 0, "fresh store has no distinct values");
    }

    #[tokio::test]
    async fn s11_ws1_24_rows_count_distinct_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_count_distinct(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s11_ws1_24_rows_key_exists_returns_false_for_missing_key() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let params = Query(RowsKeyExistsQuery { key: "nonexistent".to_string() });
        let (status, Json(body)) = rows_key_exists(State(state), headers, params).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert!(!body.exists, "non-existent key must return exists = false");
    }

    #[tokio::test]
    async fn s11_ws1_24_rows_key_exists_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let params = Query(RowsKeyExistsQuery { key: "k".to_string() });
        let result = rows_key_exists(State(state), headers, params).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ── S11-WS1-25: rows value search + wal record count tests ────────────────────────
    #[tokio::test]
    async fn s11_ws1_25_rows_value_search_returns_ok() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let params = Query(RowsValueSearchQuery { value: "test".to_string() });
        let (status, Json(body)) = rows_value_search(State(state), headers, params).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
    }

    #[tokio::test]
    async fn s11_ws1_25_rows_value_search_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let params = Query(RowsValueSearchQuery { value: "test".to_string() });
        let result = rows_value_search(State(state), headers, params).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s11_ws1_25_wal_record_count_returns_ok() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_record_count(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
    }

    #[tokio::test]
    async fn s11_ws1_25_wal_record_count_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_record_count(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ── S11-WS1-26: rows count range + wal checkpoint age tests ───────────────────────
    #[tokio::test]
    async fn s11_ws1_26_rows_count_range_returns_ok() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let params = Query(RowsCountRangeQuery { prefix: None });
        let (status, Json(body)) = rows_count_range(State(state), headers, params).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
    }

    #[tokio::test]
    async fn s11_ws1_26_rows_count_range_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let params = Query(RowsCountRangeQuery { prefix: None });
        let result = rows_count_range(State(state), headers, params).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s11_ws1_26_wal_checkpoint_age_returns_ok() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_checkpoint_age(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
    }

    #[tokio::test]
    async fn s11_ws1_26_wal_checkpoint_age_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_checkpoint_age(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // ── S11-WS1-27: rows payload size + wal flush count tests ───────────────────────
    #[tokio::test]
    async fn s11_ws1_27_rows_payload_size_returns_ok() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_payload_size(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
    }

    #[tokio::test]
    async fn s11_ws1_27_rows_payload_size_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = rows_payload_size(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn s11_ws1_27_wal_flush_count_returns_ok() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_flush_count(State(state), headers).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
    }

    #[tokio::test]
    async fn s11_ws1_27_wal_flush_count_missing_auth_returns_401() {
        let state = state_with_key(Some("test-key"));
        let headers = HeaderMap::new();
        let result = wal_flush_count(State(state), headers).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-28: rows_field_count tests

    #[tokio::test]
    async fn s11_ws1_28_rows_field_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_field_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
    }

    #[tokio::test]
    async fn s11_ws1_28_rows_field_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_field_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-28: wal_entry_latest tests

    #[tokio::test]
    async fn s11_ws1_28_wal_entry_latest_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_entry_latest(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
    }

    #[tokio::test]
    async fn s11_ws1_28_wal_entry_latest_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_entry_latest(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-29: wal_write_count tests

    #[tokio::test]
    async fn s11_ws1_29_wal_write_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_write_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.write_count, 0, "fresh WAL must have zero writes");
    }

    #[tokio::test]
    async fn s11_ws1_29_wal_write_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_write_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-29: rows_key_longest tests

    #[tokio::test]
    async fn s11_ws1_29_rows_key_longest_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_key_longest(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.row_count, 0, "fresh store must return zero rows");
        assert_eq!(body.key_length, 0, "empty store must have zero longest key length");
    }

    #[tokio::test]
    async fn s11_ws1_29_rows_key_longest_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_key_longest(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-30: wal_age tests (reuse existing wal_age endpoint from S22)

    #[tokio::test]
    async fn s11_ws1_30_wal_age_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_age(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.sequence_span, 0, "fresh WAL must have zero sequence span");
    }

    #[tokio::test]
    async fn s11_ws1_30_wal_age_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_age(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-30: rows_key_shortest tests

    #[tokio::test]
    async fn s11_ws1_30_rows_key_shortest_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_key_shortest(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.row_count, 0, "fresh store must return zero rows");
        assert_eq!(body.key_length, 0, "empty store must have zero shortest key length");
    }

    #[tokio::test]
    async fn s11_ws1_30_rows_key_shortest_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_key_shortest(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-31: wal_min_seq tests

    #[tokio::test]
    async fn s11_ws1_31_wal_min_seq_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_min_seq(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert!(!body.has_records, "fresh WAL must have no records");
        assert_eq!(body.min_sequence, 0, "fresh WAL must have min_sequence = 0");
    }

    #[tokio::test]
    async fn s11_ws1_31_wal_min_seq_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_min_seq(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-31: rows_count_all tests

    #[tokio::test]
    async fn s11_ws1_31_rows_count_all_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_count_all(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.total_count, 0, "fresh store must have zero total rows");
    }

    #[tokio::test]
    async fn s11_ws1_31_rows_count_all_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_count_all(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-32: wal_max_seq tests

    #[tokio::test]
    async fn s11_ws1_32_wal_max_seq_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_max_seq(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert!(!body.has_records, "fresh WAL must have no records");
        assert_eq!(body.max_sequence, 0, "fresh WAL must have max_sequence = 0");
    }

    #[tokio::test]
    async fn s11_ws1_32_wal_max_seq_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_max_seq(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-32: rows_snapshot_size tests

    #[tokio::test]
    async fn s11_ws1_32_rows_snapshot_size_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_snapshot_size(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.snapshot_row_count, 0, "fresh store must have zero snapshot rows");
    }

    #[tokio::test]
    async fn s11_ws1_32_rows_snapshot_size_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_snapshot_size(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-33: wal_entry_count tests

    #[tokio::test]
    async fn s11_ws1_33_wal_entry_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_entry_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.entry_count, 0, "fresh WAL must have zero entries");
    }

    #[tokio::test]
    async fn s11_ws1_33_wal_entry_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_entry_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-33: rows_version_latest tests

    #[tokio::test]
    async fn s11_ws1_33_rows_version_latest_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_version_latest(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.latest_version, 0, "fresh WAL must have latest_version = 0");
    }

    #[tokio::test]
    async fn s11_ws1_33_rows_version_latest_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_version_latest(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-34: wal_size_bytes tests

    #[tokio::test]
    async fn s11_ws1_34_wal_size_bytes_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_size_bytes(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.size_bytes, 0, "fresh WAL must report zero bytes");
    }

    #[tokio::test]
    async fn s11_ws1_34_wal_size_bytes_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_size_bytes(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-34: rows_distinct_count tests

    #[tokio::test]
    async fn s11_ws1_34_rows_distinct_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_distinct_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.distinct_count, 0, "fresh store must have zero distinct rows");
    }

    #[tokio::test]
    async fn s11_ws1_34_rows_distinct_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_distinct_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-35: wal_delete_count tests

    #[tokio::test]
    async fn s11_ws1_35_wal_delete_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_delete_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.delete_count, 0, "fresh WAL must have zero delete records");
    }

    #[tokio::test]
    async fn s11_ws1_35_wal_delete_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_delete_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-35: rows_key_median tests

    #[tokio::test]
    async fn s11_ws1_35_rows_key_median_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_key_median(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert!(!body.has_key, "fresh store must report no median key");
        assert!(body.median_key.is_empty(), "fresh store must return empty median key");
    }

    #[tokio::test]
    async fn s11_ws1_35_rows_key_median_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_key_median(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-36: wal_validate tests

    #[tokio::test]
    async fn s11_ws1_36_wal_validate_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_validate(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert!(body.valid, "fresh WAL sequence ordering must be valid");
        assert_eq!(body.record_count, 0, "fresh WAL must have zero records");
    }

    #[tokio::test]
    async fn s11_ws1_36_wal_validate_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_validate(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-36: rows_checksum tests

    #[tokio::test]
    async fn s11_ws1_36_rows_checksum_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_checksum(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.row_count, 0, "fresh store must have zero rows");
        assert_eq!(body.checksum, 0, "fresh store checksum must be zero");
    }

    #[tokio::test]
    async fn s11_ws1_36_rows_checksum_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_checksum(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-37: wal_entry_oldest tests

    #[tokio::test]
    async fn s11_ws1_37_wal_entry_oldest_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_entry_oldest(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert!(!body.has_entry, "fresh WAL must have no oldest entry");
        assert_eq!(body.entry_sequence, 0, "fresh WAL oldest sequence must be 0");
    }

    #[tokio::test]
    async fn s11_ws1_37_wal_entry_oldest_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_entry_oldest(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-37: rows_field_types tests

    #[tokio::test]
    async fn s11_ws1_37_rows_field_types_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_field_types(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.field_count, 0, "fresh store must have zero fields");
        assert_eq!(body.unique_type_count, 0, "fresh store must have zero unique field types");
    }

    #[tokio::test]
    async fn s11_ws1_37_rows_field_types_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_field_types(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-38: wal_seq_span tests

    #[tokio::test]
    async fn s11_ws1_38_wal_seq_span_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_seq_span(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.oldest_sequence, 0, "fresh WAL oldest sequence must be 0");
        assert_eq!(body.newest_sequence, 0, "fresh WAL newest sequence must be 0");
        assert_eq!(body.sequence_span, 0, "fresh WAL span must be 0");
    }

    #[tokio::test]
    async fn s11_ws1_38_wal_seq_span_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_seq_span(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-38: rows_key_empty_count tests

    #[tokio::test]
    async fn s11_ws1_38_rows_key_empty_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_key_empty_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.empty_key_count, 0, "fresh store must have zero empty keys");
    }

    #[tokio::test]
    async fn s11_ws1_38_rows_key_empty_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_key_empty_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-39: wal_record_active tests

    #[tokio::test]
    async fn s11_ws1_39_wal_record_active_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_record_active(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.active_count, 0, "fresh WAL must have zero active records");
    }

    #[tokio::test]
    async fn s11_ws1_39_wal_record_active_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_record_active(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-39: rows_key_min tests

    #[tokio::test]
    async fn s11_ws1_39_rows_key_min_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_key_min(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert!(!body.has_key, "fresh store must report no min key");
        assert!(body.min_key.is_empty(), "fresh store must return empty min key");
    }

    #[tokio::test]
    async fn s11_ws1_39_rows_key_min_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_key_min(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-40: wal_record_mutations tests

    #[tokio::test]
    async fn s11_ws1_40_wal_record_mutations_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_record_mutations(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.mutation_count, 0, "fresh WAL must have zero mutation records");
    }

    #[tokio::test]
    async fn s11_ws1_40_wal_record_mutations_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_record_mutations(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-40: rows_field_cardinality tests

    #[tokio::test]
    async fn s11_ws1_40_rows_field_cardinality_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_field_cardinality(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.distinct_field_count, 0, "fresh store must have zero distinct fields");
    }

    #[tokio::test]
    async fn s11_ws1_40_rows_field_cardinality_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_field_cardinality(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-41: wal_record_deleted tests

    #[tokio::test]
    async fn s11_ws1_41_wal_record_deleted_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_record_deleted(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.deleted_count, 0, "fresh WAL must have zero deleted records");
    }

    #[tokio::test]
    async fn s11_ws1_41_wal_record_deleted_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_record_deleted(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-41: rows_key_max tests

    #[tokio::test]
    async fn s11_ws1_41_rows_key_max_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_key_max(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert!(!body.has_key, "fresh store must report no max key");
        assert!(body.max_key.is_empty(), "fresh store must return empty max key");
    }

    #[tokio::test]
    async fn s11_ws1_41_rows_key_max_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_key_max(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-42: wal_mutation_span tests

    #[tokio::test]
    async fn s11_ws1_42_wal_mutation_span_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_mutation_span(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.oldest_sequence, 0, "fresh WAL mutation oldest sequence must be 0");
        assert_eq!(body.newest_sequence, 0, "fresh WAL mutation newest sequence must be 0");
        assert_eq!(body.mutation_span, 0, "fresh WAL mutation span must be 0");
    }

    #[tokio::test]
    async fn s11_ws1_42_wal_mutation_span_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_mutation_span(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-42: rows_value_non_null_count tests

    #[tokio::test]
    async fn s11_ws1_42_rows_value_non_null_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_value_non_null_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.non_null_value_count, 0, "fresh store must have zero non-null values");
    }

    #[tokio::test]
    async fn s11_ws1_42_rows_value_non_null_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_value_non_null_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-43: wal_mutation_non_deleted_count tests

    #[tokio::test]
    async fn s11_ws1_43_wal_mutation_non_deleted_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_mutation_non_deleted_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.non_deleted_count, 0, "fresh WAL must have zero non-deleted mutations");
    }

    #[tokio::test]
    async fn s11_ws1_43_wal_mutation_non_deleted_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_mutation_non_deleted_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-43: rows_value_empty_count tests

    #[tokio::test]
    async fn s11_ws1_43_rows_value_empty_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_value_empty_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.empty_value_count, 0, "fresh store must have zero empty values");
    }

    #[tokio::test]
    async fn s11_ws1_43_rows_value_empty_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_value_empty_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-44: wal_non_deleted_span tests

    #[tokio::test]
    async fn s11_ws1_44_wal_non_deleted_span_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_non_deleted_span(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.oldest_sequence, 0, "fresh WAL oldest non-deleted sequence must be 0");
        assert_eq!(body.newest_sequence, 0, "fresh WAL newest non-deleted sequence must be 0");
        assert_eq!(body.non_deleted_span, 0, "fresh WAL non-deleted span must be 0");
    }

    #[tokio::test]
    async fn s11_ws1_44_wal_non_deleted_span_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_non_deleted_span(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-44: rows_value_non_empty_count tests

    #[tokio::test]
    async fn s11_ws1_44_rows_value_non_empty_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_value_non_empty_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.non_empty_value_count, 0, "fresh store must have zero non-empty values");
    }

    #[tokio::test]
    async fn s11_ws1_44_rows_value_non_empty_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_value_non_empty_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-45: wal_non_deleted_count tests

    #[tokio::test]
    async fn s11_ws1_45_wal_non_deleted_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_non_deleted_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.non_deleted_count, 0, "fresh WAL must have zero non-deleted records");
    }

    #[tokio::test]
    async fn s11_ws1_45_wal_non_deleted_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_non_deleted_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-45: rows_key_non_empty_count tests

    #[tokio::test]
    async fn s11_ws1_45_rows_key_non_empty_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_key_non_empty_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.non_empty_key_count, 0, "fresh store must have zero non-empty keys");
    }

    #[tokio::test]
    async fn s11_ws1_45_rows_key_non_empty_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_key_non_empty_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-46: wal_non_deleted_latest tests

    #[tokio::test]
    async fn s11_ws1_46_wal_non_deleted_latest_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_non_deleted_latest(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.latest_non_deleted_sequence, 0, "fresh WAL must have no non-deleted latest sequence");
    }

    #[tokio::test]
    async fn s11_ws1_46_wal_non_deleted_latest_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_non_deleted_latest(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-46: rows_value_non_blank_count tests

    #[tokio::test]
    async fn s11_ws1_46_rows_value_non_blank_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_value_non_blank_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.non_blank_value_count, 0, "fresh store must have zero non-blank values");
    }

    #[tokio::test]
    async fn s11_ws1_46_rows_value_non_blank_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_value_non_blank_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-47: wal_non_deleted_oldest tests

    #[tokio::test]
    async fn s11_ws1_47_wal_non_deleted_oldest_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_non_deleted_oldest(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.oldest_non_deleted_sequence, 0, "fresh WAL must have no non-deleted oldest sequence");
    }

    #[tokio::test]
    async fn s11_ws1_47_wal_non_deleted_oldest_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_non_deleted_oldest(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-47: rows_key_non_blank_count tests

    #[tokio::test]
    async fn s11_ws1_47_rows_key_non_blank_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_key_non_blank_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.non_blank_key_count, 0, "fresh store must have zero non-blank keys");
    }

    #[tokio::test]
    async fn s11_ws1_47_rows_key_non_blank_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_key_non_blank_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-48: wal_non_deleted_newest tests

    #[tokio::test]
    async fn s11_ws1_48_wal_non_deleted_newest_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_non_deleted_newest(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.newest_non_deleted_sequence, 0, "fresh WAL must have no non-deleted newest sequence");
    }

    #[tokio::test]
    async fn s11_ws1_48_wal_non_deleted_newest_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_non_deleted_newest(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-48: rows_value_blank_count tests

    #[tokio::test]
    async fn s11_ws1_48_rows_value_blank_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_value_blank_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.blank_value_count, 0, "fresh store must have zero blank values");
    }

    #[tokio::test]
    async fn s11_ws1_48_rows_value_blank_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_value_blank_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-49: wal_record_total tests

    #[tokio::test]
    async fn s11_ws1_49_wal_record_total_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_record_total(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.total_record_count, 0, "fresh WAL must have zero records");
    }

    #[tokio::test]
    async fn s11_ws1_49_wal_record_total_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_record_total(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-49: rows_key_duplicates_count tests

    #[tokio::test]
    async fn s11_ws1_49_rows_key_duplicates_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_key_duplicates_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.duplicate_key_count, 0, "fresh store must have zero duplicate keys");
    }

    #[tokio::test]
    async fn s11_ws1_49_rows_key_duplicates_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_key_duplicates_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-50: wal_value_duplicates_count tests

    #[tokio::test]
    async fn s11_ws1_50_wal_value_duplicates_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_value_duplicates_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.duplicate_value_count, 0, "fresh WAL must have zero duplicate values");
    }

    #[tokio::test]
    async fn s11_ws1_50_wal_value_duplicates_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_value_duplicates_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-50: rows_value_duplicates_count tests

    #[tokio::test]
    async fn s11_ws1_50_rows_value_duplicates_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_value_duplicates_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.duplicate_value_count, 0, "fresh store must have zero duplicate values");
    }

    #[tokio::test]
    async fn s11_ws1_50_rows_value_duplicates_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_value_duplicates_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-51: wal_value_distinct_count tests

    #[tokio::test]
    async fn s11_ws1_51_wal_value_distinct_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_value_distinct_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.distinct_value_count, 0, "fresh WAL must have zero distinct values");
    }

    #[tokio::test]
    async fn s11_ws1_51_wal_value_distinct_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_value_distinct_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-51: rows_value_distinct_count tests

    #[tokio::test]
    async fn s11_ws1_51_rows_value_distinct_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_value_distinct_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.distinct_value_count, 0, "fresh store must have zero distinct values");
    }

    #[tokio::test]
    async fn s11_ws1_51_rows_value_distinct_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_value_distinct_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-52: wal_value_unique_count tests

    #[tokio::test]
    async fn s11_ws1_52_wal_value_unique_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_value_unique_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.unique_value_count, 0, "fresh WAL must have zero unique values");
    }

    #[tokio::test]
    async fn s11_ws1_52_wal_value_unique_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_value_unique_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-52: rows_value_unique_count tests

    #[tokio::test]
    async fn s11_ws1_52_rows_value_unique_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_value_unique_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.unique_value_count, 0, "fresh store must have zero unique values");
    }

    #[tokio::test]
    async fn s11_ws1_52_rows_value_unique_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_value_unique_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-53: wal_value_trimmed_count tests

    #[tokio::test]
    async fn s11_ws1_53_wal_value_trimmed_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_value_trimmed_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.trimmed_value_count, 0, "fresh WAL must have zero trimmed values");
    }

    #[tokio::test]
    async fn s11_ws1_53_wal_value_trimmed_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_value_trimmed_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-53: rows_value_trimmed_count tests

    #[tokio::test]
    async fn s11_ws1_53_rows_value_trimmed_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_value_trimmed_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.trimmed_value_count, 0, "fresh store must have zero trimmed values");
    }

    #[tokio::test]
    async fn s11_ws1_53_rows_value_trimmed_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_value_trimmed_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-54: wal_value_case_variant_count tests

    #[tokio::test]
    async fn s11_ws1_54_wal_value_case_variant_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_value_case_variant_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.case_variant_count, 0, "fresh WAL must have zero case-variant values");
    }

    #[tokio::test]
    async fn s11_ws1_54_wal_value_case_variant_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_value_case_variant_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-54: rows_value_case_variant_count tests

    #[tokio::test]
    async fn s11_ws1_54_rows_value_case_variant_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_value_case_variant_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.case_variant_count, 0, "fresh store must have zero case-variant values");
    }

    #[tokio::test]
    async fn s11_ws1_54_rows_value_case_variant_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_value_case_variant_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-55: wal_order_by_desc_direction_count tests

    #[tokio::test]
    async fn s11_ws1_55_wal_order_by_desc_direction_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_order_by_desc_direction_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.desc_direction_count, 0, "fresh store must have zero DESC directions");
    }

    #[tokio::test]
    async fn s11_ws1_55_wal_order_by_desc_direction_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_order_by_desc_direction_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-55: rows_order_by_desc_direction_count tests

    #[tokio::test]
    async fn s11_ws1_55_rows_order_by_desc_direction_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_order_by_desc_direction_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.desc_direction_count, 0, "fresh store must have zero DESC directions");
    }

    #[tokio::test]
    async fn s11_ws1_55_rows_order_by_desc_direction_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_order_by_desc_direction_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-56: wal_order_by_random_count tests

    #[tokio::test]
    async fn s11_ws1_56_wal_order_by_random_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_order_by_random_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.random_order_count, 0, "fresh store must have zero RANDOM order counts");
    }

    #[tokio::test]
    async fn s11_ws1_56_wal_order_by_random_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_order_by_random_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-56: rows_order_by_random_count tests

    #[tokio::test]
    async fn s11_ws1_56_rows_order_by_random_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_order_by_random_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.random_order_count, 0, "fresh store must have zero RANDOM order counts");
    }

    #[tokio::test]
    async fn s11_ws1_56_rows_order_by_random_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_order_by_random_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-57: wal_order_by_random_seeded_count tests

    #[tokio::test]
    async fn s11_ws1_57_wal_order_by_random_seeded_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_order_by_random_seeded_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.random_seeded_order_count, 0, "fresh store must have zero RANDOM(seed) order counts");
    }

    #[tokio::test]
    async fn s11_ws1_57_wal_order_by_random_seeded_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_order_by_random_seeded_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-57: rows_order_by_random_seeded_count tests

    #[tokio::test]
    async fn s11_ws1_57_rows_order_by_random_seeded_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_order_by_random_seeded_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.random_seeded_order_count, 0, "fresh store must have zero RANDOM(seed) order counts");
    }

    #[tokio::test]
    async fn s11_ws1_57_rows_order_by_random_seeded_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_order_by_random_seeded_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-58: wal_order_by_asc_direction_count tests

    #[tokio::test]
    async fn s11_ws1_58_wal_order_by_asc_direction_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_order_by_asc_direction_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.asc_direction_count, 0, "fresh store must have zero ASC direction counts");
    }

    #[tokio::test]
    async fn s11_ws1_58_wal_order_by_asc_direction_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_order_by_asc_direction_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-58: rows_order_by_asc_direction_count tests

    #[tokio::test]
    async fn s11_ws1_58_rows_order_by_asc_direction_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_order_by_asc_direction_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.asc_direction_count, 0, "fresh store must have zero ASC direction counts");
    }

    #[tokio::test]
    async fn s11_ws1_58_rows_order_by_asc_direction_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_order_by_asc_direction_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-59: wal_order_by_rand_alias_count tests

    #[tokio::test]
    async fn s11_ws1_59_wal_order_by_rand_alias_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_order_by_rand_alias_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.rand_alias_count, 0, "fresh store must have zero RAND alias counts");
    }

    #[tokio::test]
    async fn s11_ws1_59_wal_order_by_rand_alias_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_order_by_rand_alias_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-59: rows_order_by_rand_alias_count tests

    #[tokio::test]
    async fn s11_ws1_59_rows_order_by_rand_alias_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_order_by_rand_alias_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.rand_alias_count, 0, "fresh store must have zero RAND alias counts");
    }

    #[tokio::test]
    async fn s11_ws1_59_rows_order_by_rand_alias_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_order_by_rand_alias_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-60: wal_order_by_multi_column_count tests

    #[tokio::test]
    async fn s11_ws1_60_wal_order_by_multi_column_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_order_by_multi_column_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.multi_column_order_count, 0, "fresh store must have zero multi-column ORDER BY counts");
    }

    #[tokio::test]
    async fn s11_ws1_60_wal_order_by_multi_column_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_order_by_multi_column_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-60: rows_order_by_multi_column_count tests

    #[tokio::test]
    async fn s11_ws1_60_rows_order_by_multi_column_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_order_by_multi_column_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.multi_column_order_count, 0, "fresh store must have zero multi-column ORDER BY counts");
    }

    #[tokio::test]
    async fn s11_ws1_60_rows_order_by_multi_column_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_order_by_multi_column_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-61: wal_pagination_limit_offset_count tests

    #[tokio::test]
    async fn s11_ws1_61_wal_pagination_limit_offset_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_pagination_limit_offset_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.limit_offset_pagination_count, 0, "fresh store must have zero LIMIT+OFFSET pagination counts");
    }

    #[tokio::test]
    async fn s11_ws1_61_wal_pagination_limit_offset_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_pagination_limit_offset_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-61: rows_pagination_limit_offset_count tests

    #[tokio::test]
    async fn s11_ws1_61_rows_pagination_limit_offset_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_pagination_limit_offset_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.limit_offset_pagination_count, 0, "fresh store must have zero LIMIT+OFFSET pagination counts");
    }

    #[tokio::test]
    async fn s11_ws1_61_rows_pagination_limit_offset_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_pagination_limit_offset_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-62: wal_pagination_offset_only_count tests

    #[tokio::test]
    async fn s11_ws1_62_wal_pagination_offset_only_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_pagination_offset_only_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.offset_only_pagination_count, 0, "fresh store must have zero OFFSET-only pagination counts");
    }

    #[tokio::test]
    async fn s11_ws1_62_wal_pagination_offset_only_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_pagination_offset_only_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-62: rows_pagination_offset_only_count tests

    #[tokio::test]
    async fn s11_ws1_62_rows_pagination_offset_only_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_pagination_offset_only_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.offset_only_pagination_count, 0, "fresh store must have zero OFFSET-only pagination counts");
    }

    #[tokio::test]
    async fn s11_ws1_62_rows_pagination_offset_only_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_pagination_offset_only_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-63: wal_having_without_group_by_count tests

    #[tokio::test]
    async fn s11_ws1_63_wal_having_without_group_by_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_having_without_group_by_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.having_without_group_by_count, 0, "fresh WAL must have zero HAVING-without-GROUP-BY counts");
    }

    #[tokio::test]
    async fn s11_ws1_63_wal_having_without_group_by_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_having_without_group_by_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-63: rows_having_without_group_by_count tests

    #[tokio::test]
    async fn s11_ws1_63_rows_having_without_group_by_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_having_without_group_by_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.having_without_group_by_count, 0, "fresh rows must have zero HAVING-without-GROUP-BY counts");
    }

    #[tokio::test]
    async fn s11_ws1_63_rows_having_without_group_by_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_having_without_group_by_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-64: wal_having_with_group_by_count tests

    #[tokio::test]
    async fn s11_ws1_64_wal_having_with_group_by_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_having_with_group_by_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.having_with_group_by_count, 0, "fresh WAL must have zero HAVING-with-GROUP-BY counts");
    }

    #[tokio::test]
    async fn s11_ws1_64_wal_having_with_group_by_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_having_with_group_by_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-64: rows_having_with_group_by_count tests

    #[tokio::test]
    async fn s11_ws1_64_rows_having_with_group_by_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_having_with_group_by_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.having_with_group_by_count, 0, "fresh rows must have zero HAVING-with-GROUP-BY counts");
    }

    #[tokio::test]
    async fn s11_ws1_64_rows_having_with_group_by_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_having_with_group_by_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-65: wal_group_by_rollup_count tests

    #[tokio::test]
    async fn s11_ws1_65_wal_group_by_rollup_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_group_by_rollup_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.group_by_rollup_count, 0, "fresh WAL must have zero GROUP-BY-ROLLUP counts");
    }

    #[tokio::test]
    async fn s11_ws1_65_wal_group_by_rollup_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_group_by_rollup_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-65: rows_group_by_rollup_count tests

    #[tokio::test]
    async fn s11_ws1_65_rows_group_by_rollup_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_group_by_rollup_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.group_by_rollup_count, 0, "fresh rows must have zero GROUP-BY-ROLLUP counts");
    }

    #[tokio::test]
    async fn s11_ws1_65_rows_group_by_rollup_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_group_by_rollup_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-66: wal_group_by_cube_count tests

    #[tokio::test]
    async fn s11_ws1_66_wal_group_by_cube_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_group_by_cube_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.group_by_cube_count, 0, "fresh WAL must have zero GROUP-BY-CUBE counts");
    }

    #[tokio::test]
    async fn s11_ws1_66_wal_group_by_cube_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_group_by_cube_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-66: rows_group_by_cube_count tests

    #[tokio::test]
    async fn s11_ws1_66_rows_group_by_cube_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_group_by_cube_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.group_by_cube_count, 0, "fresh rows must have zero GROUP-BY-CUBE counts");
    }

    #[tokio::test]
    async fn s11_ws1_66_rows_group_by_cube_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_group_by_cube_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-67: wal_select_distinct_on_count tests

    #[tokio::test]
    async fn s11_ws1_67_wal_select_distinct_on_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_select_distinct_on_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.select_distinct_on_count, 0, "fresh WAL must have zero SELECT-DISTINCT-ON counts");
    }

    #[tokio::test]
    async fn s11_ws1_67_wal_select_distinct_on_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_select_distinct_on_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-67: rows_select_distinct_on_count tests

    #[tokio::test]
    async fn s11_ws1_67_rows_select_distinct_on_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_select_distinct_on_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.select_distinct_on_count, 0, "fresh rows must have zero SELECT-DISTINCT-ON counts");
    }

    #[tokio::test]
    async fn s11_ws1_67_rows_select_distinct_on_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_select_distinct_on_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-68: wal_for_update_count tests

    #[tokio::test]
    async fn s11_ws1_68_wal_for_update_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_for_update_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.for_update_count, 0, "fresh WAL must have zero FOR-UPDATE counts");
    }

    #[tokio::test]
    async fn s11_ws1_68_wal_for_update_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_for_update_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-68: rows_for_update_count tests

    #[tokio::test]
    async fn s11_ws1_68_rows_for_update_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_for_update_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.for_update_count, 0, "fresh rows must have zero FOR-UPDATE counts");
    }

    #[tokio::test]
    async fn s11_ws1_68_rows_for_update_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_for_update_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-69: wal_left_join_count tests

    #[tokio::test]
    async fn s11_ws1_69_wal_left_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_left_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.left_join_count, 0, "fresh WAL must have zero LEFT-JOIN counts");
    }

    #[tokio::test]
    async fn s11_ws1_69_wal_left_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_left_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-69: rows_left_join_count tests

    #[tokio::test]
    async fn s11_ws1_69_rows_left_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_left_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.left_join_count, 0, "fresh rows must have zero LEFT-JOIN counts");
    }

    #[tokio::test]
    async fn s11_ws1_69_rows_left_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_left_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-70: wal_right_join_count tests

    #[tokio::test]
    async fn s11_ws1_70_wal_right_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_right_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.right_join_count, 0, "fresh WAL must have zero RIGHT-JOIN counts");
    }

    #[tokio::test]
    async fn s11_ws1_70_wal_right_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_right_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-70: rows_right_join_count tests

    #[tokio::test]
    async fn s11_ws1_70_rows_right_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_right_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.right_join_count, 0, "fresh rows must have zero RIGHT-JOIN counts");
    }

    #[tokio::test]
    async fn s11_ws1_70_rows_right_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_right_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-71: wal_full_outer_join_count tests

    #[tokio::test]
    async fn s11_ws1_71_wal_full_outer_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_full_outer_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.full_outer_join_count,
            0,
            "fresh WAL must have zero FULL-OUTER-JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_71_wal_full_outer_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_full_outer_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-71: rows_full_outer_join_count tests

    #[tokio::test]
    async fn s11_ws1_71_rows_full_outer_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_full_outer_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.full_outer_join_count,
            0,
            "fresh rows must have zero FULL-OUTER-JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_71_rows_full_outer_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_full_outer_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-72: wal_inner_join_count tests

    #[tokio::test]
    async fn s11_ws1_72_wal_inner_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_inner_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.inner_join_count,
            0,
            "fresh WAL must have zero INNER-JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_72_wal_inner_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_inner_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-72: rows_inner_join_count tests

    #[tokio::test]
    async fn s11_ws1_72_rows_inner_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_inner_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.inner_join_count,
            0,
            "fresh rows must have zero INNER-JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_72_rows_inner_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_inner_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-73: wal_straight_join_count tests

    #[tokio::test]
    async fn s11_ws1_73_wal_straight_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_straight_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.straight_join_count,
            0,
            "fresh WAL must have zero STRAIGHT_JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_73_wal_straight_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_straight_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-73: rows_straight_join_count tests

    #[tokio::test]
    async fn s11_ws1_73_rows_straight_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_straight_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.straight_join_count,
            0,
            "fresh rows must have zero STRAIGHT_JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_73_rows_straight_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_straight_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-74: wal_semi_join_count tests

    #[tokio::test]
    async fn s11_ws1_74_wal_semi_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_semi_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.semi_join_count,
            0,
            "fresh WAL must have zero SEMI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_74_wal_semi_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_semi_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-74: rows_semi_join_count tests

    #[tokio::test]
    async fn s11_ws1_74_rows_semi_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_semi_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.semi_join_count,
            0,
            "fresh rows must have zero SEMI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_74_rows_semi_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_semi_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-75: wal_anti_join_count tests

    #[tokio::test]
    async fn s11_ws1_75_wal_anti_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_anti_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.anti_join_count, 0, "fresh WAL must have zero ANTI JOIN counts");
    }

    #[tokio::test]
    async fn s11_ws1_75_wal_anti_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_anti_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-75: rows_anti_join_count tests

    #[tokio::test]
    async fn s11_ws1_75_rows_anti_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_anti_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.anti_join_count,
            0,
            "fresh rows must have zero ANTI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_75_rows_anti_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_anti_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-76: wal_cross_apply_count tests

    #[tokio::test]
    async fn s11_ws1_76_wal_cross_apply_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_cross_apply_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.cross_apply_count,
            0,
            "fresh WAL must have zero CROSS APPLY counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_76_wal_cross_apply_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_cross_apply_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-76: rows_cross_apply_count tests

    #[tokio::test]
    async fn s11_ws1_76_rows_cross_apply_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_cross_apply_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.cross_apply_count,
            0,
            "fresh rows must have zero CROSS APPLY counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_76_rows_cross_apply_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_cross_apply_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-77: wal_outer_apply_count tests

    #[tokio::test]
    async fn s11_ws1_77_wal_outer_apply_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_outer_apply_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.outer_apply_count,
            0,
            "fresh WAL must have zero OUTER APPLY counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_77_wal_outer_apply_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_outer_apply_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-77: rows_outer_apply_count tests

    #[tokio::test]
    async fn s11_ws1_77_rows_outer_apply_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_outer_apply_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.outer_apply_count,
            0,
            "fresh rows must have zero OUTER APPLY counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_77_rows_outer_apply_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_outer_apply_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-78: wal_apply_count tests

    #[tokio::test]
    async fn s11_ws1_78_wal_apply_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_apply_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.apply_count, 0, "fresh WAL must have zero APPLY counts");
    }

    #[tokio::test]
    async fn s11_ws1_78_wal_apply_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_apply_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-78: rows_apply_count tests

    #[tokio::test]
    async fn s11_ws1_78_rows_apply_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_apply_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(body.apply_count, 0, "fresh rows must have zero APPLY counts");
    }

    #[tokio::test]
    async fn s11_ws1_78_rows_apply_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_apply_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-79: wal_left_semi_join_count tests

    #[tokio::test]
    async fn s11_ws1_79_wal_left_semi_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_left_semi_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.left_semi_join_count,
            0,
            "fresh WAL must have zero LEFT SEMI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_79_wal_left_semi_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_left_semi_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-79: rows_left_semi_join_count tests

    #[tokio::test]
    async fn s11_ws1_79_rows_left_semi_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_left_semi_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.left_semi_join_count,
            0,
            "fresh rows must have zero LEFT SEMI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_79_rows_left_semi_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_left_semi_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-80: wal_left_anti_join_count tests

    #[tokio::test]
    async fn s11_ws1_80_wal_left_anti_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_left_anti_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.left_anti_join_count,
            0,
            "fresh WAL must have zero LEFT ANTI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_80_wal_left_anti_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_left_anti_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-80: rows_left_anti_join_count tests

    #[tokio::test]
    async fn s11_ws1_80_rows_left_anti_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_left_anti_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.left_anti_join_count,
            0,
            "fresh rows must have zero LEFT ANTI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_80_rows_left_anti_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_left_anti_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-81: wal_right_semi_join_count tests

    #[tokio::test]
    async fn s11_ws1_81_wal_right_semi_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_right_semi_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.right_semi_join_count,
            0,
            "fresh WAL must have zero RIGHT SEMI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_81_wal_right_semi_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_right_semi_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-81: rows_right_semi_join_count tests

    #[tokio::test]
    async fn s11_ws1_81_rows_right_semi_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_right_semi_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.right_semi_join_count,
            0,
            "fresh rows must have zero RIGHT SEMI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_81_rows_right_semi_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_right_semi_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-82: wal_right_anti_join_count tests

    #[tokio::test]
    async fn s11_ws1_82_wal_right_anti_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_right_anti_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.right_anti_join_count,
            0,
            "fresh WAL must have zero RIGHT ANTI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_82_wal_right_anti_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_right_anti_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-82: rows_right_anti_join_count tests

    #[tokio::test]
    async fn s11_ws1_82_rows_right_anti_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_right_anti_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.right_anti_join_count,
            0,
            "fresh rows must have zero RIGHT ANTI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_82_rows_right_anti_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_right_anti_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-83: wal_full_semi_join_count tests

    #[tokio::test]
    async fn s11_ws1_83_wal_full_semi_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_full_semi_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.full_semi_join_count,
            0,
            "fresh WAL must have zero FULL SEMI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_83_wal_full_semi_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_full_semi_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-83: rows_full_semi_join_count tests

    #[tokio::test]
    async fn s11_ws1_83_rows_full_semi_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_full_semi_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.full_semi_join_count,
            0,
            "fresh rows must have zero FULL SEMI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_83_rows_full_semi_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_full_semi_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-84: wal_full_anti_join_count tests

    #[tokio::test]
    async fn s11_ws1_84_wal_full_anti_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_full_anti_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.full_anti_join_count,
            0,
            "fresh WAL must have zero FULL ANTI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_84_wal_full_anti_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_full_anti_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-84: rows_full_anti_join_count tests

    #[tokio::test]
    async fn s11_ws1_84_rows_full_anti_join_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_full_anti_join_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.full_anti_join_count,
            0,
            "fresh rows must have zero FULL ANTI JOIN counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_84_rows_full_anti_join_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_full_anti_join_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-85: wal_union_all_count tests

    #[tokio::test]
    async fn s11_ws1_85_wal_union_all_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_union_all_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.union_all_count,
            0,
            "fresh WAL must have zero UNION ALL counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_85_wal_union_all_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_union_all_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-85: rows_union_all_count tests

    #[tokio::test]
    async fn s11_ws1_85_rows_union_all_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_union_all_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.union_all_count,
            0,
            "fresh rows must have zero UNION ALL counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_85_rows_union_all_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_union_all_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-86: wal_aggregate_distinct_count tests

    #[tokio::test]
    async fn s11_ws1_86_wal_aggregate_distinct_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_aggregate_distinct_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.aggregate_distinct_count,
            0,
            "fresh WAL must have zero aggregate DISTINCT counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_86_wal_aggregate_distinct_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_aggregate_distinct_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-86: rows_aggregate_distinct_count tests

    #[tokio::test]
    async fn s11_ws1_86_rows_aggregate_distinct_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_aggregate_distinct_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.aggregate_distinct_count,
            0,
            "fresh rows must have zero aggregate DISTINCT counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_86_rows_aggregate_distinct_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_aggregate_distinct_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-87: wal_table_alias_count tests

    #[tokio::test]
    async fn s11_ws1_87_wal_table_alias_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_table_alias_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.table_alias_count,
            0,
            "fresh WAL must have zero table-alias counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_87_wal_table_alias_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_table_alias_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-87: rows_table_alias_count tests

    #[tokio::test]
    async fn s11_ws1_87_rows_table_alias_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_table_alias_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.table_alias_count,
            0,
            "fresh rows must have zero table-alias counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_87_rows_table_alias_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_table_alias_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-88: wal_column_alias_count + rows_column_alias_count tests

    #[tokio::test]
    async fn s11_ws1_88_wal_column_alias_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_column_alias_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.column_alias_count,
            0,
            "fresh WAL must have zero column-alias counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_88_wal_column_alias_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = wal_column_alias_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    // S3-WS1-88: rows_column_alias_count tests

    #[tokio::test]
    async fn s11_ws1_88_rows_column_alias_count_ok() {
        let state = state_with_key(Some("test-key"));
        let hdrs = operator_headers("test-key", "admin");
        let (status, Json(body)) = rows_column_alias_count(State(state), hdrs).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.status, "ok");
        assert_eq!(
            body.column_alias_count,
            0,
            "fresh rows must have zero column-alias counts"
        );
    }

    #[tokio::test]
    async fn s11_ws1_88_rows_column_alias_count_missing_auth() {
        let state = state_with_key(Some("test-key"));
        let hdrs = HeaderMap::new();
        let res = rows_column_alias_count(State(state), hdrs).await;
        assert!(res.is_err(), "missing auth should be rejected");
        assert_eq!(res.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_cluster_topology_reports_runtime_counts() {
        let state = state_with_key(Some("secret"));
        {
            let mut sessions = state.driver_sessions.lock().unwrap();
            sessions.insert("sess-a".to_string(), DriverSession {
                driver_name: "rust".to_string(),
                driver_version: "1.0.0".to_string(),
                connected_at_ms: 1,
                assigned_node_id: "node-1".to_string(),
                pooled_connection_id: None,
            });
        }
        {
            let mut acid = state.acid_transactions.lock().unwrap();
            acid.begin("tx-1", "node-1", "read_committed", now_unix_ms());
        }
        let (status, Json(body)) = admin_cluster_topology(State(state), admin_headers("secret")).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.total_nodes, 1);
        assert_eq!(body.active_sessions, 1);
        assert_eq!(body.live_transactions, 1);
        assert_eq!(body.nodes[0].node_id, "node-1");
    }

    #[tokio::test]
    async fn admin_transaction_control_can_rollback_and_release_locks() {
        let state = state_with_key(Some("secret"));
        {
            let mut acid = state.acid_transactions.lock().unwrap();
            acid.begin("tx-admin-1", "node-1", "serializable", now_unix_ms());
        }
        {
            let mut locks = state.pessimistic_locks.lock().unwrap();
            locks.insert("lock-1".to_string(), PessimisticLockRecord {
                lock_id: "lock-1".to_string(),
                transaction_id: "tx-admin-1".to_string(),
                resource: "users:1".to_string(),
                owner: "test-owner".to_string(),
                acquired_unix_ms: now_unix_ms(),
                expires_unix_ms: now_unix_ms() + 30_000,
            });
        }
        let req = AdminTransactionControlRequest {
            action: "rollback".to_string(),
            transaction_id: Some("tx-admin-1".to_string()),
            reason: Some("test".to_string()),
        };
        let (status, Json(body)) = admin_sql_transaction_control(State(state.clone()), admin_headers("secret"), Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.affected_count, 1);
        assert!(state.pessimistic_locks.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn admin_lock_control_can_kill_deadlock_victim() {
        let state = state_with_key(Some("secret"));
        {
            let mut acid = state.acid_transactions.lock().unwrap();
            acid.begin("tx-dead", "node-1", "read_committed", now_unix_ms());
        }
        {
            let mut locks = state.pessimistic_locks.lock().unwrap();
            locks.insert("lock-dead".to_string(), PessimisticLockRecord {
                lock_id: "lock-dead".to_string(),
                transaction_id: "tx-dead".to_string(),
                resource: "orders:7".to_string(),
                owner: "test-owner".to_string(),
                acquired_unix_ms: now_unix_ms(),
                expires_unix_ms: now_unix_ms() + 30_000,
            });
        }
        let req = AdminLockControlRequest {
            action: "kill_deadlock".to_string(),
            lock_id: None,
            transaction_id: Some("tx-dead".to_string()),
            reason: Some("cycle_detected".to_string()),
        };
        let (status, Json(body)) = admin_sql_lock_control(State(state.clone()), admin_headers("secret"), Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.released_lock_count, 1);
        assert!(body.affected_transactions.contains(&"tx-dead".to_string()));
    }

    #[tokio::test]
    async fn admin_cluster_node_manage_removes_node_and_migrates_work() {
        let state = state_with_key(Some("secret"));
        {
            let mut nodes = state.cluster_nodes.lock().unwrap();
            nodes.insert("node-2".to_string(), ClusterNodeRuntime {
                node_id: "node-2".to_string(),
                role: "follower".to_string(),
                status: "active".to_string(),
                total_cpu_cores: 4,
                total_ram_mb: 8192,
                draining: false,
                last_heartbeat_ms: now_unix_ms_u64(),
            });
        }
        {
            let mut sessions = state.driver_sessions.lock().unwrap();
            sessions.insert("sess-node-2".to_string(), DriverSession {
                driver_name: "rust".to_string(),
                driver_version: "1.0.0".to_string(),
                connected_at_ms: 1,
                assigned_node_id: "node-2".to_string(),
                pooled_connection_id: None,
            });
        }
        {
            let mut acid = state.acid_transactions.lock().unwrap();
            acid.begin("tx-node-2", "node-2", "read_committed", now_unix_ms());
        }
        let req = AdminClusterNodeManageRequest {
            action: "remove".to_string(),
            node_id: "node-2".to_string(),
            role: None,
            desired_status: None,
            total_cpu_cores: None,
            total_ram_mb: None,
            target_node_id: Some("node-1".to_string()),
            reason: Some("scale_in".to_string()),
        };
        let (status, Json(body)) = admin_cluster_node_manage(State(state.clone()), admin_headers("secret"), Json(req)).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.migrated_transactions, 1);
        assert_eq!(body.migrated_sessions, 1);
        assert!(!state.cluster_nodes.lock().unwrap().contains_key("node-2"));
        let acid = state.acid_transactions.lock().unwrap();
        assert_eq!(acid.transactions.get("tx-node-2").unwrap().assigned_node_id, "node-1");
    }

    // ── NT-S6-001: native Auth bearer token tests ─────────────────────────────

    fn native_config_with_bearer(token: Option<&str>) -> NativeListenerConfig {
        NativeListenerConfig {
            enabled: true,
            bind: "127.0.0.1:7542".to_string(),
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
            tls_client_ca_path: None,
            max_connections: 2048,
            idle_timeout_ms: 60000,
            handshake_timeout_ms: 5000,
            heartbeat_interval_ms: 15000,
            max_frame_bytes: 1_048_576,
            compression_enabled: false,
            compression_threshold_bytes: 4096,
            bearer_token: token.map(|t| t.to_string()),
        }
    }

    #[test]
    fn native_auth_bearer_token_accepted_when_configured() {
        // admin_api_key = None, bearer_token = Some("tok-abc")
        let state = state_with_key(None);
        let config = native_config_with_bearer(Some("tok-abc"));
        let payload = json!({ "bearer_token": "tok-abc" });
        assert!(
            native_auth_payload_matches_runtime(&state, &config, &payload),
            "correct bearer token must be accepted"
        );
    }

    #[test]
    fn native_auth_bearer_token_rejected_when_wrong() {
        let state = state_with_key(None);
        let config = native_config_with_bearer(Some("tok-abc"));
        let payload = json!({ "bearer_token": "tok-wrong" });
        assert!(
            !native_auth_payload_matches_runtime(&state, &config, &payload),
            "wrong bearer token must be rejected"
        );
    }

    #[test]
    fn native_auth_admin_key_still_accepted_alongside_bearer_config() {
        // Both admin_api_key and bearer_token configured; sending the admin key must work.
        let state = state_with_key(Some("admin-secret"));
        let config = native_config_with_bearer(Some("tok-abc"));
        let payload = json!({ "admin_api_key": "admin-secret" });
        assert!(
            native_auth_payload_matches_runtime(&state, &config, &payload),
            "admin_api_key must still be accepted when bearer_token is also configured"
        );
    }

    #[test]
    fn native_auth_open_listener_accepts_empty_payload() {
        // Neither credential configured → open listener.
        let state = state_with_key(None);
        let config = native_config_with_bearer(None);
        let payload = json!({});
        assert!(
            native_auth_payload_matches_runtime(&state, &config, &payload),
            "open listener must accept any payload"
        );
    }

}
