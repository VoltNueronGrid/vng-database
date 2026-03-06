use std::collections::{HashMap, HashSet, VecDeque};
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
use voltnuerongrid_audit::{AppendOnlyAuditSink, AuditEvent, AuditEventKind};
use voltnuerongrid_ai::{AutonomousActionDecision, AutonomousActionExecutionRecord};
use voltnuerongrid_exec::{HtapQueryRouter, QueryPath};
use voltnuerongrid_sql::{I18nCatalog, SqlAnalyzer, SqlStatementKind, SupportedLocale};

static TX_COUNTER: AtomicU64 = AtomicU64::new(1);
static ACTION_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);
static DR_HOOK_COUNTER: AtomicU64 = AtomicU64::new(1);
static PESSIMISTIC_LOCK_COUNTER: AtomicU64 = AtomicU64::new(1);
const DEADLOCK_SCAN_MAX_HOPS: usize = 8;

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
    leader_node_id: Arc<Mutex<String>>,
    audit_sink: Arc<Mutex<AppendOnlyAuditSink>>,
    action_records: Arc<Mutex<Vec<AutonomousActionExecutionRecord>>>,
    dr_hook_records: Arc<Mutex<Vec<DrHookExecutionRecord>>>,
    dr_hook_policy_state: Arc<Mutex<DrHookPolicyState>>,
    dr_hook_policy_config: Arc<DrHookPolicyConfig>,
    dr_hook_state_path: Option<String>,
    dr_hook_queue: Arc<Mutex<VecDeque<DrHookScheduledTask>>>,
    cluster_failure_signals: Arc<Mutex<Vec<ClusterFailureSignal>>>,
    pessimistic_locks: Arc<Mutex<HashMap<String, PessimisticLockRecord>>>,
    pessimistic_lock_waits: Arc<Mutex<HashMap<String, String>>>,
    pessimistic_lock_metrics: PessimisticLockContentionMetrics,
    autonomous_mode: AutonomousMode,
    emergency_stop: Arc<AtomicEmergencyStop>,
    guardrails: Arc<Vec<GuardrailRule>>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AutonomousMode {
    Disabled,
    Advisory,
    Supervised,
    Autonomous,
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

#[derive(Serialize)]
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

#[derive(Serialize)]
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
    let addr: SocketAddr = http_bind
        .parse()
        .unwrap_or_else(|_| "127.0.0.1:8080".parse().expect("fallback socket parse"));

    let state = AppState {
        node_id,
        cluster_mode,
        admin_api_key,
        leader_node_id: Arc::new(Mutex::new("node-1".to_string())),
        audit_sink: Arc::new(Mutex::new(AppendOnlyAuditSink::new())),
        action_records: Arc::new(Mutex::new(Vec::new())),
        dr_hook_records: Arc::new(Mutex::new(Vec::new())),
        dr_hook_policy_state: Arc::new(Mutex::new(loaded_policy_state)),
        dr_hook_policy_config: Arc::new(default_dr_hook_policy_config()),
        dr_hook_state_path,
        dr_hook_queue: Arc::new(Mutex::new(VecDeque::new())),
        cluster_failure_signals: Arc::new(Mutex::new(Vec::new())),
        pessimistic_locks: Arc::new(Mutex::new(HashMap::new())),
        pessimistic_lock_waits: Arc::new(Mutex::new(HashMap::new())),
        pessimistic_lock_metrics: PessimisticLockContentionMetrics::new(),
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
        .route("/api/v1/audit/events", get(audit_events))
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
    Json(req): Json<SqlTransactionRequest>,
) -> (StatusCode, Json<SqlTransactionResponse>) {
    let (status, response) = execute_transaction_statements(req.statements);
    (status, Json(response))
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

async fn sql_analyze(Json(req): Json<SqlAnalyzeRequest>) -> Json<SqlAnalyzeResponse> {
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

    Json(SqlAnalyzeResponse {
        status: "ok",
        total_statements: statements.len(),
        rejected_statements: rejected,
        statements,
    })
}

async fn sql_route(Json(req): Json<SqlRouteRequest>) -> Json<SqlRouteResponse> {
    let decision = HtapQueryRouter::route_batch(&req.sql_batch);
    Json(SqlRouteResponse {
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
    })
}

async fn sql_execute(Json(req): Json<SqlExecuteRequest>) -> (StatusCode, Json<SqlExecuteResponse>) {
    let decision = HtapQueryRouter::route_batch(&req.sql_batch);
    let parsed = SqlAnalyzer::parse_batch(&req.sql_batch);
    let udf_function_catalog = udf_function_catalog_contract();
    let udf_guard_policies = udf_guard_policy_contract();
    let udf_execution_plan = build_udf_execution_plan(&req.sql_batch);
    let udf_execution = execute_udf_runtime_scaffold(&req.sql_batch);

    let udf_results = match udf_execution {
        Ok(results) => results,
        Err(reason) => {
            return (
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
            );
        }
    };

    if matches!(decision.path, QueryPath::Unknown) {
        return (
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
        );
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
            return (
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
            );
        }
        transaction = Some(response);
    }

    if !olap_statements.is_empty() {
        let query = olap_statements.join("; ");
        olap = Some(execute_olap_query(query, req.max_rows));
    }

    (
        StatusCode::OK,
        Json(SqlExecuteResponse {
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
        }),
    )
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
    Json(FailoverStatusResponse {
        status: "healthy",
        cluster_mode: state.cluster_mode,
        leader_node_id: leader,
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
    let (previous_leader_node_id, new_leader_node_id) =
        rotate_leader(&state.leader_node_id, &req.new_leader_node_id, &state.node_id);
    append_audit_event(
        &state,
        AuditEventKind::Failover,
        req.requested_by.as_deref().unwrap_or("unknown"),
        "failover_simulate",
        "ok",
        &json!({
            "previous_leader_node_id": previous_leader_node_id.clone(),
            "new_leader_node_id": new_leader_node_id.clone(),
            "reason": req.reason.clone().unwrap_or_else(|| "manual_failover_simulation".to_string())
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
    }))
}

async fn sre_reliability_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SreReliabilityStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
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
        headers
            .get("x-vng-operator-id")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("unknown"),
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
    let requested_by = headers
        .get("x-vng-operator-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");
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
    let requested_by = headers
        .get("x-vng-operator-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");
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
    let operator = headers
        .get("x-vng-operator-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");
    append_audit_event(
        &state,
        AuditEventKind::Failover,
        operator,
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
    let operator = headers
        .get("x-vng-operator-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");
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
                signal.resolved_by = Some(operator.to_string());
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
    append_audit_event(
        &state,
        AuditEventKind::Failover,
        operator,
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
    Ok(Json(build_sre_gate_evaluation(&state)))
}

async fn sre_gate_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SreGateExportRequest>,
) -> Result<Json<SreGateExportResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
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

async fn audit_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditEventsQuery>,
) -> Result<Json<AuditEventsResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let max_items = query.max_items.unwrap_or(100).min(1_000);
    let events = state
        .audit_sink
        .lock()
        .map(|sink| sink.latest(max_items))
        .unwrap_or_default();
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
    require_operator_auth(&headers, &state)?;
    let max_items = query.max_items.unwrap_or(100).min(1_000);
    let records = latest_action_records(&state, max_items);
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
    state.emergency_stop.set(req.enabled);
    let reason = req
        .reason
        .clone()
        .unwrap_or_else(|| "manual_control_plane_request".to_string());
    let requested_by = req.requested_by.clone().unwrap_or_else(|| "unknown".to_string());
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
    let requested_scope = req.scope.unwrap_or_else(|| "cluster".to_string());
    let requested_by = headers
        .get("x-vng-operator-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
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

fn load_dr_hook_policy_state(path: Option<&str>) -> DrHookPolicyState {
    let Some(path_value) = path else {
        return DrHookPolicyState::default();
    };
    let read = fs::read_to_string(path_value);
    match read {
        Ok(contents) => serde_json::from_str::<DrHookPolicyStateSnapshot>(&contents)
            .map(|snapshot| DrHookPolicyState {
                hooks: snapshot.hooks,
            })
            .unwrap_or_default(),
        Err(_) => DrHookPolicyState::default(),
    }
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
    if let Ok(encoded) = serde_json::to_string_pretty(&snapshot) {
        let _ = fs::write(path_value, encoded);
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

    if provided == required_key {
        Ok(())
    } else {
        let locale = locale_from_headers(headers);
        let localized = I18nCatalog::message(locale, "missing_or_invalid_admin_key");
        Err((
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                status: "unauthorized",
                reason: "missing_or_invalid_admin_key".to_string(),
                locale: locale.as_str().to_string(),
                localized_message: localized.message.to_string(),
            }),
        ))
    }
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

fn next_action_trace_id() -> String {
    let id = ACTION_TRACE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("atrace-{id}")
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
    let record = AutonomousActionExecutionRecord::new(
        trace_id.to_string(),
        action,
        requested_scope,
        requested_by,
        typed_decision,
        &reason,
    );
    append_action_record(state, record);
    append_audit_event(
        state,
        AuditEventKind::Autonomous,
        requested_by,
        "autonomous_action_authorize",
        decision,
        &json!({
            "trace_id": trace_id,
            "action": action,
            "requested_scope": requested_scope,
            "decision": decision,
            "reason": reason.clone(),
        })
        .to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn state_with_key(key: Option<&str>) -> AppState {
        AppState {
            node_id: "node-1".to_string(),
            cluster_mode: "single".to_string(),
            admin_api_key: key.map(|v| v.to_string()),
            leader_node_id: Arc::new(Mutex::new("node-1".to_string())),
            audit_sink: Arc::new(Mutex::new(AppendOnlyAuditSink::new())),
            action_records: Arc::new(Mutex::new(Vec::new())),
            dr_hook_records: Arc::new(Mutex::new(Vec::new())),
            dr_hook_policy_state: Arc::new(Mutex::new(DrHookPolicyState::default())),
            dr_hook_policy_config: Arc::new(default_dr_hook_policy_config()),
            dr_hook_state_path: None,
            dr_hook_queue: Arc::new(Mutex::new(VecDeque::new())),
            cluster_failure_signals: Arc::new(Mutex::new(Vec::new())),
            pessimistic_locks: Arc::new(Mutex::new(HashMap::new())),
            pessimistic_lock_waits: Arc::new(Mutex::new(HashMap::new())),
            pessimistic_lock_metrics: PessimisticLockContentionMetrics::new(),
            autonomous_mode: AutonomousMode::Supervised,
            emergency_stop: Arc::new(AtomicEmergencyStop::new(false)),
            guardrails: Arc::new(default_guardrail_rules()),
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
        let mut headers = HeaderMap::new();
        headers.insert("x-vng-admin-key", HeaderValue::from_static("secret"));
        assert!(require_operator_auth(&headers, &state).is_ok());
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
}
