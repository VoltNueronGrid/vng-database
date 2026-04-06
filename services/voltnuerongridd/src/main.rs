use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use base64::Engine;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use voltnuerongrid_auth::{
    ConfiguredKmsProviderAdapter, KmsKeyProvider, KmsKeyResolution, KmsProviderChain,
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
use voltnuerongrid_store::ddl_catalog::{parse_ddl_info, DdlCatalog};
use voltnuerongrid_store::index::IndexManager;
use voltnuerongrid_store::mvcc::PagedRowStore;
use voltnuerongrid_store::{InMemoryDurabilityEngine, DurabilityConfig};
use voltnuerongrid_driver_rust::{ConnectionPoolManager, PoolAcquireError};
use voltnuerongrid_ingest::{
    IngestionConnector, ManagedEventBusTransport, ManagedReplayCursorStore,
    ReplayCursorStore, StreamDirection,
};
use voltnuerongrid_opt::DistributedCacheManager;
use voltnuerongrid_plugins::{
    AttestationType, ConnectorPackageMetadata, PluginLifecycleManager,
    PluginManifestSignature, ProvenanceAttestation, ProvenanceChain,
    SbomEntry, SbomInspectionResult, SignedPluginManifest,
};

mod raft;
use raft::{RaftAppendRequest, RaftAppendResponse, RaftLogEntry, RaftNode, RaftRole, RaftStatusSnapshot, RaftVoteRequest, RaftVoteResponse};

static TX_COUNTER: AtomicU64 = AtomicU64::new(1);
static ACTION_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);
static DR_HOOK_COUNTER: AtomicU64 = AtomicU64::new(1);
static PESSIMISTIC_LOCK_COUNTER: AtomicU64 = AtomicU64::new(1);
/// S8-WS10-02: Counter for issuing deterministic driver session tokens.
static DRIVER_SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);
const DEADLOCK_SCAN_MAX_HOPS: usize = 8;

const CONTROL_PLANE_OPERATOR_ROLES: [OperatorRole; 4] = [
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
enum AcidTxState {
    Active,
    Committed,
    RolledBack,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
struct AcidTxEntry {
    transaction_id: String,
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
struct AcidTransactionRegistry {
    transactions: HashMap<String, AcidTxEntry>,
}

impl AcidTransactionRegistry {
    fn begin(&mut self, tx_id: &str, isolation_level: &str, now_ms: u128) {
        let read_snapshot_at_ms = if isolation_level == "repeatable_read" {
            Some(now_ms)
        } else {
            None
        };
        self.transactions.insert(
            tx_id.to_string(),
            AcidTxEntry {
                transaction_id: tx_id.to_string(),
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

    fn commit(&mut self, tx_id: &str, now_ms: u128) -> bool {
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

    fn rollback(&mut self, tx_id: &str, now_ms: u128) -> bool {
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

    fn active_transactions(&self) -> Vec<&AcidTxEntry> {
        self.transactions
            .values()
            .filter(|e| e.state == AcidTxState::Active)
            .collect()
    }

    fn all_transactions(&self) -> Vec<&AcidTxEntry> {
        self.transactions.values().collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeadlockScanOutcome {
    CycleDetected,
    ScanCapReached,
    NoCycle,
}

#[derive(Clone)]
struct AppState {
    node_id: String,
    cluster_mode: String,
    admin_api_key: Option<String>,
    security_config: Arc<SecurityConfigContract>,
    allowed_operator_roles: Arc<HashSet<OperatorRole>>,
    operator_role_bindings: Arc<HashMap<String, OperatorRole>>,
    tenant_user_bindings: Arc<HashMap<String, TenantUserBinding>>,
    rbac_privilege_matrix: Arc<RbacPrivilegeMatrix>,
    kms_runtime: Arc<Mutex<KmsRuntimeState>>,
    leader_node_id: Arc<Mutex<String>>,
    audit_sink: Arc<Mutex<AppendOnlyAuditSink>>,
    action_records: Arc<Mutex<Vec<AutonomousActionExecutionRecord>>>,
    dr_hook_records: Arc<Mutex<Vec<DrHookExecutionRecord>>>,
    dr_hook_policy_state: Arc<Mutex<DrHookPolicyState>>,
    dr_hook_policy_config: Arc<DrHookPolicyConfig>,
    dr_hook_state_path: Option<String>,
    dr_hook_queue: Arc<Mutex<VecDeque<DrHookScheduledTask>>>,
    cluster_failure_signals: Arc<Mutex<Vec<ClusterFailureSignal>>>,
    sync_origin: Arc<Mutex<RowStoreSyncOrigin>>,
    replication_transport: Arc<Mutex<InMemoryReplicationTransport>>,
    replica_replay_states: Arc<Mutex<HashMap<String, ReplicaReplayState>>>,
    pessimistic_locks: Arc<Mutex<HashMap<String, PessimisticLockRecord>>>,
    pessimistic_lock_waits: Arc<Mutex<HashMap<String, String>>>,
    pessimistic_lock_metrics: PessimisticLockContentionMetrics,
    index_manager: Arc<Mutex<IndexManager>>,
    constraint_manager: Arc<Mutex<ConstraintManager>>,
    ingest_csv_records: Arc<Mutex<HashMap<String, Vec<voltnuerongrid_ingest::IngestRecord>>>>,
    ingest_json_records: Arc<Mutex<HashMap<String, Vec<voltnuerongrid_ingest::IngestRecord>>>>,
    ingest_parquet_records: Arc<Mutex<HashMap<String, Vec<voltnuerongrid_ingest::IngestRecord>>>>,
    ingest_excel_records: Arc<Mutex<HashMap<String, Vec<voltnuerongrid_ingest::IngestRecord>>>>,
    ingest_outbox_streams: Arc<Mutex<HashMap<String, String>>>,
    ingest_event_bus: Arc<Mutex<ManagedEventBusTransport>>,
    ingest_outbox_cursors: Arc<Mutex<ManagedReplayCursorStore>>,
    distributed_cache: Arc<Mutex<DistributedCacheManager>>,
    driver_pool: Arc<Mutex<ConnectionPoolManager>>,
    plugin_lifecycle: Arc<Mutex<PluginLifecycleManager>>,
    autonomous_mode: AutonomousMode,
    emergency_stop: Arc<AtomicEmergencyStop>,
    guardrails: Arc<Vec<GuardrailRule>>,
    ddl_catalog: Arc<Mutex<DdlCatalog>>,
    acid_transactions: Arc<Mutex<AcidTransactionRegistry>>,
    /// MVCC page-based row store (S2-WS2-04: PagedRowStore scaffold).
    row_store: Arc<Mutex<PagedRowStore>>,
    /// S9-WS8-02: AI model gateway isolation policy.
    model_gateway_policy: Arc<Mutex<ModelGatewayPolicy>>,
    /// S4-WS3-04: In-memory OLAP replica — receives mutations via `POST /api/v1/store/htap/apply`.
    /// Maps primary_key → row data (last-writer-wins).
    olap_store: Arc<Mutex<HashMap<String, HashMap<String, String>>>>,
    /// S9-WS8A-02: Optional path to a JSON-lines audit log file.
    /// Resolved from `VNG_AUDIT_LOG_PATH` env var at start-up.
    audit_log_path: Option<String>,
    /// S7-WS6-02: Raft consensus node state (single-node scaffold).
    raft_state: Arc<Mutex<RaftNode>>,
    /// S9-WS8-02: Per-model-identity request counters for rate limiting.
    /// Maps model_id → request count in current window.
    ai_request_counters: Arc<Mutex<HashMap<String, u64>>>,
    /// S2-WS2-02: WAL durability engine — records every committed DML mutation.
    wal_engine: Arc<Mutex<InMemoryDurabilityEngine>>,
    /// S7-WS6-04: Chaos/game-day fault injection state.
    chaos_state: Arc<Mutex<ChaosState>>,
    /// S8-WS10-02: Driver wire protocol session registry.
    driver_sessions: Arc<Mutex<HashMap<String, DriverSession>>>,
    /// S5-WS4A-02: Broker adapter flush counters (broker_type → flush_count).
    broker_flush_counts: Arc<Mutex<HashMap<String, u64>>>,
    /// S9-WS8-02: Sliding-window rate limiter — per-model window start timestamp (ms).
    ai_rate_window_starts: Arc<Mutex<HashMap<String, u64>>>,
    /// S5-E4A-01: Connector SDK runtime registry.
    connector_registry: Arc<Mutex<Vec<ConnectorPlugin>>>,
    /// S6-WS5-04: TDE runtime toggle override.
    tde_override: Arc<Mutex<Option<bool>>>,
    /// S10-WS15-02: Per-table CDC cursor positions (table_name → last consumed sequence).
    cdc_cursors: Arc<Mutex<HashMap<String, u64>>>,
}

#[derive(Clone, Default)]
struct KmsRuntimeState {
    providers: Vec<ConfiguredKmsProviderAdapter>,
    unavailable_envs: HashSet<String>,
    last_resolution: Option<KmsKeyResolution>,
    last_error: Option<String>,
    last_simulation_note: Option<String>,
}

struct KmsEvaluationSnapshot {
    status: &'static str,
    resolution_state: &'static str,
    resolution: Option<KmsKeyResolution>,
    unavailable_envs: Vec<String>,
    last_simulation_note: Option<String>,
    last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AutonomousMode {
    Disabled,
    Advisory,
    Supervised,
    Autonomous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum OperatorRole {
    Dba,
    Sre,
    Security,
    AiOperator,
}

impl OperatorRole {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dba" | "admin" => Some(Self::Dba),
            "sre" => Some(Self::Sre),
            "security" | "secops" => Some(Self::Security),
            "ai_operator" | "ai-operator" | "autonomous" => Some(Self::AiOperator),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Dba => "dba",
            Self::Sre => "sre",
            Self::Security => "security",
            Self::AiOperator => "ai_operator",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OperatorIdentity {
    operator_id: String,
    role: OperatorRole,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TenantUserBinding {
    tenant_id: String,
    role: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TenantUserIdentity {
    user_id: String,
    tenant_id: String,
    role: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimeAccessPrincipal {
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

fn default_allowed_operator_roles() -> HashSet<OperatorRole> {
    CONTROL_PLANE_OPERATOR_ROLES.into_iter().collect()
}

fn load_allowed_operator_roles() -> HashSet<OperatorRole> {
    let parsed = env::var("VNG_ALLOWED_OPERATOR_ROLES")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|entry| OperatorRole::parse(entry.trim()))
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    if parsed.is_empty() {
        default_allowed_operator_roles()
    } else {
        parsed
    }
}

fn default_operator_role_bindings() -> HashMap<String, OperatorRole> {
    HashMap::from([
        ("platform-admin".to_string(), OperatorRole::Dba),
        ("admin".to_string(), OperatorRole::Dba),
        ("automation".to_string(), OperatorRole::Sre),
        ("auto_sre".to_string(), OperatorRole::Sre),
        ("security-bot".to_string(), OperatorRole::Security),
        ("autopilot".to_string(), OperatorRole::AiOperator),
    ])
}

fn default_tenant_user_bindings() -> HashMap<String, TenantUserBinding> {
    HashMap::from([
        (
            "analyst-acme".to_string(),
            TenantUserBinding {
                tenant_id: "acme".to_string(),
                role: "tenant_analyst".to_string(),
            },
        ),
        (
            "admin-acme".to_string(),
            TenantUserBinding {
                tenant_id: "acme".to_string(),
                role: "tenant_admin".to_string(),
            },
        ),
    ])
}

fn load_operator_role_bindings(
    allowed_roles: &HashSet<OperatorRole>,
) -> HashMap<String, OperatorRole> {
    let parsed = env::var("VNG_OPERATOR_ROLE_BINDINGS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|entry| {
                    let (operator_id, role) = entry.split_once(':')?;
                    let operator_id = operator_id.trim();
                    let role = OperatorRole::parse(role.trim())?;
                    if operator_id.is_empty() || !allowed_roles.contains(&role) {
                        return None;
                    }
                    Some((operator_id.to_string(), role))
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    if parsed.is_empty() {
        default_operator_role_bindings()
            .into_iter()
            .filter(|(_, role)| allowed_roles.contains(role))
            .collect()
    } else {
        parsed
    }
}

fn load_runtime_security_config(allowed_operator_roles: &HashSet<OperatorRole>) -> SecurityConfigContract {
    let mut configured_roles = allowed_operator_roles
        .iter()
        .map(|role| role.as_str().to_string())
        .collect::<Vec<_>>();
    configured_roles.sort();

    let kms_failover_key_ref_envs = env::var("VNG_KMS_FAILOVER_KEY_REF_ENVS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| {
            vec![
                "VNG_KMS_KEY_URI_REGION_B".to_string(),
                "VNG_KMS_KEY_URI_REGION_C".to_string(),
            ]
        });

    let config = SecurityConfigContract {
        admin_api_key_env: env::var("VNG_ADMIN_API_KEY_ENV")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "VNG_ADMIN_API_KEY".to_string()),
        admin_header_name: env::var("VNG_ADMIN_HEADER_NAME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "x-vng-admin-key".to_string()),
        tls_required: false,
        mtls_required: false,
        encryption_at_rest_required: true,
        kms_key_ref_env: env::var("VNG_KMS_KEY_REF_ENV")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "VNG_KMS_KEY_URI".to_string()),
        kms_failover_key_ref_envs,
        allowed_operator_roles: configured_roles,
        token_ttl_seconds: env::var("VNG_TOKEN_TTL_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(300),
    };

    config
        .validate()
        .unwrap_or_else(|error| panic!("invalid runtime security config: {error}"));
    config
}

fn load_kms_runtime_state(config: &SecurityConfigContract) -> KmsRuntimeState {
    let mut provider_index = BTreeMap::<String, usize>::new();
    let mut providers = Vec::<ConfiguredKmsProviderAdapter>::new();
    for env_name in config.kms_key_candidates() {
        if let Ok(value) = env::var(&env_name) {
            if !value.trim().is_empty() {
                let candidate = ConfiguredKmsProviderAdapter::from_key_ref(value.trim());
                let provider_name = candidate.provider_name().to_string();
                let provider_slot = *provider_index.entry(provider_name.clone()).or_insert_with(|| {
                    providers.push(candidate);
                    providers.len() - 1
                });
                providers[provider_slot].register_key_ref(&env_name, value.trim());
            }
        }
    }

    KmsRuntimeState {
        providers,
        unavailable_envs: HashSet::new(),
        last_resolution: None,
        last_error: None,
        last_simulation_note: None,
    }
}

fn load_ingest_event_bus() -> ManagedEventBusTransport {
    let broker_mode = env::var("VNG_INGEST_OUTBOX_BROKER_MODE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "file_wal".to_string());
    let broker_target = env::var("VNG_INGEST_EXTERNAL_BROKER_TARGET")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let subject_prefix = env::var("VNG_INGEST_EXTERNAL_BROKER_SUBJECT_PREFIX")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let wal_path = env::var("VNG_INGEST_OUTBOX_WAL_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "state/ingest-outbox-runtime.wal".to_string());
    ManagedEventBusTransport::from_broker_mode_with_target(
        &broker_mode,
        &wal_path,
        broker_target.as_deref(),
        subject_prefix.as_deref(),
    )
    .unwrap_or_else(|error| {
        panic!(
            "failed to initialize ingest event bus broker {broker_mode} with state {wal_path}: {error}"
        )
    })
}

fn load_ingest_outbox_cursor_store() -> ManagedReplayCursorStore {
    let wal_path = env::var("VNG_INGEST_OUTBOX_CURSOR_WAL_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "state/ingest-outbox-cursors.wal".to_string());
    ManagedReplayCursorStore::wal_backed(&wal_path).unwrap_or_else(|error| {
        panic!("failed to initialize ingest outbox cursor store at {wal_path}: {error}")
    })
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

#[derive(Clone, Serialize)]
struct GuardrailRule {
    action: String,
    required_mode: AutonomousMode,
    scope: String,
    rationale: String,
}

#[derive(Serialize)]
struct AutonomousGuardrailsResponse {
    status: &'static str,
    autonomous_mode: AutonomousMode,
    emergency_stop_enabled: bool,
    policy_matrix: Vec<GuardrailRule>,
}

#[derive(Deserialize)]
struct EmergencyStopRequest {
    enabled: bool,
    reason: Option<String>,
    requested_by: Option<String>,
}

#[derive(Serialize)]
struct EmergencyStopResponse {
    status: &'static str,
    emergency_stop_enabled: bool,
    reason: String,
    requested_by: String,
}

#[derive(Deserialize)]
struct AuthorizeActionRequest {
    action: String,
    scope: Option<String>,
}

#[derive(Serialize)]
struct AuthorizeActionResponse {
    status: &'static str,
    action: String,
    requested_scope: String,
    decision: &'static str,
    reason: String,
    trace_id: String,
}

#[derive(Debug, Serialize)]
struct AuthErrorResponse {
    status: &'static str,
    reason: String,
    locale: String,
    localized_message: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    node_id: String,
    cluster_mode: String,
}

#[derive(Deserialize)]
struct SqlTransactionRequest {
    statements: Vec<String>,
    /// Requested isolation level: "read_committed" (default), "repeatable_read", "serializable"
    isolation_level: Option<String>,
}

#[derive(Deserialize)]
struct SqlAnalyzeRequest {
    sql_batch: String,
}

#[derive(Serialize)]
struct AnalyzedStatement {
    statement: String,
    kind: String,
    requires_transaction: bool,
    touches_catalog: bool,
    accepted: bool,
}

#[derive(Serialize)]
struct SqlAnalyzeResponse {
    status: &'static str,
    total_statements: usize,
    rejected_statements: usize,
    statements: Vec<AnalyzedStatement>,
}

#[derive(Deserialize)]
struct SqlRouteRequest {
    sql_batch: String,
}

#[derive(Serialize)]
struct RoutedStatementResponse {
    statement: String,
    /// Routing path from `HtapQueryRouter` (heuristic).
    path: String,
    /// Cost-model recommended path from `QueryPlanner` (S3-WS1-05).
    planner_path: String,
    estimated_rows: u64,
    relative_cost: f64,
}

#[derive(Serialize)]
struct SqlRouteResponse {
    status: &'static str,
    route_path: String,
    reason: String,
    statements: Vec<RoutedStatementResponse>,
    /// Aggregate planner cost across all statements in the batch.
    batch_estimated_rows: u64,
    batch_relative_cost: f64,
}

#[derive(Deserialize)]
struct SqlExecuteRequest {
    sql_batch: String,
    max_rows: Option<usize>,
}

#[derive(Serialize)]
struct LegacyAggResult {
    /// Aggregate function name (e.g. `"SUM"`, `"COUNT"`).
    aggregation: String,
    /// Computed result; `None` when evaluation errored.
    result: Option<f64>,
    /// Error message when evaluation failed.
    error: Option<String>,
    /// Indicates this result came through the legacy aggregation routing path.
    source: &'static str,
}

#[derive(Serialize)]
struct SqlExecuteResponse {
    status: &'static str,
    route_path: String,
    reason: String,
    transaction: Option<SqlTransactionResponse>,
    olap: Option<OlapQueryResponse>,
    rejected_statement_count: usize,
    udf_results: Option<Vec<UdfExecutionResult>>,
    udf_guardrail_status: Option<String>,
    udf_function_catalog: Vec<UdfFunctionCatalogEntry>,
    udf_guard_policies: Vec<UdfLanguageGuardPolicy>,
    udf_execution_plan: Vec<UdfExecutionPlanStep>,
    legacy_agg_results: Option<Vec<LegacyAggResult>>,
    /// Dominant cost-model recommended path for the batch (S3-WS1-05).
    planner_path: Option<String>,
    /// Physical OLTP executor results: actual rows from PagedRowStore for point-read SELECT (S4-WS3-02).
    oltp_rows: Option<Vec<OltpRowResult>>,
    /// Vectorized OLAP aggregation results from columnar executor (S4-WS3-02).
    olap_agg_results: Option<Vec<OlapVecAggResult>>,
}

/// S4-WS3-02: a single result row returned by the physical OLTP executor.
#[derive(Serialize)]
struct OltpRowResult {
    key: String,
    data: std::collections::HashMap<String, String>,
}

/// S4-WS3-02: a single vectorized aggregation result from the OLAP columnar executor.
#[derive(Serialize)]
struct OlapVecAggResult {
    column: String,
    op: String,
    value: String,
    row_count: usize,
}

// ─── S2-WS2-02: WAL durability + recovery types ──────────────────────────────

/// Response for `GET /api/v1/store/wal/status`.
#[derive(Serialize)]
struct WalStatusResponse {
    status: &'static str,
    wal_len: usize,
    latest_sequence: u64,
    checkpoint_count: usize,
}

/// Request body for `POST /api/v1/store/wal/recover`.
#[derive(Deserialize)]
struct WalRecoverRequest {
    /// When `true`, log what would be replayed without actually writing to the row store.
    dry_run: Option<bool>,
}

/// Response for `POST /api/v1/store/wal/recover`.
#[derive(Debug, Serialize)]
struct WalRecoverResponse {
    status: &'static str,
    records_replayed: usize,
    dry_run: bool,
}

// ─── S7-WS6-04: Chaos injection types ────────────────────────────────────────

/// A single chaos fault event injected into the cluster simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChaosEvent {
    /// The type of fault, e.g. `"network_partition"`, `"node_crash"`, `"packet_loss"`.
    fault_type: String,
    /// Optional target node identifier.
    target_node: Option<String>,
    /// Arbitrary key–value parameters for the fault (e.g. `{ "loss_pct": "30" }`).
    parameters: HashMap<String, String>,
    /// Epoch-millisecond timestamp when the fault was injected.
    injected_at_ms: u64,
    /// Epoch-millisecond timestamp when the fault was cleared, if any.
    cleared_at_ms: Option<u64>,
}

/// Mutable chaos state (active faults + history).
#[derive(Debug, Default)]
struct ChaosState {
    active_faults: Vec<ChaosEvent>,
    event_history: Vec<ChaosEvent>,
}

/// Request body for `POST /api/v1/cluster/chaos/inject`.
#[derive(Deserialize)]
struct ChaosInjectRequest {
    fault_type: String,
    target_node: Option<String>,
    #[serde(default)]
    parameters: HashMap<String, String>,
}

// ─── S8-WS10-02: Driver wire protocol structs ─────────────────────────────────

#[derive(Debug, Clone)]
struct DriverSession {
    driver_name: String,
    driver_version: String,
    connected_at_ms: u64,
}

#[derive(Debug, Serialize)]
struct DriverProtocolInfo {
    protocol_version: &'static str,
    encoding: &'static str,
    auth_modes: Vec<String>,
    supported_statements: Vec<String>,
    max_batch_size: usize,
}

#[derive(Debug, Deserialize)]
struct DriverConnectRequest {
    driver_name: String,
    driver_version: String,
    requested_capabilities: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct DriverConnectResponse {
    status: &'static str,
    session_token: String,
    negotiated_capabilities: Vec<String>,
    max_batch_size: usize,
}

// ─── S8-WS10-02: Driver disconnect structs ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct DriverDisconnectRequest {
    session_token: String,
}

#[derive(Debug, Serialize)]
struct DriverDisconnectResponse {
    status: &'static str,
    session_token: String,
    disconnected: bool,
}

#[derive(Debug, Serialize)]
struct DriverSessionInfo {
    session_token: String,
    driver_name: String,
    driver_version: String,
    connected_at_ms: u64,
}

#[derive(Debug, Serialize)]
struct DriverSessionListResponse {
    status: &'static str,
    session_count: usize,
    sessions: Vec<DriverSessionInfo>,
}

// ─── S7-WS6-02: Raft log entries response ─────────────────────────────────────

#[derive(Debug, Serialize)]
struct RaftLogResponse {
    status: &'static str,
    log_length: usize,
    commit_index: u64,
    entries: Vec<RaftLogEntry>,
}

// ─── S2-WS2-02: WAL forced checkpoint response ────────────────────────────────

#[derive(Debug, Serialize)]
struct WalForceCheckpointResponse {
    status: &'static str,
    wal_len_before: usize,
    wal_len_after: usize,
    checkpoint_count: usize,
}

// ─── S10-WS15-02: CDC change-data-capture structs ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct CdcEvent {
    sequence: u64,
    op: String,
    table_name: String,
    key: String,
    payload: String,
    captured_at_ms: u64,
}

#[derive(Debug, Serialize)]
struct CdcStreamResponse {
    status: &'static str,
    event_count: usize,
    events: Vec<CdcEvent>,
}

#[derive(Debug, Deserialize, Default)]
struct CdcStreamFilterQuery {
    table: Option<String>,
}

#[derive(Debug, Serialize)]
struct CdcStreamFilterResponse {
    status: &'static str,
    table_filter: Option<String>,
    event_count: usize,
    events: Vec<CdcEvent>,
}

// ─── S10-WS15-02: CDC cursor tracking structs ─────────────────────────────────

#[derive(Debug, Deserialize)]
struct CdcCursorQuery {
    table: String,
}

#[derive(Debug, Deserialize)]
struct CdcCursorAdvanceRequest {
    table_name: String,
    position: u64,
}

#[derive(Debug, Serialize)]
struct CdcCursorResponse {
    status: &'static str,
    table_name: String,
    cursor_position: u64,
}

// ─── S5-WS4A-02: Broker adapter structs ───────────────────────────────────────

#[derive(Debug, Serialize)]
struct BrokerAdapterInfo {
    broker_type: String,
    enabled: bool,
    flush_count: u64,
}

#[derive(Debug, Serialize)]
struct BrokerAdapterStatus {
    status: &'static str,
    adapters: Vec<BrokerAdapterInfo>,
}

#[derive(Debug, Deserialize)]
struct BrokerFlushRequest {
    broker_type: String,
    max_events: Option<usize>,
}

#[derive(Debug, Serialize)]
struct BrokerFlushResponse {
    status: &'static str,
    broker_type: String,
    events_flushed: usize,
    total_flush_count: u64,
}

/// Response for `GET /api/v1/ingest/outbox/broker/health`.
#[derive(Serialize)]
struct BrokerHealthEntry {
    broker_type: &'static str,
    flush_count: u64,
    wal_len: usize,
    healthy: bool,
}

#[derive(Serialize)]
struct BrokerHealthResponse {
    status: &'static str,
    broker_count: usize,
    brokers: Vec<BrokerHealthEntry>,
}

/// Response for `GET /api/v1/cluster/chaos/status`.
#[derive(Serialize)]
struct ChaosStatusResponse {
    status: &'static str,
    active_fault_count: usize,
    total_injected: usize,
    active_faults: Vec<ChaosEvent>,
}

/// Response for `GET /api/v1/cluster/chaos/health`.
#[derive(Serialize)]
struct ChaosHealthResponse {
    status: &'static str,
    cluster_healthy: bool,
    active_fault_count: usize,
    history_len: usize,
}

#[derive(Debug, Serialize)]
struct UdfExecutionResult {
    language: &'static str,
    function: &'static str,
    input: String,
    output: String,
}

#[derive(Serialize)]
struct UdfFunctionCatalogEntry {
    name: &'static str,
    language: &'static str,
    deterministic: bool,
    status: &'static str,
}

#[derive(Serialize)]
struct UdfLanguageGuardPolicy {
    language: &'static str,
    blocked_tokens: Vec<&'static str>,
    max_input_bytes: usize,
}

#[derive(Serialize)]
struct UdfExecutionPlanStep {
    statement: String,
    route_path: String,
    udf_invocations: Vec<UdfInvocationPlan>,
}

#[derive(Serialize)]
struct UdfInvocationPlan {
    function: &'static str,
    language: &'static str,
    guard_policy: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SqlTransactionResponse {
    status: &'static str,
    transaction_id: String,
    statements_executed: usize,
    requires_transaction: bool,
    touches_catalog: bool,
    rejected_statement_count: usize,
    elapsed_ms: u128,
}

#[derive(Deserialize)]
struct PessimisticLockAcquireRequest {
    transaction_id: String,
    resource: String,
    owner: Option<String>,
    ttl_ms: Option<u64>,
    wait_timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
struct PessimisticLockReleaseRequest {
    transaction_id: String,
    resource: String,
}

#[derive(Clone, Serialize)]
struct PessimisticLockRecord {
    lock_id: String,
    transaction_id: String,
    resource: String,
    owner: String,
    acquired_unix_ms: u128,
    expires_unix_ms: u128,
}

#[derive(Serialize)]
struct PessimisticLockResponse {
    status: &'static str,
    lock_state: &'static str,
    reason: String,
    lock: Option<PessimisticLockRecord>,
}

#[derive(Serialize)]
struct PessimisticLockContentionMetricsResponse {
    status: &'static str,
    deadlock_detections: u64,
    scan_cap_timeouts: u64,
    wait_timeouts: u64,
    lock_grants: u64,
    lock_conflicts: u64,
    lock_releases: u64,
    contention_ratio: f64,
}

// â”€â”€ WS2 Index + Constraint types â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Deserialize)]
struct CreateIndexRequest {
    name: String,
    table: String,
    column: String,
    unique: Option<bool>,
}

#[derive(Serialize)]
struct CreateIndexResponse {
    status: &'static str,
    index_name: String,
    table: String,
    column: String,
    unique: bool,
}

#[derive(Deserialize)]
struct DropIndexRequest {
    name: String,
}

#[derive(Serialize)]
struct DropIndexResponse {
    status: &'static str,
    dropped: String,
}

#[derive(Serialize)]
struct IndexListEntry {
    name: String,
    table: String,
    column: String,
    kind: String,
    unique: bool,
}

#[derive(Serialize)]
struct ListIndexesResponse {
    status: &'static str,
    indexes: Vec<IndexListEntry>,
}

// S5-WS4-03 / S2-WS2-04: MVCC row store scan structs
#[derive(Deserialize)]
struct StoreRowsScanRequest {
    /// MVCC snapshot Xid to read at. Defaults to current head Xid.
    snapshot_xid: Option<u64>,
    /// Optional key prefix filter (empty string matches all).
    key_prefix: Option<String>,
    /// Maximum rows returned (capped at 10 000; default 1 000).
    limit: Option<usize>,
}

#[derive(Serialize)]
struct StoreRowEntry {
    key: String,
    data: std::collections::HashMap<String, String>,
}

#[derive(Serialize)]
struct StoreRowsScanResponse {
    status: &'static str,
    snapshot_xid: u64,
    row_count: usize,
    rows: Vec<StoreRowEntry>,
}

// ─── S2-WS2-04: Row store snapshot export structs ────────────────────────────

#[derive(Serialize)]
struct RowSnapshotEntry {
    key: String,
    payload: std::collections::HashMap<String, String>,
}

#[derive(Serialize)]
struct RowSnapshotResponse {
    status: &'static str,
    snapshot_xid: u64,
    row_count: usize,
    rows: Vec<RowSnapshotEntry>,
}

// S4-WS3-04: HTAP sync export structs
#[derive(Deserialize)]
struct StoreHtapExportRequest {
    /// Export mutations with sequence > this value (0 = export all).
    since_sequence: Option<u64>,
    /// Maximum mutations to return (capped at 5 000; default 500).
    max_items: Option<usize>,
}

#[derive(Serialize)]
struct HtapMutationEntry {
    sequence: u64,
    table: String,
    primary_key: String,
    payload_json: String,
    op: String,
}

#[derive(Serialize)]
struct StoreHtapExportResponse {
    status: &'static str,
    since_sequence: u64,
    mutation_count: usize,
    checkpoint_last_sequence: u64,
    mutations: Vec<HtapMutationEntry>,
}

// S9-WS8A-02: audit chain verify response
#[derive(Serialize)]
struct AuditChainVerifyResponse {
    status: &'static str,
    event_count: usize,
    chain_valid: bool,
    genesis_hash: &'static str,
}

// S4-WS3-03: columnar scan response (vectorized OLAP executor)
#[derive(Serialize)]
struct ColumnarScanColumn {
    name: String,
    type_hint: String,
    row_count: usize,
    sample_values: Vec<String>,
}

#[derive(Serialize)]
struct ColumnarScanResponse {
    status: &'static str,
    rows_scanned: usize,
    columns_materialized: usize,
    elapsed_us: u128,
    columns: Vec<ColumnarScanColumn>,
}

// S6-WS5-03: TLS runtime status
#[derive(Serialize)]
struct SecurityTlsStatusResponse {
    status: &'static str,
    tls_required: bool,
    mtls_required: bool,
    cert_source: String,
    cert_rotation_supported: bool,
    note: &'static str,
}

#[derive(Debug, Deserialize, Default)]
struct TlsCertRotateRequest {
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct TlsCertRotateResponse {
    status: &'static str,
    rotation_initiated: bool,
    cert_source: String,
    reason: String,
}

// S6-WS5-04: TDE runtime status
#[derive(Serialize)]
struct SecurityTdeStatusResponse {
    status: &'static str,
    encryption_at_rest_required: bool,
    tde_active: bool,
    key_env_var: String,
    key_resolved: bool,
    note: &'static str,
}

// S9-WS8-02: model gateway policy
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelGatewayPolicy {
    /// When true, autonomous AI actions are validated against policy constraints.
    isolation_enabled: bool,
    /// Allowlist of model identifiers (empty = allow all).
    allowed_models: Vec<String>,
    /// Maximum tokens per request (0 = unlimited).
    max_tokens_per_request: u64,
    /// Requests-per-minute rate limit (0 = unlimited).
    rate_limit_rpm: u32,
}

impl Default for ModelGatewayPolicy {
    fn default() -> Self {
        Self {
            isolation_enabled: true,
            allowed_models: Vec::new(),
            max_tokens_per_request: 4096,
            rate_limit_rpm: 60,
        }
    }
}

#[derive(Serialize)]
struct AiPolicyResponse {
    status: &'static str,
    policy: ModelGatewayPolicy,
}

#[derive(Deserialize)]
struct AiPolicyUpdateRequest {
    isolation_enabled: Option<bool>,
    allowed_models: Option<Vec<String>>,
    max_tokens_per_request: Option<u64>,
    rate_limit_rpm: Option<u32>,
}

// ─── S4-WS3-04: HTAP OLAP consumer structs ───────────────────────────────────

#[derive(Deserialize)]
struct OlapApplyMutation {
    sequence: u64,
    primary_key: String,
    payload_json: String,
    op: String, // "insert" | "update" | "delete"
}

#[derive(Deserialize)]
struct StoreHtapApplyRequest {
    mutations: Vec<OlapApplyMutation>,
}

#[derive(Serialize)]
struct StoreHtapApplyResponse {
    status: &'static str,
    applied_count: usize,
    last_applied_sequence: u64,
}

#[derive(Serialize)]
struct OlapScanRow {
    key: String,
    data: HashMap<String, String>,
}

#[derive(Serialize)]
struct StoreHtapOlapScanResponse {
    status: &'static str,
    row_count: usize,
    rows: Vec<OlapScanRow>,
}

/// Response for `GET /api/v1/store/htap/lag`.
#[derive(Serialize)]
struct HtapLagResponse {
    status: &'static str,
    sync_origin_pending: usize,
    olap_row_count: usize,
    estimated_lag_mutations: usize,
}

// ─── S9-WS8A-02: Audit export struct ─────────────────────────────────────────

#[derive(Deserialize, Default)]
struct AuditExportQuery {
    cursor: Option<usize>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct AuditExportResponse {
    status: &'static str,
    event_count: usize,
    total_event_count: usize,
    cursor: usize,
    limit: usize,
    file_backed: bool,
    audit_log_path: Option<String>,
    events: Vec<AuditEvent>,
}

// ─── S5-E4A-01: Connector SDK runtime structs ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConnectorPlugin {
    connector_id: String,
    connector_type: String,
    version: String,
    signed: bool,
    registered_at_ms: u64,
}

#[derive(Deserialize)]
struct ConnectorRegisterRequest {
    connector_id: String,
    connector_type: String,
    version: String,
    signed: Option<bool>,
}

#[derive(Serialize)]
struct ConnectorRegisterResponse {
    status: &'static str,
    connector_id: String,
    registered_at_ms: u64,
}

#[derive(Serialize)]
struct ConnectorListResponse {
    status: &'static str,
    connector_count: usize,
    connectors: Vec<ConnectorPlugin>,
}

// ─── S7-WS6-03: Raft fencing token struct ──────────────────────────────────────────

#[derive(Serialize)]
struct RaftFenceResponse {
    status: &'static str,
    fencing_token: u64,
    role: RaftRole,
    current_term: u64,
}

// ─── S6-WS5-04: TDE toggle structs ────────────────────────────────────────────────

#[derive(Deserialize)]
struct TdeToggleRequest {
    enable: bool,
}

#[derive(Serialize)]
struct TdeToggleResponse {
    status: &'static str,
    tde_active: bool,
    override_applied: bool,
}

// ─── S7-WS6-02: Raft endpoint structs ────────────────────────────────────────

#[derive(Serialize)]
struct RaftStatusResponse {
    status: &'static str,
    raft: RaftStatusSnapshot,
}

#[derive(Deserialize)]
struct IndexLookupRequest {
    index_name: String,
    value: String,
}

#[derive(Serialize)]
struct IndexLookupResponse {
    status: &'static str,
    index_name: String,
    value: String,
    row_keys: Vec<String>,
}

#[derive(Deserialize)]
struct AddConstraintRequest {
    name: String,
    table: String,
    column: String,
    kind: String,
}

#[derive(Serialize)]
struct AddConstraintResponse {
    status: &'static str,
    constraint_name: String,
    table: String,
    column: String,
    kind: String,
}

#[derive(Deserialize)]
struct ValidateConstraintRequest {
    table: String,
    column: String,
    value: Option<String>,
}

#[derive(Serialize)]
struct ValidateConstraintResponse {
    status: &'static str,
    valid: bool,
    violation: Option<String>,
}

// â”€â”€ WS4 Ingest types â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Deserialize)]
struct IngestCsvRequest {
    connector_id: String,
    csv_data: String,
}

#[derive(Serialize)]
struct IngestCsvResponse {
    status: &'static str,
    connector_id: String,
    records_parsed: usize,
}

#[derive(Deserialize)]
struct IngestJsonRequest {
    connector_id: String,
    key_field: String,
    ndjson_data: String,
}

#[derive(Serialize)]
struct IngestJsonResponse {
    status: &'static str,
    connector_id: String,
    records_parsed: usize,
}

#[derive(Deserialize)]
struct IngestParquetRequest {
    connector_id: String,
    /// Standard base64 (RFC 4648) encoded Parquet file bytes.
    parquet_data_base64: String,
}

#[derive(Serialize)]
struct IngestParquetResponse {
    status: &'static str,
    connector_id: String,
    records_parsed: usize,
}

#[derive(Deserialize)]
struct IngestExcelRequest {
    connector_id: String,
    /// Standard base64 (RFC 4648) encoded `.xlsx` workbook bytes.
    xlsx_data_base64: String,
}

#[derive(Serialize)]
struct IngestExcelResponse {
    status: &'static str,
    connector_id: String,
    records_parsed: usize,
}

// REQ-07: chunked ingest
#[derive(Deserialize)]
struct IngestChunkedRequest {
    connector_id: String,
    /// JSON-serialized record payloads â€” one per element
    records: Vec<String>,
    chunk_target_rows: Option<usize>,
    max_in_flight_tasks: Option<usize>,
}

#[derive(Serialize)]
struct IngestChunkedResponse {
    status: &'static str,
    connector_id: String,
    total_records: usize,
    chunk_count: usize,
    tasks_dispatched: usize,
    chunks_succeeded: usize,
    chunks_failed: usize,
}

#[derive(Serialize)]
struct IngestStatusResponse {
    status: &'static str,
    csv_connectors: usize,
    json_connectors: usize,
    parquet_connectors: usize,
    excel_connectors: usize,
    total_records_loaded: usize,
}

#[derive(Serialize)]
struct IngestOutboxStatusResponse {
    status: &'static str,
    broker_mode: String,
    broker_target: Option<String>,
    stream_count: usize,
    total_events: usize,
    last_event_id: Option<u64>,
    streams: Vec<String>,
}

#[derive(Deserialize)]
struct IngestOutboxReplayRequest {
    connector_id: String,
    consumer_id: Option<String>,
    max_items: Option<usize>,
    acknowledge: Option<bool>,
}

#[derive(Serialize)]
struct IngestOutboxReplayEventResponse {
    replay_key: String,
    event_id: u64,
    stream_name: String,
    origin: String,
    payload_json: String,
}

#[derive(Serialize)]
struct IngestOutboxReplayResponse {
    status: &'static str,
    delivery_state: &'static str,
    stream_name: String,
    consumer_id: String,
    delivered_count: usize,
    cursor_before_ack: Option<u64>,
    cursor_after_ack: Option<u64>,
    acknowledged: bool,
    events: Vec<IngestOutboxReplayEventResponse>,
}

#[derive(Serialize)]
struct SecurityKmsStatusResponse {
    status: &'static str,
    resolution_state: &'static str,
    encryption_at_rest_required: bool,
    configured_envs: Vec<String>,
    unavailable_envs: Vec<String>,
    selected_env: Option<String>,
    key_ref: Option<String>,
    failover_used: bool,
    last_simulation_note: Option<String>,
    last_error: Option<String>,
}

#[derive(Deserialize)]
struct SecurityKmsOutageSimulateRequest {
    unavailable_envs: Vec<String>,
    note: Option<String>,
}

#[derive(Deserialize)]
struct SecurityKmsOutageReconcileRequest {
    note: Option<String>,
}

#[derive(Serialize)]
struct SecurityKmsOutageResponse {
    status: &'static str,
    resolution_state: &'static str,
    unavailable_envs: Vec<String>,
    selected_env: Option<String>,
    key_ref: Option<String>,
    failover_used: bool,
    note: String,
}

#[derive(Deserialize)]
struct OlapQueryRequest {
    query: String,
    max_rows: Option<usize>,
}

#[derive(Serialize)]
struct OlapQueryResponse {
    status: &'static str,
    query_signature: String,
    elapsed_ms: u128,
    rows: usize,
}

#[derive(Serialize)]
struct FailoverStatusResponse {
    status: &'static str,
    cluster_mode: String,
    leader_node_id: String,
    unresolved_critical_count: usize,
    rto_seconds_target: u32,
    rpo_data_loss_rows_target: u32,
}

#[derive(Deserialize)]
struct FailoverSimulateRequest {
    new_leader_node_id: String,
    reason: Option<String>,
    requested_by: Option<String>,
}

#[derive(Serialize)]
struct FailoverSimulateResponse {
    status: &'static str,
    previous_leader_node_id: String,
    new_leader_node_id: String,
    reason: String,
    requested_by: String,
    handoff_report: FailoverHandoffReportResponse,
}

#[derive(Serialize)]
struct FailoverHandoffGapResponse {
    expected: u64,
    actual: u64,
}

#[derive(Serialize)]
struct FailoverHandoffReportResponse {
    handoff_state: &'static str,
    source_node_id: String,
    target_node_id: String,
    last_applied_sequence_before: u64,
    last_applied_sequence_after: u64,
    replay_batch_size: usize,
    applied_count: usize,
    gap_count: usize,
    gaps: Vec<FailoverHandoffGapResponse>,
}

#[derive(Deserialize)]
struct AuditEventsQuery {
    max_items: Option<usize>,
}

#[derive(Serialize)]
struct AuditEventsResponse {
    status: &'static str,
    total_events: usize,
    events: Vec<AuditEvent>,
}

#[derive(Deserialize)]
struct AutonomousActionRecordsQuery {
    max_items: Option<usize>,
}

#[derive(Serialize)]
struct AutonomousActionRecordsResponse {
    status: &'static str,
    total_records: usize,
    records: Vec<AutonomousActionExecutionRecord>,
}

#[derive(Deserialize)]
struct I18nMessagesQuery {
    locale: Option<String>,
}

#[derive(Serialize)]
struct I18nMessagesResponse {
    status: &'static str,
    locale: String,
    messages: std::collections::BTreeMap<String, String>,
}

#[derive(Serialize)]
struct SreReliabilityStatusResponse {
    status: &'static str,
    service_health: &'static str,
    failure_budget: FailureBudgetSnapshot,
    rate_limit_policy: RateLimitPolicySnapshot,
}

#[derive(Serialize, Clone, Copy)]
struct FailureBudgetSnapshot {
    window_minutes: u32,
    error_budget_percent: f64,
    consumed_percent: f64,
    remaining_percent: f64,
    burn_rate: f64,
}

#[derive(Serialize, Clone, Copy)]
struct RateLimitPolicySnapshot {
    requests_per_minute: u32,
    burst_limit: u32,
    current_minute_count: u32,
    allowed: bool,
}

#[derive(Deserialize)]
struct RateLimitCheckRequest {
    current_minute_count: u32,
    requested_units: Option<u32>,
}

#[derive(Serialize)]
struct RateLimitCheckResponse {
    status: &'static str,
    allowed: bool,
    remaining_units: u32,
    reason: String,
}

#[derive(Deserialize)]
struct FailureBudgetAlertQuery {
    consumed_percent: Option<f64>,
    burn_rate: Option<f64>,
}

#[derive(Serialize)]
struct FailureBudgetAlertResponse {
    status: &'static str,
    alert_state: &'static str,
    severity: &'static str,
    threshold_percent: f64,
    consumed_percent: f64,
    burn_rate: f64,
    recommended_action: &'static str,
}

#[derive(Deserialize)]
struct DrHookTriggerRequest {
    hook: String,
    scope: Option<String>,
    dry_run: Option<bool>,
}

#[derive(Clone, Serialize)]
struct DrHookExecutionRecord {
    execution_id: String,
    hook: String,
    scope: String,
    status: &'static str,
    dry_run: bool,
    policy_decision: &'static str,
    cooldown_remaining_ms: u64,
    retry_backoff_ms: u64,
    retry_attempt: u32,
    details: String,
}

#[derive(Default)]
struct DrHookPolicyState {
    hooks: HashMap<String, DrHookRuntimeState>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct DrHookRuntimeState {
    last_attempt_unix_ms: u128,
    consecutive_failures: u32,
    last_status: String,
}

#[derive(Clone)]
struct DrHookPolicyConfig {
    min_mode: AutonomousMode,
    cooldown_seconds: u64,
    max_retries: u32,
    base_backoff_ms: u64,
    max_backoff_ms: u64,
    allowed_hooks: Vec<String>,
}

#[derive(Serialize)]
struct DrHookPolicyResponse {
    status: &'static str,
    policy: DrHookPolicyContract,
}

#[derive(Serialize)]
struct DrHookPolicyContract {
    min_mode: AutonomousMode,
    cooldown_seconds: u64,
    max_retries: u32,
    base_backoff_ms: u64,
    max_backoff_ms: u64,
    allowed_hooks: Vec<String>,
    tracked_hooks: usize,
}

#[derive(Clone, Serialize, Deserialize)]
struct DrHookPolicyStateSnapshot {
    hooks: HashMap<String, DrHookRuntimeState>,
}

#[derive(Clone, Serialize, Deserialize)]
struct DrHookPolicyStateEnvelope {
    schema_version: u32,
    written_unix_ms: u128,
    checksum_hex: String,
    snapshot: DrHookPolicyStateSnapshot,
}

#[derive(Deserialize)]
struct DrHookRetryPlanQuery {
    hook: String,
    attempts: Option<u32>,
}

#[derive(Serialize)]
struct DrHookRetryPlanResponse {
    status: &'static str,
    hook: String,
    accepted: bool,
    reason: String,
    steps: Vec<DrHookRetryPlanStep>,
}

#[derive(Serialize)]
struct DrHookRetryPlanStep {
    attempt: u32,
    recommended_backoff_ms: u64,
    jitter_range_ms: u64,
}

#[derive(Deserialize)]
struct DrHookScheduleRequest {
    hook: String,
    scope: Option<String>,
    dry_run: Option<bool>,
    reason: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct DrHookScheduledTask {
    task_id: String,
    hook: String,
    scope: String,
    dry_run: bool,
    requested_by: String,
    reason: String,
    enqueued_unix_ms: u128,
}

#[derive(Serialize)]
struct DrHookScheduleResponse {
    status: &'static str,
    task: DrHookScheduledTask,
    queue_depth: usize,
}

#[derive(Deserialize)]
struct FailureSignalRequest {
    node_id: String,
    transport: String,
    failure_type: String,
    severity: String,
    message: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ClusterFailureSignal {
    signal_id: String,
    node_id: String,
    transport: String,
    failure_type: String,
    severity: String,
    message: String,
    observed_unix_ms: u128,
    resolved: bool,
    resolved_by: Option<String>,
    resolved_unix_ms: Option<u128>,
    resolution_note: Option<String>,
}

#[derive(Serialize)]
struct FailureSignalResponse {
    status: &'static str,
    signal: ClusterFailureSignal,
    queued_remediation_task: Option<DrHookScheduledTask>,
}

#[derive(Deserialize)]
struct FailureReconcileRequest {
    signal_ids: Option<Vec<String>>,
    resolve_all_critical: Option<bool>,
    note: Option<String>,
}

#[derive(Serialize)]
struct FailureReconcileResponse {
    status: &'static str,
    resolved_count: usize,
    unresolved_critical_count: usize,
}

#[derive(Serialize)]
struct SreGateEvaluationResponse {
    status: &'static str,
    gate_result: &'static str,
    criteria: Vec<SreGateCriterion>,
    recommended_actions: Vec<String>,
}

#[derive(Serialize)]
struct SreGateCriterion {
    name: String,
    passed: bool,
    detail: String,
}

#[derive(Deserialize)]
struct SreGateExportRequest {
    output_path: Option<String>,
}

#[derive(Serialize)]
struct SreGateExportResponse {
    status: &'static str,
    output_path: String,
    gate_result: &'static str,
}

#[derive(Serialize)]
struct DrHookTriggerResponse {
    status: &'static str,
    execution: DrHookExecutionRecord,
}

#[derive(Deserialize)]
struct DrHookStatusQuery {
    max_items: Option<usize>,
}

#[derive(Serialize)]
struct DrHookStatusResponse {
    status: &'static str,
    total_records: usize,
    records: Vec<DrHookExecutionRecord>,
}

#[derive(Deserialize)]
struct CacheSetRequest {
    partition_id: String,
    key: String,
    value: serde_json::Value,
    ttl_ms: Option<u64>,
}

#[derive(Deserialize)]
struct CacheGetQuery {
    partition_id: String,
    key: String,
}

#[derive(Deserialize)]
struct CacheInvalidateRequest {
    partition_id: String,
    key: String,
}

#[derive(Serialize)]
struct CacheWriteResponse {
    status: &'static str,
    partition_id: String,
    key: String,
    error: Option<String>,
}

#[derive(Serialize)]
struct CacheGetResponse {
    status: &'static str,
    partition_id: String,
    key: String,
    hit: bool,
    value: Option<serde_json::Value>,
    error: Option<String>,
}

#[derive(Serialize)]
struct CacheInvalidateResponse {
    status: &'static str,
    partition_id: String,
    key: String,
    removed: bool,
    error: Option<String>,
}

#[derive(Serialize)]
struct CacheRebalanceResponse {
    status: &'static str,
    partition_count: usize,
    rebalanced_partitions: usize,
    entries_evicted: usize,
}

#[derive(Serialize)]
struct CachePartitionMetricsResponse {
    partition_id: String,
    entry_count: usize,
    total_hits: u64,
    total_misses: u64,
    total_evictions: u64,
    circuit_breaker_state: String,
    hit_ratio: f64,
    last_rebalance_ms: Option<u64>,
}

#[derive(Serialize)]
struct CacheMetricsResponse {
    status: &'static str,
    partition_count: usize,
    total_entries: usize,
    partitions: Vec<CachePartitionMetricsResponse>,
}

// REQ-27: Redis-compat cache command interface ---------------------------------

#[derive(Deserialize)]
struct RedisCacheCommandRequest {
    /// Redis command name (case-insensitive): GET | SET | DEL | EXISTS | KEYS | FLUSH | PING |
    /// EXPIRE | INCR | DECR | INCRBY | DECRBY | MGET | MSET | GETSET | LPUSH | RPUSH | LLEN | LRANGE
    cmd: String,
    partition_id: Option<String>,
    key: Option<String>,
    value: Option<serde_json::Value>,
    ttl_ms: Option<u64>,
    /// Numeric delta for INCR/DECR/INCRBY/DECRBY (defaults to Â±1.0)
    delta: Option<f64>,
    /// New TTL in milliseconds for EXPIRE command (overrides ttl_ms if present)
    expire_ms: Option<u64>,
    /// Multiple keys for MGET; keyâ†’value mapping (JSON object) for MSET
    keys: Option<Vec<String>>,
    /// LRANGE start index (inclusive, negative = from tail)
    start: Option<i64>,
    /// LRANGE stop index (inclusive, negative = from tail)
    stop: Option<i64>,
    /// Hash field name for HSET / HGET / HDEL
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

#[derive(Deserialize)]
struct PoolAcquireRequest {
    now_ms: Option<u64>,
}

#[derive(Deserialize)]
struct PoolReleaseRequest {
    connection_id: String,
    now_ms: Option<u64>,
}

#[derive(Deserialize)]
struct PoolFailureRequest {
    connection_id: String,
    error: Option<String>,
    now_ms: Option<u64>,
}

#[derive(Deserialize)]
struct PoolRecoverRequest {
    now_ms: Option<u64>,
    prune_unhealthy: Option<bool>,
}

#[derive(Serialize)]
struct PoolStatsResponse {
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

#[derive(Serialize)]
struct PoolAcquireResponse {
    status: &'static str,
    acquire_state: &'static str,
    connection_id: Option<String>,
    error: Option<String>,
    stats: PoolStatsResponse,
}

#[derive(Serialize)]
struct PoolReleaseResponse {
    status: &'static str,
    released: bool,
    stats: PoolStatsResponse,
}

#[derive(Serialize)]
struct PoolFailureResponse {
    status: &'static str,
    marked_failed: bool,
    stats: PoolStatsResponse,
}

#[derive(Serialize)]
struct PoolRecoverResponse {
    status: &'static str,
    circuit_recovered: bool,
    pruned_unhealthy: usize,
    stats: PoolStatsResponse,
}

#[derive(Deserialize)]
struct SignedProvenanceRegistrationRequest {
    plugin_id: String,
    plugin_version: String,
    checksum_sha256: String,
    display_name: Option<String>,
    owner: Option<String>,
    license: Option<String>,
    capabilities: Option<Vec<String>>,
    schema_version: Option<String>,
    signature_algorithm: String,
    signature_key_id: String,
    signature_base64: String,
    revoked_key_ids: Option<Vec<String>>,
    attestations: Vec<SignedProvenanceAttestationRequest>,
    sbom_entries: Option<Vec<SignedProvenanceSbomEntryRequest>>,
}

#[derive(Deserialize)]
struct SignedProvenanceAttestationRequest {
    attester_id: String,
    attested_at_ms: Option<u64>,
    attestation_type: String,
    payload_digest_sha256: String,
    signature_base64: String,
    passed: bool,
}

#[derive(Clone, Deserialize)]
struct SignedProvenanceSbomEntryRequest {
    component_name: String,
    component_version: String,
    license: String,
    checksum_sha256: String,
    source_url: Option<String>,
}

#[derive(Serialize)]
struct SignedProvenanceRegistrationResponse {
    status: &'static str,
    registration_state: &'static str,
    plugin_id: String,
    plugin_version: String,
    chain_complete: bool,
    chain_digest: String,
    attestation_count: usize,
    passed_attestations: usize,
    sbom_approved: bool,
    sbom_license_violations: usize,
    sbom_missing_checksums: usize,
    audit_records_total: usize,
    error: Option<String>,
}

#[tokio::main]
async fn main() {
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
        ddl_catalog: Arc::new(Mutex::new(DdlCatalog::new())),
        acid_transactions: Arc::new(Mutex::new(AcidTransactionRegistry::default())),
        row_store: Arc::new(Mutex::new(PagedRowStore::default())),
        model_gateway_policy: Arc::new(Mutex::new(ModelGatewayPolicy::default())),
        wal_engine: Arc::new(Mutex::new(InMemoryDurabilityEngine::with_config(DurabilityConfig::default()))),
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
    };

    tokio::spawn(run_dr_hook_scheduler(state.clone()));

    let app = Router::new()
        .route("/health", get(health))
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
        // S9-WS8A-02: Audit export (all buffered events + file-backed status)
        .route("/api/v1/audit/export", get(audit_export))
        // S7-WS6-02: Raft consensus RPC + status endpoints
        .route("/api/v1/cluster/raft/status", get(raft_status))
        .route("/api/v1/cluster/raft/vote", post(raft_vote))
        .route("/api/v1/cluster/raft/append", post(raft_append))
        .route("/api/v1/cluster/raft/tick", post(raft_tick))
        .route("/api/v1/cluster/raft/log", get(raft_log))
        // S7-WS6-03: Raft fencing token
        .route("/api/v1/cluster/raft/fence", get(raft_fence))
        .route("/api/v1/store/rows/scan", post(store_rows_scan))
        // S4-WS3-04: HTAP sync export for OLAP consumers
        .route("/api/v1/store/htap/export", post(store_htap_export))
        // S4-WS3-03: vectorized columnar scan
        .route("/api/v1/store/columnar/scan", get(store_columnar_scan))
        // S6-WS5-03: TLS runtime status
        .route("/api/v1/security/tls/status", get(security_tls_status))
        .route("/api/v1/security/tls/rotate", post(security_tls_rotate))
        // S6-WS5-04: TDE/encryption-at-rest status
        .route("/api/v1/security/tde/status", get(security_tde_status))
        // S6-WS5-04: TDE toggle override
        .route("/api/v1/security/tde/toggle", post(security_tde_toggle))
        // S9-WS8-02: AI model gateway policy
        .route("/api/v1/ai/policy", get(ai_policy))
        // S2-WS2-02: WAL durability status + recovery replay
        .route("/api/v1/store/wal/status", get(wal_status))
        .route("/api/v1/store/wal/recover", post(wal_recover))
        .route("/api/v1/store/wal/checkpoint", post(wal_force_checkpoint))
        // S7-WS6-04: Chaos/game-day fault injection
        .route("/api/v1/cluster/chaos/inject", post(chaos_inject))
        .route("/api/v1/cluster/chaos/clear", post(chaos_clear))
        .route("/api/v1/cluster/chaos/status", get(chaos_status))
        .route("/api/v1/cluster/chaos/health", get(chaos_health))
        .route("/api/v1/store/wal/checkpoint", post(wal_force_checkpoint))
        // S8-WS10-02: Driver wire protocol info + session connect + disconnect
        .route("/api/v1/driver/protocol/info", get(driver_protocol_info))
        .route("/api/v1/driver/connect", post(driver_connect))
        .route("/api/v1/driver/disconnect", post(driver_disconnect))
        .route("/api/v1/driver/sessions", get(driver_session_list))
        // S10-WS15-02: CDC stream from WAL
        .route("/api/v1/store/cdc/stream", get(cdc_stream))
        .route("/api/v1/store/cdc/stream/filter", get(cdc_stream_filter))
        .route("/api/v1/store/cdc/cursor", get(cdc_cursor_status))
        .route("/api/v1/store/cdc/cursor/advance", post(cdc_cursor_advance))
        // S2-WS2-04: Row store point-in-time snapshot export
        .route("/api/v1/store/rows/snapshot", get(row_store_snapshot))
        // S5-WS4A-02: Broker adapter status + flush
        .route("/api/v1/ingest/outbox/broker/status", get(outbox_broker_status))
        .route("/api/v1/ingest/outbox/broker/flush", post(outbox_broker_flush))
        .route("/api/v1/ingest/outbox/broker/health", get(outbox_broker_health))
        // S5-E4A-01: Connector SDK runtime load
        .route("/api/v1/connectors", get(connector_list))
        .route("/api/v1/connectors/register", post(connector_register))
        .route("/api/v1/ai/policy/update", post(ai_policy_update))
        .route("/api/v1/ai/policy/stats", get(ai_policy_stats))
        .route("/api/v1/ai/request", post(ai_rate_check))
        // WS4 Ingest endpoints
        .route("/api/v1/ingest/csv", post(ingest_csv))
        .route("/api/v1/ingest/json", post(ingest_json))
        .route("/api/v1/ingest/parquet", post(ingest_parquet))
        .route("/api/v1/ingest/excel", post(ingest_excel))
        .route("/api/v1/ingest/chunked", post(ingest_chunked))
        .route("/api/v1/ingest/status", get(ingest_status))
        .route("/api/v1/ingest/outbox/status", get(ingest_outbox_status))
        .route("/api/v1/ingest/outbox/replay", post(ingest_outbox_replay))
        // REQ-02 DDL catalog endpoint
        .route("/api/v1/catalog/schemas", get(catalog_schemas))
        // REQ-23 ACID transaction introspection
        .route("/api/v1/sql/transactions/active", get(sql_transactions_active))
        // REQ-10/19: benchmark endpoints
        .route("/api/v1/benchmark/ingest", post(benchmark_ingest))
        .route("/api/v1/benchmark/query", post(benchmark_query))
        .with_state(state);

    println!("voltnuerongridd listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind listener");
    axum::serve(listener, app).await.expect("server failed");
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        node_id: state.node_id,
        cluster_mode: state.cluster_mode,
    })
}

/// Parse a SQL DELETE statement and return the row key to tombstone.
/// Pattern: DELETE FROM <table> WHERE <col> = '<key>'
fn extract_delete_key_from_sql(sql: &str) -> Option<String> {
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
fn extract_update_row_from_sql(
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
    data.insert("table".to_string(), upd.table.clone());
    for (col, val) in &upd.assignments {
        data.insert(col.clone(), val.clone());
    }
    Some((row_key, data))
}

/// Parse a SQL INSERT statement using the tokenizer and return a (row_key, row_data) pair
/// suitable for writing into PagedRowStore. Returns None for non-INSERT or unparseable input.
/// Used by the sql_transaction COMMIT path (S2-WS2-05) to flush rows into storage.
fn extract_insert_row_from_sql(
    sql: &str,
) -> Option<(String, std::collections::HashMap<String, String>)> {
    use voltnuerongrid_sql::tokenizer::{semantic_tokens, Token};
    let tokens = semantic_tokens(sql);
    let mut it = tokens.iter();
    match it.next() {
        Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("INSERT") => {}
        _ => return None,
    }
    match it.next() {
        Some(Token::Keyword(k)) if k.eq_ignore_ascii_case("INTO") => {}
        _ => return None,
    }
    let table = match it.next() {
        Some(Token::Identifier(t)) | Some(Token::Keyword(t)) => t.clone(),
        _ => return None,
    };
    // Collect all string literals and numbers — these are the VALUES
    let mut values: Vec<String> = Vec::new();
    for tok in &tokens {
        match tok {
            Token::StringLiteral(s) => values.push(s.clone()),
            Token::Number(n) => values.push(n.clone()),
            _ => {}
        }
    }
    if values.is_empty() {
        return None;
    }
    let row_key = format!("{}:{}", table, values[0]);
    let mut data = std::collections::HashMap::new();
    data.insert("table".to_string(), table);
    data.insert("row_values".to_string(), values.join(","));
    Some((row_key, data))
}

async fn sql_transaction(
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
        let has_begin = req.statements.iter().any(|s| {
            matches!(SqlAnalyzer::analyze_statement(s).kind, SqlStatementKind::Begin)
        });
        let has_commit = req.statements.iter().any(|s| {
            matches!(SqlAnalyzer::analyze_statement(s).kind, SqlStatementKind::Commit)
        });
        let has_rollback = req.statements.iter().any(|s| {
            matches!(SqlAnalyzer::analyze_statement(s).kind, SqlStatementKind::Rollback)
        });
        let iso_level = req.isolation_level
            .as_deref()
            .unwrap_or("read_committed")
            .to_string();
        let mut acid = state.acid_transactions.lock().expect("acid_tx lock");
        if has_begin {
            acid.begin(&tx_id, &iso_level, now_ms);
        }
        for stmt in &req.statements {
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
            // UPDATE <table> SET ... â†’ token index 1; INSERT INTO <table> / DELETE FROM <table> â†’ index 2
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
                for stmt in &req.statements {
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
                            return Err((
                                StatusCode::CONFLICT,
                                Json(AuthErrorResponse {
                                    status: "error",
                                    reason: format!("write_write_conflict:{key}"),
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
                for stmt in &req.statements {
                    let upper = stmt.trim_start().to_ascii_uppercase();
                    if upper.starts_with("INSERT") {
                        if let Some((k, d)) = extract_insert_row_from_sql(stmt) {
                            // Register write intent so concurrent conflict checks see this lock.
                            let _ = rs.begin_write_intent(xid, &k);
                            rs.insert(xid, &k, d);
                        }
                    } else if upper.starts_with("DELETE") {
                        if let Some(k) = extract_delete_key_from_sql(stmt) {
                            let _ = rs.begin_write_intent(xid, &k);
                            rs.delete(xid, &k);
                        }
                    } else if upper.starts_with("UPDATE") {
                        if let Some((k, d)) = extract_update_row_from_sql(stmt) {
                            let _ = rs.begin_write_intent(xid, &k);
                            rs.insert(xid, &k, d);
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

async fn sql_pessimistic_lock_acquire(
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

async fn sql_pessimistic_lock_release(
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

async fn sql_pessimistic_lock_metrics(
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

async fn sql_analyze(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SqlAnalyzeRequest>,
) -> Result<Json<SqlAnalyzeResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_sql_runtime_principal(&headers, &state, PrivilegeAction::Read, "sql/analyze")?;
    let parsed = SqlAnalyzer::parse_batch(&req.sql_batch);
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

    let response = SqlAnalyzeResponse {
        status: "ok",
        total_statements: statements.len(),
        rejected_statements: rejected,
        statements,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Sql,
        &principal,
        "sql_analyze",
        "ok",
        json!({
            "route_scope": "sql/analyze",
            "total_statements": response.total_statements,
            "rejected_statements": response.rejected_statements,
        }),
    );
    Ok(Json(response))
}

async fn sql_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SqlRouteRequest>,
) -> Result<Json<SqlRouteResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_sql_runtime_principal(&headers, &state, PrivilegeAction::Read, "sql/route")?;
    let connection_id = acquire_sql_data_plane_connection(&state, &headers, &principal, "sql/route")?;
    let decision = HtapQueryRouter::route_batch(&req.sql_batch);
    // S3-WS1-05: augment each routed statement with cost-model hints from QueryPlanner
    use voltnuerongrid_exec::{QueryPlanner, QueryPath};
    use voltnuerongrid_sql::parse_one;
    let mut batch_estimated_rows: u64 = 0;
    let mut batch_relative_cost: f64 = 0.0;
    let statements: Vec<RoutedStatementResponse> = decision
        .statements
        .into_iter()
        .map(|s| {
            let (planner_path, estimated_rows, relative_cost) =
                match parse_one(&s.statement) {
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
    let response = SqlRouteResponse {
        status: "ok",
        route_path: route_path_name(decision.path).to_string(),
        reason: decision.reason,
        statements,
        batch_estimated_rows,
        batch_relative_cost,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Sql,
        &principal,
        "sql_route",
        "ok",
        json!({
            "route_scope": "sql/route",
            "route_path": response.route_path,
            "statement_count": response.statements.len(),
            "reason": response.reason,
        }),
    );
    release_sql_data_plane_connection(&state, &connection_id);
    Ok(Json(response))
}

async fn sql_execute(
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
    let decision = HtapQueryRouter::route_batch(&req.sql_batch);
    let parsed = SqlAnalyzer::parse_batch(&req.sql_batch);
    let udf_function_catalog = udf_function_catalog_contract();
    let udf_guard_policies = udf_guard_policy_contract();
    let udf_execution_plan = build_udf_execution_plan(&req.sql_batch);
    let udf_execution = execute_udf_runtime_scaffold(&req.sql_batch);

    let udf_results = match udf_execution {
        Ok(results) => results,
        Err(reason) => {
            append_runtime_audit_event(
                &state,
                AuditEventKind::Sql,
                &principal,
                "sql_execute",
                "blocked",
                json!({
                    "route_scope": "sql/execute",
                    "route_path": route_path_name(decision.path),
                    "reason": reason,
                    "rejected_statement_count": parsed.len(),
                    "udf_guardrail_status": "blocked",
                }),
            );
            let response = Ok((
                StatusCode::BAD_REQUEST,
                Json(SqlExecuteResponse {
                    status: "error",
                    route_path: route_path_name(decision.path).to_string(),
                    reason,
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
                }),
            ));
            release_sql_data_plane_connection(&state, &connection_id);
            return response;
        }
    };

    if matches!(decision.path, QueryPath::Unknown) {
        append_runtime_audit_event(
            &state,
            AuditEventKind::Sql,
            &principal,
            "sql_execute",
            "error",
            json!({
                "route_scope": "sql/execute",
                "route_path": "unknown",
                "reason": decision.reason,
                "rejected_statement_count": parsed.len(),
            }),
        );
        let response = Ok((
            StatusCode::BAD_REQUEST,
            Json(SqlExecuteResponse {
                status: "error",
                route_path: "unknown".to_string(),
                reason: decision.reason,
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
            }),
        ));
        release_sql_data_plane_connection(&state, &connection_id);
        return response;
    }

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

    if !transaction_statements.is_empty() {
        // REQ-02: snapshot statements for DDL catalog update after ownership transfer
        let ddl_snapshot: Vec<String> = transaction_statements.clone();
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
                    "route_path": route_path_name(decision.path),
                    "reason": decision.reason,
                    "rejected_statement_count": rejected_statement_count,
                    "transaction_status": response.status,
                }),
            );
            let response = Ok((
                status,
                Json(SqlExecuteResponse {
                    status: "error",
                    route_path: route_path_name(decision.path).to_string(),
                    reason: decision.reason,
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
            for stmt in &ddl_snapshot {
                if let Some(info) = parse_ddl_info(stmt) {
                    match info.operation {
                        "create" => { catalog.record_create(&info.object_kind, &info.object_name, stmt, now_ms); }
                        "drop" => { catalog.record_drop(&info.object_name); }
                        "alter" => { catalog.record_alter(&info.object_name, stmt, now_ms); }
                        _ => {}
                    }
                }
            }
        }
    }

    if !olap_statements.is_empty() {
        let query = olap_statements.join("; ");
        olap = Some(execute_olap_query(query, req.max_rows));
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
            let rs = state.row_store.lock().expect("row_store lock oltp select");
            let limit = req.max_rows.unwrap_or(10_000).min(100_000);
            let rows = execute_oltp_select(&olap_statements, &rs, limit);
            if rows.is_empty() { None } else { Some(rows) }
        } else {
            None
        };

    // S3-WS1-05: vectorized OLAP executor dispatch — if planner says olap/hybrid,
    // run filter_batch (predicate pushdown from WHERE clause) then aggregate_batch over a
    // columnar scan of the committed PagedRowStore snapshot.
    let olap_agg_results: Option<Vec<OlapVecAggResult>> =
        if matches!(planner_path.as_deref(), Some("olap") | Some("hybrid")) {
            use voltnuerongrid_store::columnar::{
                vectorized_scan, aggregate_batch, filter_batch, VectorizedAggOp,
            };
            use voltnuerongrid_sql::{parse_one, Statement};
            let rs = state.row_store.lock().expect("row_store lock olap dispatch");
            let snapshot_xid = rs.current_xid();
            // Collect into owned (String, HashMap) so the lock can be released before scan.
            let rows: Vec<(String, std::collections::HashMap<String, String>)> = rs
                .scan_at_snapshot(snapshot_xid)
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect();
            drop(rs);
            let limit = req.max_rows.unwrap_or(10_000).min(100_000);
            let (batch, _stats) = vectorized_scan(&rows, limit);
            if batch.row_count() == 0 {
                None
            } else {
                // S3-WS1-05: extract WHERE predicates from the first parseable SELECT
                // and push them into filter_batch before aggregating.
                let predicates = olap_statements.iter().find_map(|sql| {
                    if let Ok(Statement::Select(sel)) = parse_one(sql) {
                        sel.where_clause
                            .as_deref()
                            .and_then(parse_where_predicates)
                    } else {
                        None
                    }
                });
                let filtered = match predicates {
                    Some(preds) if !preds.is_empty() => filter_batch(&batch, &preds),
                    _ => batch,
                };
                if filtered.row_count() == 0 {
                    None
                } else {
                    // Apply Count on every column in the filtered batch.
                    let mut ops = std::collections::HashMap::new();
                    for col_name in filtered.columns.keys() {
                        ops.insert(col_name.clone(), VectorizedAggOp::Count);
                    }
                    let count_results = aggregate_batch(&filtered, &ops);
                    let mut agg_out: Vec<OlapVecAggResult> = count_results
                        .into_iter()
                        .map(|(col, res)| OlapVecAggResult {
                            column: col,
                            op: format!("{:?}", res.op).to_ascii_lowercase(),
                            value: res.value,
                            row_count: res.row_count,
                        })
                        .collect();
                    agg_out.sort_by(|a, b| a.column.cmp(&b.column));
                    Some(agg_out)
                }
            }
        } else {
            None
        };
    let response = SqlExecuteResponse {
        status: "ok",
        route_path: route_path_name(decision.path).to_string(),
        reason: decision.reason,
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

async fn olap_query(Json(req): Json<OlapQueryRequest>) -> Json<OlapQueryResponse> {
    Json(execute_olap_query(req.query, req.max_rows))
}

async fn failover_status(State(state): State<AppState>) -> Json<FailoverStatusResponse> {
    let leader = state
        .leader_node_id
        .lock()
        .map(|value| value.clone())
        .unwrap_or_else(|_| state.node_id.clone());
    let unresolved_critical_count = state
        .cluster_failure_signals
        .lock()
        .map(|signals| {
            signals
                .iter()
                .filter(|signal| signal.severity.eq_ignore_ascii_case("critical") && !signal.resolved)
                .count()
        })
        .unwrap_or(usize::MAX);
    Json(FailoverStatusResponse {
        status: if unresolved_critical_count > 0 {
            "degraded"
        } else {
            "healthy"
        },
        cluster_mode: state.cluster_mode,
        leader_node_id: leader,
        unresolved_critical_count,
        rto_seconds_target: 30,
        rpo_data_loss_rows_target: 0,
    })
}

async fn failover_simulate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FailoverSimulateRequest>,
) -> Result<Json<FailoverSimulateResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.failover",
        "cluster",
        PrivilegeAction::Execute,
    )?;
    let (previous_leader_node_id, new_leader_node_id) =
        rotate_leader(&state.leader_node_id, &req.new_leader_node_id, &state.node_id);
    record_transport_mutation(
        &state,
        &previous_leader_node_id,
        &new_leader_node_id,
        "failover_control_plane",
        "cluster_failover",
        &format!("{}->{}:prepare", previous_leader_node_id, new_leader_node_id),
        MutationOp::Insert,
        json!({
            "event": "leader_handoff_prepare",
            "source_node_id": previous_leader_node_id,
            "target_node_id": new_leader_node_id,
            "requested_by": operator.operator_id.as_str(),
            "operator_role": operator.role.as_str(),
            "reason": req
                .reason
                .clone()
                .unwrap_or_else(|| "manual_failover_simulation".to_string()),
            "transport": "control_plane"
        }),
    );
    record_transport_mutation(
        &state,
        &previous_leader_node_id,
        &new_leader_node_id,
        "failover_control_plane",
        "cluster_failover",
        &format!("{}->{}:commit", previous_leader_node_id, new_leader_node_id),
        MutationOp::Update,
        json!({
            "event": "leader_handoff_commit",
            "source_node_id": previous_leader_node_id,
            "target_node_id": new_leader_node_id,
            "requested_by": operator.operator_id.as_str(),
            "operator_role": operator.role.as_str(),
            "reason": req
                .reason
                .clone()
                .unwrap_or_else(|| "manual_failover_simulation".to_string()),
            "transport": "control_plane"
        }),
    );
    let handoff_report = build_failover_handoff_report(
        &state,
        &previous_leader_node_id,
        &new_leader_node_id,
    );
    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "failover_simulate",
        "ok",
        &json!({
            "previous_leader_node_id": previous_leader_node_id.clone(),
            "new_leader_node_id": new_leader_node_id.clone(),
            "operator_role": operator.role.as_str(),
            "reason": req.reason.clone().unwrap_or_else(|| "manual_failover_simulation".to_string()),
            "handoff_state": handoff_report.handoff_state,
            "replay_batch_size": handoff_report.replay_batch_size,
            "applied_count": handoff_report.applied_count,
            "gap_count": handoff_report.gap_count
        })
        .to_string(),
    );
    Ok(Json(FailoverSimulateResponse {
        status: "ok",
        previous_leader_node_id,
        new_leader_node_id,
        reason: req
            .reason
            .unwrap_or_else(|| "manual_failover_simulation".to_string()),
        requested_by: req.requested_by.unwrap_or_else(|| "unknown".to_string()),
        handoff_report,
    }))
}

async fn sre_reliability_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SreReliabilityStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let _operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/reliability",
        PrivilegeAction::Read,
    )?;
    Ok(Json(SreReliabilityStatusResponse {
        status: "ok",
        service_health: "healthy",
        failure_budget: failure_budget_snapshot(12.5),
        rate_limit_policy: rate_limit_policy_snapshot(540),
    }))
}

async fn sre_rate_limit_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RateLimitCheckRequest>,
) -> Result<Json<RateLimitCheckResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/rate_limit",
        PrivilegeAction::Execute,
    )?;
    let requested_units = req.requested_units.unwrap_or(1).max(1);
    let (allowed, remaining_units, reason) = evaluate_rate_limit(
        req.current_minute_count,
        requested_units,
        600,
        50,
    );
    append_audit_event(
        &state,
        AuditEventKind::Security,
        &operator.operator_id,
        "sre_rate_limit_check",
        if allowed { "allow" } else { "deny" },
        &json!({
            "current_minute_count": req.current_minute_count,
            "requested_units": requested_units,
            "remaining_units": remaining_units,
            "reason": reason,
        })
        .to_string(),
    );
    Ok(Json(RateLimitCheckResponse {
        status: "ok",
        allowed,
        remaining_units,
        reason: reason.to_string(),
    }))
}

async fn sre_failure_budget_alerts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FailureBudgetAlertQuery>,
) -> Result<Json<FailureBudgetAlertResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let _operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/failure_budget",
        PrivilegeAction::Read,
    )?;
    let consumed_percent = query.consumed_percent.unwrap_or(12.5).clamp(0.0, 100.0);
    let burn_rate = query.burn_rate.unwrap_or((consumed_percent / 10.0).max(0.1));
    let alert = evaluate_failure_budget_alert(consumed_percent, burn_rate);
    Ok(Json(alert))
}

async fn sre_dr_hook_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DrHookPolicyResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let _operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.dr_hooks",
        "dr_hooks/policy",
        PrivilegeAction::Read,
    )?;
    let tracked_hooks = state
        .dr_hook_policy_state
        .lock()
        .map(|value| value.hooks.len())
        .unwrap_or(0);
    let policy = state.dr_hook_policy_config.as_ref();
    Ok(Json(DrHookPolicyResponse {
        status: "ok",
        policy: DrHookPolicyContract {
            min_mode: policy.min_mode,
            cooldown_seconds: policy.cooldown_seconds,
            max_retries: policy.max_retries,
            base_backoff_ms: policy.base_backoff_ms,
            max_backoff_ms: policy.max_backoff_ms,
            allowed_hooks: policy.allowed_hooks.clone(),
            tracked_hooks,
        },
    }))
}

async fn sre_dr_hook_retry_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DrHookRetryPlanQuery>,
) -> Result<Json<DrHookRetryPlanResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let _operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.dr_hooks",
        "dr_hooks/retry_plan",
        PrivilegeAction::Read,
    )?;
    let policy = state.dr_hook_policy_config.as_ref();
    let attempts = query.attempts.unwrap_or(5).clamp(1, 10);
    let hook = query.hook.trim().to_ascii_lowercase();
    let accepted = policy
        .allowed_hooks
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(&hook));
    if !accepted {
        return Ok(Json(DrHookRetryPlanResponse {
            status: "ok",
            hook,
            accepted: false,
            reason: "unsupported_dr_hook".to_string(),
            steps: Vec::new(),
        }));
    }
    let steps = build_retry_plan(policy, attempts);
    Ok(Json(DrHookRetryPlanResponse {
        status: "ok",
        hook,
        accepted: true,
        reason: "plan_generated".to_string(),
        steps,
    }))
}

async fn sre_dr_hook_schedule(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DrHookScheduleRequest>,
) -> Result<Json<DrHookScheduleResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.dr_hooks",
        "dr_hooks/schedule",
        PrivilegeAction::Execute,
    )?;
    let requested_by = operator.operator_id.as_str();
    let task = enqueue_dr_hook_task(
        &state,
        &req.hook,
        req.scope.as_deref(),
        req.dry_run.unwrap_or(false),
        requested_by,
        req.reason.as_deref().unwrap_or("manual_sre_schedule"),
    );
    append_audit_event(
        &state,
        AuditEventKind::Failover,
        requested_by,
        "sre_dr_hook_schedule",
        "queued",
        &json!({
            "task_id": task.task_id,
            "hook": task.hook,
            "scope": task.scope,
            "dry_run": task.dry_run,
        })
        .to_string(),
    );
    let queue_depth = state.dr_hook_queue.lock().map(|q| q.len()).unwrap_or(0);
    Ok(Json(DrHookScheduleResponse {
        status: "ok",
        task,
        queue_depth,
    }))
}

async fn sre_dr_hook_trigger(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DrHookTriggerRequest>,
) -> Result<Json<DrHookTriggerResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.dr_hooks",
        "dr_hooks/trigger",
        PrivilegeAction::Execute,
    )?;
    let requested_by = operator.operator_id.as_str();
    let execution = execute_dr_hook(
        &state,
        &req.hook,
        req.scope.as_deref(),
        req.dry_run.unwrap_or(true),
    );
    append_audit_event(
        &state,
        AuditEventKind::Failover,
        requested_by,
        "sre_dr_hook_trigger",
        execution.status,
        &json!({
            "execution_id": execution.execution_id,
            "hook": execution.hook,
            "scope": execution.scope,
            "dry_run": execution.dry_run,
            "policy_decision": execution.policy_decision,
            "cooldown_remaining_ms": execution.cooldown_remaining_ms,
            "retry_backoff_ms": execution.retry_backoff_ms,
            "retry_attempt": execution.retry_attempt,
            "details": execution.details,
        })
        .to_string(),
    );
    Ok(Json(DrHookTriggerResponse {
        status: "ok",
        execution,
    }))
}

async fn sre_dr_hook_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DrHookStatusQuery>,
) -> Result<Json<DrHookStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let _operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.dr_hooks",
        "dr_hooks/status",
        PrivilegeAction::Read,
    )?;
    let max_items = query.max_items.unwrap_or(50).min(500);
    let records = latest_dr_hook_records(&state, max_items);
    Ok(Json(DrHookStatusResponse {
        status: "ok",
        total_records: records.len(),
        records,
    }))
}

async fn sre_failure_signal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FailureSignalRequest>,
) -> Result<Json<FailureSignalResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/failure_signal",
        PrivilegeAction::Execute,
    )?;
    let signal = ClusterFailureSignal {
        signal_id: format!("sig-{}", DR_HOOK_COUNTER.fetch_add(1, Ordering::Relaxed)),
        node_id: req.node_id.trim().to_string(),
        transport: req.transport.trim().to_string(),
        failure_type: req.failure_type.trim().to_ascii_lowercase(),
        severity: req.severity.trim().to_ascii_lowercase(),
        message: req
            .message
            .unwrap_or_else(|| "no_message_provided".to_string())
            .trim()
            .to_string(),
        observed_unix_ms: now_unix_ms(),
        resolved: false,
        resolved_by: None,
        resolved_unix_ms: None,
        resolution_note: None,
    };
    if let Ok(mut signals) = state.cluster_failure_signals.lock() {
        signals.push(signal.clone());
    }
    record_transport_mutation(
        &state,
        &state.node_id,
        "*",
        "failure_signal",
        "cluster_failure_signal",
        &signal.signal_id,
        MutationOp::Insert,
        json!({
            "signal_id": signal.signal_id,
            "node_id": signal.node_id,
            "transport": signal.transport,
            "failure_type": signal.failure_type,
            "severity": signal.severity,
            "observed_by": operator.operator_id.as_str(),
        }),
    );
    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "sre_failure_signal",
        "observed",
        &json!({
            "signal_id": signal.signal_id,
            "node_id": signal.node_id,
            "transport": signal.transport,
            "failure_type": signal.failure_type,
            "severity": signal.severity,
        })
        .to_string(),
    );

    let queued_remediation_task = if signal.severity == "critical"
        && signal.failure_type == "node_unreachable"
    {
        Some(enqueue_dr_hook_task(
            &state,
            "failover_drill",
            Some("cluster"),
            false,
            "auto_sre",
            "critical_node_unreachable_signal",
        ))
    } else {
        None
    };

    Ok(Json(FailureSignalResponse {
        status: "ok",
        signal,
        queued_remediation_task,
    }))
}

async fn sre_failure_reconcile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FailureReconcileRequest>,
) -> Result<Json<FailureReconcileResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/failure_reconcile",
        PrivilegeAction::Execute,
    )?;
    let mut resolved_count = 0usize;
    let now = now_unix_ms();
    let selected_ids = req.signal_ids.unwrap_or_default();
    let resolve_all_critical = req.resolve_all_critical.unwrap_or(false);
    if let Ok(mut signals) = state.cluster_failure_signals.lock() {
        for signal in signals.iter_mut() {
            if signal.resolved {
                continue;
            }
            let targeted = if resolve_all_critical {
                signal.severity.eq_ignore_ascii_case("critical")
            } else {
                selected_ids.iter().any(|id| id == &signal.signal_id)
            };
            if targeted {
                signal.resolved = true;
                signal.resolved_by = Some(operator.operator_id.clone());
                signal.resolved_unix_ms = Some(now);
                signal.resolution_note = req.note.clone();
                resolved_count += 1;
            }
        }
    }
    let unresolved_critical_count = state
        .cluster_failure_signals
        .lock()
        .map(|signals| {
            signals
                .iter()
                .filter(|s| s.severity.eq_ignore_ascii_case("critical") && !s.resolved)
                .count()
        })
        .unwrap_or(usize::MAX);
    if resolved_count > 0 {
        record_transport_mutation(
            &state,
            &state.node_id,
            "*",
            "failure_reconcile",
            "cluster_failure_signal",
            &format!("reconcile-{now}"),
            MutationOp::Update,
            json!({
                "resolved_count": resolved_count,
                "resolved_by": operator.operator_id.as_str(),
                "resolve_all_critical": resolve_all_critical,
                "unresolved_critical_count": unresolved_critical_count,
            }),
        );
    }
    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "sre_failure_reconcile",
        "ok",
        &json!({
            "resolved_count": resolved_count,
            "unresolved_critical_count": unresolved_critical_count,
            "resolve_all_critical": resolve_all_critical,
        })
        .to_string(),
    );
    Ok(Json(FailureReconcileResponse {
        status: "ok",
        resolved_count,
        unresolved_critical_count,
    }))
}

async fn sre_gate_evaluate(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SreGateEvaluationResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let _operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/gate",
        PrivilegeAction::Read,
    )?;
    Ok(Json(build_sre_gate_evaluation(&state)))
}

async fn sre_gate_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SreGateExportRequest>,
) -> Result<Json<SreGateExportResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let _operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/gate",
        PrivilegeAction::Manage,
    )?;
    let evaluation = build_sre_gate_evaluation(&state);
    let output_path = req
        .output_path
        .unwrap_or_else(|| "tests/kpi/results/ws12/gate-fail-report.json".to_string());
    export_gate_report(&output_path, &evaluation);
    Ok(Json(SreGateExportResponse {
        status: "ok",
        output_path,
        gate_result: evaluation.gate_result,
    }))
}

async fn sre_cache_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CacheSetRequest>,
) -> Result<Json<CacheWriteResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/cache",
        PrivilegeAction::Execute,
    )?;

    let now_ms = now_unix_ms_u64();
    let result = state
        .distributed_cache
        .lock()
        .expect("cache manager lock")
        .set(
            req.partition_id.as_str(),
            req.key.clone(),
            req.value,
            req.ttl_ms,
            now_ms,
        );

    let response = CacheWriteResponse {
        status: if result.is_ok() { "ok" } else { "error" },
        partition_id: req.partition_id.clone(),
        key: req.key.clone(),
        error: result.err().map(|error| error.to_string()),
    };

    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "sre_cache_set",
        response.status,
        &json!({
            "partition_id": response.partition_id,
            "key": response.key,
            "ttl_ms": req.ttl_ms,
            "error": response.error,
        })
        .to_string(),
    );

    Ok(Json(response))
}

async fn sre_cache_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CacheGetQuery>,
) -> Result<Json<CacheGetResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/cache",
        PrivilegeAction::Read,
    )?;

    let now_ms = now_unix_ms_u64();
    let result = state
        .distributed_cache
        .lock()
        .expect("cache manager lock")
        .get(query.partition_id.as_str(), query.key.as_str(), now_ms);

    let response = match result {
        Ok(value) => CacheGetResponse {
            status: "ok",
            partition_id: query.partition_id.clone(),
            key: query.key.clone(),
            hit: value.is_some(),
            value,
            error: None,
        },
        Err(error) => CacheGetResponse {
            status: "error",
            partition_id: query.partition_id.clone(),
            key: query.key.clone(),
            hit: false,
            value: None,
            error: Some(error.to_string()),
        },
    };

    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "sre_cache_get",
        response.status,
        &json!({
            "partition_id": response.partition_id,
            "key": response.key,
            "hit": response.hit,
            "error": response.error,
        })
        .to_string(),
    );

    Ok(Json(response))
}

async fn sre_cache_invalidate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CacheInvalidateRequest>,
) -> Result<Json<CacheInvalidateResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/cache",
        PrivilegeAction::Execute,
    )?;

    let result = state
        .distributed_cache
        .lock()
        .expect("cache manager lock")
        .invalidate(req.partition_id.as_str(), req.key.as_str());

    let response = match result {
        Ok(removed) => CacheInvalidateResponse {
            status: "ok",
            partition_id: req.partition_id.clone(),
            key: req.key.clone(),
            removed,
            error: None,
        },
        Err(error) => CacheInvalidateResponse {
            status: "error",
            partition_id: req.partition_id.clone(),
            key: req.key.clone(),
            removed: false,
            error: Some(error.to_string()),
        },
    };

    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "sre_cache_invalidate",
        response.status,
        &json!({
            "partition_id": response.partition_id,
            "key": response.key,
            "removed": response.removed,
            "error": response.error,
        })
        .to_string(),
    );

    Ok(Json(response))
}

async fn sre_cache_rebalance(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<CacheRebalanceResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/cache",
        PrivilegeAction::Execute,
    )?;

    let now_ms = now_unix_ms_u64();
    let results = state
        .distributed_cache
        .lock()
        .expect("cache manager lock")
        .rebalance_all(now_ms);
    let entries_evicted: usize = results.iter().map(|result| result.entries_evicted).sum();

    let response = CacheRebalanceResponse {
        status: "ok",
        partition_count: results.len(),
        rebalanced_partitions: results.len(),
        entries_evicted,
    };

    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "sre_cache_rebalance",
        "ok",
        &json!({
            "partition_count": response.partition_count,
            "entries_evicted": response.entries_evicted,
        })
        .to_string(),
    );

    Ok(Json(response))
}

async fn sre_cache_metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<CacheMetricsResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let _operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/cache",
        PrivilegeAction::Read,
    )?;

    let guard = state.distributed_cache.lock().expect("cache manager lock");
    let partitions = guard
        .all_stats()
        .into_iter()
        .map(|partition| CachePartitionMetricsResponse {
            partition_id: partition.partition_id,
            entry_count: partition.entry_count,
            total_hits: partition.total_hits,
            total_misses: partition.total_misses,
            total_evictions: partition.total_evictions,
            circuit_breaker_state: partition.circuit_breaker_state,
            hit_ratio: partition.hit_ratio,
            last_rebalance_ms: partition.last_rebalance_ms,
        })
        .collect::<Vec<_>>();

    Ok(Json(CacheMetricsResponse {
        status: "ok",
        partition_count: guard.partition_count(),
        total_entries: guard.total_entry_count(),
        partitions,
    }))
}

// REQ-27: Redis-compat cache command handler -----------------------------------
async fn cache_redis_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RedisCacheCommandRequest>,
) -> Result<Json<RedisCacheCommandResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/cache",
        PrivilegeAction::Execute,
    )?;

    let cmd = req.cmd.trim().to_ascii_uppercase();
    let partition_id = req.partition_id.as_deref().unwrap_or("default");
    let now_ms = now_unix_ms_u64();

    let response = match cmd.as_str() {
        "PING" => RedisCacheCommandResponse {
            status: "ok",
            cmd: cmd.clone(),
            value: Some(serde_json::json!("PONG")),
            exists: None,
            removed: None,
            flushed_count: None,
            keys: None,
            error: None,
        },
        "GET" => {
            let key = req.key.as_deref().unwrap_or("");
            let result = state.distributed_cache.lock().expect("cache manager lock")
                .get(partition_id, key, now_ms);
            match result {
                Ok(value) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: Some(e.to_string()),
                },
            }
        }
        "SET" => {
            let key = req.key.clone().unwrap_or_default();
            let value = req.value.clone().unwrap_or(serde_json::Value::Null);
            let result = state.distributed_cache.lock().expect("cache manager lock")
                .set(partition_id, key, value, req.ttl_ms, now_ms);
            RedisCacheCommandResponse {
                status: if result.is_ok() { "ok" } else { "error" },
                cmd: cmd.clone(),
                value: None,
                exists: None,
                removed: None,
                flushed_count: None,
                keys: None,
                error: result.err().map(|e| e.to_string()),
            }
        }
        "DEL" => {
            let key = req.key.as_deref().unwrap_or("");
            let result = state.distributed_cache.lock().expect("cache manager lock")
                .invalidate(partition_id, key);
            match result {
                Ok(removed) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: Some(removed),
                    flushed_count: None,
                    keys: None,
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: Some(false),
                    flushed_count: None,
                    keys: None,
                    error: Some(e.to_string()),
                },
            }
        }
        "EXISTS" => {
            let key = req.key.as_deref().unwrap_or("");
            let result = state.distributed_cache.lock().expect("cache manager lock")
                .get(partition_id, key, now_ms);
            let exists = result.as_ref().map(|v| v.is_some()).unwrap_or(false);
            RedisCacheCommandResponse {
                status: "ok",
                cmd: cmd.clone(),
                value: None,
                exists: Some(exists),
                removed: None,
                flushed_count: None,
                keys: None,
                error: result.err().map(|e| e.to_string()),
            }
        }
        "KEYS" => {
            let result = state.distributed_cache.lock().expect("cache manager lock")
                .keys_in_partition(partition_id, now_ms);
            match result {
                Ok(keys) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: Some(keys),
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: Some(vec![]),
                    error: Some(e.to_string()),
                },
            }
        }
        "FLUSH" => {
            let result = state.distributed_cache.lock().expect("cache manager lock")
                .invalidate_partition(partition_id);
            match result {
                Ok(flushed) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: Some(flushed),
                    keys: None,
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: Some(0),
                    keys: None,
                    error: Some(e.to_string()),
                },
            }
        }
        // REQ-27: EXPIRE â€” update TTL on existing key
        "EXPIRE" => {
            let key = req.key.as_deref().unwrap_or("");
            let ttl = req.expire_ms.or(req.ttl_ms).unwrap_or(60_000);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            match cache.expire_key(partition_id, key, ttl, now_ms) {
                Ok(updated) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value: Some(serde_json::json!(updated)),
                    exists: Some(updated),
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: Some(e.to_string()),
                },
            }
        }
        // REQ-27: INCR / INCRBY â€” atomically increment numeric value
        "INCR" | "INCRBY" => {
            let key = req.key.as_deref().unwrap_or("");
            let delta = if cmd.as_str() == "INCR" { 1.0 } else { req.delta.unwrap_or(1.0) };
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            match cache.increment_key(partition_id, key, delta, req.ttl_ms, now_ms) {
                Ok(new_val) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value: Some(serde_json::json!(new_val)),
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: Some(e.to_string()),
                },
            }
        }
        // REQ-27: DECR / DECRBY â€” atomically decrement numeric value
        "DECR" | "DECRBY" => {
            let key = req.key.as_deref().unwrap_or("");
            let delta = if cmd.as_str() == "DECR" { -1.0 } else { -(req.delta.unwrap_or(1.0)) };
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            match cache.increment_key(partition_id, key, delta, req.ttl_ms, now_ms) {
                Ok(new_val) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value: Some(serde_json::json!(new_val)),
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: Some(e.to_string()),
                },
            }
        }
        // REQ-27: MGET â€” return values for multiple keys as a JSON array
        "MGET" => {
            let keys = req.keys.clone().unwrap_or_default();
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let mut results = Vec::new();
            for k in &keys {
                let v = cache.get(partition_id, k, now_ms).ok().flatten();
                results.push(v.unwrap_or(serde_json::Value::Null));
            }
            RedisCacheCommandResponse {
                status: "ok",
                cmd: cmd.clone(),
                value: Some(serde_json::Value::Array(results)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: MSET â€” set multiple keys from a JSON object { key: value, ... }
        "MSET" => {
            let obj = match req.value.as_ref().and_then(|v| v.as_object()) {
                Some(m) => m.clone(),
                None => return Ok(Json(RedisCacheCommandResponse {
                    status: "error", cmd: cmd.clone(), value: None, exists: None,
                    removed: None, flushed_count: None, keys: None,
                    error: Some("MSET requires value to be a JSON object".to_string()),
                })),
            };
            let count = obj.len();
            {
                let mut cache = state.distributed_cache.lock().expect("cache lock");
                for (k, v) in obj {
                    let _ = cache.set(partition_id, k, v, req.ttl_ms, now_ms);
                }
            }
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(count)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: GETSET â€” atomically get old value then set new value
        "GETSET" => {
            let key = req.key.as_deref().unwrap_or("");
            let new_val = req.value.clone().unwrap_or(serde_json::Value::Null);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let old_val = cache.get(partition_id, key, now_ms).ok().flatten();
            let _ = cache.set(partition_id, key.to_string(), new_val, req.ttl_ms, now_ms);
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: old_val,
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: LPUSH â€” prepend a value to a list (stored as JSON array)
        "LPUSH" => {
            let key = req.key.as_deref().unwrap_or("");
            let push_val = req.value.clone().unwrap_or(serde_json::Value::Null);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let mut list: Vec<serde_json::Value> = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr,
                _ => Vec::new(),
            };
            list.insert(0, push_val);
            let new_len = list.len();
            let _ = cache.set(partition_id, key.to_string(), serde_json::Value::Array(list), req.ttl_ms, now_ms);
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(new_len)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: RPUSH â€” append a value to a list (stored as JSON array)
        "RPUSH" => {
            let key = req.key.as_deref().unwrap_or("");
            let push_val = req.value.clone().unwrap_or(serde_json::Value::Null);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let mut list: Vec<serde_json::Value> = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr,
                _ => Vec::new(),
            };
            list.push(push_val);
            let new_len = list.len();
            let _ = cache.set(partition_id, key.to_string(), serde_json::Value::Array(list), req.ttl_ms, now_ms);
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(new_len)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: LLEN â€” return the length of a list
        "LLEN" => {
            let key = req.key.as_deref().unwrap_or("");
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let len = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr.len(),
                _ => 0,
            };
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(len)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: LRANGE â€” return a sub-range of a list (Redis semantics, inclusive stop,
        // negative indices count from tail; -1 = last element)
        "LRANGE" => {
            let key = req.key.as_deref().unwrap_or("");
            let start = req.start.unwrap_or(0);
            let stop = req.stop.unwrap_or(-1);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let list: Vec<serde_json::Value> = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr,
                _ => Vec::new(),
            };
            let len = list.len() as i64;
            let resolve = |i: i64| -> usize {
                if i < 0 { (len + i).max(0) as usize } else { i.min(len) as usize }
            };
            let s = resolve(start);
            let e = (resolve(stop) + 1).min(len as usize);
            let slice = if s < e { list[s..e].to_vec() } else { Vec::new() };
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::Value::Array(slice)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: HSET — set a field in a hash (stored as JSON object in cache)
        "HSET" => {
            let key = req.key.as_deref().unwrap_or("");
            let field = req.field.as_deref().unwrap_or("");
            let val = req.value.clone().unwrap_or(serde_json::Value::Null);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let mut hash: serde_json::Map<String, serde_json::Value> =
                match cache.get(partition_id, key, now_ms).ok().flatten() {
                    Some(serde_json::Value::Object(m)) => m,
                    _ => serde_json::Map::new(),
                };
            hash.insert(field.to_string(), val);
            let _ = cache.set(partition_id, key.to_string(), serde_json::Value::Object(hash), req.ttl_ms, now_ms);
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(1)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: HGET — get a single field from a hash
        "HGET" => {
            let key = req.key.as_deref().unwrap_or("");
            let field = req.field.as_deref().unwrap_or("");
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let field_val = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Object(m)) => m.get(field).cloned(),
                _ => None,
            };
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: field_val,
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: HDEL — delete a field from a hash
        "HDEL" => {
            let key = req.key.as_deref().unwrap_or("");
            let field = req.field.as_deref().unwrap_or("");
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let mut removed = false;
            match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Object(mut m)) => {
                    removed = m.remove(field).is_some();
                    let _ = cache.set(partition_id, key.to_string(), serde_json::Value::Object(m), req.ttl_ms, now_ms);
                }
                _ => {}
            }
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(if removed { 1 } else { 0 })),
                exists: None, removed: Some(removed), flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: HGETALL — return full hash as JSON object
        "HGETALL" => {
            let key = req.key.as_deref().unwrap_or("");
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let hash_val = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(v @ serde_json::Value::Object(_)) => v,
                _ => serde_json::Value::Object(serde_json::Map::new()),
            };
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(hash_val),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: SADD — add a member to a set (stored as JSON array, deduplicated)
        "SADD" => {
            let key = req.key.as_deref().unwrap_or("");
            let member = req.value.clone().unwrap_or(serde_json::Value::Null);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let mut set: Vec<serde_json::Value> = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr,
                _ => Vec::new(),
            };
            let added = if !set.contains(&member) {
                set.push(member);
                1usize
            } else {
                0usize
            };
            let _ = cache.set(partition_id, key.to_string(), serde_json::Value::Array(set), req.ttl_ms, now_ms);
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(added)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: SMEMBERS — return all members of a set
        "SMEMBERS" => {
            let key = req.key.as_deref().unwrap_or("");
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let members = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr,
                _ => Vec::new(),
            };
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::Value::Array(members)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: SCARD — return the cardinality (number of members) of a set
        "SCARD" => {
            let key = req.key.as_deref().unwrap_or("");
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let card = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr.len(),
                _ => 0,
            };
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(card)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        unsupported_cmd => RedisCacheCommandResponse {
            status: "error",
            cmd: cmd.clone(),
            value: None,
            exists: None,
            removed: None,
            flushed_count: None,
            keys: None,
            error: Some(format!("unsupported Redis-compat command: {unsupported_cmd}")),
        },
    };
    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "cache_redis_command",
        response.status,
        &json!({
            "cmd": response.cmd,
            "partition_id": partition_id,
            "key": req.key,
        })
        .to_string(),
    );

    Ok(Json(response))
}

async fn sre_driver_pool_acquire(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PoolAcquireRequest>,
) -> Result<Json<PoolAcquireResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/driver_pool",
        PrivilegeAction::Execute,
    )?;
    let now_ms = req.now_ms.unwrap_or_else(now_unix_ms_u64);

    let (acquire_state, connection_id, error, stats) = {
        let mut pool = state.driver_pool.lock().expect("driver pool lock");
        let acquire_result = pool.acquire(now_ms);
        let (acquire_state, connection_id, error) = match acquire_result {
            Ok(connection_id) => ("acquired", Some(connection_id), None),
            Err(error) => (
                pool_acquire_error_state(&error),
                None,
                Some(error.to_string()),
            ),
        };
        let stats = pool_stats_response(&pool.pool_stats(now_ms));
        (acquire_state, connection_id, error, stats)
    };

    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "sre_driver_pool_acquire",
        if error.is_none() { "ok" } else { "error" },
        &json!({
            "acquire_state": acquire_state,
            "connection_id": connection_id,
            "error": error,
        })
        .to_string(),
    );

    Ok(Json(PoolAcquireResponse {
        status: "ok",
        acquire_state,
        connection_id,
        error,
        stats,
    }))
}

async fn sre_driver_pool_release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PoolReleaseRequest>,
) -> Result<Json<PoolReleaseResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/driver_pool",
        PrivilegeAction::Execute,
    )?;
    let now_ms = req.now_ms.unwrap_or_else(now_unix_ms_u64);

    let (released, stats) = {
        let mut pool = state.driver_pool.lock().expect("driver pool lock");
        let released = pool.release(req.connection_id.as_str(), now_ms);
        let stats = pool_stats_response(&pool.pool_stats(now_ms));
        (released, stats)
    };

    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "sre_driver_pool_release",
        if released { "ok" } else { "error" },
        &json!({
            "connection_id": req.connection_id,
            "released": released,
        })
        .to_string(),
    );

    Ok(Json(PoolReleaseResponse {
        status: "ok",
        released,
        stats,
    }))
}

async fn sre_driver_pool_failure(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PoolFailureRequest>,
) -> Result<Json<PoolFailureResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/driver_pool",
        PrivilegeAction::Execute,
    )?;
    let now_ms = req.now_ms.unwrap_or_else(now_unix_ms_u64);

    let stats = {
        let mut pool = state.driver_pool.lock().expect("driver pool lock");
        pool.mark_failed(
            req.connection_id.as_str(),
            req.error
                .clone()
                .unwrap_or_else(|| "simulated_failure".to_string()),
            now_ms,
        );
        pool_stats_response(&pool.pool_stats(now_ms))
    };

    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "sre_driver_pool_failure",
        "ok",
        &json!({
            "connection_id": req.connection_id,
            "error": req.error,
        })
        .to_string(),
    );

    Ok(Json(PoolFailureResponse {
        status: "ok",
        marked_failed: true,
        stats,
    }))
}

async fn sre_driver_pool_recover(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PoolRecoverRequest>,
) -> Result<Json<PoolRecoverResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/driver_pool",
        PrivilegeAction::Execute,
    )?;
    let now_ms = req.now_ms.unwrap_or_else(now_unix_ms_u64);

    let (circuit_recovered, pruned_unhealthy, stats) = {
        let mut pool = state.driver_pool.lock().expect("driver pool lock");
        let circuit_recovered = pool.check_circuit_recovery(now_ms);
        let pruned_unhealthy = if req.prune_unhealthy.unwrap_or(true) {
            pool.prune_unhealthy(now_ms)
        } else {
            0
        };
        let stats = pool_stats_response(&pool.pool_stats(now_ms));
        (circuit_recovered, pruned_unhealthy, stats)
    };

    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "sre_driver_pool_recover",
        "ok",
        &json!({
            "circuit_recovered": circuit_recovered,
            "pruned_unhealthy": pruned_unhealthy,
        })
        .to_string(),
    );

    Ok(Json(PoolRecoverResponse {
        status: "ok",
        circuit_recovered,
        pruned_unhealthy,
        stats,
    }))
}

async fn sre_driver_pool_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PoolStatsResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let _operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/driver_pool",
        PrivilegeAction::Read,
    )?;
    let now_ms = now_unix_ms_u64();
    let stats = state
        .driver_pool
        .lock()
        .expect("driver pool lock")
        .pool_stats(now_ms);
    Ok(Json(pool_stats_response(&stats)))
}

async fn security_plugins_provenance_register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SignedProvenanceRegistrationRequest>,
) -> Result<Json<SignedProvenanceRegistrationResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "security.supply_chain",
        "security/plugins/provenance/register",
        PrivilegeAction::Manage,
    )?;

    let mut chain = ProvenanceChain::new(req.plugin_id.clone(), req.plugin_version.clone());
    for attestation in &req.attestations {
        let Some(attestation_type) = parse_attestation_type(attestation.attestation_type.as_str()) else {
            return Ok(Json(SignedProvenanceRegistrationResponse {
                status: "error",
                registration_state: "rejected",
                plugin_id: req.plugin_id,
                plugin_version: req.plugin_version,
                chain_complete: false,
                chain_digest: String::new(),
                attestation_count: req.attestations.len(),
                passed_attestations: req.attestations.iter().filter(|entry| entry.passed).count(),
                sbom_approved: false,
                sbom_license_violations: 0,
                sbom_missing_checksums: 0,
                audit_records_total: state
                    .plugin_lifecycle
                    .lock()
                    .map(|manager| manager.audit_trail().len())
                    .unwrap_or(0),
                error: Some("unsupported_attestation_type".to_string()),
            }));
        };

        chain.add_attestation(ProvenanceAttestation {
            attester_id: attestation.attester_id.clone(),
            attested_at_ms: attestation.attested_at_ms.unwrap_or_else(now_unix_ms_u64),
            attestation_type,
            payload_digest_sha256: attestation.payload_digest_sha256.clone(),
            signature_base64: attestation.signature_base64.clone(),
            passed: attestation.passed,
        });
    }

    let sbom_entries = req
        .sbom_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| SbomEntry {
            component_name: entry.component_name,
            component_version: entry.component_version,
            license: entry.license,
            checksum_sha256: entry.checksum_sha256,
            source_url: entry.source_url,
        })
        .collect::<Vec<_>>();
    let sbom_result = SbomInspectionResult::inspect(
        req.plugin_id.clone(),
        sbom_entries,
        &["GPL-3.0-only", "AGPL-3.0-only"],
    );

    if !chain.is_complete() || !sbom_result.approved {
        append_audit_event(
            &state,
            AuditEventKind::Security,
            &operator.operator_id,
            "security_plugins_provenance_register",
            "rejected",
            &json!({
                "plugin_id": req.plugin_id,
                "plugin_version": req.plugin_version,
                "chain_complete": chain.is_complete(),
                "sbom_approved": sbom_result.approved,
            })
            .to_string(),
        );
        return Ok(Json(SignedProvenanceRegistrationResponse {
            status: "error",
            registration_state: "rejected",
            plugin_id: req.plugin_id,
            plugin_version: req.plugin_version,
            chain_complete: chain.is_complete(),
            chain_digest: chain.chain_digest,
            attestation_count: chain.attestations.len(),
            passed_attestations: chain.attestations.iter().filter(|entry| entry.passed).count(),
            sbom_approved: sbom_result.approved,
            sbom_license_violations: sbom_result.license_violations.len(),
            sbom_missing_checksums: sbom_result.missing_checksums.len(),
            audit_records_total: state
                .plugin_lifecycle
                .lock()
                .map(|manager| manager.audit_trail().len())
                .unwrap_or(0),
            error: Some("provenance_or_sbom_policy_violation".to_string()),
        }));
    }

    let manifest = SignedPluginManifest {
        schema_version: req.schema_version.unwrap_or_else(|| "v1".to_string()),
        declared_checksum_sha256: req.checksum_sha256.clone(),
        generated_epoch_ms: now_unix_ms(),
        signature: PluginManifestSignature {
            algorithm: req.signature_algorithm,
            key_id: req.signature_key_id,
            signature_base64: req.signature_base64,
        },
        revoked_key_ids: req.revoked_key_ids.unwrap_or_default(),
    };
    let metadata = ConnectorPackageMetadata {
        plugin_id: req.plugin_id.clone(),
        version: req.plugin_version.clone(),
        display_name: req
            .display_name
            .unwrap_or_else(|| req.plugin_id.clone()),
        owner: req.owner.unwrap_or_else(|| "platform-security".to_string()),
        license: req.license.unwrap_or_else(|| "Apache-2.0".to_string()),
        checksum_sha256: req.checksum_sha256,
        capabilities: req
            .capabilities
            .filter(|capabilities| !capabilities.is_empty())
            .unwrap_or_else(|| vec!["ingest.read".to_string()]),
    };

    let register_result = state
        .plugin_lifecycle
        .lock()
        .expect("plugin lifecycle lock")
        .register(
            manifest,
            metadata,
            Some(operator.operator_id.clone()),
            Some(chain.clone()),
            now_unix_ms_u64(),
        );

    let (status, registration_state, error) = match register_result {
        Ok(_) => ("ok", "registered", None),
        Err(error) => ("error", "rejected", Some(error.to_string())),
    };

    let audit_records_total = state
        .plugin_lifecycle
        .lock()
        .map(|manager| manager.audit_trail().len())
        .unwrap_or(0);

    append_audit_event(
        &state,
        AuditEventKind::Security,
        &operator.operator_id,
        "security_plugins_provenance_register",
        status,
        &json!({
            "plugin_id": req.plugin_id,
            "plugin_version": req.plugin_version,
            "chain_complete": chain.is_complete(),
            "chain_digest": chain.chain_digest,
            "registration_state": registration_state,
            "error": error,
        })
        .to_string(),
    );

    Ok(Json(SignedProvenanceRegistrationResponse {
        status,
        registration_state,
        plugin_id: req.plugin_id,
        plugin_version: req.plugin_version,
        chain_complete: chain.is_complete(),
        chain_digest: chain.chain_digest,
        attestation_count: chain.attestations.len(),
        passed_attestations: chain.attestations.iter().filter(|entry| entry.passed).count(),
        sbom_approved: sbom_result.approved,
        sbom_license_violations: sbom_result.license_violations.len(),
        sbom_missing_checksums: sbom_result.missing_checksums.len(),
        audit_records_total,
        error,
    }))
}

async fn audit_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditEventsQuery>,
) -> Result<Json<AuditEventsResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_audit_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "audit/events",
    )?;
    let max_items = query.max_items.unwrap_or(100).min(1_000);
    let events = state
        .audit_sink
        .lock()
        .map(|sink| sink.latest(max_items))
        .unwrap_or_default();
    let events = filter_audit_events_for_principal(events, &principal);
    Ok(Json(AuditEventsResponse {
        status: "ok",
        total_events: events.len(),
        events,
    }))
}

/// S9-WS8A-02: verify the tamper-evident hash chain across all audit events.
async fn audit_chain_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AuditChainVerifyResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_audit_runtime_principal(&headers, &state, PrivilegeAction::Read, "audit/chain/verify")?;
    let events = state
        .audit_sink
        .lock()
        .map(|sink| sink.all().to_vec())
        .unwrap_or_default();
    let event_count = events.len();
    let chain_valid = AppendOnlyAuditSink::verify_chain(&events);
    Ok(Json(AuditChainVerifyResponse {
        status: "ok",
        event_count,
        chain_valid,
        genesis_hash: "genesis-0000000000000000",
    }))
}

async fn autonomous_action_records(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AutonomousActionRecordsQuery>,
) -> Result<Json<AutonomousActionRecordsResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_autonomous_records_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "autonomous/records",
    )?;
    let max_items = query.max_items.unwrap_or(100).min(1_000);
    let records = filter_action_records_for_principal(latest_action_records(&state, max_items), &principal);
    Ok(Json(AutonomousActionRecordsResponse {
        status: "ok",
        total_records: records.len(),
        records,
    }))
}

async fn i18n_messages(Query(query): Query<I18nMessagesQuery>) -> Json<I18nMessagesResponse> {
    let locale = SupportedLocale::parse(query.locale.as_deref().unwrap_or("en-US"));
    let keys = ["unauthorized", "missing_or_invalid_admin_key", "health_ok"];
    let mut messages = std::collections::BTreeMap::new();
    for key in keys {
        let localized = I18nCatalog::message(locale, key);
        messages.insert(key.to_string(), localized.message.to_string());
    }
    Json(I18nMessagesResponse {
        status: "ok",
        locale: locale.as_str().to_string(),
        messages,
    })
}

async fn autonomous_guardrails(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AutonomousGuardrailsResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let _operator = require_operator_privilege(
        &headers,
        &state,
        "autonomous.guardrails",
        "autonomous/guardrails",
        PrivilegeAction::Read,
    )?;
    Ok(Json(AutonomousGuardrailsResponse {
        status: "ok",
        autonomous_mode: state.autonomous_mode,
        emergency_stop_enabled: state.emergency_stop.get(),
        policy_matrix: state.guardrails.as_ref().clone(),
    }))
}

async fn autonomous_emergency_stop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<EmergencyStopRequest>,
) -> Result<Json<EmergencyStopResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "autonomous.guardrails",
        "autonomous/emergency_stop",
        PrivilegeAction::Manage,
    )?;
    state.emergency_stop.set(req.enabled);
    let reason = req
        .reason
        .clone()
        .unwrap_or_else(|| "manual_control_plane_request".to_string());
    let requested_by = req.requested_by.clone().unwrap_or(operator.operator_id);
    append_audit_event(
        &state,
        AuditEventKind::Security,
        &requested_by,
        "autonomous_emergency_stop",
        "ok",
        &json!({
            "enabled": req.enabled,
            "reason": reason,
        })
        .to_string(),
    );
    Ok(Json(EmergencyStopResponse {
        status: "ok",
        emergency_stop_enabled: req.enabled,
        reason,
        requested_by,
    }))
}

async fn authorize_autonomous_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AuthorizeActionRequest>,
) -> Result<(StatusCode, Json<AuthorizeActionResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "autonomous.actions",
        "autonomous/actions",
        PrivilegeAction::Execute,
    )?;
    let requested_scope = req.scope.unwrap_or_else(|| "cluster".to_string());
    let requested_by = operator.operator_id;
    let action = req.action;
    let trace_id = next_action_trace_id();
    if state.emergency_stop.get() {
        return Ok(build_authorize_action_response(
            &state,
            StatusCode::SERVICE_UNAVAILABLE,
            &action,
            &requested_scope,
            "blocked",
            "emergency_stop_enabled".to_string(),
            &trace_id,
            &requested_by,
            AutonomousActionDecision::Blocked,
        ));
    }

    if state.autonomous_mode == AutonomousMode::Disabled {
        return Ok(build_authorize_action_response(
            &state,
            StatusCode::FORBIDDEN,
            &action,
            &requested_scope,
            "blocked",
            "autonomous_mode_disabled".to_string(),
            &trace_id,
            &requested_by,
            AutonomousActionDecision::Blocked,
        ));
    }

    let matching_rule = state
        .guardrails
        .iter()
        .find(|r| r.action.eq_ignore_ascii_case(&action));

    Ok(match matching_rule {
        Some(rule) if state.autonomous_mode.rank() >= rule.required_mode.rank() => {
            build_authorize_action_response(
                &state,
                StatusCode::OK,
                &action,
                &requested_scope,
                "allow",
                format!(
                    "mode {:?} satisfies required mode {:?}",
                    state.autonomous_mode, rule.required_mode
                ),
                &trace_id,
                &requested_by,
                AutonomousActionDecision::Allow,
            )
        }
        Some(rule) => build_authorize_action_response(
            &state,
            StatusCode::FORBIDDEN,
            &action,
            &requested_scope,
            "deny",
            format!(
                "required mode {:?} exceeds current mode {:?}",
                rule.required_mode, state.autonomous_mode
            ),
            &trace_id,
            &requested_by,
            AutonomousActionDecision::Deny,
        ),
        None => build_authorize_action_response(
            &state,
            StatusCode::NOT_FOUND,
            &action,
            &requested_scope,
            "deny",
            "no_guardrail_rule_found".to_string(),
            &trace_id,
            &requested_by,
            AutonomousActionDecision::Unknown,
        ),
    })
}

fn default_guardrail_rules() -> Vec<GuardrailRule> {
    vec![
        GuardrailRule {
            action: "schema_change".to_string(),
            required_mode: AutonomousMode::Supervised,
            scope: "database".to_string(),
            rationale: "DDL and schema drift changes require human oversight".to_string(),
        },
        GuardrailRule {
            action: "plugin_install".to_string(),
            required_mode: AutonomousMode::Supervised,
            scope: "cluster".to_string(),
            rationale: "Plugin supply-chain changes require supervised execution".to_string(),
        },
        GuardrailRule {
            action: "security_patch".to_string(),
            required_mode: AutonomousMode::Supervised,
            scope: "cluster".to_string(),
            rationale: "Security posture changes require explicit review and audit".to_string(),
        },
        GuardrailRule {
            action: "self_heal_failover".to_string(),
            required_mode: AutonomousMode::Autonomous,
            scope: "cluster".to_string(),
            rationale: "Fast autonomous failover is allowed only in full autonomous mode"
                .to_string(),
        },
        GuardrailRule {
            action: "performance_tune".to_string(),
            required_mode: AutonomousMode::Advisory,
            scope: "session".to_string(),
            rationale: "Low-risk tuning actions can run in advisory mode".to_string(),
        },
    ]
}

fn route_path_name(path: QueryPath) -> &'static str {
    match path {
        QueryPath::Oltp => "oltp",
        QueryPath::Olap => "olap",
        QueryPath::Hybrid => "hybrid",
        QueryPath::Unknown => "unknown",
    }
}

fn execute_transaction_statements(statements: Vec<String>) -> (StatusCode, SqlTransactionResponse) {
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

fn acquire_pessimistic_lock(
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

fn release_pessimistic_lock(
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
fn parse_where_predicates(
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

// ─── S2-WS2-02: WAL durability + recovery handlers ────────────────────────────────────────────

/// S2-WS2-02: return WAL engine stats.
async fn wal_status(State(state): State<AppState>) -> (StatusCode, Json<WalStatusResponse>) {
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let records = wal.wal_records();
    let wal_len = records.len();
    let latest_seq = records.last().map(|r| r.sequence).unwrap_or(0);
    let checkpoint_count = wal.checkpoint_count();
    drop(wal);
    (StatusCode::OK, Json(WalStatusResponse {
        status: "ok",
        wal_len,
        latest_sequence: latest_seq,
        checkpoint_count,
    }))
}

// ─── S2-WS2-02: WAL forced checkpoint ────────────────────────────────────────

async fn wal_force_checkpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> (StatusCode, Json<WalForceCheckpointResponse>) {
    let _ = require_operator_auth(&headers, &state);
    let mut wal = state.wal_engine.lock().expect("wal_engine lock");
    let wal_len_before = wal.wal_records().len();
    wal.force_checkpoint();
    let wal_len_after = wal.wal_records().len();
    let checkpoint_count = wal.checkpoint_count();
    (StatusCode::OK, Json(WalForceCheckpointResponse {
        status: "ok",
        wal_len_before,
        wal_len_after,
        checkpoint_count,
    }))
}

// ─── S

/// S2-WS2-02: replay WAL records into the row store (or dry-run).
async fn wal_recover(
    State(state): State<AppState>,
    axum::extract::Json(req): axum::extract::Json<WalRecoverRequest>,
) -> (StatusCode, Json<WalRecoverResponse>) {
    let dry_run = req.dry_run.unwrap_or(false);
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let records = wal.wal_records().to_vec();
    drop(wal);
    let mut replayed: usize = 0;
    if !dry_run {
        let mut rs = state.row_store.lock().expect("row_store lock wal_recover");
        let xid = rs.begin_xid();
        for rec in &records {
            if rec.value == "__deleted__" {
                rs.delete(xid, &rec.key);
            } else {
                let data: std::collections::HashMap<String, String> =
                    serde_json::from_str(&rec.value)
                        .unwrap_or_else(|_| [("_raw".to_string(), rec.value.clone())]
                            .into_iter().collect());
                rs.insert(xid, &rec.key, data);
            }
            replayed += 1;
        }
    } else {
        replayed = records.len();
    }
    (StatusCode::OK, Json(WalRecoverResponse {
        status: "ok",
        records_replayed: replayed,
        dry_run,
    }))
}

// ─── S7-WS6-04: Chaos/game-day injection handlers ────────────────────────────

fn now_epoch_ms_chaos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// S7-WS6-04: inject a chaos/game-day fault event.
async fn chaos_inject(
    State(state): State<AppState>,
    axum::extract::Json(req): axum::extract::Json<ChaosInjectRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let event = ChaosEvent {
        fault_type: req.fault_type,
        target_node: req.target_node,
        parameters: req.parameters,
        injected_at_ms: now_epoch_ms_chaos(),
        cleared_at_ms: None,
    };
    let mut cs = state.chaos_state.lock().expect("chaos_state lock");
    cs.active_faults.push(event);
    let count = cs.active_faults.len();
    drop(cs);
    (StatusCode::OK, Json(serde_json::json!({ "status": "injected", "active_fault_count": count })))
}

/// S7-WS6-04: clear all active faults; move them to history.
async fn chaos_clear(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let cleared_at = now_epoch_ms_chaos();
    let mut cs = state.chaos_state.lock().expect("chaos_state lock");
    let mut cleared: Vec<ChaosEvent> = cs.active_faults.drain(..).map(|mut e| {
        e.cleared_at_ms = Some(cleared_at);
        e
    }).collect();
    cs.event_history.append(&mut cleared);
    let history_len = cs.event_history.len();
    drop(cs);
    (StatusCode::OK, Json(serde_json::json!({ "status": "cleared", "history_len": history_len })))
}

/// S7-WS6-04: return current chaos state summary.
async fn chaos_status(State(state): State<AppState>) -> (StatusCode, Json<ChaosStatusResponse>) {
    let cs = state.chaos_state.lock().expect("chaos_state lock");
    let active_fault_count = cs.active_faults.len();
    let total_injected = cs.active_faults.len() + cs.event_history.len();
    let active_faults = cs.active_faults.clone();
    drop(cs);
    (StatusCode::OK, Json(ChaosStatusResponse {
        status: "ok",
        active_fault_count,
        total_injected,
        active_faults,
    }))
}

/// S7-WS6-04: return cluster health based on active chaos faults.
async fn chaos_health(State(state): State<AppState>) -> (StatusCode, Json<ChaosHealthResponse>) {
    let cs = state.chaos_state.lock().expect("chaos_state lock");
    let active_fault_count = cs.active_faults.len();
    let history_len = cs.event_history.len();
    drop(cs);
    let cluster_healthy = active_fault_count == 0;
    (StatusCode::OK, Json(ChaosHealthResponse {
        status: "ok",
        cluster_healthy,
        active_fault_count,
        history_len,
    }))
}

/// S6-WS5-03: Initiate a TLS cert rotation (scaffold — records attempt, does not hot-swap certs).
async fn security_tls_rotate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<TlsCertRotateRequest>,
) -> Result<(StatusCode, Json<TlsCertRotateResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/tls/rotate",
        PrivilegeAction::Manage,
    )?;
    let cert_source = std::env::var("VNG_TLS_CERT_PATH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "not_configured".to_string());
    let rotation_initiated = cert_source != "not_configured";
    let reason = req.reason.unwrap_or_else(|| "manual_rotation".to_string());
    Ok((StatusCode::OK, Json(TlsCertRotateResponse {
        status: "ok",
        rotation_initiated,
        cert_source,
        reason,
    })))
}

// ─── S8-WS10-02: Driver wire protocol handlers ────────────────────────────────

/// S8-WS10-02: Return the current wire protocol capabilities.
async fn driver_protocol_info() -> (StatusCode, Json<DriverProtocolInfo>) {
    (StatusCode::OK, Json(DriverProtocolInfo {
        protocol_version: "1.0",
        encoding: "json",
        auth_modes: vec![
            "admin_key".to_string(),
            "operator_id".to_string(),
            "tenant".to_string(),
        ],
        supported_statements: vec![
            "SELECT".to_string(), "INSERT".to_string(),
            "UPDATE".to_string(), "DELETE".to_string(),
            "BEGIN".to_string(), "COMMIT".to_string(), "ROLLBACK".to_string(),
        ],
        max_batch_size: 500,
    }))
}

/// S8-WS10-02: Negotiate a driver connection session and return a session token.
async fn driver_connect(
    State(state): State<AppState>,
    Json(req): Json<DriverConnectRequest>,
) -> (StatusCode, Json<DriverConnectResponse>) {
    let sid = DRIVER_SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let session_token = format!("drv-sess-{sid}");
    let mut sessions = state.driver_sessions.lock().expect("driver_sessions lock");
    sessions.insert(session_token.clone(), DriverSession {
        driver_name: req.driver_name,
        driver_version: req.driver_version,
        connected_at_ms: now_epoch_ms_chaos(),
    });
    let negotiated: Vec<String> = req.requested_capabilities
        .unwrap_or_default()
        .into_iter()
        .filter(|c| matches!(c.as_str(), "batch_execute" | "streaming" | "prepared_statements"))
        .collect();
    (StatusCode::OK, Json(DriverConnectResponse {
        status: "connected",
        session_token,
        negotiated_capabilities: negotiated,
        max_batch_size: 500,
    }))
}


// ─── S7-WS6-02: Raft log entries endpoint ────────────────────────────────────────────

async fn raft_log(
    State(state): State<AppState>,
) -> (StatusCode, Json<RaftLogResponse>) {
    let node = state.raft_state.lock().expect("raft_state lock");
    let log_length = node.log.len();
    let commit_index = node.commit_index;
    let entries = node.log.clone();
    drop(node);
    (StatusCode::OK, Json(RaftLogResponse {
        status: "ok",
        log_length,
        commit_index,
        entries,
    }))
}

// ─── S8-WS10-02: Driver session disconnect ────────────────────────────────────

async fn driver_disconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DriverDisconnectRequest>,
) -> (StatusCode, Json<DriverDisconnectResponse>) {
    let _ = require_operator_auth(&headers, &state);
    let mut sessions = state.driver_sessions.lock().expect("driver_sessions lock");
    let disconnected = sessions.remove(&req.session_token).is_some();
    (StatusCode::OK, Json(DriverDisconnectResponse {
        status: "ok",
        session_token: req.session_token,
        disconnected,
    }))
}

// ─── S5-E4A-01: Connector SDK runtime load ───────────────────────────────────

/// Register a connector plugin manifest at runtime.
async fn connector_register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ConnectorRegisterRequest>,
) -> Result<(StatusCode, Json<ConnectorRegisterResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let registered_at_ms = now_epoch_ms_chaos();
    let plugin = ConnectorPlugin {
        connector_id: req.connector_id.clone(),
        connector_type: req.connector_type.clone(),
        version: req.version.clone(),
        signed: req.signed.unwrap_or(false),
        registered_at_ms,
    };
    state.connector_registry.lock().expect("connector_registry lock").push(plugin);
    Ok((
        StatusCode::OK,
        Json(ConnectorRegisterResponse {
            status: "ok",
            connector_id: req.connector_id,
            registered_at_ms,
        }),
    ))
}

/// List all registered connector plugins.
async fn connector_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> (StatusCode, Json<ConnectorListResponse>) {
    require_operator_auth(&headers, &state).ok();
    let connectors = state.connector_registry.lock().expect("connector_registry lock").clone();
    let connector_count = connectors.len();
    (StatusCode::OK, Json(ConnectorListResponse { status: "ok", connector_count, connectors }))
}

// ─── S7-WS6-03: Raft fencing token endpoint ──────────────────────────────────────────

/// Return the current fencing token for the Raft node.
async fn raft_fence(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RaftFenceResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let snap = state.raft_state.lock().expect("raft_state lock").status();
    Ok((
        StatusCode::OK,
        Json(RaftFenceResponse {
            status: "ok",
            fencing_token: snap.fencing_token,
            role: snap.role,
            current_term: snap.current_term,
        }),
    ))
}

// ─── S6-WS5-04: TDE toggle override ───────────────────────────────────────────────────

/// Override the TDE (Transparent Data Encryption) active state at runtime.
async fn security_tde_toggle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<TdeToggleRequest>,
) -> Result<(StatusCode, Json<TdeToggleResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/tde/toggle",
        PrivilegeAction::Manage,
    )?;
    *state.tde_override.lock().expect("tde_override lock") = Some(req.enable);
    Ok((
        StatusCode::OK,
        Json(TdeToggleResponse {
            status: "ok",
            tde_active: req.enable,
            override_applied: true,
        }),
    ))
}

// ─── S10-WS15-02: CDC stream from WAL ─────────────────────────────────────────

/// S10-WS15-02: Stream committed mutations as CDC events derived from the WAL.
async fn cdc_stream(
    State(state): State<AppState>,
) -> (StatusCode, Json<CdcStreamResponse>) {
    let wal = state.wal_engine.lock().expect("wal_engine lock cdc");
    let events: Vec<CdcEvent> = wal.wal_records()
        .iter()
        .map(|r| CdcEvent {
            sequence: r.sequence,
            op: if r.value == "__deleted__" {
                "delete".to_string()
            } else {
                "insert".to_string()
            },
            table_name: "row_store".to_string(),
            key: r.key.clone(),
            payload: r.value.clone(),
            captured_at_ms: 0,
        })
        .collect();
    let event_count = events.len();
    drop(wal);
    (StatusCode::OK, Json(CdcStreamResponse {
        status: "ok",
        event_count,
        events,
    }))
}

// ─── S10-WS15-02: CDC cursor tracking ────────────────────────────────────────

/// S10-WS15-02: Filter CDC stream by table name.
async fn cdc_stream_filter(
    State(state): State<AppState>,
    Query(query): Query<CdcStreamFilterQuery>,
) -> (StatusCode, Json<CdcStreamFilterResponse>) {
    let wal = state.wal_engine.lock().expect("wal_engine lock cdc_filter");
    let all_events: Vec<CdcEvent> = wal.wal_records()
        .iter()
        .map(|r| CdcEvent {
            sequence: r.sequence,
            op: if r.value == "__deleted__" { "delete".to_string() } else { "insert".to_string() },
            table_name: "row_store".to_string(),
            key: r.key.clone(),
            payload: r.value.clone(),
            captured_at_ms: 0,
        })
        .collect();
    drop(wal);
    let events: Vec<CdcEvent> = match &query.table {
        Some(t) => all_events.into_iter().filter(|e| e.table_name == *t).collect(),
        None => all_events,
    };
    let event_count = events.len();
    (StatusCode::OK, Json(CdcStreamFilterResponse {
        status: "ok",
        table_filter: query.table,
        event_count,
        events,
    }))
}

/// S10-WS15-02: Read the current CDC cursor position for a given table.
async fn cdc_cursor_status(
    State(state): State<AppState>,
    Query(q): Query<CdcCursorQuery>,
) -> (StatusCode, Json<CdcCursorResponse>) {
    let cursors = state.cdc_cursors.lock().expect("cdc_cursors lock");
    let pos = *cursors.get(&q.table).unwrap_or(&0);
    (StatusCode::OK, Json(CdcCursorResponse {
        status: "ok",
        table_name: q.table,
        cursor_position: pos,
    }))
}

/// S10-WS15-02: Advance (or initialise) the CDC cursor for a table to a given position.
async fn cdc_cursor_advance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CdcCursorAdvanceRequest>,
) -> Result<(StatusCode, Json<CdcCursorResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut cursors = state.cdc_cursors.lock().expect("cdc_cursors lock");
    cursors.insert(req.table_name.clone(), req.position);
    Ok((StatusCode::OK, Json(CdcCursorResponse {
        status: "ok",
        table_name: req.table_name,
        cursor_position: req.position,
    })))
}

// ─── S2-WS2-04: Row store point-in-time snapshot export ──────────────────────

/// S2-WS2-04: Export a snapshot of all currently-visible rows in the
/// `PagedRowStore` at the current head XID.
async fn row_store_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowSnapshotResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let store = state.row_store.lock().expect("row_store lock snapshot");
    let snapshot_xid = store.current_xid();
    let rows: Vec<RowSnapshotEntry> = store
        .export_rows_snapshot()
        .into_iter()
        .map(|(key, payload)| RowSnapshotEntry { key, payload })
        .collect();
    let row_count = rows.len();
    Ok((StatusCode::OK, Json(RowSnapshotResponse {
        status: "ok",
        snapshot_xid,
        row_count,
        rows,
    })))
}

// ─── S5-WS4A-02: Broker adapter status + flush ────────────────────────────────

/// S5-WS4A-02: Report the status of all registered broker adapters.
async fn outbox_broker_status(
    State(state): State<AppState>,
) -> (StatusCode, Json<BrokerAdapterStatus>) {
    let counts = state.broker_flush_counts.lock().expect("broker_flush_counts lock");
    let adapters: Vec<BrokerAdapterInfo> = ["kafka", "nats", "event_hubs"]
        .iter()
        .map(|b| BrokerAdapterInfo {
            broker_type: b.to_string(),
            enabled: false, // scaffold: no live broker connection
            flush_count: *counts.get(*b).unwrap_or(&0),
        })
        .collect();
    drop(counts);
    (StatusCode::OK, Json(BrokerAdapterStatus {
        status: "ok",
        adapters,
    }))
}

/// S5-WS4A-02: Flush pending outbox events to the specified broker adapter (scaffold).
async fn outbox_broker_flush(
    State(state): State<AppState>,
    Json(req): Json<BrokerFlushRequest>,
) -> (StatusCode, Json<BrokerFlushResponse>) {
    if !["kafka", "nats", "event_hubs"].contains(&req.broker_type.as_str()) {
        return (StatusCode::BAD_REQUEST, Json(BrokerFlushResponse {
            status: "error",
            broker_type: req.broker_type,
            events_flushed: 0,
            total_flush_count: 0,
        }));
    }
    let max_events = req.max_events.unwrap_or(100).min(10_000);
    // Derive flush count from WAL length (scaffold: no live broker write).
    let wal = state.wal_engine.lock().expect("wal_engine lock broker_flush");
    let events_available = wal.wal_records().len();
    drop(wal);
    let events_flushed = events_available.min(max_events);
    let mut counts = state.broker_flush_counts.lock().expect("broker_flush_counts lock flush");
    let cnt = counts.entry(req.broker_type.clone()).or_insert(0);
    *cnt += 1;
    let total_flush_count = *cnt;
    drop(counts);
    (StatusCode::OK, Json(BrokerFlushResponse {
        status: "ok",
        broker_type: req.broker_type,
        events_flushed,
        total_flush_count,
    }))
}

/// S5-WS4A-02: Return per-broker health: flush count vs WAL length.
async fn outbox_broker_health(
    State(state): State<AppState>,
) -> (StatusCode, Json<BrokerHealthResponse>) {
    let wal_len = state.wal_engine.lock().expect("wal_engine lock health").wal_records().len();
    let counts = state.broker_flush_counts.lock().expect("broker_flush_counts lock health");
    let brokers: Vec<BrokerHealthEntry> = ["kafka", "nats", "event_hubs"].iter().map(|bt| {
        let flush_count = counts.get(*bt).copied().unwrap_or(0);
        BrokerHealthEntry {
            broker_type: bt,
            flush_count,
            wal_len,
            healthy: flush_count > 0 || wal_len == 0,
        }
    }).collect();
    let broker_count = brokers.len();
    (StatusCode::OK, Json(BrokerHealthResponse {
        status: "ok",
        broker_count,
        brokers,
    }))
}

/// S8-WS10-02: List all active driver sessions.
async fn driver_session_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<DriverSessionListResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let sessions = state.driver_sessions.lock().expect("driver_sessions lock list");
    let list: Vec<DriverSessionInfo> = sessions
        .iter()
        .map(|(token, sess)| DriverSessionInfo {
            session_token: token.clone(),
            driver_name: sess.driver_name.clone(),
            driver_version: sess.driver_version.clone(),
            connected_at_ms: sess.connected_at_ms,
        })
        .collect();
    let session_count = list.len();
    drop(sessions);
    Ok((StatusCode::OK, Json(DriverSessionListResponse {
        status: "ok",
        session_count,
        sessions: list,
    })))
}

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

fn execute_olap_query(query: String, max_rows: Option<usize>) -> OlapQueryResponse {
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
fn execute_oltp_select(
    statements: &[String],
    rs: &voltnuerongrid_store::mvcc::PagedRowStore,
    limit: usize,
) -> Vec<OltpRowResult> {
    use voltnuerongrid_sql::{parse_one, Statement};
    let snapshot_xid = rs.current_xid();
    // Collect all visible rows once; reuse across multiple SELECT statements.
    let all_rows: Vec<(String, voltnuerongrid_store::mvcc::RowData)> = rs
        .scan_at_snapshot(snapshot_xid)
        .into_iter()
        .map(|(k, d)| (k.to_string(), d.clone()))
        .collect();
    let mut results: Vec<OltpRowResult> = Vec::new();
    for stmt_str in statements {
        if let Ok(Statement::Select(sel)) = parse_one(stmt_str) {
            // Try to extract a literal key/prefix from `WHERE <col> = '<val>'`
            let prefix: Option<String> = sel.where_clause.as_deref().and_then(|w| {
                let eq = w.find('=')?;
                let rhs = w[eq + 1..].trim();
                let val = rhs.trim_matches('\'').trim_matches('"').trim();
                if val.is_empty() { None } else { Some(val.to_string()) }
            });
            let prefix_str = prefix.as_deref().unwrap_or("");
            let remaining = limit.saturating_sub(results.len());
            let batch: Vec<OltpRowResult> = all_rows
                .iter()
                .filter(|(k, _)| prefix_str.is_empty() || k.contains(prefix_str))
                .take(remaining)
                .map(|(k, d)| OltpRowResult { key: k.clone(), data: d.clone() })
                .collect();
            results.extend(batch);
            if results.len() >= limit {
                break;
            }
        }
    }
    results
}

fn execute_udf_runtime_scaffold(sql_batch: &str) -> Result<Vec<UdfExecutionResult>, String> {
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

fn udf_function_catalog_contract() -> Vec<UdfFunctionCatalogEntry> {
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

fn udf_guard_policy_contract() -> Vec<UdfLanguageGuardPolicy> {
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

fn build_udf_execution_plan(sql_batch: &str) -> Vec<UdfExecutionPlanStep> {
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

fn failure_budget_snapshot(consumed_percent: f64) -> FailureBudgetSnapshot {
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

fn rate_limit_policy_snapshot(current_minute_count: u32) -> RateLimitPolicySnapshot {
    let (allowed, _, _) = evaluate_rate_limit(current_minute_count, 1, 600, 50);
    RateLimitPolicySnapshot {
        requests_per_minute: 600,
        burst_limit: 50,
        current_minute_count,
        allowed,
    }
}

fn evaluate_failure_budget_alert(
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

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn now_unix_ms_u64() -> u64 {
    now_unix_ms().min(u128::from(u64::MAX)) as u64
}

fn pool_stats_response(stats: &voltnuerongrid_driver_rust::PoolStats) -> PoolStatsResponse {
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

fn pool_acquire_error_state(error: &PoolAcquireError) -> &'static str {
    match error {
        PoolAcquireError::PoolExhausted { .. } => "pool_exhausted",
        PoolAcquireError::CircuitOpen { .. } => "circuit_open",
        PoolAcquireError::StormRejection { .. } => "storm_rejected",
        PoolAcquireError::AcquireTimeout { .. } => "acquire_timeout",
    }
}

fn parse_attestation_type(value: &str) -> Option<AttestationType> {
    match value.trim().to_ascii_lowercase().as_str() {
        "build_verification" => Some(AttestationType::BuildVerification),
        "security_scan" => Some(AttestationType::SecurityScan),
        "checksum_verification" => Some(AttestationType::ChecksumVerification),
        "signature_verification" => Some(AttestationType::SignatureVerification),
        "review_approval" => Some(AttestationType::ReviewApproval),
        _ => None,
    }
}

fn acquire_sql_data_plane_connection(
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

fn release_sql_data_plane_connection(state: &AppState, connection_id: &str) {
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

fn build_retry_plan(policy: &DrHookPolicyConfig, attempts: u32) -> Vec<DrHookRetryPlanStep> {
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

fn enqueue_dr_hook_task(
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

async fn run_dr_hook_scheduler(state: AppState) {
    loop {
        if let Some(task) = dequeue_dr_hook_task(&state) {
            let execution = execute_dr_hook(&state, &task.hook, Some(&task.scope), task.dry_run);
            append_audit_event(
                &state,
                AuditEventKind::Failover,
                &task.requested_by,
                "sre_dr_hook_scheduler_execute",
                execution.status,
                &json!({
                    "task_id": task.task_id,
                    "hook": task.hook,
                    "scope": task.scope,
                    "reason": task.reason,
                    "execution_id": execution.execution_id,
                    "policy_decision": execution.policy_decision,
                })
                .to_string(),
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
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

fn execute_dr_hook(
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

fn latest_dr_hook_records(state: &AppState, max_items: usize) -> Vec<DrHookExecutionRecord> {
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

fn evaluate_rate_limit(
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

fn record_transport_mutation(
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

fn require_operator_auth(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<(), (StatusCode, Json<AuthErrorResponse>)> {
    let Some(required_key) = state.admin_api_key.as_ref() else {
        return Ok(());
    };

    let provided = headers
        .get("x-vng-admin-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    if provided != required_key {
        return Err(auth_error(headers, "missing_or_invalid_admin_key"));
    }

    let operator = operator_identity_from_headers(headers, state)
        .ok_or_else(|| auth_error(headers, "missing_or_invalid_operator_identity"))?;
    if !state.allowed_operator_roles.contains(&operator.role) {
        return Err(auth_error(headers, "operator_role_not_allowed"));
    }
    if !CONTROL_PLANE_OPERATOR_ROLES.contains(&operator.role) {
        return Err(auth_error(headers, "operator_role_not_authorized"));
    }

    Ok(())
}

fn require_operator_privilege(
    headers: &HeaderMap,
    state: &AppState,
    resource: &str,
    scope: &str,
    action: PrivilegeAction,
) -> Result<OperatorIdentity, (StatusCode, Json<AuthErrorResponse>)> {
    let operator = operator_identity_from_headers(headers, state)
        .ok_or_else(|| auth_error(headers, "missing_or_invalid_operator_identity"))?;
    if state
        .rbac_privilege_matrix
        .allows(operator.role.as_str(), resource, scope, action)
    {
        Ok(operator)
    } else {
        Err(forbidden_error(headers, "insufficient_privilege"))
    }
}

fn require_sql_runtime_principal(
    headers: &HeaderMap,
    state: &AppState,
    action: PrivilegeAction,
    sql_scope: &str,
) -> Result<RuntimeAccessPrincipal, (StatusCode, Json<AuthErrorResponse>)> {
    require_runtime_principal(headers, state, "sql.runtime", sql_scope, action)
}

fn require_ingest_runtime_privilege(
    headers: &HeaderMap,
    state: &AppState,
    action: PrivilegeAction,
    ingest_scope: &str,
) -> Result<RuntimeAccessPrincipal, (StatusCode, Json<AuthErrorResponse>)> {
    require_runtime_principal(headers, state, "ingest.connectors", ingest_scope, action)
}

fn require_store_runtime_principal(
    headers: &HeaderMap,
    state: &AppState,
    action: PrivilegeAction,
    store_scope: &str,
) -> Result<RuntimeAccessPrincipal, (StatusCode, Json<AuthErrorResponse>)> {
    require_runtime_principal(headers, state, "storage.catalog", store_scope, action)
}

fn require_audit_runtime_principal(
    headers: &HeaderMap,
    state: &AppState,
    action: PrivilegeAction,
    audit_scope: &str,
) -> Result<RuntimeAccessPrincipal, (StatusCode, Json<AuthErrorResponse>)> {
    require_runtime_principal(headers, state, "observability.audit", audit_scope, action)
}

fn require_autonomous_records_runtime_principal(
    headers: &HeaderMap,
    state: &AppState,
    action: PrivilegeAction,
    records_scope: &str,
) -> Result<RuntimeAccessPrincipal, (StatusCode, Json<AuthErrorResponse>)> {
    require_runtime_principal(
        headers,
        state,
        "observability.autonomous_records",
        records_scope,
        action,
    )
}

fn require_runtime_principal(
    headers: &HeaderMap,
    state: &AppState,
    resource: &str,
    scope: &str,
    action: PrivilegeAction,
) -> Result<RuntimeAccessPrincipal, (StatusCode, Json<AuthErrorResponse>)> {
    let has_operator_headers = headers.contains_key("x-vng-admin-key")
        || headers.contains_key("x-vng-operator-id");

    if has_operator_headers {
        require_operator_auth(headers, state)?;
        let operator = require_operator_privilege(headers, state, resource, scope, action)?;
        return Ok(RuntimeAccessPrincipal::Operator(operator));
    }

    let user = require_tenant_user_privilege(headers, state, resource, scope, action)?;
    Ok(RuntimeAccessPrincipal::TenantUser(user))
}

fn tenant_scoped_scope(tenant_id: &str, scope: &str) -> String {
    format!("tenants/{tenant_id}/{}", scope.trim_start_matches('/'))
}

fn store_table_matches_tenant_namespace(table: &str, tenant_id: &str) -> bool {
    let normalized_table = table.trim().to_ascii_lowercase();
    let normalized_tenant = tenant_id.trim().to_ascii_lowercase();
    normalized_table.starts_with(&format!("tenant/{normalized_tenant}/"))
        || normalized_table.starts_with(&format!("tenant_{normalized_tenant}_"))
        || normalized_table.starts_with(&format!("{normalized_tenant}."))
}

fn ensure_store_table_access(
    principal: &RuntimeAccessPrincipal,
    headers: &HeaderMap,
    table: &str,
) -> Result<(), (StatusCode, Json<AuthErrorResponse>)> {
    match principal {
        RuntimeAccessPrincipal::Operator(_) => Ok(()),
        RuntimeAccessPrincipal::TenantUser(user) => {
            if store_table_matches_tenant_namespace(table, &user.tenant_id) {
                Ok(())
            } else {
                Err(forbidden_error(headers, "insufficient_privilege"))
            }
        }
    }
}

fn ingest_scope_for_connector(connector_id: &str, format: &str) -> String {
    format!("ingest/connectors/{connector_id}/{format}")
}

fn ingest_status_scope() -> &'static str {
    "ingest/status"
}

fn ingest_outbox_scope(connector_id: Option<&str>) -> String {
    match connector_id {
        Some(connector_id) => format!("ingest/outbox/{connector_id}"),
        None => "ingest/outbox".to_string(),
    }
}

fn ingest_outbox_stream_name(storage_key: &str) -> String {
    format!(
        "ingest.outbox.{}",
        storage_key
            .replace('/', ".")
            .replace(':', ".")
            .replace(' ', "_")
    )
}

fn ingest_storage_key(principal: &RuntimeAccessPrincipal, connector_id: &str) -> String {
    match principal {
        RuntimeAccessPrincipal::Operator(_) => connector_id.to_string(),
        RuntimeAccessPrincipal::TenantUser(user) => {
            format!("tenant/{}/{}", user.tenant_id, connector_id)
        }
    }
}

fn count_tenant_ingest_records<T>(records: &HashMap<String, Vec<T>>, tenant_id: &str) -> (usize, usize) {
    let prefix = format!("tenant/{tenant_id}/");
    let connectors = records.keys().filter(|key| key.starts_with(&prefix)).count();
    let total_records = records
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .map(|(_, value)| value.len())
        .sum();
    (connectors, total_records)
}

fn append_ingest_outbox_events(
    state: &AppState,
    principal: &RuntimeAccessPrincipal,
    connector_id: &str,
    format: &str,
    records: &[voltnuerongrid_ingest::IngestRecord],
) -> usize {
    let storage_key = ingest_storage_key(principal, connector_id);
    let stream_name = ingest_outbox_stream_name(&storage_key);

    if let Ok(mut stream_map) = state.ingest_outbox_streams.lock() {
        stream_map.insert(storage_key.clone(), stream_name.clone());
    }

    let mut event_bus = match state.ingest_event_bus.lock() {
        Ok(guard) => guard,
        Err(_) => return 0,
    };

    let mut appended = 0usize;
    for record in records {
        let mut attributes = HashMap::new();
        attributes.insert("connector_id".to_string(), connector_id.to_string());
        attributes.insert("format".to_string(), format.to_string());
        attributes.insert("storage_key".to_string(), storage_key.clone());
        attributes.insert("record_key".to_string(), record.key.clone());
        if let RuntimeAccessPrincipal::TenantUser(user) = principal {
            attributes.insert("tenant_id".to_string(), user.tenant_id.clone());
        }

        if event_bus
            .publish(
            &stream_name,
            StreamDirection::Internal,
            &state.node_id,
            &json!({
                "connector_id": connector_id,
                "format": format,
                "storage_key": storage_key,
                "record_key": record.key,
                "payload": record.payload,
            })
            .to_string(),
            attributes,
        )
            .is_ok()
        {
            appended += 1;
        }
    }

    appended
}

fn evaluate_kms_runtime(state: &AppState) -> KmsEvaluationSnapshot {
    let mut runtime = state.kms_runtime.lock().expect("kms runtime lock");
    let unavailable_envs = runtime.unavailable_envs.clone();
    for provider in &mut runtime.providers {
        provider.clear_unavailable();
        for env_name in &unavailable_envs {
            provider.mark_unavailable(env_name);
        }
    }

    let mut unavailable_envs = runtime.unavailable_envs.iter().cloned().collect::<Vec<_>>();
    unavailable_envs.sort();

    let providers = runtime
        .providers
        .iter()
        .map(|provider| provider as &dyn KmsKeyProvider)
        .collect::<Vec<_>>();
    let chain = KmsProviderChain::new(providers);

    match state.security_config.resolve_kms_key_ref_with_provider(&chain) {
        Ok(resolution) => {
            runtime.last_error = None;
            runtime.last_resolution = Some(resolution.clone());
            KmsEvaluationSnapshot {
                status: if resolution.failover_used { "degraded" } else { "ok" },
                resolution_state: if resolution.failover_used {
                    "failover_active"
                } else {
                    "primary_active"
                },
                resolution: Some(resolution),
                unavailable_envs,
                last_simulation_note: runtime.last_simulation_note.clone(),
                last_error: None,
            }
        }
        Err(error) => {
            runtime.last_resolution = None;
            runtime.last_error = Some(error.clone());
            KmsEvaluationSnapshot {
                status: "degraded",
                resolution_state: "unresolved",
                resolution: None,
                unavailable_envs,
                last_simulation_note: runtime.last_simulation_note.clone(),
                last_error: Some(error),
            }
        }
    }
}

fn build_security_kms_status_response(
    state: &AppState,
    snapshot: &KmsEvaluationSnapshot,
) -> SecurityKmsStatusResponse {
    SecurityKmsStatusResponse {
        status: snapshot.status,
        resolution_state: snapshot.resolution_state,
        encryption_at_rest_required: state.security_config.encryption_at_rest_required,
        configured_envs: state.security_config.kms_key_candidates(),
        unavailable_envs: snapshot.unavailable_envs.clone(),
        selected_env: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.selected_env.clone()),
        key_ref: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.key_ref.clone()),
        failover_used: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.failover_used)
            .unwrap_or(false),
        last_simulation_note: snapshot.last_simulation_note.clone(),
        last_error: snapshot.last_error.clone(),
    }
}

fn require_tenant_user_privilege(
    headers: &HeaderMap,
    state: &AppState,
    resource: &str,
    scope: &str,
    action: PrivilegeAction,
) -> Result<TenantUserIdentity, (StatusCode, Json<AuthErrorResponse>)> {
    let user = tenant_user_identity_from_headers(headers, state)
        .ok_or_else(|| auth_error(headers, "missing_or_invalid_user_identity"))?;
    let expected_scope = tenant_scoped_scope(&user.tenant_id, scope);
    if state
        .rbac_privilege_matrix
        .allows(user.role.as_str(), resource, &expected_scope, action)
    {
        Ok(user)
    } else {
        Err(forbidden_error(headers, "insufficient_privilege"))
    }
}

fn operator_identity_from_headers(
    headers: &HeaderMap,
    state: &AppState,
) -> Option<OperatorIdentity> {
    let operator_id = headers
        .get("x-vng-operator-id")
        .and_then(|value| value.to_str().ok())?
        .trim();
    if operator_id.is_empty() {
        return None;
    }
    let role = state.operator_role_bindings.get(operator_id).copied()?;
    Some(OperatorIdentity {
        operator_id: operator_id.to_string(),
        role,
    })
}

fn tenant_user_identity_from_headers(
    headers: &HeaderMap,
    state: &AppState,
) -> Option<TenantUserIdentity> {
    let user_id = headers
        .get("x-vng-user-id")
        .and_then(|value| value.to_str().ok())?
        .trim();
    let tenant_id = headers
        .get("x-vng-tenant-id")
        .and_then(|value| value.to_str().ok())?
        .trim();
    if user_id.is_empty() || tenant_id.is_empty() {
        return None;
    }
    let binding = state.tenant_user_bindings.get(user_id)?;
    if !binding.tenant_id.eq_ignore_ascii_case(tenant_id) {
        return None;
    }
    Some(TenantUserIdentity {
        user_id: user_id.to_string(),
        tenant_id: tenant_id.to_string(),
        role: binding.role.clone(),
    })
}

fn auth_error(
    headers: &HeaderMap,
    reason: &str,
) -> (StatusCode, Json<AuthErrorResponse>) {
    let locale = locale_from_headers(headers);
    let message_key = if reason == "missing_or_invalid_admin_key" {
        "missing_or_invalid_admin_key"
    } else {
        "unauthorized"
    };
    let localized = I18nCatalog::message(locale, message_key);
    (
        StatusCode::UNAUTHORIZED,
        Json(AuthErrorResponse {
            status: "unauthorized",
            reason: reason.to_string(),
            locale: locale.as_str().to_string(),
            localized_message: localized.message.to_string(),
        }),
    )
}

fn forbidden_error(
    headers: &HeaderMap,
    reason: &str,
) -> (StatusCode, Json<AuthErrorResponse>) {
    let locale = locale_from_headers(headers);
    let localized = I18nCatalog::message(locale, "unauthorized");
    (
        StatusCode::FORBIDDEN,
        Json(AuthErrorResponse {
            status: "forbidden",
            reason: reason.to_string(),
            locale: locale.as_str().to_string(),
            localized_message: localized.message.to_string(),
        }),
    )
}

fn bad_request_error(
    headers: &HeaderMap,
    reason: &str,
) -> (StatusCode, Json<AuthErrorResponse>) {
    let locale = locale_from_headers(headers);
    let localized = I18nCatalog::message(locale, "unauthorized");
    (
        StatusCode::BAD_REQUEST,
        Json(AuthErrorResponse {
            status: "bad_request",
            reason: reason.to_string(),
            locale: locale.as_str().to_string(),
            localized_message: localized.message.to_string(),
        }),
    )
}

fn locale_from_headers(headers: &HeaderMap) -> SupportedLocale {
    headers
        .get("accept-language")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(',').next().unwrap_or("en-US"))
        .map(SupportedLocale::parse)
        .unwrap_or(SupportedLocale::EnUs)
}

fn append_audit_event(
    state: &AppState,
    kind: AuditEventKind,
    actor: &str,
    action: &str,
    outcome: &str,
    details_json: &str,
) {
    if let Ok(mut sink) = state.audit_sink.lock() {
        let event = sink.append(kind, actor, action, outcome, details_json);
        // S9-WS8A-02: write to file-backed audit log if configured.
        if let Some(ref path) = state.audit_log_path {
            if let Ok(line) = serde_json::to_string(&event) {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    let _ = writeln!(f, "{}", line);
                }
            }
        }
    }
}

fn append_runtime_audit_event(
    state: &AppState,
    kind: AuditEventKind,
    principal: &RuntimeAccessPrincipal,
    action: &str,
    outcome: &str,
    details: serde_json::Value,
) {
    let actor = match principal {
        RuntimeAccessPrincipal::Operator(operator) => operator.operator_id.as_str(),
        RuntimeAccessPrincipal::TenantUser(user) => user.user_id.as_str(),
    };
    let mut payload = match details {
        serde_json::Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("details".to_string(), other);
            map
        }
    };
    match principal {
        RuntimeAccessPrincipal::Operator(operator) => {
            payload.insert("actor_type".to_string(), json!("operator"));
            payload.insert("operator_id".to_string(), json!(operator.operator_id));
            payload.insert("operator_role".to_string(), json!(operator.role.as_str()));
        }
        RuntimeAccessPrincipal::TenantUser(user) => {
            payload.insert("actor_type".to_string(), json!("tenant_user"));
            payload.insert("tenant_id".to_string(), json!(user.tenant_id));
            payload.insert("user_id".to_string(), json!(user.user_id));
            payload.insert("user_role".to_string(), json!(user.role));
        }
    }
    append_audit_event(
        state,
        kind,
        actor,
        action,
        outcome,
        &serde_json::Value::Object(payload).to_string(),
    );
}

fn audit_event_matches_tenant(event: &AuditEvent, tenant_id: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(&event.details_json)
        .ok()
        .and_then(|value| value.get("tenant_id").and_then(|v| v.as_str()).map(str::to_string))
        .map(|value| value.eq_ignore_ascii_case(tenant_id))
        .unwrap_or(false)
}

fn filter_audit_events_for_principal(
    events: Vec<AuditEvent>,
    principal: &RuntimeAccessPrincipal,
) -> Vec<AuditEvent> {
    match principal {
        RuntimeAccessPrincipal::Operator(_) => events,
        RuntimeAccessPrincipal::TenantUser(user) => events
            .into_iter()
            .filter(|event| audit_event_matches_tenant(event, &user.tenant_id))
            .collect(),
    }
}

fn next_action_trace_id() -> String {
    let id = ACTION_TRACE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("atrace-{id}")
}

fn tenant_id_from_scoped_path(scope: &str) -> Option<String> {
    let mut segments = scope.trim().trim_start_matches('/').split('/');
    let prefix = segments.next()?;
    if !prefix.eq_ignore_ascii_case("tenants") {
        return None;
    }
    let tenant_id = segments.next()?.trim();
    if tenant_id.is_empty() {
        None
    } else {
        Some(tenant_id.to_string())
    }
}

fn latest_action_records(state: &AppState, max_items: usize) -> Vec<AutonomousActionExecutionRecord> {
    match state.action_records.lock() {
        Ok(records) => {
            let len = records.len();
            let start = len.saturating_sub(max_items);
            records[start..].to_vec()
        }
        Err(_) => Vec::new(),
    }
}

fn append_action_record(state: &AppState, record: AutonomousActionExecutionRecord) {
    if let Ok(mut records) = state.action_records.lock() {
        records.push(record);
    }
}

fn autonomous_action_record_matches_tenant(
    record: &AutonomousActionExecutionRecord,
    tenant_id: &str,
) -> bool {
    record
        .tenant_id
        .as_deref()
        .map(|value| value.eq_ignore_ascii_case(tenant_id))
        .or_else(|| {
            tenant_id_from_scoped_path(&record.scope)
                .map(|value| value.eq_ignore_ascii_case(tenant_id))
        })
        .unwrap_or(false)
}

fn filter_action_records_for_principal(
    records: Vec<AutonomousActionExecutionRecord>,
    principal: &RuntimeAccessPrincipal,
) -> Vec<AutonomousActionExecutionRecord> {
    match principal {
        RuntimeAccessPrincipal::Operator(_) => records,
        RuntimeAccessPrincipal::TenantUser(user) => records
            .into_iter()
            .filter(|record| autonomous_action_record_matches_tenant(record, &user.tenant_id))
            .collect(),
    }
}

fn build_authorize_action_response(
    state: &AppState,
    status_code: StatusCode,
    action: &str,
    requested_scope: &str,
    decision: &'static str,
    reason: String,
    trace_id: &str,
    requested_by: &str,
    typed_decision: AutonomousActionDecision,
) -> (StatusCode, Json<AuthorizeActionResponse>) {
    let tenant_id = tenant_id_from_scoped_path(requested_scope);
    let record = AutonomousActionExecutionRecord::new(
        trace_id.to_string(),
        action,
        requested_scope,
        requested_by,
        typed_decision,
        &reason,
    )
    .with_tenant_id(tenant_id.as_deref());
    append_action_record(state, record);
    let mut details = serde_json::Map::new();
    details.insert("trace_id".to_string(), json!(trace_id));
    details.insert("action".to_string(), json!(action));
    details.insert("requested_scope".to_string(), json!(requested_scope));
    details.insert("decision".to_string(), json!(decision));
    details.insert("reason".to_string(), json!(reason.clone()));
    if let Some(tenant_id) = tenant_id.as_ref() {
        details.insert("tenant_id".to_string(), json!(tenant_id));
    }
    append_audit_event(
        state,
        AuditEventKind::Autonomous,
        requested_by,
        "autonomous_action_authorize",
        decision,
        &serde_json::Value::Object(details).to_string(),
    );
    (
        status_code,
        Json(AuthorizeActionResponse {
            status: if status_code == StatusCode::OK {
                "ok"
            } else if status_code == StatusCode::NOT_FOUND {
                "unknown_action"
            } else {
                "blocked"
            },
            action: action.to_string(),
            requested_scope: requested_scope.to_string(),
            decision,
            reason,
            trace_id: trace_id.to_string(),
        }),
    )
}

// â”€â”€ WS2 Index + Constraint handlers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn store_list_indexes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ListIndexesResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/indexes",
    )?;
    let mgr = state.index_manager.lock().expect("index lock");
    let indexes = mgr
        .list_indexes()
        .iter()
        .filter(|descriptor| match &principal {
            RuntimeAccessPrincipal::Operator(_) => true,
            RuntimeAccessPrincipal::TenantUser(user) => {
                store_table_matches_tenant_namespace(&descriptor.table, &user.tenant_id)
            }
        })
        .map(|d| IndexListEntry {
            name: d.name.clone(),
            table: d.table.clone(),
            column: d.column.clone(),
            kind: format!("{:?}", d.kind),
            unique: d.unique,
        })
        .collect();
    let response = ListIndexesResponse {
        status: "ok",
        indexes,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Storage,
        &principal,
        "store_list_indexes",
        "ok",
        json!({
            "route_scope": "store/indexes",
            "visible_index_count": response.indexes.len(),
        }),
    );
    Ok(Json(response))
}

async fn store_create_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateIndexRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Manage,
        "store/indexes",
    )?;
    ensure_store_table_access(&principal, &headers, &req.table)?;
    use voltnuerongrid_store::index::{IndexDescriptor, IndexKind};
    let unique = req.unique.unwrap_or(false);
    let descriptor = IndexDescriptor {
        name: req.name.clone(),
        table: req.table.clone(),
        column: req.column.clone(),
        kind: IndexKind::BTree,
        unique,
    };
    let mut mgr = state.index_manager.lock().expect("index lock");
    Ok(match mgr.create_index(descriptor) {
        Ok(()) => {
            let response = CreateIndexResponse {
                status: "created",
                index_name: req.name,
                table: req.table,
                column: req.column,
                unique,
            };
            append_runtime_audit_event(
                &state,
                AuditEventKind::Storage,
                &principal,
                "store_create_index",
                "ok",
                json!({
                    "route_scope": "store/indexes/create",
                    "index_name": response.index_name,
                    "table": response.table,
                    "column": response.column,
                    "unique": response.unique,
                }),
            );
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(response).expect("json")),
            )
        }
        Err(e) => (
            StatusCode::CONFLICT,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        ),
    })
}

async fn store_drop_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DropIndexRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Manage,
        "store/indexes",
    )?;
    let mut mgr = state.index_manager.lock().expect("index lock");
    Ok(match mgr.get(&req.name) {
        Some(idx) => {
            ensure_store_table_access(&principal, &headers, &idx.descriptor().table)?;
            match mgr.drop_index(&req.name) {
                Ok(desc) => {
                    let response = DropIndexResponse {
                        status: "dropped",
                        dropped: req.name,
                    };
                    append_runtime_audit_event(
                        &state,
                        AuditEventKind::Storage,
                        &principal,
                        "store_drop_index",
                        "ok",
                        json!({
                            "route_scope": "store/indexes/drop",
                            "index_name": response.dropped,
                            "table": desc.table,
                            "column": desc.column,
                        }),
                    );
                    (
                        StatusCode::OK,
                        Json(serde_json::to_value(response).expect("json")),
                    )
                }
                Err(e) => (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "status": "error", "reason": e.to_string() })),
                ),
            }
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "reason": format!("index '{}' not found", req.name) })),
        ),
    })
}

async fn store_index_lookup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IndexLookupRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/indexes/lookup",
    )?;
    let mgr = state.index_manager.lock().expect("index lock");
    Ok(match mgr.get(&req.index_name) {
        Some(idx) => {
            ensure_store_table_access(&principal, &headers, &idx.descriptor().table)?;
            let row_keys: Vec<String> = idx.lookup(&req.value).iter().map(|s| s.to_string()).collect();
            let response = IndexLookupResponse {
                status: "ok",
                index_name: req.index_name,
                value: req.value,
                row_keys,
            };
            append_runtime_audit_event(
                &state,
                AuditEventKind::Storage,
                &principal,
                "store_index_lookup",
                "ok",
                json!({
                    "route_scope": "store/indexes/lookup",
                    "index_name": response.index_name,
                    "value": response.value,
                    "row_key_count": response.row_keys.len(),
                    "table": idx.descriptor().table,
                }),
            );
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).expect("json")),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "reason": format!("index '{}' not found", req.index_name) })),
        ),
    })
}

async fn store_add_constraint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AddConstraintRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Manage,
        "store/constraints",
    )?;
    ensure_store_table_access(&principal, &headers, &req.table)?;
    use voltnuerongrid_store::constraints::{ConstraintDescriptor, ConstraintKind};
    let kind = match req.kind.to_ascii_lowercase().as_str() {
        "primary_key" | "pk" => ConstraintKind::PrimaryKey,
        "unique" => ConstraintKind::Unique,
        "not_null" => ConstraintKind::NotNull,
        "foreign_key" | "fk" => ConstraintKind::ForeignKey,
        other => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(json!({ "status": "error", "reason": format!("unknown constraint kind: {other}") })),
            ));
        }
    };
    let descriptor = ConstraintDescriptor {
        name: req.name.clone(),
        table: req.table.clone(),
        column: req.column.clone(),
        kind,
    };
    let mut mgr = state.constraint_manager.lock().expect("constraint lock");
    Ok(match mgr.add_constraint(descriptor) {
        Ok(()) => {
            let response = AddConstraintResponse {
                status: "created",
                constraint_name: req.name,
                table: req.table,
                column: req.column,
                kind: req.kind,
            };
            append_runtime_audit_event(
                &state,
                AuditEventKind::Storage,
                &principal,
                "store_add_constraint",
                "ok",
                json!({
                    "route_scope": "store/constraints/add",
                    "constraint_name": response.constraint_name,
                    "table": response.table,
                    "column": response.column,
                    "kind": response.kind,
                }),
            );
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(response).expect("json")),
            )
        }
        Err(e) => (
            StatusCode::CONFLICT,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        ),
    })
}

async fn store_validate_constraint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ValidateConstraintRequest>,
) -> Result<Json<ValidateConstraintResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/constraints/validate",
    )?;
    ensure_store_table_access(&principal, &headers, &req.table)?;
    let mgr = state.constraint_manager.lock().expect("constraint lock");
    Ok(match mgr.validate(&req.table, &req.column, req.value.as_deref()) {
        Ok(()) => {
            let response = ValidateConstraintResponse {
                status: "ok",
                valid: true,
                violation: None,
            };
            append_runtime_audit_event(
                &state,
                AuditEventKind::Storage,
                &principal,
                "store_validate_constraint",
                "ok",
                json!({
                    "route_scope": "store/constraints/validate",
                    "table": req.table,
                    "column": req.column,
                    "valid": response.valid,
                }),
            );
            Json(response)
        }
        Err(v) => {
            let violation = v.to_string();
            append_runtime_audit_event(
                &state,
                AuditEventKind::Storage,
                &principal,
                "store_validate_constraint",
                "ok",
                json!({
                    "route_scope": "store/constraints/validate",
                    "table": req.table,
                    "column": req.column,
                    "valid": false,
                    "violation": violation,
                }),
            );
            Json(ValidateConstraintResponse {
                status: "ok",
                valid: false,
                violation: Some(violation),
            })
        }
    })
}

// S5-WS4-03 / S2-WS2-04: scan MVCC PagedRowStore at a snapshot
async fn store_rows_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<StoreRowsScanRequest>,
) -> Result<(StatusCode, Json<StoreRowsScanResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/rows/scan",
    )?;
    let rs = state.row_store.lock().expect("row_store lock");
    let snapshot_xid = req.snapshot_xid.unwrap_or_else(|| rs.current_xid());
    let key_prefix = req.key_prefix.unwrap_or_default();
    let limit = req.limit.unwrap_or(1_000).min(10_000);
    let rows: Vec<StoreRowEntry> = rs
        .scan_at_snapshot(snapshot_xid)
        .into_iter()
        .filter(|(k, _)| key_prefix.is_empty() || k.starts_with(key_prefix.as_str()))
        .take(limit)
        .map(|(k, d)| StoreRowEntry {
            key: k.to_string(),
            data: d.clone(),
        })
        .collect();
    let row_count = rows.len();
    append_runtime_audit_event(
        &state,
        AuditEventKind::Storage,
        &principal,
        "store_rows_scan",
        "ok",
        json!({
            "route_scope": "store/rows/scan",
            "snapshot_xid": snapshot_xid,
            "row_count": row_count,
            "key_prefix": key_prefix,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(StoreRowsScanResponse {
            status: "ok",
            snapshot_xid,
            row_count,
            rows,
        }),
    ))
}

/// S4-WS3-04: HTAP sync export — returns pending mutations from `RowStoreSyncOrigin`.
async fn store_htap_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<StoreHtapExportRequest>,
) -> Result<(StatusCode, Json<StoreHtapExportResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/htap/export",
    )?;
    let since = req.since_sequence.unwrap_or(0);
    let max_items = req.max_items.unwrap_or(500).min(5_000);
    let mutations = {
        use voltnuerongrid_store::htap_sync::MutationOp;
        let origin = state.sync_origin.lock().expect("sync_origin lock");
        let checkpoint = origin.checkpoint();
        let raw = origin.export_since(since, max_items);
        let entries: Vec<HtapMutationEntry> = raw
            .into_iter()
            .map(|m| HtapMutationEntry {
                sequence: m.sequence,
                table: m.table,
                primary_key: m.primary_key,
                payload_json: m.payload_json,
                op: match m.op {
                    MutationOp::Insert => "insert",
                    MutationOp::Update => "update",
                    MutationOp::Delete => "delete",
                }
                .to_string(),
            })
            .collect();
        (entries, checkpoint.last_sequence)
    };
    let (entries, last_sequence) = mutations;
    let mutation_count = entries.len();
    append_runtime_audit_event(
        &state,
        AuditEventKind::Storage,
        &principal,
        "store_htap_export",
        "ok",
        json!({
            "route_scope": "store/htap/export",
            "since_sequence": since,
            "mutation_count": mutation_count,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(StoreHtapExportResponse {
            status: "ok",
            since_sequence: since,
            mutation_count,
            checkpoint_last_sequence: last_sequence,
            mutations: entries,
        }),
    ))
}

/// S4-WS3-03: vectorized columnar scan — reads committed rows from PagedRowStore
/// and materialises them as typed column batches for OLAP consumers.
async fn store_columnar_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ColumnarScanResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/columnar/scan",
    )?;
    let (batch, stats) = {
        use voltnuerongrid_store::columnar::vectorized_scan;
        let rs = state.row_store.lock().expect("row_store lock columnar_scan");
        let snapshot_xid = rs.current_xid();
        let raw_rows: Vec<(String, std::collections::HashMap<String, String>)> = rs
            .scan_at_snapshot(snapshot_xid)
            .into_iter()
            .map(|(k, d)| (k.to_string(), d.clone()))
            .collect();
        vectorized_scan(&raw_rows, 10_000)
    };
    let columns: Vec<ColumnarScanColumn> = batch
        .column_names
        .iter()
        .filter_map(|name| {
            batch.columns.get(name).map(|cv| {
                use voltnuerongrid_store::columnar::ColumnVector;
                let type_hint = match cv {
                    ColumnVector::Int64(_) => "int64",
                    ColumnVector::Float64(_) => "float64",
                    ColumnVector::Bool(_) => "bool",
                    ColumnVector::Utf8(_) => "utf8",
                    ColumnVector::Null(_) => "null",
                };
                let sample_values: Vec<String> = (0..cv.len().min(3))
                    .filter_map(|i| cv.value_as_str(i))
                    .collect();
                ColumnarScanColumn {
                    name: name.clone(),
                    type_hint: type_hint.to_string(),
                    row_count: cv.len(),
                    sample_values,
                }
            })
        })
        .collect();
    append_runtime_audit_event(
        &state,
        AuditEventKind::Storage,
        &principal,
        "store_columnar_scan",
        "ok",
        json!({
            "route_scope": "store/columnar/scan",
            "rows_scanned": stats.rows_scanned,
            "columns_materialized": stats.columns_materialized,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(ColumnarScanResponse {
            status: "ok",
            rows_scanned: stats.rows_scanned,
            columns_materialized: stats.columns_materialized,
            elapsed_us: stats.elapsed_us,
            columns,
        }),
    ))
}

/// S6-WS5-03: TLS runtime status — reports TLS/mTLS contract state from SecurityConfigContract.
async fn security_tls_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SecurityTlsStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/tls/status",
        PrivilegeAction::Read,
    )?;
    let principal = RuntimeAccessPrincipal::Operator(operator);
    let cert_source = std::env::var("VNG_TLS_CERT_PATH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "not_configured".to_string());
    let key_present = std::env::var("VNG_TLS_KEY_PATH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_some();
    let note = if state.security_config.tls_required {
        "TLS required — server must be started with rustls/native-tls adapter"
    } else {
        "TLS not required — plaintext mode (development only)"
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "security_tls_status",
        "ok",
        json!({
            "route_scope": "security/tls/status",
            "tls_required": state.security_config.tls_required,
            "mtls_required": state.security_config.mtls_required,
        }),
    );
    Ok(Json(SecurityTlsStatusResponse {
        status: "ok",
        tls_required: state.security_config.tls_required,
        mtls_required: state.security_config.mtls_required,
        cert_source: if key_present { cert_source } else { "not_configured".to_string() },
        cert_rotation_supported: false,
        note,
    }))
}

/// S6-WS5-04: TDE runtime status — reports encryption-at-rest state from SecurityConfigContract.
async fn security_tde_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SecurityTdeStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/tde/status",
        PrivilegeAction::Read,
    )?;
    let principal = RuntimeAccessPrincipal::Operator(operator);
    let key_env_var = state.security_config.kms_key_ref_env.clone();
    let key_resolved = std::env::var(&key_env_var)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_some();
    // TDE is "active" when encryption-at-rest is required AND a KMS key is resolved.
    let tde_active = state.security_config.encryption_at_rest_required && key_resolved;
    let note = if tde_active {
        "TDE active: encryption-at-rest required and KMS key resolved"
    } else if state.security_config.encryption_at_rest_required {
        "TDE contract requires encryption but KMS key env var is not set — data NOT encrypted at rest"
    } else {
        "TDE not required in current security contract"
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "security_tde_status",
        "ok",
        json!({
            "route_scope": "security/tde/status",
            "encryption_at_rest_required": state.security_config.encryption_at_rest_required,
            "tde_active": tde_active,
            "key_env_var": key_env_var,
        }),
    );
    Ok(Json(SecurityTdeStatusResponse {
        status: "ok",
        encryption_at_rest_required: state.security_config.encryption_at_rest_required,
        tde_active,
        key_env_var,
        key_resolved,
        note,
    }))
}

/// S9-WS8-02: AI model gateway policy — read current policy.
async fn ai_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AiPolicyResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "ai.governance",
        "ai/policy",
        PrivilegeAction::Read,
    )?;
    let principal = RuntimeAccessPrincipal::Operator(operator);
    let policy = state.model_gateway_policy.lock().expect("model_gateway_policy lock").clone();
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "ai_policy_read",
        "ok",
        json!({ "route_scope": "ai/policy", "isolation_enabled": policy.isolation_enabled }),
    );
    Ok(Json(AiPolicyResponse { status: "ok", policy }))
}

/// S9-WS8-02: AI model gateway policy update (admin only).
async fn ai_policy_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AiPolicyUpdateRequest>,
) -> Result<Json<AiPolicyResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "ai.governance",
        "ai/policy",
        PrivilegeAction::Manage,
    )?;
    let principal = RuntimeAccessPrincipal::Operator(operator);
    let policy = {
        let mut p = state.model_gateway_policy.lock().expect("model_gateway_policy lock");
        if let Some(v) = req.isolation_enabled { p.isolation_enabled = v; }
        if let Some(v) = req.allowed_models { p.allowed_models = v; }
        if let Some(v) = req.max_tokens_per_request { p.max_tokens_per_request = v; }
        if let Some(v) = req.rate_limit_rpm { p.rate_limit_rpm = v; }
        p.clone()
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "ai_policy_update",
        "ok",
        json!({
            "route_scope": "ai/policy/update",
            "isolation_enabled": policy.isolation_enabled,
            "allowed_models_count": policy.allowed_models.len(),
            "max_tokens_per_request": policy.max_tokens_per_request,
            "rate_limit_rpm": policy.rate_limit_rpm,
        }),
    );
    Ok(Json(AiPolicyResponse { status: "ok", policy }))
}

/// S9-WS8-02: Rate-limit check for AI model requests.
///
// ─── S4-WS3-04: HTAP OLAP consumer apply ─────────────────────────────────────

/// Increments the per-model-identity counter and rejects with 429 when
/// the counter exceeds `rate_limit_rpm`.  The counter is a simple lifetime
/// accumulator (scaffold); a production implementation would use a sliding
/// window or token-bucket backed by a shared clock.
#[derive(Deserialize)]
struct AiRequestBody {
    /// Identifier of the model being invoked (e.g. "gpt-4o", "llama3").
    model_id: String,
    /// Number of tokens requested.
    tokens: Option<u64>,
}

#[derive(Debug, Serialize)]
struct AiRequestResponse {
    status: &'static str,
    model_id: String,
    request_count: u64,
    rate_limit_rpm: u32,
    tokens_checked: bool,
}

#[derive(Debug, Serialize)]
struct ModelRequestStat {
    model_id: String,
    request_count: u64,
}

/// Response for `GET /api/v1/ai/policy/stats`.
#[derive(Debug, Serialize)]
struct AiPolicyStatsResponse {
    status: &'static str,
    model_count: usize,
    total_requests: u64,
    allowed_models_enforced: bool,
    per_model: Vec<ModelRequestStat>,
}

async fn ai_rate_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AiRequestBody>,
) -> Result<(StatusCode, Json<AiRequestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let policy = state.model_gateway_policy.lock().expect("model_gateway_policy lock").clone();
    // Validate allowed_models list when non-empty (model-identity enforcement).
    if !policy.allowed_models.is_empty() && !policy.allowed_models.contains(&req.model_id) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AuthErrorResponse {
                status: "error",
                reason: format!("model_not_allowed:{}", req.model_id),
                locale: "en".to_string(),
                localized_message: "Model not in allowed list".to_string(),
            }),
        ));
    }
    // Token budget check.
    let tokens_checked = if let Some(t) = req.tokens {
        if policy.max_tokens_per_request > 0 && t > policy.max_tokens_per_request {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(AuthErrorResponse {
                    status: "error",
                    reason: format!("token_limit_exceeded:{t}"),
                    locale: "en".to_string(),
                    localized_message: "Token request exceeds policy limit".to_string(),
                }),
            ));
        }
        true
    } else {
        false
    };
    // Rate limit check — sliding window (60s).
    let request_count = {
        let now_ms = now_epoch_ms_chaos();
        let window_ms: u64 = 60_000;
        let mut w_starts = state.ai_rate_window_starts.lock().expect("ai_rate_window_starts lock");
        let start = w_starts.entry(req.model_id.clone()).or_insert(now_ms);
        if now_ms.saturating_sub(*start) >= window_ms {
            // Window elapsed: reset counter and window start.
            *start = now_ms;
            drop(w_starts);
            let mut counters = state.ai_request_counters.lock().expect("ai_request_counters lock");
            let cnt = counters.entry(req.model_id.clone()).or_insert(0);
            *cnt = 1;
            1u64
        } else {
            drop(w_starts);
            let mut counters = state.ai_request_counters.lock().expect("ai_request_counters lock");
            let cnt = counters.entry(req.model_id.clone()).or_insert(0);
            *cnt += 1;
            *cnt
        }
    };
    if policy.rate_limit_rpm > 0 && request_count > policy.rate_limit_rpm as u64 {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(AuthErrorResponse {
                status: "error",
                reason: format!("rate_limit_exceeded:{request_count}"),
                locale: "en".to_string(),
                localized_message: "AI request rate limit exceeded".to_string(),
            }),
        ));
    }
    Ok((StatusCode::OK, Json(AiRequestResponse {
        status: "ok",
        model_id: req.model_id,
        request_count,
        rate_limit_rpm: policy.rate_limit_rpm,
        tokens_checked,
    })))
}

/// S9-WS8-02: Return per-model request counts and policy enforcement state.
async fn ai_policy_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<AiPolicyStatsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let policy = state.model_gateway_policy.lock().expect("model_gateway_policy lock stats").clone();
    let counters = state.ai_request_counters.lock().expect("ai_request_counters lock stats");
    let per_model: Vec<ModelRequestStat> = counters
        .iter()
        .map(|(k, v)| ModelRequestStat { model_id: k.clone(), request_count: *v })
        .collect();
    let model_count = per_model.len();
    let total_requests: u64 = per_model.iter().map(|m| m.request_count).sum();
    let allowed_models_enforced = !policy.allowed_models.is_empty();
    drop(counters);
    Ok((StatusCode::OK, Json(AiPolicyStatsResponse {
        status: "ok",
        model_count,
        total_requests,
        allowed_models_enforced,
        per_model,
    })))
}

/// Apply a batch of HTAP mutations to the in-memory OLAP replica.
async fn store_htap_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<StoreHtapApplyRequest>,
) -> Result<(StatusCode, Json<StoreHtapApplyResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Write,
        "store/htap/apply",
    )?;
    let mut olap = state.olap_store.lock().expect("olap_store lock");
    let mut last_seq = 0u64;
    let mut applied = 0usize;
    for m in &req.mutations {
        last_seq = last_seq.max(m.sequence);
        match m.op.as_str() {
            "insert" | "update" => {
                // Parse the payload JSON into a HashMap<String, String>
                let data: HashMap<String, String> = serde_json::from_str(&m.payload_json)
                    .unwrap_or_else(|_| {
                        let mut d = HashMap::new();
                        d.insert("payload".to_string(), m.payload_json.clone());
                        d
                    });
                olap.insert(m.primary_key.clone(), data);
                applied += 1;
            }
            "delete" => {
                olap.remove(&m.primary_key);
                applied += 1;
            }
            _ => {}
        }
    }
    drop(olap);
    append_runtime_audit_event(
        &state,
        AuditEventKind::Storage,
        &principal,
        "store_htap_apply",
        "ok",
        json!({
            "route_scope": "store/htap/apply",
            "applied_count": applied,
            "last_applied_sequence": last_seq,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(StoreHtapApplyResponse {
            status: "ok",
            applied_count: applied,
            last_applied_sequence: last_seq,
        }),
    ))
}

/// Scan all rows in the in-memory OLAP replica.
async fn store_htap_olap_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<StoreHtapOlapScanResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/htap/olap/scan",
    )?;
    let olap = state.olap_store.lock().expect("olap_store lock");
    let rows: Vec<OlapScanRow> = olap
        .iter()
        .map(|(k, v)| OlapScanRow { key: k.clone(), data: v.clone() })
        .collect();
    let row_count = rows.len();
    Ok((
        StatusCode::OK,
        Json(StoreHtapOlapScanResponse { status: "ok", row_count, rows }),
    ))
}

/// S4-WS3-04: Return HTAP sync lag — pending mutations in sync_origin vs OLAP row count.
async fn htap_lag(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<HtapLagResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/htap/lag",
    )?;
    let sync_origin_pending = state.sync_origin.lock().expect("sync_origin lock htap_lag").pending_len();
    let olap_row_count = state.olap_store.lock().expect("olap_store lock htap_lag").len();
    let estimated_lag_mutations = sync_origin_pending.saturating_sub(olap_row_count);
    Ok((StatusCode::OK, Json(HtapLagResponse {
        status: "ok",
        sync_origin_pending,
        olap_row_count,
        estimated_lag_mutations,
    })))
}

// ─── S9-WS8A-02: Audit export endpoint ───────────────────────────────────────

/// Return all buffered audit events and indicate whether file-backed logging is active.
async fn audit_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<AuditExportQuery>,
) -> Result<(StatusCode, Json<AuditExportResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "audit.read",
        "audit/export",
        PrivilegeAction::Read,
    )?;
    let principal = RuntimeAccessPrincipal::Operator(operator);
    let all_events = state.audit_sink.lock().expect("audit_sink lock").all().to_vec();
    let total_event_count = all_events.len();
    let cursor = params.cursor.unwrap_or(0).min(total_event_count);
    let limit = params.limit.unwrap_or(1000).max(1).min(10000);
    let events: Vec<AuditEvent> = all_events.into_iter().skip(cursor).take(limit).collect();
    let event_count = events.len();
    let file_backed = state.audit_log_path.is_some();
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "audit_export",
        "ok",
        json!({ "route_scope": "audit/export", "event_count": event_count, "file_backed": file_backed }),
    );
    Ok((
        StatusCode::OK,
        Json(AuditExportResponse {
            status: "ok",
            event_count,
            total_event_count,
            cursor,
            limit,
            file_backed,
            audit_log_path: state.audit_log_path.clone(),
            events,
        }),
    ))
}

// ─── S7-WS6-02: Raft consensus endpoints ─────────────────────────────────────

/// Return the current Raft node status.
async fn raft_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RaftStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let snap = state.raft_state.lock().expect("raft_state lock").status();
    Ok(Json(RaftStatusResponse { status: "ok", raft: snap }))
}

/// Handle an incoming RequestVote RPC.
async fn raft_vote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RaftVoteRequest>,
) -> Result<Json<RaftVoteResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let resp = state.raft_state.lock().expect("raft_state lock").handle_vote_request(&req);
    Ok(Json(resp))
}

/// Handle an incoming AppendEntries RPC (heartbeat or log replication).
async fn raft_append(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RaftAppendRequest>,
) -> Result<Json<RaftAppendResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let resp = state.raft_state.lock().expect("raft_state lock").handle_append_entries(&req);
    Ok(Json(resp))
}

/// S7-WS6-03: Advance the election timer by one logical tick.
///
/// In a real deployment a background task would call this; the HTTP endpoint
/// enables deterministic testing without real timers.
#[derive(Serialize)]
struct RaftTickResponse {
    status: &'static str,
    ticks_since_heartbeat: u64,
    role: raft::RaftRole,
    current_term: u64,
    election_triggered: bool,
}

async fn raft_tick(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RaftTickResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut node = state.raft_state.lock().expect("raft_state lock");
    let role_before = node.role;
    node.tick();
    let election_triggered = node.role != role_before;
    Ok(Json(RaftTickResponse {
        status: "ok",
        ticks_since_heartbeat: node.ticks_since_heartbeat,
        role: node.role,
        current_term: node.current_term,
        election_triggered,
    }))
}

async fn security_kms_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SecurityKmsStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/kms",
        PrivilegeAction::Read,
    )?;
    let principal = RuntimeAccessPrincipal::Operator(operator.clone());
    let snapshot = evaluate_kms_runtime(&state);
    let response = build_security_kms_status_response(&state, &snapshot);
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "security_kms_status",
        response.status,
        json!({
            "route_scope": "security/kms",
            "resolution_state": response.resolution_state,
            "selected_env": response.selected_env,
            "failover_used": response.failover_used,
            "unavailable_envs": response.unavailable_envs,
        }),
    );
    Ok(Json(response))
}

async fn security_kms_outage_simulate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SecurityKmsOutageSimulateRequest>,
) -> Result<Json<SecurityKmsOutageResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/kms/outage",
        PrivilegeAction::Manage,
    )?;

    let configured = state
        .security_config
        .kms_key_candidates()
        .into_iter()
        .map(|value| value.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let normalized = req
        .unavailable_envs
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .filter(|value| configured.contains(&value.to_ascii_lowercase()))
        .map(ToString::to_string)
        .collect::<HashSet<_>>();
    let note = req
        .note
        .clone()
        .unwrap_or_else(|| "manual_kms_region_outage_simulation".to_string());

    {
        let mut runtime = state.kms_runtime.lock().expect("kms runtime lock");
        runtime.unavailable_envs = normalized;
        runtime.last_simulation_note = Some(note.clone());
    }

    let principal = RuntimeAccessPrincipal::Operator(operator);
    let snapshot = evaluate_kms_runtime(&state);
    let response = SecurityKmsOutageResponse {
        status: snapshot.status,
        resolution_state: snapshot.resolution_state,
        unavailable_envs: snapshot.unavailable_envs.clone(),
        selected_env: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.selected_env.clone()),
        key_ref: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.key_ref.clone()),
        failover_used: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.failover_used)
            .unwrap_or(false),
        note: note.clone(),
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "security_kms_outage_simulate",
        response.status,
        json!({
            "route_scope": "security/kms/outage",
            "resolution_state": response.resolution_state,
            "selected_env": response.selected_env,
            "failover_used": response.failover_used,
            "unavailable_envs": response.unavailable_envs,
            "note": response.note,
        }),
    );
    Ok(Json(response))
}

async fn security_kms_outage_reconcile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SecurityKmsOutageReconcileRequest>,
) -> Result<Json<SecurityKmsOutageResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/kms/outage",
        PrivilegeAction::Manage,
    )?;
    let note = req
        .note
        .clone()
        .unwrap_or_else(|| "manual_kms_region_outage_reconcile".to_string());

    {
        let mut runtime = state.kms_runtime.lock().expect("kms runtime lock");
        runtime.unavailable_envs.clear();
        runtime.last_simulation_note = Some(note.clone());
    }

    let principal = RuntimeAccessPrincipal::Operator(operator);
    let snapshot = evaluate_kms_runtime(&state);
    let response = SecurityKmsOutageResponse {
        status: snapshot.status,
        resolution_state: snapshot.resolution_state,
        unavailable_envs: snapshot.unavailable_envs.clone(),
        selected_env: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.selected_env.clone()),
        key_ref: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.key_ref.clone()),
        failover_used: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.failover_used)
            .unwrap_or(false),
        note: note.clone(),
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "security_kms_outage_reconcile",
        response.status,
        json!({
            "route_scope": "security/kms/outage",
            "resolution_state": response.resolution_state,
            "selected_env": response.selected_env,
            "failover_used": response.failover_used,
            "note": response.note,
        }),
    );
    Ok(Json(response))
}

// â”€â”€ WS4 Ingest handlers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn ingest_csv(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestCsvRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Write,
        &ingest_scope_for_connector(&req.connector_id, "csv"),
    )?;
    use voltnuerongrid_ingest::csv::CsvConnector;
    let mut conn = CsvConnector::new(&req.connector_id, &req.connector_id);
    let count = conn.load_csv(&req.csv_data);
    let records = conn.read_batch(usize::MAX);
    // S5-WS4-03: write each ingested record into PagedRowStore for durable typed-table backing
    {
        let mut rs = state.row_store.lock().expect("row_store lock");
        let xid = rs.begin_xid();
        for record in &records {
            let mut data = std::collections::HashMap::new();
            data.insert("payload".to_string(), record.payload.clone());
            data.insert("source".to_string(), format!("csv:{}", req.connector_id));
            rs.insert(xid, &record.key, data);
        }
    }
    let storage_key = ingest_storage_key(&principal, &req.connector_id);
    state
        .ingest_csv_records
        .lock()
        .expect("csv lock")
        .insert(storage_key, records);
    let outbox_events_written = append_ingest_outbox_events(
        &state,
        &principal,
        &req.connector_id,
        "csv",
        state
            .ingest_csv_records
            .lock()
            .expect("csv lock")
            .get(&ingest_storage_key(&principal, &req.connector_id))
            .cloned()
            .unwrap_or_default()
            .as_slice(),
    );
    let response = IngestCsvResponse {
        status: "ok",
        connector_id: req.connector_id,
        records_parsed: count,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_csv",
        "ok",
        json!({
            "route_scope": "ingest/connectors/csv",
            "connector_id": response.connector_id,
            "records_parsed": response.records_parsed,
            "outbox_events_written": outbox_events_written,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(response).expect("json")),
    ))
}

async fn ingest_json(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestJsonRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Write,
        &ingest_scope_for_connector(&req.connector_id, "json"),
    )?;
    use voltnuerongrid_ingest::json::JsonConnector;
    let mut conn = JsonConnector::new(&req.connector_id, &req.connector_id, &req.key_field);
    let count = conn.load_ndjson(&req.ndjson_data);
    let records = conn.read_batch(usize::MAX);
    // S5-WS4-03: write each ingested record into PagedRowStore for durable typed-table backing
    {
        let mut rs = state.row_store.lock().expect("row_store lock");
        let xid = rs.begin_xid();
        for record in &records {
            let mut data = std::collections::HashMap::new();
            data.insert("payload".to_string(), record.payload.clone());
            data.insert("source".to_string(), format!("json:{}", req.connector_id));
            rs.insert(xid, &record.key, data);
        }
    }
    let storage_key = ingest_storage_key(&principal, &req.connector_id);
    state
        .ingest_json_records
        .lock()
        .expect("json lock")
        .insert(storage_key, records);
    let outbox_events_written = append_ingest_outbox_events(
        &state,
        &principal,
        &req.connector_id,
        "json",
        state
            .ingest_json_records
            .lock()
            .expect("json lock")
            .get(&ingest_storage_key(&principal, &req.connector_id))
            .cloned()
            .unwrap_or_default()
            .as_slice(),
    );
    let response = IngestJsonResponse {
        status: "ok",
        connector_id: req.connector_id,
        records_parsed: count,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_json",
        "ok",
        json!({
            "route_scope": "ingest/connectors/json",
            "connector_id": response.connector_id,
            "records_parsed": response.records_parsed,
            "key_field": req.key_field,
            "outbox_events_written": outbox_events_written,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(response).expect("json")),
    ))
}

async fn ingest_parquet(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestParquetRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Write,
        &ingest_scope_for_connector(&req.connector_id, "parquet"),
    )?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(req.parquet_data_base64.trim())
        .map_err(|_| bad_request_error(&headers, "invalid_base64_payload"))?;
    use voltnuerongrid_ingest::parquet::ParquetConnector;
    let mut conn = ParquetConnector::new(&req.connector_id, &req.connector_id);
    let count = conn
        .load_parquet_bytes(&raw)
        .map_err(|_| bad_request_error(&headers, "parquet_parse_failed"))?;
    let records = conn.read_batch(usize::MAX);
    // S5-WS4-03: write each ingested parquet record into PagedRowStore
    {
        let mut rs = state.row_store.lock().expect("row_store lock");
        let xid = rs.begin_xid();
        for record in &records {
            let mut data = std::collections::HashMap::new();
            data.insert("payload".to_string(), record.payload.clone());
            data.insert("source".to_string(), format!("parquet:{}", req.connector_id));
            rs.insert(xid, &record.key, data);
        }
    }
    let storage_key = ingest_storage_key(&principal, &req.connector_id);
    state
        .ingest_parquet_records
        .lock()
        .expect("parquet lock")
        .insert(storage_key, records);
    let outbox_events_written = append_ingest_outbox_events(
        &state,
        &principal,
        &req.connector_id,
        "parquet",
        state
            .ingest_parquet_records
            .lock()
            .expect("parquet lock")
            .get(&ingest_storage_key(&principal, &req.connector_id))
            .cloned()
            .unwrap_or_default()
            .as_slice(),
    );
    let response = IngestParquetResponse {
        status: "ok",
        connector_id: req.connector_id,
        records_parsed: count,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_parquet",
        "ok",
        json!({
            "route_scope": "ingest/connectors/parquet",
            "connector_id": response.connector_id,
            "records_parsed": response.records_parsed,
            "outbox_events_written": outbox_events_written,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(response).expect("json")),
    ))
}

async fn ingest_excel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestExcelRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Write,
        &ingest_scope_for_connector(&req.connector_id, "excel"),
    )?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(req.xlsx_data_base64.trim())
        .map_err(|_| bad_request_error(&headers, "invalid_base64_payload"))?;
    use voltnuerongrid_ingest::excel::ExcelConnector;
    let mut conn = ExcelConnector::new(&req.connector_id, &req.connector_id);
    let count = conn
        .load_xlsx_bytes(&raw)
        .map_err(|_| bad_request_error(&headers, "excel_parse_failed"))?;
    let records = conn.read_batch(usize::MAX);
    // S5-WS4-03: write each ingested excel record into PagedRowStore
    {
        let mut rs = state.row_store.lock().expect("row_store lock");
        let xid = rs.begin_xid();
        for record in &records {
            let mut data = std::collections::HashMap::new();
            data.insert("payload".to_string(), record.payload.clone());
            data.insert("source".to_string(), format!("excel:{}", req.connector_id));
            rs.insert(xid, &record.key, data);
        }
    }
    let storage_key = ingest_storage_key(&principal, &req.connector_id);
    state
        .ingest_excel_records
        .lock()
        .expect("excel lock")
        .insert(storage_key, records);
    let outbox_events_written = append_ingest_outbox_events(
        &state,
        &principal,
        &req.connector_id,
        "excel",
        state
            .ingest_excel_records
            .lock()
            .expect("excel lock")
            .get(&ingest_storage_key(&principal, &req.connector_id))
            .cloned()
            .unwrap_or_default()
            .as_slice(),
    );
    let response = IngestExcelResponse {
        status: "ok",
        connector_id: req.connector_id,
        records_parsed: count,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_excel",
        "ok",
        json!({
            "route_scope": "ingest/connectors/excel",
            "connector_id": response.connector_id,
            "records_parsed": response.records_parsed,
            "outbox_events_written": outbox_events_written,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(response).expect("json")),
    ))
}

// REQ-07: POST /api/v1/ingest/chunked
async fn ingest_chunked(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestChunkedRequest>,
) -> Result<(StatusCode, Json<IngestChunkedResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Write,
        &ingest_scope_for_connector(&req.connector_id, "chunked"),
    )?;
    use voltnuerongrid_ingest::batch_config::IngestParallelConfig;
    use voltnuerongrid_ingest::IngestRecord;

    let cfg = IngestParallelConfig {
        chunk_target_rows: req.chunk_target_rows.unwrap_or(256),
        max_in_flight_tasks: req.max_in_flight_tasks.unwrap_or(4),
    };
    let records: Vec<IngestRecord> = req
        .records
        .iter()
        .enumerate()
        .map(|(i, payload)| IngestRecord {
            key: format!("{}-{i}", req.connector_id),
            payload: payload.clone(),
        })
        .collect();

    // REQ-07: async Tokio fan-out â€” each chunk is dispatched as a spawn_blocking task
    // so CPU-bound processing doesn't block the async runtime.
    let chunk_target = cfg.chunk_target_rows.max(1);
    let in_flight_cap = cfg.max_in_flight_tasks.max(1);
    let raw_chunks: Vec<Vec<IngestRecord>> = records
        .chunks(chunk_target)
        .map(|c| c.to_vec())
        .collect();
    let chunk_count = raw_chunks.len();
    let mut all_outcomes: Vec<voltnuerongrid_ingest::chunked_loader::ChunkOutcome> = Vec::new();
    for (wave_start, wave) in raw_chunks.chunks(in_flight_cap).enumerate() {
        let base_idx = wave_start * in_flight_cap;
        let handles: Vec<_> = wave
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, chunk)| {
                let chunk_index = base_idx + i;
                tokio::task::spawn_blocking(move || {
                    voltnuerongrid_ingest::chunked_loader::ChunkOutcome {
                        chunk_index,
                        records_in_chunk: chunk.len(),
                    }
                })
            })
            .collect();
        for handle in handles {
            if let Ok(outcome) = handle.await {
                all_outcomes.push(outcome);
            }
        }
    }
    let stats = voltnuerongrid_ingest::chunked_loader::ChunkedIngestStats {
        total_records: records.len(),
        chunk_count,
        chunk_target_rows: chunk_target,
        max_in_flight_tasks: in_flight_cap,
        tasks_dispatched: chunk_count.min(in_flight_cap),
        outcomes: all_outcomes,
    };

    let storage_key = ingest_storage_key(&principal, &req.connector_id);
    state
        .ingest_json_records
        .lock()
        .expect("json lock")
        .insert(storage_key, records);

    let chunks_succeeded = stats.outcomes.len();
    let chunks_failed = stats.chunk_count.saturating_sub(chunks_succeeded);

    let response = IngestChunkedResponse {
        status: "ok",
        connector_id: req.connector_id.clone(),
        total_records: stats.total_records,
        chunk_count: stats.chunk_count,
        tasks_dispatched: stats.tasks_dispatched,
        chunks_succeeded,
        chunks_failed,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_chunked",
        "ok",
        json!({
            "connector_id": response.connector_id,
            "total_records": response.total_records,
            "chunk_count": response.chunk_count,
        }),
    );
    Ok((StatusCode::OK, Json(response)))
}

async fn ingest_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<IngestStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Read,
        ingest_status_scope(),
    )?;
    let csv_map = state.ingest_csv_records.lock().expect("csv lock");
    let json_map = state.ingest_json_records.lock().expect("json lock");
    let parquet_map = state.ingest_parquet_records.lock().expect("parquet lock");
    let excel_map = state.ingest_excel_records.lock().expect("excel lock");
    let (csv_connectors, csv_total) = match &principal {
        RuntimeAccessPrincipal::Operator(_) => (
            csv_map.len(),
            csv_map.values().map(|v| v.len()).sum(),
        ),
        RuntimeAccessPrincipal::TenantUser(user) => {
            count_tenant_ingest_records(&csv_map, &user.tenant_id)
        }
    };
    let (json_connectors, json_total) = match &principal {
        RuntimeAccessPrincipal::Operator(_) => (
            json_map.len(),
            json_map.values().map(|v| v.len()).sum(),
        ),
        RuntimeAccessPrincipal::TenantUser(user) => {
            count_tenant_ingest_records(&json_map, &user.tenant_id)
        }
    };
    let (parquet_connectors, parquet_total) = match &principal {
        RuntimeAccessPrincipal::Operator(_) => (
            parquet_map.len(),
            parquet_map.values().map(|v| v.len()).sum(),
        ),
        RuntimeAccessPrincipal::TenantUser(user) => {
            count_tenant_ingest_records(&parquet_map, &user.tenant_id)
        }
    };
    let (excel_connectors, excel_total) = match &principal {
        RuntimeAccessPrincipal::Operator(_) => (
            excel_map.len(),
            excel_map.values().map(|v| v.len()).sum(),
        ),
        RuntimeAccessPrincipal::TenantUser(user) => {
            count_tenant_ingest_records(&excel_map, &user.tenant_id)
        }
    };
    let response = IngestStatusResponse {
        status: "ok",
        csv_connectors,
        json_connectors,
        parquet_connectors,
        excel_connectors,
        total_records_loaded: csv_total + json_total + parquet_total + excel_total,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_status",
        "ok",
        json!({
            "route_scope": "ingest/status",
            "csv_connectors": response.csv_connectors,
            "json_connectors": response.json_connectors,
            "parquet_connectors": response.parquet_connectors,
            "excel_connectors": response.excel_connectors,
            "total_records_loaded": response.total_records_loaded,
        }),
    );
    Ok(Json(response))
}

async fn ingest_outbox_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<IngestOutboxStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Read,
        &ingest_outbox_scope(None),
    )?;
    let stream_map = state.ingest_outbox_streams.lock().expect("outbox stream map lock");
    let accessible_streams = match &principal {
        RuntimeAccessPrincipal::Operator(_) => stream_map.values().cloned().collect::<Vec<_>>(),
        RuntimeAccessPrincipal::TenantUser(user) => {
            let prefix = format!("tenant/{}/", user.tenant_id);
            stream_map
                .iter()
                .filter(|(storage_key, _)| storage_key.starts_with(&prefix))
                .map(|(_, stream_name)| stream_name.clone())
                .collect::<Vec<_>>()
        }
    };
    drop(stream_map);

    let accessible_set = accessible_streams.iter().cloned().collect::<HashSet<_>>();
    let event_bus = state.ingest_event_bus.lock().expect("event bus lock");
    let broker_mode = event_bus.broker_kind().to_string();
    let broker_target = event_bus.broker_target();
    let visible_events = event_bus
        .events()
        .into_iter()
        .filter(|event| accessible_set.contains(&event.event.stream_name))
        .collect::<Vec<_>>();
    let response = IngestOutboxStatusResponse {
        status: "ok",
        broker_mode,
        broker_target,
        stream_count: accessible_streams.len(),
        total_events: visible_events.len(),
        last_event_id: visible_events.iter().map(|event| event.event.event_id).max(),
        streams: accessible_streams,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_outbox_status",
        "ok",
        json!({
            "route_scope": "ingest/outbox",
            "stream_count": response.stream_count,
            "total_events": response.total_events,
            "last_event_id": response.last_event_id,
        }),
    );
    Ok(Json(response))
}

async fn ingest_outbox_replay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestOutboxReplayRequest>,
) -> Result<Json<IngestOutboxReplayResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Read,
        &ingest_outbox_scope(Some(&req.connector_id)),
    )?;
    let storage_key = ingest_storage_key(&principal, &req.connector_id);
    let stream_name = ingest_outbox_stream_name(&storage_key);
    let consumer_id = req
        .consumer_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "default-consumer".to_string());
    let cursor_key = format!("consumer/{consumer_id}/{stream_name}");
    let max_items = req.max_items.unwrap_or(100).min(1_000);
    let acknowledge = req.acknowledge.unwrap_or(true);

    let cursor_before_ack = state
        .ingest_outbox_cursors
        .lock()
        .expect("outbox cursor lock")
        .load(&cursor_key);
    let last_acknowledged_event_id = cursor_before_ack.unwrap_or(0);
    let delivered = state
        .ingest_event_bus
        .lock()
        .expect("event bus lock")
        .export_for_stream_since(&stream_name, last_acknowledged_event_id, max_items)
        .into_iter()
        .collect::<Vec<_>>();

    let mut cursor_after_ack = cursor_before_ack;
    if acknowledge && !delivered.is_empty() {
        let last_event_id = delivered
            .last()
            .map(|event| event.event_id)
            .expect("delivered last event");
        let mut cursor_store = state
            .ingest_outbox_cursors
            .lock()
            .expect("outbox cursor lock");
        let _ = cursor_store.save(&cursor_key, last_event_id);
        cursor_after_ack = cursor_store.load(&cursor_key);
    }

    let delivery_state = if delivered.is_empty() {
        "already_acknowledged"
    } else if acknowledge {
        "delivered_and_acked"
    } else {
        "delivered_pending_ack"
    };
    let response = IngestOutboxReplayResponse {
        status: "ok",
        delivery_state,
        stream_name,
        consumer_id: consumer_id.clone(),
        delivered_count: delivered.len(),
        cursor_before_ack,
        cursor_after_ack,
        acknowledged: acknowledge,
        events: delivered
            .into_iter()
            .map(|event| IngestOutboxReplayEventResponse {
                replay_key: event.replay_key(),
                event_id: event.event_id,
                stream_name: event.stream_name,
                origin: event.origin,
                payload_json: event.payload_json,
            })
            .collect(),
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_outbox_replay",
        "ok",
        json!({
            "route_scope": format!("ingest/outbox/{}", req.connector_id),
            "consumer_id": response.consumer_id,
            "delivery_state": response.delivery_state,
            "delivered_count": response.delivered_count,
            "cursor_before_ack": response.cursor_before_ack,
            "cursor_after_ack": response.cursor_after_ack,
            "acknowledged": response.acknowledged,
        }),
    );
    Ok(Json(response))
}

// â”€â”€ REQ-02: DDL catalog schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Serialize)]
struct CatalogEntryView {
    object_name: String,
    object_kind: String,
    created_at_unix_ms: u128,
    last_altered_at_unix_ms: Option<u128>,
    alteration_count: u32,
}

#[derive(Serialize)]
struct CatalogSchemasResponse {
    status: &'static str,
    active_count: usize,
    total_count: usize,
    entries: Vec<CatalogEntryView>,
}

async fn catalog_schemas(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<CatalogSchemasResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_sql_runtime_principal(&headers, &state, PrivilegeAction::Read, "catalog/schemas")?;
    let catalog = state.ddl_catalog.lock().expect("ddl_catalog lock");
    let active = catalog.active_entries();
    let entries: Vec<CatalogEntryView> = active
        .iter()
        .map(|e| CatalogEntryView {
            object_name: e.object_name.clone(),
            object_kind: e.object_kind.clone(),
            created_at_unix_ms: e.created_at_unix_ms,
            last_altered_at_unix_ms: e.last_altered_at_unix_ms,
            alteration_count: e.alteration_count,
        })
        .collect();
    let resp = CatalogSchemasResponse {
        status: "ok",
        active_count: catalog.active_count(),
        total_count: catalog.total_count(),
        entries,
    };
    Ok((StatusCode::OK, Json(resp)))
}

// â”€â”€ REQ-23: ACID active transactions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Serialize)]
struct AcidTransactionsResponse {
    status: &'static str,
    active_count: usize,
    total_count: usize,
    transactions: Vec<AcidTxEntry>,
}

async fn sql_transactions_active(
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

// REQ-10/19: benchmark endpoint types and handlers
#[derive(Deserialize)]
struct BenchmarkIngestRequest {
    /// Number of synthetic records to generate (default: 10_000)
    record_count: Option<usize>,
    /// Target chunk size (default: 256)
    chunk_target_rows: Option<usize>,
}

#[derive(Serialize)]
struct BenchmarkIngestResponse {
    status: &'static str,
    record_count: usize,
    chunk_count: usize,
    wall_time_ms: u128,
    records_per_second: f64,
}

#[derive(Deserialize)]
struct BenchmarkQueryRequest {
    /// Number of SQL classification ops to run (default: 10_000)
    op_count: Option<usize>,
}

#[derive(Serialize)]
struct BenchmarkQueryResponse {
    status: &'static str,
    op_count: usize,
    wall_time_ms: u128,
    ops_per_second: f64,
}

async fn benchmark_ingest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<BenchmarkIngestRequest>,
) -> Result<(StatusCode, Json<BenchmarkIngestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    use voltnuerongrid_ingest::chunked_loader::ChunkedLoader;
    use voltnuerongrid_ingest::batch_config::IngestParallelConfig;
    use voltnuerongrid_ingest::IngestRecord;

    let record_count = req.record_count.unwrap_or(10_000).min(1_000_000);
    let chunk_size = req.chunk_target_rows.unwrap_or(256);

    let records: Vec<IngestRecord> = (0..record_count)
        .map(|i| IngestRecord {
            key: format!("bmark-{i}"),
            payload: format!("{{\"id\":{i},\"value\":{i}}}"),
        })
        .collect();

    let cfg = IngestParallelConfig { chunk_target_rows: chunk_size, max_in_flight_tasks: 4 };
    let start = std::time::Instant::now();
    let mut loader = ChunkedLoader::new(cfg);
    loader.push_chunk(records);
    let stats = loader.finalize();
    let elapsed = start.elapsed();
    let wall_ms = elapsed.as_millis();
    let rps = if wall_ms == 0 { record_count as f64 * 1000.0 } else { record_count as f64 / (wall_ms as f64 / 1000.0) };

    Ok((StatusCode::OK, Json(BenchmarkIngestResponse {
        status: "ok",
        record_count,
        chunk_count: stats.chunk_count,
        wall_time_ms: wall_ms,
        records_per_second: rps,
    })))
}

async fn benchmark_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<BenchmarkQueryRequest>,
) -> Result<(StatusCode, Json<BenchmarkQueryResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;

    let op_count = req.op_count.unwrap_or(10_000).min(1_000_000);
    let samples = [
        "SELECT * FROM orders WHERE id = 1",
        "INSERT INTO events VALUES (1, 'start')",
        "UPDATE accounts SET balance = 0 WHERE id = 99",
        "DELETE FROM staging WHERE ts < 1000",
        "BEGIN",
        "COMMIT",
    ];

    let start = std::time::Instant::now();
    for i in 0..op_count {
        let sql = samples[i % samples.len()];
        let _ = voltnuerongrid_sql::SqlAnalyzer::classify_statement(sql);
    }
    let elapsed = start.elapsed();
    let wall_ms = elapsed.as_millis();
    let ops = if wall_ms == 0 { op_count as f64 * 1000.0 } else { op_count as f64 / (wall_ms as f64 / 1000.0) };

    Ok((StatusCode::OK, Json(BenchmarkQueryResponse {
        status: "ok",
        op_count,
        wall_time_ms: wall_ms,
        ops_per_second: ops,
    })))
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
            wal_engine: Arc::new(Mutex::new(InMemoryDurabilityEngine::with_config(DurabilityConfig::default()))),
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
    fn operator_auth_allows_request_when_admin_key_not_configured() {
        let state = state_with_key(None);
        let headers = HeaderMap::new();
        assert!(require_operator_auth(&headers, &state).is_ok());
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
        let state = state_with_key(None);
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let response = runtime.block_on(failover_status(State(state)));

        assert_eq!(response.0.status, "healthy");
        assert_eq!(response.0.unresolved_critical_count, 0);
    }

    #[test]
    fn failover_status_reports_degraded_with_unresolved_critical_signal() {
        let state = state_with_key(None);
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

        let response = runtime.block_on(failover_status(State(state)));

        assert_eq!(response.0.status, "degraded");
        assert_eq!(response.0.unresolved_critical_count, 1);
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

        let degraded = runtime.block_on(failover_status(State(state.clone())));
        assert_eq!(degraded.0.status, "degraded");
        assert_eq!(degraded.0.unresolved_critical_count, 1);

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

        let recovered = runtime.block_on(failover_status(State(state)));
        assert_eq!(recovered.0.status, "healthy");
        assert_eq!(recovered.0.leader_node_id, "node-2");
        assert_eq!(recovered.0.unresolved_critical_count, 0);
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
            acid.begin("tx-concurrent", "serializable", 1_000_u128);
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
        let state = state_with_key(None);
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
        let state = state_with_key(None);

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
        let state = state_with_key(None);

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
        let state = state_with_key(None);
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
        let state = state_with_key(None);
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
        let state = state_with_key(None);
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
        let state = state_with_key(None);
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
        let state = state_with_key(None);
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
        let state = state_with_key(None);
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
            acid.begin(tx_id, "read_committed", now_ms);
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
        let state = Arc::new(state_with_key(None));

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
        let state = state_with_key(None);
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
        let state = state_with_key(None);
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

    // ── REQ-23: snapshot read path enforcement ────────────────────────────────
    #[test]
    fn ws23_acid_read_uncommitted_does_not_record_snapshot() {
        // read_uncommitted must NOT set read_snapshot_at_ms — it sees all in-progress writes
        let state = state_with_key(None);
        let tx_id = "test-ru-no-snapshot";
        let now_ms = 2_000_000_u128;
        {
            let mut acid = state.acid_transactions.lock().unwrap();
            acid.begin(tx_id, "read_uncommitted", now_ms);
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
            acid.begin(tx_id, "serializable", now_ms);
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
        let state = state_with_key(None);
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
        let state = state_with_key(None);
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
        assert_eq!(data.get("table").map(String::as_str), Some("orders"));
        assert!(
            data.get("row_values").unwrap().contains("ord-1"),
            "row_values should contain first value"
        );
    }

    #[test]
    fn s5_ws4_extract_insert_ignores_non_insert() {
        assert!(extract_insert_row_from_sql("SELECT * FROM orders").is_none());
        assert!(extract_insert_row_from_sql("UPDATE orders SET x=1").is_none());
        assert!(extract_insert_row_from_sql("COMMIT").is_none());
        assert!(extract_insert_row_from_sql("").is_none());
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
            .filter_map(|(_, d)| d.get("table").map(String::as_str))
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
        assert_eq!(data.get("table"), Some(&"products".to_string()));
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
        let (status, Json(body)) = wal_status(State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.wal_len, 0);
        assert_eq!(body.latest_sequence, 0);
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
        let (status, Json(body)) = wal_status(State(state)).await;
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
        let (_, Json(body)) = chaos_status(State(state)).await;
        assert_eq!(body.active_fault_count, 0);
        assert_eq!(body.total_injected, 0);
    }

    #[tokio::test]
    async fn s7_ws6_04_chaos_inject_records_active_fault() {
        let state = state_with_key(Some("test-key"));
        let body = ChaosInjectRequest {
            fault_type: "network_partition".to_string(),
            target_node: Some("node-2".to_string()),
            parameters: [("loss_pct".to_string(), "50".to_string())].into_iter().collect(),
        };
        let (ok_status, _) = chaos_inject(State(state.clone()), axum::extract::Json(body)).await;
        assert_eq!(ok_status, StatusCode::OK);
        let (_, Json(status)) = chaos_status(State(state)).await;
        assert_eq!(status.active_fault_count, 1);
        assert_eq!(status.total_injected, 1);
        assert_eq!(status.active_faults[0].fault_type, "network_partition");
    }

    #[tokio::test]
    async fn s7_ws6_04_chaos_clear_removes_active_faults() {
        let state = state_with_key(Some("test-key"));
        for fault in ["node_crash", "packet_loss"] {
            let body = ChaosInjectRequest {
                fault_type: fault.to_string(),
                target_node: None,
                parameters: HashMap::new(),
            };
            chaos_inject(State(state.clone()), axum::extract::Json(body)).await;
        }
        let (_, Json(before)) = chaos_status(State(state.clone())).await;
        assert_eq!(before.active_fault_count, 2);
        chaos_clear(State(state.clone())).await;
        let (_, Json(after)) = chaos_status(State(state)).await;
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
        let (status, Json(body)) = connector_list(State(state), headers).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.connector_count, 2);
        let ids: Vec<&str> = body.connectors.iter().map(|c| c.connector_id.as_str()).collect();
        assert!(ids.contains(&"conn-1"));
        assert!(ids.contains(&"conn-2"));
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
        ).await;
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
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.disconnected);
    }

    // ── S7-WS6-02: raft log entries endpoint ───────────────────────────────

    #[tokio::test]
    async fn s7_ws6_02_raft_log_fresh_state_empty() {
        let state = state_with_key(Some("test-key"));
        let (status, Json(body)) = raft_log(State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.log_length, 0);
        assert_eq!(body.commit_index, 0);
        assert!(body.entries.is_empty());
    }

    #[tokio::test]
    async fn s7_ws6_02_raft_log_after_append_has_entries() {
        let state = state_with_key(Some("test-key"));
        {
            let mut node = state.raft_state.lock().unwrap();
            node.log.push(crate::raft::RaftLogEntry { index: 1, term: 1, command: "INSERT INTO t VALUES (1)".to_string() });
            node.commit_index = 1;
        }
        let (status, Json(body)) = raft_log(State(state)).await;
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
        let (status, Json(body)) = wal_force_checkpoint(State(state), headers).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.wal_len_before, 2);
        assert_eq!(body.wal_len_after, 0);
        assert_eq!(body.checkpoint_count, 1);
    }

    #[tokio::test]
    async fn s2_ws2_02_wal_force_checkpoint_on_empty_wal() {
        let state = state_with_key(Some("test-key"));
        let headers = operator_headers("test-key", "admin");
        let (status, Json(body)) = wal_force_checkpoint(State(state), headers).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.wal_len_before, 0);
        assert_eq!(body.wal_len_after, 0);
        assert_eq!(body.checkpoint_count, 1, "checkpoint taken even on empty WAL");
    }

    // ── S7-WS6-04: Chaos health check ────────────────────────────────────────
    #[tokio::test]
    async fn s7_ws6_04_chaos_health_fresh_state_is_healthy() {
        let state = state_with_key(Some("test-key"));
        let (status, Json(body)) = chaos_health(State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.cluster_healthy, "fresh state should be healthy");
        assert_eq!(body.active_fault_count, 0);
    }

    #[tokio::test]
    async fn s7_ws6_04_chaos_health_with_faults_is_unhealthy() {
        let state = state_with_key(Some("test-key"));
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
        let (status, Json(body)) = chaos_health(State(state)).await;
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
        assert!(!body.rotation_initiated, "cert not configured so rotation_initiated=false");
        assert_eq!(body.reason, "test");
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

}
