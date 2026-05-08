//! DR hook, SRE gate, failure budget, rate limiting helpers.
use std::fs;
use std::sync::atomic::Ordering;
use serde_json::json;
use voltnuerongrid_store::htap_sync::MutationOp;
use crate::AppState;
use crate::{DR_HOOK_COUNTER, AutonomousMode, now_unix_ms};
use crate::{
    DrHookExecutionRecord, DrHookPolicyConfig, DrHookPolicyState,
    DrHookPolicyStateEnvelope, DrHookPolicyStateSnapshot, DrHookRetryPlanStep,
    DrHookRuntimeState, DrHookScheduledTask,
    FailureBudgetAlertResponse, FailureBudgetSnapshot,
    RateLimitPolicySnapshot,
};
use crate::{record_transport_mutation, rotate_leader};


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


pub(crate) fn default_dr_hook_policy_config() -> DrHookPolicyConfig {
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


pub(crate) fn compute_retry_backoff_ms(attempt: u32, base_backoff_ms: u64, max_backoff_ms: u64) -> u64 {
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


pub(crate) fn dr_hook_policy_backup_path(path: &str) -> String {
    format!("{path}.bak")
}


pub(crate) fn compute_dr_hook_policy_checksum(snapshot: &DrHookPolicyStateSnapshot) -> String {
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


pub(crate) fn decode_dr_hook_policy_state(contents: &str) -> Option<DrHookPolicyState> {
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


pub(crate) fn load_dr_hook_policy_state(path: Option<&str>) -> DrHookPolicyState {
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


pub(crate) fn persist_dr_hook_policy_state(state: &AppState) {
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


pub(crate) fn dequeue_dr_hook_task(state: &AppState) -> Option<DrHookScheduledTask> {
    state
        .dr_hook_queue
        .lock()
        .ok()
        .and_then(|mut queue| queue.pop_front())
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


pub(crate) fn append_dr_hook_record(state: &AppState, record: DrHookExecutionRecord) {
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

