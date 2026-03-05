use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use voltnuerongrid_exec::{HtapQueryRouter, QueryPath};
use voltnuerongrid_sql::{SqlAnalyzer, SqlStatementKind};

static TX_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
struct AppState {
    node_id: String,
    cluster_mode: String,
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
        autonomous_mode,
        emergency_stop: Arc::new(emergency_stop),
        guardrails: Arc::new(default_guardrail_rules()),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/sql/transaction", post(sql_transaction))
        .route("/api/v1/sql/analyze", post(sql_analyze))
        .route("/api/v1/sql/route", post(sql_route))
        .route("/api/v1/olap/query", post(olap_query))
        .route("/api/v1/failover/status", get(failover_status))
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
    if req.statements.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(SqlTransactionResponse {
                status: "error",
                transaction_id: String::new(),
                statements_executed: 0,
                requires_transaction: false,
                touches_catalog: false,
                rejected_statement_count: 0,
                elapsed_ms: 0,
            }),
        );
    }

    let mut requires_transaction = false;
    let mut touches_catalog = false;
    let mut rejected_statement_count = 0usize;
    for stmt in &req.statements {
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
            Json(SqlTransactionResponse {
                status: "error",
                transaction_id: String::new(),
                statements_executed: 0,
                requires_transaction,
                touches_catalog,
                rejected_statement_count,
                elapsed_ms: 0,
            }),
        );
    }

    let started = Instant::now();
    let tx_id = TX_COUNTER.fetch_add(1, Ordering::Relaxed);
    let elapsed = started.elapsed().as_millis();
    (
        StatusCode::OK,
        Json(SqlTransactionResponse {
            status: "committed",
            transaction_id: format!("tx-{tx_id}"),
            statements_executed: req.statements.len(),
            requires_transaction,
            touches_catalog,
            rejected_statement_count,
            elapsed_ms: elapsed,
        }),
    )
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

async fn olap_query(Json(req): Json<OlapQueryRequest>) -> Json<OlapQueryResponse> {
    let started = Instant::now();
    let elapsed = started.elapsed().as_millis();
    let max_rows = req.max_rows.unwrap_or(1000);
    Json(OlapQueryResponse {
        status: "ok",
        query_signature: req.query.chars().take(64).collect(),
        elapsed_ms: elapsed,
        rows: max_rows.min(10_000),
    })
}

async fn failover_status(State(state): State<AppState>) -> Json<FailoverStatusResponse> {
    Json(FailoverStatusResponse {
        status: "healthy",
        cluster_mode: state.cluster_mode,
        leader_node_id: state.node_id,
        rto_seconds_target: 30,
        rpo_data_loss_rows_target: 0,
    })
}

async fn autonomous_guardrails(State(state): State<AppState>) -> Json<AutonomousGuardrailsResponse> {
    Json(AutonomousGuardrailsResponse {
        status: "ok",
        autonomous_mode: state.autonomous_mode,
        emergency_stop_enabled: state.emergency_stop.get(),
        policy_matrix: state.guardrails.as_ref().clone(),
    })
}

async fn autonomous_emergency_stop(
    State(state): State<AppState>,
    Json(req): Json<EmergencyStopRequest>,
) -> Json<EmergencyStopResponse> {
    state.emergency_stop.set(req.enabled);
    Json(EmergencyStopResponse {
        status: "ok",
        emergency_stop_enabled: req.enabled,
        reason: req
            .reason
            .unwrap_or_else(|| "manual_control_plane_request".to_string()),
        requested_by: req.requested_by.unwrap_or_else(|| "unknown".to_string()),
    })
}

async fn authorize_autonomous_action(
    State(state): State<AppState>,
    Json(req): Json<AuthorizeActionRequest>,
) -> (StatusCode, Json<AuthorizeActionResponse>) {
    let requested_scope = req.scope.unwrap_or_else(|| "cluster".to_string());
    if state.emergency_stop.get() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AuthorizeActionResponse {
                status: "blocked",
                action: req.action,
                requested_scope,
                decision: "deny",
                reason: "emergency_stop_enabled".to_string(),
            }),
        );
    }

    if state.autonomous_mode == AutonomousMode::Disabled {
        return (
            StatusCode::FORBIDDEN,
            Json(AuthorizeActionResponse {
                status: "blocked",
                action: req.action,
                requested_scope,
                decision: "deny",
                reason: "autonomous_mode_disabled".to_string(),
            }),
        );
    }

    let matching_rule = state
        .guardrails
        .iter()
        .find(|r| r.action.eq_ignore_ascii_case(&req.action));

    match matching_rule {
        Some(rule) if state.autonomous_mode.rank() >= rule.required_mode.rank() => (
            StatusCode::OK,
            Json(AuthorizeActionResponse {
                status: "ok",
                action: req.action,
                requested_scope,
                decision: "allow",
                reason: format!(
                    "mode {:?} satisfies required mode {:?}",
                    state.autonomous_mode, rule.required_mode
                ),
            }),
        ),
        Some(rule) => (
            StatusCode::FORBIDDEN,
            Json(AuthorizeActionResponse {
                status: "blocked",
                action: req.action,
                requested_scope,
                decision: "deny",
                reason: format!(
                    "required mode {:?} exceeds current mode {:?}",
                    rule.required_mode, state.autonomous_mode
                ),
            }),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(AuthorizeActionResponse {
                status: "unknown_action",
                action: req.action,
                requested_scope,
                decision: "deny",
                reason: "no_guardrail_rule_found".to_string(),
            }),
        ),
    }
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
