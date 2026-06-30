//! Shared, side-effecting (but synchronous) execution logic for the autonomous
//! agents (Tasks-7 group A). These helpers are deliberately framework-free so
//! they can be unit-tested directly with a `state_with_key()` `AppState` and
//! reused by both the HTTP handlers and the background ops-agent scheduler.

use crate::AppState;
use serde::Serialize;
use std::collections::HashMap;

// ───────────────────────── A-4 · self-heal remediation ─────────────────────────

/// Outcome of a single self-heal remediation attempt.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct RemediationOutcome {
    /// Stable remediation action name (e.g. `cache_eviction`, `leader_promotion`).
    pub(crate) action: &'static str,
    /// `applied` | `skipped` | `failed`.
    pub(crate) outcome: &'static str,
    /// Human-readable reason / evidence summary.
    pub(crate) reason: String,
    /// Structured evidence (freed entries, probe results, new term, …).
    pub(crate) evidence: serde_json::Value,
}

/// Map a failure signal to a remediation action and *actually perform it*
/// against local subsystems. This is the real-execution core for A-4.
pub(crate) fn execute_remediation(
    state: &AppState,
    failure_type: &str,
    message: &str,
) -> RemediationOutcome {
    let msg_lower = message.to_lowercase();
    match failure_type.to_lowercase().as_str() {
        "network" | "transport" | "connection_timeout" => diagnostic_probe(state),
        "raft_election" | "leader_election" | "no_leader" => leader_promotion(state),
        "disk" | "storage" | "io_error" | "memory" | "oom" | "allocation" => {
            cache_eviction(state)
        }
        "sql_execution" | "query_timeout" | "deadlock" => query_kill(state),
        "auth" | "rbac" | "credential" => RemediationOutcome {
            action: "credential_rotation_alert",
            outcome: "skipped",
            reason: "security-sensitive remediation requires governed rotation (see A-7)".to_string(),
            evidence: serde_json::json!({ "failure_type": failure_type }),
        },
        _ => {
            if msg_lower.contains("crash") || msg_lower.contains("panic") {
                // Best-effort: collect a diagnostic probe for crash/panic signals.
                let mut probe = diagnostic_probe(state);
                probe.action = "process_restart_diagnostics";
                probe
            } else {
                diagnostic_probe(state)
            }
        }
    }
}

/// Evict cached entries via the distributed cache manager and report freed count.
fn cache_eviction(state: &AppState) -> RemediationOutcome {
    let now_ms = crate::helpers::time::now_unix_ms_u64();
    let mut cache = state
        .ops
        .distributed_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let before = cache.total_entry_count();
    let results = cache.rebalance_all(now_ms);
    let freed: usize = results.iter().map(|r| r.entries_evicted).sum();
    let after = cache.total_entry_count();
    RemediationOutcome {
        action: "cache_eviction",
        outcome: "applied",
        reason: format!("evicted {freed} cache entries across {} partitions", results.len()),
        evidence: serde_json::json!({
            "entries_before": before,
            "entries_evicted": freed,
            "entries_after": after,
            "partitions": results.len(),
        }),
    }
}

/// Release the offending pessimistic lock(s) + their transactions (query_kill).
fn query_kill(state: &AppState) -> RemediationOutcome {
    let mut locks = state
        .storage
        .pessimistic_locks
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut waits = state
        .storage
        .pessimistic_lock_waits
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    // Collect (transaction_id, resource) pairs first to avoid mutating while iterating.
    let targets: Vec<(String, String)> = locks
        .iter()
        .map(|(resource, rec)| (rec.transaction_id.clone(), resource.clone()))
        .collect();

    let mut released = 0usize;
    for (tx, resource) in &targets {
        let (status, _resp) = crate::helpers::execution::release_pessimistic_lock(
            &mut locks, &mut waits, tx, resource,
        );
        if status == axum::http::StatusCode::OK {
            released += 1;
        }
    }

    let outcome = if released > 0 { "applied" } else { "skipped" };
    RemediationOutcome {
        action: "query_kill",
        outcome,
        reason: format!("released {released} blocked lock(s)/transaction(s)"),
        evidence: serde_json::json!({ "locks_released": released, "candidates": targets.len() }),
    }
}

/// Probe local subsystems (row-store, WAL, raft) and attach the results.
fn diagnostic_probe(state: &AppState) -> RemediationOutcome {
    let row_count = state
        .storage
        .row_store
        .lock()
        .map(|rs| rs.scan_at_snapshot(rs.current_xid()).len())
        .unwrap_or(0);
    let (wal_engine, wal_persists) = state
        .storage
        .wal_engine
        .lock()
        .map(|w| (w.engine_kind().to_string(), w.persists_rows()))
        .unwrap_or_else(|_| ("unknown".to_string(), false));
    let (raft_role, raft_term) = state
        .cluster
        .raft_state
        .lock()
        .map(|r| (format!("{:?}", r.role), r.current_term))
        .unwrap_or_else(|_| ("unknown".to_string(), 0));

    RemediationOutcome {
        action: "diagnostic_probe",
        outcome: "applied",
        reason: format!(
            "probe ok: rows={row_count}, wal={wal_engine}, raft={raft_role}@term{raft_term}"
        ),
        evidence: serde_json::json!({
            "row_store_rows": row_count,
            "wal_engine": wal_engine,
            "wal_persists_rows": wal_persists,
            "raft_role": raft_role,
            "raft_term": raft_term,
        }),
    }
}

/// Trigger the Raft election path on the local node when eligible.
fn leader_promotion(state: &AppState) -> RemediationOutcome {
    let mut raft = state
        .cluster
        .raft_state
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let before_role = format!("{:?}", raft.role);
    let before_term = raft.current_term;
    if matches!(raft.role, crate::raft::RaftRole::Leader) {
        return RemediationOutcome {
            action: "leader_promotion",
            outcome: "skipped",
            reason: "local node is already the leader".to_string(),
            evidence: serde_json::json!({ "role": before_role, "term": before_term }),
        };
    }
    raft.become_candidate();
    RemediationOutcome {
        action: "leader_promotion",
        outcome: "applied",
        reason: format!(
            "started election: {before_role}@term{before_term} -> Candidate@term{}",
            raft.current_term
        ),
        evidence: serde_json::json!({
            "previous_role": before_role,
            "previous_term": before_term,
            "new_role": format!("{:?}", raft.role),
            "new_term": raft.current_term,
        }),
    }
}

// ───────────────────────── A-1 · guardrail evaluation ─────────────────────────

/// The result of evaluating an action against the guardrail policy + mode.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct GuardrailDecision {
    pub(crate) action: String,
    pub(crate) decision: &'static str, // allow | deny | blocked | unknown
    pub(crate) reason: String,
}

/// Evaluate a single action against emergency-stop, mode, and the guardrail
/// matrix. This mirrors `authorize_autonomous_action` so the controller (A-1)
/// and the orchestrator (A-2) gate every step the same way.
pub(crate) fn evaluate_guardrail(state: &AppState, action: &str) -> GuardrailDecision {
    if state.ai.emergency_stop.get() {
        return GuardrailDecision {
            action: action.to_string(),
            decision: "blocked",
            reason: "emergency_stop_enabled".to_string(),
        };
    }
    if state.ai.autonomous_mode == crate::AutonomousMode::Disabled {
        return GuardrailDecision {
            action: action.to_string(),
            decision: "blocked",
            reason: "autonomous_mode_disabled".to_string(),
        };
    }
    match state
        .ai
        .guardrails
        .iter()
        .find(|r| r.action.eq_ignore_ascii_case(action))
    {
        Some(rule) if state.ai.autonomous_mode.rank() >= rule.required_mode.rank() => {
            GuardrailDecision {
                action: action.to_string(),
                decision: "allow",
                reason: format!(
                    "mode {:?} satisfies required mode {:?}",
                    state.ai.autonomous_mode, rule.required_mode
                ),
            }
        }
        Some(rule) => GuardrailDecision {
            action: action.to_string(),
            decision: "deny",
            reason: format!(
                "required mode {:?} exceeds current mode {:?}",
                rule.required_mode, state.ai.autonomous_mode
            ),
        },
        None => GuardrailDecision {
            action: action.to_string(),
            decision: "unknown",
            reason: "no_guardrail_rule_found".to_string(),
        },
    }
}

// ───────────────────────── A-7 · compliance assessment ─────────────────────────

/// Reusable compliance scoring (shared by `compliance_report` and the A-7 agent).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ComplianceAssessment {
    pub(crate) score: u8,
    pub(crate) findings: Vec<String>,
    pub(crate) rbac_role_count: usize,
    pub(crate) audit_event_count: usize,
    pub(crate) encryption_at_rest_enabled: bool,
    pub(crate) tls_enabled: bool,
    pub(crate) constraint_count: usize,
    pub(crate) active_ddl_objects: usize,
    pub(crate) active_operator_count: usize,
}

/// Compute the compliance posture assessment from current `AppState`.
pub(crate) fn compute_compliance_assessment(state: &AppState) -> ComplianceAssessment {
    let rbac_role_count = state.auth.rbac_privilege_matrix.grants_by_role.len();
    let audit_event_count = state
        .ops
        .audit_sink
        .lock()
        .map(|s| s.len())
        .unwrap_or(0);
    let constraint_count = state
        .storage
        .constraint_manager
        .lock()
        .map(|m| m.constraint_count())
        .unwrap_or(0);
    let active_ddl_objects = state
        .storage
        .ddl_catalog
        .lock()
        .map(|c| c.active_entries().len())
        .unwrap_or(0);
    let active_operator_count = state
        .auth
        .user_store
        .lock()
        .map(|u| u.all().count())
        .unwrap_or(0);

    let encryption_at_rest_enabled =
        std::env::var("VNG_KMS_KEY_ID").is_ok() || std::env::var("VNG_ENCRYPTION_KEY").is_ok();
    let tls_enabled = std::env::var("VNG_TLS_CERT_PATH").is_ok()
        || std::env::var("VNG_NATIVE_LISTENER_ENABLED")
            .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
            .unwrap_or(false);

    let mut findings = Vec::new();
    let mut score: u8 = 100;
    if rbac_role_count == 0 {
        findings.push("no_rbac_bindings: no role bindings configured".to_string());
        score = score.saturating_sub(20);
    }
    if !encryption_at_rest_enabled {
        findings.push(
            "encryption_at_rest_disabled: VNG_KMS_KEY_ID/VNG_ENCRYPTION_KEY not set".to_string(),
        );
        score = score.saturating_sub(25);
    }
    if !tls_enabled {
        findings.push(
            "tls_not_configured: VNG_TLS_CERT_PATH not set and native listener disabled".to_string(),
        );
        score = score.saturating_sub(15);
    }
    if state.auth.admin_api_key.is_none() {
        findings.push(
            "admin_key_missing: VNG_ADMIN_API_KEY not set — server is unprotected".to_string(),
        );
        score = score.saturating_sub(30);
    }
    if audit_event_count == 0 {
        findings.push("no_audit_events: audit log is empty or not configured".to_string());
        score = score.saturating_sub(10);
    }

    ComplianceAssessment {
        score,
        findings,
        rbac_role_count,
        audit_event_count,
        encryption_at_rest_enabled,
        tls_enabled,
        constraint_count,
        active_ddl_objects,
        active_operator_count,
    }
}

// ───────────────────────── A-8 · incident classification ─────────────────────────

/// Rules-based incident classification, shared by `sre_incident_diagnose` (A-8)
/// and the autonomous incident-remediation flow. Checks configurable
/// `state.ai.diagnosis_rules` first, then built-in patterns.
pub(crate) fn classify_incident(
    state: &AppState,
    failure_type: &str,
    severity: &str,
    message: &str,
) -> (String, String, String) {
    let failure_type = failure_type.to_ascii_lowercase();
    let severity = severity.to_ascii_lowercase();
    let message = message.to_ascii_lowercase();

    let custom_match: Option<(String, String, String)> = {
        let rules = state
            .ai
            .diagnosis_rules
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        rules
            .iter()
            .find(|rule| {
                let ft_match = rule
                    .failure_type
                    .as_deref()
                    .map(|ft| ft.to_ascii_lowercase() == failure_type)
                    .unwrap_or(true);
                let kw_match = rule.keywords.is_empty()
                    || rule
                        .keywords
                        .iter()
                        .any(|kw| message.contains(&kw.to_ascii_lowercase()));
                ft_match && kw_match
            })
            .map(|r| {
                (
                    r.root_cause.clone(),
                    r.confidence.clone(),
                    r.recommended_action.clone(),
                )
            })
    };
    if let Some(hit) = custom_match {
        return hit;
    }

    let (rc, conf, action) = match failure_type.as_str() {
        "network" | "transport" | "connection_timeout" => (
            "network_partition_or_latency",
            "high",
            "check_node_connectivity;verify_firewall_rules;restart_transport_listener",
        ),
        "raft_election" | "leader_election" | "no_leader" => (
            "quorum_loss_or_split_brain",
            "high",
            "check_raft_peer_health;verify_quorum_size;restart_minority_nodes",
        ),
        "disk" | "storage" | "io_error" => (
            "disk_failure_or_full",
            "high",
            "check_disk_usage;rotate_logs;run_fsck;expand_storage",
        ),
        "memory" | "oom" | "allocation" => (
            "memory_pressure",
            "medium",
            "check_heap_usage;reduce_cache_size;add_swap",
        ),
        "sql_execution" | "query_timeout" | "deadlock" => (
            "query_plan_degradation_or_lock_contention",
            "medium",
            "run_analyze_on_affected_tables;check_lock_waiters;kill_blocked_queries",
        ),
        "auth" | "rbac" | "credential" => (
            "security_policy_violation_or_misconfiguration",
            "high",
            "rotate_credentials;audit_rbac_grants;check_admin_key_env",
        ),
        _ => {
            if message.contains("timeout") || message.contains("timed out") {
                (
                    "operation_timeout",
                    "medium",
                    "increase_statement_timeout;check_server_load",
                )
            } else if message.contains("crash") || message.contains("panic") {
                (
                    "process_crash_or_panic",
                    "high",
                    "review_panic_log;capture_core_dump;restart_with_backtrace",
                )
            } else {
                (
                    "unknown_failure",
                    "low",
                    "collect_logs;run_sre_reliability_status;escalate",
                )
            }
        }
    };
    let final_confidence = if severity == "critical" && conf == "medium" {
        "high"
    } else {
        conf
    };
    (rc.to_string(), final_confidence.to_string(), action.to_string())
}

/// Map a diagnosed root cause to a self-heal `failure_type` so a diagnosis can
/// drive the A-4 remediation engine (A-8 fix execution).
pub(crate) fn remediation_failure_type_for_root_cause(root_cause: &str) -> &'static str {
    let rc = root_cause.to_ascii_lowercase();
    if rc.contains("quorum") || rc.contains("leader") || rc.contains("split_brain") {
        "raft_election"
    } else if rc.contains("disk") || rc.contains("storage") || rc.contains("memory") {
        "disk"
    } else if rc.contains("lock") || rc.contains("query") || rc.contains("deadlock") {
        "sql_execution"
    } else {
        "network"
    }
}

// ───────────────────────── A-3 · table statistics ─────────────────────────

/// Recompute approximate statistics for a table from the row store and store
/// them in `stats_registry` (the real `ANALYZE` execution path for A-3).
/// Returns the row count recorded.
pub(crate) fn analyze_table_stats(state: &AppState, table: &str) -> usize {
    let table_lower = table.to_ascii_lowercase();
    let prefix = format!("{table_lower}:");
    let mut row_count = 0usize;
    let mut distinct: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    if let Ok(rs) = state.storage.row_store.lock() {
        for (key, data) in rs.scan_at_snapshot(rs.current_xid()) {
            if key.to_ascii_lowercase().starts_with(&prefix) {
                row_count += 1;
                for (col, val) in data.iter() {
                    distinct
                        .entry(col.clone())
                        .or_default()
                        .insert(val.clone());
                }
            }
        }
    }
    let distinct_counts: HashMap<String, usize> =
        distinct.into_iter().map(|(k, v)| (k, v.len())).collect();
    if let Ok(mut reg) = state.storage.stats_registry.lock() {
        reg.update_table(&table_lower, row_count, distinct_counts);
    }
    row_count
}

// ───────────────────────── A-2/A-7 · ops-agent scheduler config ─────────────────────────

/// Configuration for the background ops-agent orchestrator (A-2). Disabled by
/// default for safety; each agent is opt-in via env.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpsAgentConfig {
    pub(crate) enabled: bool,
    pub(crate) tune_enabled: bool,
    pub(crate) self_heal_enabled: bool,
    pub(crate) compliance_enabled: bool,
    pub(crate) security_rotation_enabled: bool,
    pub(crate) tick_interval_secs: u64,
    pub(crate) compliance_threshold: u8,
}

impl OpsAgentConfig {
    pub(crate) fn from_env() -> Self {
        let flag = |name: &str| {
            std::env::var(name)
                .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
                .unwrap_or(false)
        };
        let enabled = flag("VNG_OPS_AGENT_ENABLED");
        OpsAgentConfig {
            enabled,
            // When the orchestrator is enabled, default the individual sweeps on
            // unless explicitly disabled; when disabled, everything stays off.
            tune_enabled: enabled && !flag("VNG_OPS_AGENT_TUNE_DISABLED"),
            self_heal_enabled: enabled && !flag("VNG_OPS_AGENT_SELF_HEAL_DISABLED"),
            compliance_enabled: enabled && !flag("VNG_OPS_AGENT_COMPLIANCE_DISABLED"),
            security_rotation_enabled: enabled && flag("VNG_OPS_AGENT_SECURITY_ROTATION_ENABLED"),
            tick_interval_secs: crate::helpers::env_helpers::read_env_usize(
                "VNG_OPS_AGENT_INTERVAL_SECS",
                60,
            ) as u64,
            compliance_threshold: crate::helpers::env_helpers::read_env_usize(
                "VNG_OPS_AGENT_COMPLIANCE_THRESHOLD",
                80,
            ) as u8,
        }
    }
}

/// One result line from an ops-agent sweep.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct OpsAgentSweepResult {
    pub(crate) agent: &'static str,
    pub(crate) outcome: String,
    pub(crate) detail: serde_json::Value,
}

/// Run one ops-agent sweep cycle (A-2 + A-7). Each enabled agent runs once and
/// emits an audit event; returns the per-agent results. Pure enough to call
/// directly from a test.
pub(crate) fn run_ops_agent_sweep_once(
    state: &AppState,
    config: &OpsAgentConfig,
) -> Vec<OpsAgentSweepResult> {
    use crate::audit_helpers::append_audit_event;
    use voltnuerongrid_audit::AuditEventKind;

    let mut results = Vec::new();
    if !config.enabled {
        return results;
    }

    if config.tune_enabled {
        let recs = crate::handlers::autonomous::build_tune_recommendations(state);
        if let Ok(mut store) = state.ai.tune_recommendations.lock() {
            *store = recs.clone();
        }
        let detail = serde_json::json!({ "recommendation_count": recs.len() });
        append_audit_event(
            state,
            AuditEventKind::Autonomous,
            "ops_agent_scheduler",
            "ops_agent_tune_sweep",
            "ok",
            &detail.to_string(),
        );
        results.push(OpsAgentSweepResult {
            agent: "performance_tune",
            outcome: "ok".to_string(),
            detail,
        });
    }

    if config.self_heal_enabled {
        let unresolved: Vec<(String, String)> = {
            let sigs = state
                .cluster
                .cluster_failure_signals
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            sigs.iter()
                .filter(|s| !s.resolved)
                .map(|s| (s.failure_type.clone(), s.message.clone()))
                .collect()
        };
        let mut applied = 0usize;
        for (ft, msg) in &unresolved {
            let outcome = execute_remediation(state, ft, msg);
            if outcome.outcome == "applied" {
                applied += 1;
            }
        }
        let detail = serde_json::json!({ "signals": unresolved.len(), "applied": applied });
        append_audit_event(
            state,
            AuditEventKind::Autonomous,
            "ops_agent_scheduler",
            "ops_agent_self_heal_sweep",
            "ok",
            &detail.to_string(),
        );
        results.push(OpsAgentSweepResult {
            agent: "self_heal",
            outcome: "ok".to_string(),
            detail,
        });
    }

    if config.compliance_enabled {
        let assessment = compute_compliance_assessment(state);
        let below = assessment.score < config.compliance_threshold;
        if below {
            // Enqueue a governed remediation action record (A-7).
            let trace_id = crate::handlers::autonomous::next_action_trace_id();
            let record = voltnuerongrid_ai::AutonomousActionExecutionRecord::new(
                trace_id,
                "compliance_remediation",
                "cluster",
                "ops_agent_scheduler",
                voltnuerongrid_ai::AutonomousActionDecision::Allow,
                &format!(
                    "compliance score {} below threshold {}",
                    assessment.score, config.compliance_threshold
                ),
            );
            crate::handlers::autonomous::append_action_record(state, record);
        }
        let detail = serde_json::json!({
            "score": assessment.score,
            "threshold": config.compliance_threshold,
            "below_threshold": below,
            "remediation_enqueued": below,
        });
        append_audit_event(
            state,
            AuditEventKind::Autonomous,
            "ops_agent_scheduler",
            "ops_agent_compliance_sweep",
            if below { "remediation_enqueued" } else { "ok" },
            &detail.to_string(),
        );
        results.push(OpsAgentSweepResult {
            agent: "compliance",
            outcome: if below { "remediation_enqueued" } else { "ok" }.to_string(),
            detail,
        });
    }

    results
}

/// Whether a rotation is due given the last-rotation timestamp and the
/// configured max age (A-7 scheduled rotation policy).
pub(crate) fn rotation_due(now_ms: u64, last_rotation_ms: u64, max_age_ms: u64) -> bool {
    if max_age_ms == 0 {
        return false;
    }
    now_ms.saturating_sub(last_rotation_ms) >= max_age_ms
}
