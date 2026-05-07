use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use voltnuerongrid_ai::{AutonomousActionDecision, AutonomousActionExecutionRecord};
use voltnuerongrid_audit::AuditEventKind;
use voltnuerongrid_auth::PrivilegeAction;
use crate::{AppState, AuthErrorResponse, AutonomousMode, RuntimeAccessPrincipal};
use crate::audit_helpers::{append_audit_event, append_runtime_audit_event};
use crate::auth::{require_autonomous_records_runtime_principal, require_operator_auth, require_operator_privilege};

// ─── Autonomous DTOs ──────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub(crate) struct GuardrailRule {
    pub(crate) action: String,
    pub(crate) required_mode: AutonomousMode,
    pub(crate) scope: String,
    pub(crate) rationale: String,
}

#[derive(Serialize)]
pub(crate) struct AutonomousGuardrailsResponse {
    pub(crate) status: &'static str,
    pub(crate) autonomous_mode: AutonomousMode,
    pub(crate) emergency_stop_enabled: bool,
    pub(crate) policy_matrix: Vec<GuardrailRule>,
}

#[derive(Deserialize)]
pub(crate) struct EmergencyStopRequest {
    pub(crate) enabled: bool,
    pub(crate) reason: Option<String>,
    pub(crate) requested_by: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct EmergencyStopResponse {
    pub(crate) status: &'static str,
    pub(crate) emergency_stop_enabled: bool,
    pub(crate) reason: String,
    pub(crate) requested_by: String,
}

#[derive(Deserialize)]
pub(crate) struct AuthorizeActionRequest {
    pub(crate) action: String,
    pub(crate) scope: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct AuthorizeActionResponse {
    pub(crate) status: &'static str,
    pub(crate) action: String,
    pub(crate) requested_scope: String,
    pub(crate) decision: &'static str,
    pub(crate) reason: String,
    pub(crate) trace_id: String,
}

// ─── Model gateway policy DTOs ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ModelGatewayPolicy {
    pub(crate) isolation_enabled: bool,
    pub(crate) allowed_models: Vec<String>,
    pub(crate) max_tokens_per_request: u64,
    pub(crate) rate_limit_rpm: u32,
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
pub(crate) struct AiPolicyResponse {
    pub(crate) status: &'static str,
    pub(crate) policy: ModelGatewayPolicy,
}

#[derive(Deserialize)]
pub(crate) struct AiPolicyUpdateRequest {
    pub(crate) isolation_enabled: Option<bool>,
    pub(crate) allowed_models: Option<Vec<String>>,
    pub(crate) max_tokens_per_request: Option<u64>,
    pub(crate) rate_limit_rpm: Option<u32>,
}

// ─── Autonomous records DTOs ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct AutonomousActionRecordsQuery {
    pub(crate) max_items: Option<usize>,
}

#[derive(Serialize)]
pub(crate) struct AutonomousActionRecordsResponse {
    pub(crate) status: &'static str,
    pub(crate) total_records: usize,
    pub(crate) records: Vec<AutonomousActionExecutionRecord>,
}

// ─── AI rate-check and stats DTOs ────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct AiRequestBody {
    pub(crate) model_id: String,
    pub(crate) tokens: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AiRequestResponse {
    pub(crate) status: &'static str,
    pub(crate) model_id: String,
    pub(crate) request_count: u64,
    pub(crate) rate_limit_rpm: u32,
    pub(crate) tokens_checked: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ModelRequestStat {
    pub(crate) model_id: String,
    pub(crate) request_count: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct AiPolicyStatsResponse {
    pub(crate) status: &'static str,
    pub(crate) model_count: usize,
    pub(crate) total_requests: u64,
    pub(crate) allowed_models_enforced: bool,
    pub(crate) per_model: Vec<ModelRequestStat>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AiPolicyResetResponse {
    pub(crate) status: &'static str,
    pub(crate) models_cleared: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct AiGovernanceAuditEntry {
    pub(crate) model_id: String,
    pub(crate) request_count: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct AiGovernanceAuditResponse {
    pub(crate) status: &'static str,
    pub(crate) total_models: usize,
    pub(crate) total_requests: u64,
    pub(crate) entries: Vec<AiGovernanceAuditEntry>,
}

// ─── Autonomous helper functions ──────────────────────────────────────────────

pub(crate) fn default_guardrail_rules() -> Vec<GuardrailRule> {
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

pub(crate) fn next_action_trace_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("atrace-{id}")
}

pub(crate) fn latest_action_records(state: &AppState, max_items: usize) -> Vec<AutonomousActionExecutionRecord> {
    match state.action_records.lock() {
        Ok(records) => {
            let len = records.len();
            let start = len.saturating_sub(max_items);
            records[start..].to_vec()
        }
        Err(_) => Vec::new(),
    }
}

pub(crate) fn append_action_record(state: &AppState, record: AutonomousActionExecutionRecord) {
    if let Ok(mut records) = state.action_records.lock() {
        records.push(record);
    }
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

pub(crate) fn build_authorize_action_response(
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

// ─── Autonomous handlers ──────────────────────────────────────────────────────

pub(crate) async fn autonomous_action_records(
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

pub(crate) async fn autonomous_guardrails(
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

pub(crate) async fn autonomous_emergency_stop(
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

pub(crate) async fn authorize_autonomous_action(
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

// ─── AI model gateway handlers ────────────────────────────────────────────────

pub(crate) async fn ai_policy(
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

pub(crate) async fn ai_policy_update(
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

pub(crate) async fn ai_rate_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AiRequestBody>,
) -> Result<(StatusCode, Json<AiRequestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let policy = state.model_gateway_policy.lock().expect("model_gateway_policy lock").clone();
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
    let request_count = {
        let now_ms = crate::now_epoch_ms_chaos();
        let window_ms: u64 = 60_000;
        let mut w_starts = state.ai_rate_window_starts.lock().expect("ai_rate_window_starts lock");
        let start = w_starts.entry(req.model_id.clone()).or_insert(now_ms);
        if now_ms.saturating_sub(*start) >= window_ms {
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

pub(crate) async fn ai_policy_stats(
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

pub(crate) async fn ai_policy_reset(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<AiPolicyResetResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut counters = state.ai_request_counters.lock().expect("ai_request_counters lock reset");
    let models_cleared = counters.len();
    counters.clear();
    drop(counters);
    Ok((StatusCode::OK, Json(AiPolicyResetResponse {
        status: "ok",
        models_cleared,
    })))
}

pub(crate) async fn ai_governance_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<AiGovernanceAuditResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let counters = state.ai_request_counters.lock().expect("ai_request_counters audit lock");
    let mut entries: Vec<AiGovernanceAuditEntry> = counters
        .iter()
        .map(|(model_id, &count)| AiGovernanceAuditEntry {
            model_id: model_id.clone(),
            request_count: count,
        })
        .collect();
    entries.sort_by(|a, b| b.request_count.cmp(&a.request_count));
    let total_models = entries.len();
    let total_requests: u64 = entries.iter().map(|e| e.request_count).sum();
    drop(counters);
    Ok((StatusCode::OK, Json(AiGovernanceAuditResponse {
        status: "ok",
        total_models,
        total_requests,
        entries,
    })))
}
