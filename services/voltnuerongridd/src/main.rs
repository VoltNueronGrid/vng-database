use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use voltnuerongrid_audit::{AppendOnlyAuditSink, AuditEvent, AuditEventKind};
use voltnuerongrid_ai::{AutonomousActionDecision, AutonomousActionExecutionRecord};
use voltnuerongrid_exec::{HtapQueryRouter, QueryPath};
use voltnuerongrid_sql::{SqlAnalyzer, SqlStatementKind};

static TX_COUNTER: AtomicU64 = AtomicU64::new(1);
static ACTION_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
struct AppState {
    node_id: String,
    cluster_mode: String,
    admin_api_key: Option<String>,
    leader_node_id: Arc<Mutex<String>>,
    audit_sink: Arc<Mutex<AppendOnlyAuditSink>>,
    action_records: Arc<Mutex<Vec<AutonomousActionExecutionRecord>>>,
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
    reason: &'static str,
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
        autonomous_mode,
        emergency_stop: Arc::new(emergency_stop),
        guardrails: Arc::new(default_guardrail_rules()),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/sql/transaction", post(sql_transaction))
        .route("/api/v1/sql/analyze", post(sql_analyze))
        .route("/api/v1/sql/route", post(sql_route))
        .route("/api/v1/sql/execute", post(sql_execute))
        .route("/api/v1/olap/query", post(olap_query))
        .route("/api/v1/failover/status", get(failover_status))
        .route("/api/v1/failover/simulate", post(failover_simulate))
        .route("/api/v1/audit/events", get(audit_events))
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
        Err((
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                status: "unauthorized",
                reason: "missing_or_invalid_admin_key",
            }),
        ))
    }
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
}
