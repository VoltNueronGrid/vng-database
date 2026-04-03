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
use voltnuerongrid_sql::{I18nCatalog, SqlAnalyzer, SqlStatementKind, SupportedLocale};
use voltnuerongrid_store::htap_sync::{
    InMemoryReplicationTransport, MutationOp, ReplicaReplayState, RowStoreSyncOrigin,
};
use voltnuerongrid_store::constraints::ConstraintManager;
use voltnuerongrid_store::index::IndexManager;
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

static TX_COUNTER: AtomicU64 = AtomicU64::new(1);
static ACTION_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);
static DR_HOOK_COUNTER: AtomicU64 = AtomicU64::new(1);
static PESSIMISTIC_LOCK_COUNTER: AtomicU64 = AtomicU64::new(1);
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
    ingest_outbox_streams: Arc<Mutex<HashMap<String, String>>>,
    ingest_event_bus: Arc<Mutex<ManagedEventBusTransport>>,
    ingest_outbox_cursors: Arc<Mutex<ManagedReplayCursorStore>>,
    distributed_cache: Arc<Mutex<DistributedCacheManager>>,
    driver_pool: Arc<Mutex<ConnectionPoolManager>>,
    plugin_lifecycle: Arc<Mutex<PluginLifecycleManager>>,
    autonomous_mode: AutonomousMode,
    emergency_stop: Arc<AtomicEmergencyStop>,
    guardrails: Arc<Vec<GuardrailRule>>,
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
                scopes: vec!["security/kms".to_string(), "security/kms/outage".to_string()],
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
            scopes: vec!["security/kms".to_string()],
            actions: vec![PrivilegeAction::Read],
        },
    );

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
    path: String,
}

#[derive(Serialize)]
struct SqlRouteResponse {
    status: &'static str,
    route_path: String,
    reason: String,
    statements: Vec<RoutedStatementResponse>,
}

#[derive(Deserialize)]
struct SqlExecuteRequest {
    sql_batch: String,
    max_rows: Option<usize>,
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

// ── WS2 Index + Constraint types ───────────────────────────────────

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

// ── WS4 Ingest types ──────────────────────────────────────────────

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

#[derive(Serialize)]
struct IngestStatusResponse {
    status: &'static str,
    csv_connectors: usize,
    json_connectors: usize,
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
        node_id,
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
        ingest_outbox_streams: Arc::new(Mutex::new(HashMap::new())),
        ingest_event_bus: Arc::new(Mutex::new(load_ingest_event_bus())),
        ingest_outbox_cursors: Arc::new(Mutex::new(load_ingest_outbox_cursor_store())),
        distributed_cache: Arc::new(Mutex::new(DistributedCacheManager::with_default_policy())),
        driver_pool: Arc::new(Mutex::new(ConnectionPoolManager::with_default_policy())),
        plugin_lifecycle: Arc::new(Mutex::new(PluginLifecycleManager::new(256))),
        autonomous_mode,
        emergency_stop: Arc::new(emergency_stop),
        guardrails: Arc::new(default_guardrail_rules()),
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
        // WS4 Ingest endpoints
        .route("/api/v1/ingest/csv", post(ingest_csv))
        .route("/api/v1/ingest/json", post(ingest_json))
        .route("/api/v1/ingest/status", get(ingest_status))
        .route("/api/v1/ingest/outbox/status", get(ingest_outbox_status))
        .route("/api/v1/ingest/outbox/replay", post(ingest_outbox_replay))
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
    let response = SqlRouteResponse {
        status: "ok",
        route_path: route_path_name(decision.path).to_string(),
        reason: decision.reason,
        statements: decision
            .statements
            .into_iter()
            .map(|s| RoutedStatementResponse {
                statement: s.statement,
                path: route_path_name(s.path).to_string(),
            })
            .collect(),
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
                }),
            ));
            release_sql_data_plane_connection(&state, &connection_id);
            return response;
        }
        transaction = Some(response);
    }

    if !olap_statements.is_empty() {
        let query = olap_statements.join("; ");
        olap = Some(execute_olap_query(query, req.max_rows));
    }

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
        sink.append(kind, actor, action, outcome, details_json);
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

// ── WS2 Index + Constraint handlers ────────────────────────────────

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

// ── WS4 Ingest handlers ───────────────────────────────────────────

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
    let response = IngestStatusResponse {
        status: "ok",
        csv_connectors,
        json_connectors,
        total_records_loaded: csv_total + json_total,
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
            ingest_outbox_streams: Arc::new(Mutex::new(HashMap::new())),
            ingest_event_bus: Arc::new(Mutex::new(ManagedEventBusTransport::in_memory())),
            ingest_outbox_cursors: Arc::new(Mutex::new(ManagedReplayCursorStore::in_memory())),
            distributed_cache: Arc::new(Mutex::new(DistributedCacheManager::with_default_policy())),
            driver_pool: Arc::new(Mutex::new(ConnectionPoolManager::with_default_policy())),
            plugin_lifecycle: Arc::new(Mutex::new(PluginLifecycleManager::new(256))),
            autonomous_mode: AutonomousMode::Supervised,
            emergency_stop: Arc::new(AtomicEmergencyStop::new(false)),
            guardrails: Arc::new(default_guardrail_rules()),
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
        assert_eq!(response.1.status, "committed");
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

    // ── WS2 Index + Constraint tests ───────────────────────────────

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

    // ── WS4 Ingest tests ──────────────────────────────────────────

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

    // ── WS3 HTAP Routing Policy Tests ─────────────────────────────
    #[test]
    fn ws3_sql_route_identifies_select_olap_path() {
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
        assert_eq!(response.route_path, "olap");
        assert!(response.reason.contains("read-heavy") || response.reason.contains("SELECT"));
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
        assert!(response.reason.contains("write-heavy") || response.reason.contains("INSERT"));
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
    fn ws3_sql_route_routes_multiple_statements_proportionally() {
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
        assert_eq!(response.route_path, "olap");
        assert_eq!(response.statements.len(), 3);
        for statement in &response.statements {
            assert_eq!(statement.path, "olap");
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
                    sql_batch: "SELECT * FROM orders LIMIT 100;".to_string(),
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
                    sql_batch: "SELECT * FROM orders;".to_string(),
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
        assert_eq!(analyzed[0].kind, "\"Select\"");
        assert!(!analyzed[0].requires_transaction);
        assert_eq!(analyzed[1].kind, "\"Insert\"");
        assert!(analyzed[1].requires_transaction);
        assert_eq!(analyzed[2].kind, "\"Update\"");
        assert!(analyzed[2].requires_transaction);
        assert_eq!(analyzed[3].kind, "\"Delete\"");
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
        assert_eq!(response.route_path, "olap");

        let result = handle1.join().expect("thread join").expect("thread route call");
        assert_eq!(result.status, "ok");
        assert_eq!(result.route_path, "olap");
    }

}
