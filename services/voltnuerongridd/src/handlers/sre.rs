use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::Ordering;
use voltnuerongrid_audit::AuditEventKind;
use voltnuerongrid_auth::PrivilegeAction;
use voltnuerongrid_store::htap_sync::MutationOp;
use crate::{
    AppState, AuthErrorResponse, AutonomousMode, DR_HOOK_COUNTER,
    PoolStatsResponse,
    now_unix_ms, now_unix_ms_u64,
    failure_budget_snapshot, rate_limit_policy_snapshot, evaluate_rate_limit,
    evaluate_failure_budget_alert, build_retry_plan, enqueue_dr_hook_task,
    execute_dr_hook, latest_dr_hook_records, pool_acquire_error_state, pool_stats_response,
    record_transport_mutation,
};
use crate::auth::{require_operator_auth, require_operator_privilege};
use crate::audit_helpers::append_audit_event;

// ─── SRE DTOs ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct SreReliabilityStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) service_health: &'static str,
    pub(crate) failure_budget: FailureBudgetSnapshot,
    pub(crate) rate_limit_policy: RateLimitPolicySnapshot,
}

#[derive(Serialize, Clone, Copy)]
pub(crate) struct FailureBudgetSnapshot {
    pub(crate) window_minutes: u32,
    pub(crate) error_budget_percent: f64,
    pub(crate) consumed_percent: f64,
    pub(crate) remaining_percent: f64,
    pub(crate) burn_rate: f64,
}

#[derive(Serialize, Clone, Copy)]
pub(crate) struct RateLimitPolicySnapshot {
    pub(crate) requests_per_minute: u32,
    pub(crate) burst_limit: u32,
    pub(crate) current_minute_count: u32,
    pub(crate) allowed: bool,
}

#[derive(Deserialize)]
pub(crate) struct RateLimitCheckRequest {
    pub(crate) current_minute_count: u32,
    pub(crate) requested_units: Option<u32>,
}

#[derive(Serialize)]
pub(crate) struct RateLimitCheckResponse {
    pub(crate) status: &'static str,
    pub(crate) allowed: bool,
    pub(crate) remaining_units: u32,
    pub(crate) reason: String,
}

#[derive(Deserialize)]
pub(crate) struct FailureBudgetAlertQuery {
    pub(crate) consumed_percent: Option<f64>,
    pub(crate) burn_rate: Option<f64>,
}

#[derive(Serialize)]
pub(crate) struct FailureBudgetAlertResponse {
    pub(crate) status: &'static str,
    pub(crate) alert_state: &'static str,
    pub(crate) severity: &'static str,
    pub(crate) threshold_percent: f64,
    pub(crate) consumed_percent: f64,
    pub(crate) burn_rate: f64,
    pub(crate) recommended_action: &'static str,
}

#[derive(Deserialize)]
pub(crate) struct DrHookTriggerRequest {
    pub(crate) hook: String,
    pub(crate) scope: Option<String>,
    pub(crate) dry_run: Option<bool>,
}

#[derive(Clone, Serialize)]
pub(crate) struct DrHookExecutionRecord {
    pub(crate) execution_id: String,
    pub(crate) hook: String,
    pub(crate) scope: String,
    pub(crate) status: &'static str,
    pub(crate) dry_run: bool,
    pub(crate) policy_decision: &'static str,
    pub(crate) cooldown_remaining_ms: u64,
    pub(crate) retry_backoff_ms: u64,
    pub(crate) retry_attempt: u32,
    pub(crate) details: String,
}

#[derive(Default)]
pub(crate) struct DrHookPolicyState {
    pub(crate) hooks: HashMap<String, DrHookRuntimeState>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct DrHookRuntimeState {
    pub(crate) last_attempt_unix_ms: u128,
    pub(crate) consecutive_failures: u32,
    pub(crate) last_status: String,
}

#[derive(Clone)]
pub(crate) struct DrHookPolicyConfig {
    pub(crate) min_mode: AutonomousMode,
    pub(crate) cooldown_seconds: u64,
    pub(crate) max_retries: u32,
    pub(crate) base_backoff_ms: u64,
    pub(crate) max_backoff_ms: u64,
    pub(crate) allowed_hooks: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct DrHookPolicyResponse {
    pub(crate) status: &'static str,
    pub(crate) policy: DrHookPolicyContract,
}

#[derive(Serialize)]
pub(crate) struct DrHookPolicyContract {
    pub(crate) min_mode: AutonomousMode,
    pub(crate) cooldown_seconds: u64,
    pub(crate) max_retries: u32,
    pub(crate) base_backoff_ms: u64,
    pub(crate) max_backoff_ms: u64,
    pub(crate) allowed_hooks: Vec<String>,
    pub(crate) tracked_hooks: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct DrHookPolicyStateSnapshot {
    pub(crate) hooks: HashMap<String, DrHookRuntimeState>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct DrHookPolicyStateEnvelope {
    pub(crate) schema_version: u32,
    pub(crate) written_unix_ms: u128,
    pub(crate) checksum_hex: String,
    pub(crate) snapshot: DrHookPolicyStateSnapshot,
}

#[derive(Deserialize)]
pub(crate) struct DrHookRetryPlanQuery {
    pub(crate) hook: String,
    pub(crate) attempts: Option<u32>,
}

#[derive(Serialize)]
pub(crate) struct DrHookRetryPlanResponse {
    pub(crate) status: &'static str,
    pub(crate) hook: String,
    pub(crate) accepted: bool,
    pub(crate) reason: String,
    pub(crate) steps: Vec<DrHookRetryPlanStep>,
}

#[derive(Serialize)]
pub(crate) struct DrHookRetryPlanStep {
    pub(crate) attempt: u32,
    pub(crate) recommended_backoff_ms: u64,
    pub(crate) jitter_range_ms: u64,
}

#[derive(Deserialize)]
pub(crate) struct DrHookScheduleRequest {
    pub(crate) hook: String,
    pub(crate) scope: Option<String>,
    pub(crate) dry_run: Option<bool>,
    pub(crate) reason: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct DrHookScheduledTask {
    pub(crate) task_id: String,
    pub(crate) hook: String,
    pub(crate) scope: String,
    pub(crate) dry_run: bool,
    pub(crate) requested_by: String,
    pub(crate) reason: String,
    pub(crate) enqueued_unix_ms: u128,
}

#[derive(Serialize)]
pub(crate) struct DrHookScheduleResponse {
    pub(crate) status: &'static str,
    pub(crate) task: DrHookScheduledTask,
    pub(crate) queue_depth: usize,
}

#[derive(Deserialize)]
pub(crate) struct FailureSignalRequest {
    pub(crate) node_id: String,
    pub(crate) transport: String,
    pub(crate) failure_type: String,
    pub(crate) severity: String,
    pub(crate) message: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ClusterFailureSignal {
    pub(crate) signal_id: String,
    pub(crate) node_id: String,
    pub(crate) transport: String,
    pub(crate) failure_type: String,
    pub(crate) severity: String,
    pub(crate) message: String,
    pub(crate) observed_unix_ms: u128,
    pub(crate) resolved: bool,
    pub(crate) resolved_by: Option<String>,
    pub(crate) resolved_unix_ms: Option<u128>,
    pub(crate) resolution_note: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct FailureSignalResponse {
    pub(crate) status: &'static str,
    pub(crate) signal: ClusterFailureSignal,
    pub(crate) queued_remediation_task: Option<DrHookScheduledTask>,
}

#[derive(Deserialize)]
pub(crate) struct FailureReconcileRequest {
    pub(crate) signal_ids: Option<Vec<String>>,
    pub(crate) resolve_all_critical: Option<bool>,
    pub(crate) note: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct FailureReconcileResponse {
    pub(crate) status: &'static str,
    pub(crate) resolved_count: usize,
    pub(crate) unresolved_critical_count: usize,
}

#[derive(Serialize)]
pub(crate) struct SreGateEvaluationResponse {
    pub(crate) status: &'static str,
    pub(crate) gate_result: &'static str,
    pub(crate) criteria: Vec<SreGateCriterion>,
    pub(crate) recommended_actions: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct SreGateCriterion {
    pub(crate) name: String,
    pub(crate) passed: bool,
    pub(crate) detail: String,
}

#[derive(Deserialize)]
pub(crate) struct SreGateExportRequest {
    pub(crate) output_path: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SreGateExportResponse {
    pub(crate) status: &'static str,
    pub(crate) output_path: String,
    pub(crate) gate_result: &'static str,
}

#[derive(Serialize)]
pub(crate) struct DrHookTriggerResponse {
    pub(crate) status: &'static str,
    pub(crate) execution: DrHookExecutionRecord,
}

#[derive(Deserialize)]
pub(crate) struct DrHookStatusQuery {
    pub(crate) max_items: Option<usize>,
}

#[derive(Serialize)]
pub(crate) struct DrHookStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) total_records: usize,
    pub(crate) records: Vec<DrHookExecutionRecord>,
}

#[derive(Deserialize)]
pub(crate) struct CacheSetRequest {
    pub(crate) partition_id: String,
    pub(crate) key: String,
    pub(crate) value: serde_json::Value,
    pub(crate) ttl_ms: Option<u64>,
}

#[derive(Deserialize)]
pub(crate) struct CacheGetQuery {
    pub(crate) partition_id: String,
    pub(crate) key: String,
}

#[derive(Deserialize)]
pub(crate) struct CacheInvalidateRequest {
    pub(crate) partition_id: String,
    pub(crate) key: String,
}

#[derive(Serialize)]
pub(crate) struct CacheWriteResponse {
    pub(crate) status: &'static str,
    pub(crate) partition_id: String,
    pub(crate) key: String,
    pub(crate) error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CacheGetResponse {
    pub(crate) status: &'static str,
    pub(crate) partition_id: String,
    pub(crate) key: String,
    pub(crate) hit: bool,
    pub(crate) value: Option<serde_json::Value>,
    pub(crate) error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CacheInvalidateResponse {
    pub(crate) status: &'static str,
    pub(crate) partition_id: String,
    pub(crate) key: String,
    pub(crate) removed: bool,
    pub(crate) error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CacheRebalanceResponse {
    pub(crate) status: &'static str,
    pub(crate) partition_count: usize,
    pub(crate) rebalanced_partitions: usize,
    pub(crate) entries_evicted: usize,
}

#[derive(Serialize)]
pub(crate) struct CachePartitionMetricsResponse {
    pub(crate) partition_id: String,
    pub(crate) entry_count: usize,
    pub(crate) total_hits: u64,
    pub(crate) total_misses: u64,
    pub(crate) total_evictions: u64,
    pub(crate) circuit_breaker_state: String,
    pub(crate) hit_ratio: f64,
    pub(crate) last_rebalance_ms: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct CacheMetricsResponse {
    pub(crate) status: &'static str,
    pub(crate) partition_count: usize,
    pub(crate) total_entries: usize,
    pub(crate) partitions: Vec<CachePartitionMetricsResponse>,
}

#[derive(Deserialize)]
pub(crate) struct PoolAcquireRequest {
    pub(crate) now_ms: Option<u64>,
}

#[derive(Deserialize)]
pub(crate) struct PoolReleaseRequest {
    pub(crate) connection_id: String,
    pub(crate) now_ms: Option<u64>,
}

#[derive(Deserialize)]
pub(crate) struct PoolFailureRequest {
    pub(crate) connection_id: String,
    pub(crate) error: Option<String>,
    pub(crate) now_ms: Option<u64>,
}

#[derive(Deserialize)]
pub(crate) struct PoolRecoverRequest {
    pub(crate) now_ms: Option<u64>,
    pub(crate) prune_unhealthy: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct PoolAcquireResponse {
    pub(crate) status: &'static str,
    pub(crate) acquire_state: &'static str,
    pub(crate) connection_id: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) stats: PoolStatsResponse,
}

#[derive(Serialize)]
pub(crate) struct PoolReleaseResponse {
    pub(crate) status: &'static str,
    pub(crate) released: bool,
    pub(crate) stats: PoolStatsResponse,
}

#[derive(Serialize)]
pub(crate) struct PoolFailureResponse {
    pub(crate) status: &'static str,
    pub(crate) marked_failed: bool,
    pub(crate) stats: PoolStatsResponse,
}

#[derive(Serialize)]
pub(crate) struct PoolRecoverResponse {
    pub(crate) status: &'static str,
    pub(crate) circuit_recovered: bool,
    pub(crate) pruned_unhealthy: usize,
    pub(crate) stats: PoolStatsResponse,
}

// ─── SRE Handlers ─────────────────────────────────────────────────────────────

pub(crate) async fn sre_reliability_status(
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

pub(crate) async fn sre_rate_limit_check(
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

pub(crate) async fn sre_failure_budget_alerts(
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

pub(crate) async fn sre_dr_hook_policy(
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

pub(crate) async fn sre_dr_hook_retry_plan(
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

pub(crate) async fn sre_dr_hook_schedule(
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

pub(crate) async fn sre_dr_hook_trigger(
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

pub(crate) async fn sre_dr_hook_status(
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

pub(crate) async fn sre_failure_signal(
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

pub(crate) async fn sre_failure_reconcile(
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

pub(crate) async fn sre_gate_evaluate(
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

pub(crate) async fn sre_gate_export(
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

pub(crate) async fn sre_cache_set(
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

pub(crate) async fn sre_cache_get(
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

pub(crate) async fn sre_cache_invalidate(
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

pub(crate) async fn sre_cache_rebalance(
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

pub(crate) async fn sre_cache_metrics(
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

// ─── SRE Driver Pool Handlers ─────────────────────────────────────────────────

pub(crate) async fn sre_driver_pool_acquire(
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

pub(crate) async fn sre_driver_pool_release(
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

pub(crate) async fn sre_driver_pool_failure(
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

pub(crate) async fn sre_driver_pool_recover(
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

pub(crate) async fn sre_driver_pool_stats(
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

// ─── SRE Gate Helpers ─────────────────────────────────────────────────────────

pub(crate) fn build_sre_gate_evaluation(state: &AppState) -> SreGateEvaluationResponse {
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

pub(crate) fn export_gate_report(path: &str, evaluation: &SreGateEvaluationResponse) {
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    if let Ok(encoded) = serde_json::to_string_pretty(evaluation) {
        let _ = fs::write(path, encoded);
    }
}
