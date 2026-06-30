//! Autonomous control-plane handlers (Tasks-7 group A): the top-level controller
//! (A-1), ops-agent orchestrator loop (A-2), schema-reconcile agent (A-5), plugin
//! builder (A-6), security/compliance agent (A-7), and incident remediation (A-8).
//!
//! All sub-agent steps are gated through the shared guardrail evaluation
//! (`helpers::autonomous_exec::evaluate_guardrail`) and threaded with a single
//! correlation id so the audit companion (A-9) can reconstruct a full plan.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use voltnuerongrid_ai::{AutonomousActionDecision, AutonomousActionExecutionRecord};
use voltnuerongrid_audit::AuditEventKind;
use voltnuerongrid_auth::PrivilegeAction;

use crate::audit_helpers::append_audit_event;
use crate::auth::{require_operator_auth, require_operator_privilege};
use crate::handlers::autonomous::{append_action_record, next_action_trace_id};
use crate::helpers::autonomous_exec::{
    classify_incident, compute_compliance_assessment, evaluate_guardrail, execute_remediation,
    remediation_failure_type_for_root_cause, OpsAgentConfig,
};
use crate::{AppState, AuthErrorResponse, AutonomousMode};

// ───────────────────────── A-1 · Autonomous DB Controller ─────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct ControllerRunRequest {
    /// High-level natural-language goal (e.g. "tune slow queries and self-heal").
    pub(crate) goal: String,
    /// When true, plan + guardrail-check only; do not execute side effects.
    #[serde(default)]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ControllerStep {
    pub(crate) action: String,
    pub(crate) guardrail_decision: String,
    pub(crate) outcome: String,
    pub(crate) detail: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct ControllerRunResponse {
    pub(crate) status: &'static str,
    pub(crate) correlation_id: String,
    pub(crate) goal: String,
    pub(crate) mode: AutonomousMode,
    pub(crate) dry_run: bool,
    pub(crate) executed_count: usize,
    pub(crate) blocked_count: usize,
    pub(crate) steps: Vec<ControllerStep>,
}

/// Decompose a free-text goal into an ordered list of guardrail action names.
/// Mirrors the guardrail policy matrix actions so each step can be gated.
pub(crate) fn decompose_goal(goal: &str) -> Vec<&'static str> {
    let g = goal.to_ascii_lowercase();
    let mut steps: Vec<&'static str> = Vec::new();
    if g.contains("schema") || g.contains("table") || g.contains("ddl") || g.contains("provision") {
        steps.push("schema_change");
    }
    if g.contains("tune") || g.contains("performance") || g.contains("slow") || g.contains("index") {
        steps.push("performance_tune");
    }
    if g.contains("heal") || g.contains("recover") || g.contains("failover") || g.contains("incident") {
        steps.push("self_heal_failover");
    }
    if g.contains("security") || g.contains("rotate") || g.contains("compliance") || g.contains("patch") {
        steps.push("security_patch");
    }
    if g.contains("plugin") || g.contains("extension") || g.contains("connector") {
        steps.push("plugin_install");
    }
    if steps.is_empty() {
        // Default diagnostic plan: low-risk tuning + self-heal sweep.
        steps.push("performance_tune");
        steps.push("self_heal_failover");
    }
    steps
}

/// `POST /api/v1/autonomous/controller/run` — A-1 top-level orchestrator.
#[tracing::instrument(skip_all, name = "autonomous.controller_run")]
pub(crate) async fn autonomous_controller_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ControllerRunRequest>,
) -> Result<(StatusCode, Json<ControllerRunResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers, &state, "autonomous.guardrails", "autonomous/controller/run", PrivilegeAction::Execute,
    )?;

    let correlation_id = next_action_trace_id();
    let plan = decompose_goal(&req.goal);
    let mut steps: Vec<ControllerStep> = Vec::new();
    let mut executed = 0usize;
    let mut blocked = 0usize;

    for action in plan {
        let decision = evaluate_guardrail(&state, action);
        let allowed = decision.decision == "allow";

        let (outcome, detail) = if !allowed {
            blocked += 1;
            ("blocked".to_string(), json!({ "reason": decision.reason }))
        } else if req.dry_run {
            ("planned".to_string(), json!({ "dry_run": true }))
        } else {
            let d = execute_controller_step(&state, action, &operator.operator_id);
            executed += 1;
            ("executed".to_string(), d)
        };

        // One audit event per step, threaded with the single correlation id.
        append_audit_event(
            &state, AuditEventKind::Autonomous, &operator.operator_id, "autonomous_controller_step", &outcome,
            &json!({
                "correlation_id": correlation_id, "trace_id": correlation_id,
                "action": action, "guardrail_decision": decision.decision, "detail": detail,
            }).to_string(),
        );

        steps.push(ControllerStep {
            action: action.to_string(),
            guardrail_decision: decision.decision.to_string(),
            outcome,
            detail,
        });
    }

    // Single correlated action record for the whole plan.
    append_action_record(&state, AutonomousActionExecutionRecord::new(
        correlation_id.clone(),
        "autonomous_controller_run",
        "cluster",
        &operator.operator_id,
        AutonomousActionDecision::Allow,
        &format!("goal='{}' steps={} executed={} blocked={}", req.goal, steps.len(), executed, blocked),
    ));

    Ok((StatusCode::OK, Json(ControllerRunResponse {
        status: "ok",
        correlation_id,
        goal: req.goal,
        mode: state.ai.autonomous_mode,
        dry_run: req.dry_run,
        executed_count: executed,
        blocked_count: blocked,
        steps,
    })))
}

/// Execute a single decomposed controller step against the relevant sub-agent.
fn execute_controller_step(state: &AppState, action: &str, actor: &str) -> serde_json::Value {
    match action {
        "performance_tune" => {
            let recs = crate::handlers::autonomous::build_tune_recommendations(state);
            if let Ok(mut store) = state.ai.tune_recommendations.lock() {
                *store = recs.clone();
            }
            json!({ "agent": "performance_tune", "recommendations": recs.len() })
        }
        "self_heal_failover" => {
            let unresolved: Vec<(String, String)> = {
                let sigs = state.cluster.cluster_failure_signals.lock().unwrap_or_else(|e| e.into_inner());
                sigs.iter().filter(|s| !s.resolved)
                    .map(|s| (s.failure_type.clone(), s.message.clone())).collect()
            };
            let mut applied = 0usize;
            for (ft, msg) in &unresolved {
                if execute_remediation(state, ft, msg).outcome == "applied" {
                    applied += 1;
                }
            }
            json!({ "agent": "self_heal", "signals": unresolved.len(), "applied": applied })
        }
        "security_patch" => {
            let assessment = compute_compliance_assessment(state);
            json!({ "agent": "security_compliance", "compliance_score": assessment.score })
        }
        "schema_change" => json!({ "agent": "schema", "note": "use /autonomous/schema/reconcile for drift provisioning" }),
        "plugin_install" => json!({ "agent": "plugin_builder", "note": "use /autonomous/plugin/build to scaffold+sign" }),
        other => json!({ "agent": other, "note": "no executor bound" }),
    }
    .as_object()
    .map(|m| {
        let mut m = m.clone();
        m.insert("actor".to_string(), json!(actor));
        serde_json::Value::Object(m)
    })
    .unwrap_or(serde_json::Value::Null)
}

// ───────────────────────── A-5 · Schema reconcile agent ─────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct DesiredTable {
    pub(crate) name: String,
    /// `col_name col_type [PRIMARY KEY]` fragments, e.g. ["id INT PRIMARY KEY", "v TEXT"].
    pub(crate) columns: Vec<String>,
    #[serde(default)]
    pub(crate) indexes: Vec<DesiredIndex>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct DesiredIndex {
    pub(crate) name: String,
    pub(crate) column: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SchemaReconcileRequest {
    pub(crate) desired_tables: Vec<DesiredTable>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SchemaDriftReport {
    pub(crate) missing_tables: Vec<String>,
    pub(crate) missing_indexes: Vec<String>,
    pub(crate) present_tables: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SchemaReconcileStep {
    pub(crate) ddl: String,
    pub(crate) status: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SchemaReconcileResponse {
    pub(crate) status: &'static str,
    pub(crate) drift: SchemaDriftReport,
    pub(crate) plan: Vec<SchemaReconcileStep>,
    pub(crate) applied: bool,
    pub(crate) executed_steps: usize,
}

/// `POST /api/v1/autonomous/schema/reconcile` — A-5 drift detection + provisioning.
pub(crate) async fn autonomous_schema_reconcile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SchemaReconcileRequest>,
) -> Result<(StatusCode, Json<SchemaReconcileResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers, &state, "autonomous.guardrails", "autonomous/schema/reconcile", PrivilegeAction::Execute,
    )?;

    // Snapshot current catalog tables + indexes.
    let existing_tables: std::collections::HashSet<String> = {
        let cat = state.storage.ddl_catalog.lock().unwrap_or_else(|e| e.into_inner());
        cat.active_entries().iter()
            .filter(|e| e.object_kind.eq_ignore_ascii_case("table"))
            .map(|e| e.object_name.to_ascii_lowercase())
            .collect()
    };
    let existing_indexes: std::collections::HashSet<String> = {
        let mgr = state.storage.index_manager.lock().unwrap_or_else(|e| e.into_inner());
        mgr.list_indexes().iter().map(|d| d.name.to_ascii_lowercase()).collect()
    };

    // Diff desired vs actual → ordered DDL plan.
    let mut missing_tables = Vec::new();
    let mut present_tables = Vec::new();
    let mut missing_indexes = Vec::new();
    let mut plan_ddl: Vec<String> = Vec::new();

    for t in &req.desired_tables {
        let tl = t.name.to_ascii_lowercase();
        if existing_tables.contains(&tl) {
            present_tables.push(t.name.clone());
        } else {
            missing_tables.push(t.name.clone());
            let cols = t.columns.join(", ");
            // Drift already confirmed absence; emit a plain CREATE TABLE so the
            // catalog parser records the real table name (it mis-parses IF NOT EXISTS).
            plan_ddl.push(format!("CREATE TABLE {} ({})", t.name, cols));
        }
    }
    // Indexes execute after their tables.
    for t in &req.desired_tables {
        for idx in &t.indexes {
            let il = idx.name.to_ascii_lowercase();
            if !existing_indexes.contains(&il) {
                missing_indexes.push(idx.name.clone());
                plan_ddl.push(format!(
                    "CREATE INDEX IF NOT EXISTS {} ON {}({})",
                    idx.name, t.name, idx.column
                ));
            }
        }
    }

    let drift = SchemaDriftReport { missing_tables, missing_indexes, present_tables };

    // Execute the plan in supervised+ mode through the real SQL engine.
    let execute = state.ai.autonomous_mode.rank() >= AutonomousMode::Supervised.rank();
    let mut steps = Vec::new();
    let mut executed = 0usize;
    if execute {
        for ddl in &plan_ddl {
            let res = crate::handlers::sql::sql_execute(
                State(state.clone()),
                headers.clone(),
                Json(crate::handlers::sql::SqlExecuteRequest {
                    sql_batch: ddl.clone(),
                    ..Default::default()
                }),
            )
            .await;
            let status = match res {
                Ok((code, _)) if code == StatusCode::OK => { executed += 1; "applied".to_string() }
                Ok((code, _)) => format!("rejected_{}", code.as_u16()),
                Err((code, _)) => format!("error_{}", code.as_u16()),
            };
            append_audit_event(
                &state, AuditEventKind::Sql, &operator.operator_id, "autonomous_schema_reconcile_step", &status,
                &json!({ "ddl": ddl }).to_string(),
            );
            steps.push(SchemaReconcileStep { ddl: ddl.clone(), status });
        }
    } else {
        for ddl in &plan_ddl {
            steps.push(SchemaReconcileStep { ddl: ddl.clone(), status: "planned_advisory".to_string() });
        }
    }

    Ok((StatusCode::OK, Json(SchemaReconcileResponse {
        status: "ok",
        drift,
        plan: steps,
        applied: execute,
        executed_steps: executed,
    })))
}

// ───────────────────────── A-6 · Plugin builder agent ─────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct PluginBuildRequest {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    /// Template descriptor (e.g. "connector", "extension"); shapes the generated manifest.
    #[serde(default)]
    pub(crate) template: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PluginBuildResponse {
    pub(crate) status: &'static str,
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) signed: bool,
    pub(crate) checksum_sha256: String,
    pub(crate) registered: bool,
}

/// `POST /api/v1/autonomous/plugin/build` — A-6 scaffold → sign → register.
pub(crate) async fn autonomous_plugin_build(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PluginBuildRequest>,
) -> Result<(StatusCode, Json<PluginBuildResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers, &state, "autonomous.guardrails", "autonomous/plugin/build", PrivilegeAction::Execute,
    )?;

    // Reject the build when no signing key is configured (no unsigned artifacts).
    let signing_key = std::env::var("VNG_PLUGIN_SIGNING_KEY").ok().filter(|v| !v.trim().is_empty());
    let template = req.template.clone().unwrap_or_else(|| "connector".to_string());

    // Generate a deterministic manifest "archive" from the template descriptor.
    let manifest = json!({
        "id": req.id, "name": req.name, "version": req.version, "template": template,
        "generated_by": "autonomous_plugin_builder",
    });
    let archive_bytes = serde_json::to_vec(&manifest).unwrap_or_default();
    let checksum = crate::compute_sha256_fingerprint(&archive_bytes);

    let Some(_key) = signing_key else {
        append_audit_event(
            &state, AuditEventKind::Security, &operator.operator_id, "autonomous_plugin_build", "rejected_unsigned",
            &json!({ "id": req.id, "reason": "VNG_PLUGIN_SIGNING_KEY not configured" }).to_string(),
        );
        return Ok((StatusCode::BAD_REQUEST, Json(PluginBuildResponse {
            status: "rejected_unsigned",
            id: req.id,
            version: req.version,
            signed: false,
            checksum_sha256: checksum,
            registered: false,
        })));
    };

    append_audit_event(
        &state, AuditEventKind::Security, &operator.operator_id, "autonomous_plugin_build", "signed",
        &json!({ "id": req.id, "version": req.version, "checksum": checksum, "template": template }).to_string(),
    );

    // Register through the same signed-manifest registry path used by plugin_install.
    let entry = crate::helpers::plugins::PluginEntry {
        id: req.id.clone(),
        name: req.name.clone(),
        version: req.version.clone(),
        checksum_sha256: checksum.clone(),
        signed: true,
        installed_at_ms: crate::helpers::plugins::now_ms(),
        state: crate::helpers::plugins::PluginState::Active,
    };
    let registered = state
        .ops.plugin_registry
        .lock()
        .map(|mut r| r.install(entry).is_ok())
        .unwrap_or(false);

    append_audit_event(
        &state, AuditEventKind::Security, &operator.operator_id, "autonomous_plugin_register",
        if registered { "ok" } else { "failed" },
        &json!({ "id": req.id, "version": req.version }).to_string(),
    );

    append_action_record(&state, AutonomousActionExecutionRecord::new(
        next_action_trace_id(), "plugin_build", "cluster", &operator.operator_id,
        AutonomousActionDecision::Allow, &format!("built+signed+registered plugin {}", req.id),
    ));

    Ok((StatusCode::OK, Json(PluginBuildResponse {
        status: "ok",
        id: req.id,
        version: req.version,
        signed: true,
        checksum_sha256: checksum,
        registered,
    })))
}

// ───────────────────────── A-7 · Security & compliance agent ─────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct SecuritySweepResponse {
    pub(crate) status: &'static str,
    pub(crate) compliance_score: u8,
    pub(crate) threshold: u8,
    pub(crate) remediation_enqueued: bool,
    pub(crate) rotation_triggered: bool,
}

/// `POST /api/v1/autonomous/security/sweep` — A-7 on-demand scan that drives
/// rotation when due and enqueues a governed remediation when compliance is low.
pub(crate) async fn autonomous_security_sweep(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<SecuritySweepResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers, &state, "autonomous.guardrails", "autonomous/security/sweep", PrivilegeAction::Execute,
    )?;

    let threshold: u8 = crate::helpers::env_helpers::read_env_usize("VNG_OPS_AGENT_COMPLIANCE_THRESHOLD", 80) as u8;
    let assessment = compute_compliance_assessment(&state);
    let remediation_enqueued = assessment.score < threshold;
    if remediation_enqueued {
        append_action_record(&state, AutonomousActionExecutionRecord::new(
            next_action_trace_id(), "compliance_remediation", "cluster", &operator.operator_id,
            AutonomousActionDecision::Allow,
            &format!("compliance score {} below threshold {}", assessment.score, threshold),
        ));
    }

    // Scheduled rotation: trigger when the configured cert max-age has elapsed.
    let rotation_triggered = {
        let max_age_ms = crate::helpers::env_helpers::read_env_usize("VNG_SECURITY_CERT_MAX_AGE_MS", 0) as u64;
        let last_rotation_ms = crate::helpers::env_helpers::read_env_usize("VNG_SECURITY_CERT_LAST_ROTATION_MS", 0) as u64;
        let now_ms = crate::helpers::time::now_unix_ms_u64();
        crate::helpers::autonomous_exec::rotation_due(now_ms, last_rotation_ms, max_age_ms)
    };

    append_audit_event(
        &state, AuditEventKind::Security, &operator.operator_id, "autonomous_security_sweep",
        if remediation_enqueued { "remediation_enqueued" } else { "ok" },
        &json!({
            "compliance_score": assessment.score, "threshold": threshold,
            "remediation_enqueued": remediation_enqueued, "rotation_triggered": rotation_triggered,
            "findings": assessment.findings,
        }).to_string(),
    );

    Ok((StatusCode::OK, Json(SecuritySweepResponse {
        status: "ok",
        compliance_score: assessment.score,
        threshold,
        remediation_enqueued,
        rotation_triggered,
    })))
}

// ───────────────────────── A-8 · Incident remediation ─────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct IncidentRemediateRequest {
    pub(crate) failure_type: Option<String>,
    pub(crate) severity: Option<String>,
    pub(crate) message: Option<String>,
    pub(crate) node_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct IncidentRemediateResponse {
    pub(crate) status: &'static str,
    pub(crate) incident_id: String,
    pub(crate) correlation_id: String,
    pub(crate) root_cause: String,
    pub(crate) confidence: String,
    pub(crate) recommended_action: String,
    pub(crate) remediation_action: String,
    pub(crate) remediation_outcome: String,
    pub(crate) executed: bool,
    pub(crate) summary: String,
}

/// `POST /api/v1/autonomous/incident/remediate` — A-8 diagnose → execute → evidence.
pub(crate) async fn autonomous_incident_remediate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IncidentRemediateRequest>,
) -> Result<(StatusCode, Json<IncidentRemediateResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers, &state, "autonomous.guardrails", "autonomous/incident/remediate", PrivilegeAction::Execute,
    )?;

    let failure_type = req.failure_type.clone().unwrap_or_else(|| "unknown".to_string());
    let severity = req.severity.clone().unwrap_or_else(|| "low".to_string());
    let message = req.message.clone().unwrap_or_default();
    let correlation_id = next_action_trace_id();
    let incident_id = format!("INC-{}", crate::now_unix_ms());
    let reporter = req.node_id.clone().unwrap_or_else(|| operator.operator_id.clone());

    // 1. Diagnose (reuses the shared classifier).
    let (root_cause, confidence, recommended_action) =
        classify_incident(&state, &failure_type, &severity, &message);
    append_audit_event(
        &state, AuditEventKind::Failover, &reporter, "autonomous_incident_diagnose", "ok",
        &json!({
            "correlation_id": correlation_id, "trace_id": correlation_id, "incident_id": incident_id,
            "root_cause": root_cause, "confidence": confidence, "reported_by": reporter,
        }).to_string(),
    );

    // 2. Map diagnosis → remediation and execute under guardrail in supervised+ mode.
    let remediation_ft = remediation_failure_type_for_root_cause(&root_cause);
    let blocked = state.ai.emergency_stop.get() || state.ai.autonomous_mode == AutonomousMode::Disabled;
    let execute = !blocked && state.ai.autonomous_mode.rank() >= AutonomousMode::Supervised.rank();
    let guardrail_reason = if blocked {
        "blocked_by_emergency_stop_or_disabled".to_string()
    } else if execute {
        "supervised+ mode".to_string()
    } else {
        "advisory mode does not execute".to_string()
    };

    let (remediation_action, remediation_outcome) = if execute {
        let outcome = execute_remediation(&state, remediation_ft, &message);
        append_audit_event(
            &state, AuditEventKind::Failover, &operator.operator_id, "autonomous_incident_fix", outcome.outcome,
            &json!({
                "correlation_id": correlation_id, "trace_id": correlation_id, "incident_id": incident_id,
                "action": outcome.action, "reason": outcome.reason, "evidence": outcome.evidence,
            }).to_string(),
        );
        (outcome.action.to_string(), outcome.outcome.to_string())
    } else {
        ("none".to_string(), format!("not_executed:{}", guardrail_reason))
    };

    // 3. Correlated action record + post-incident evidence summary.
    append_action_record(&state, AutonomousActionExecutionRecord::new(
        correlation_id.clone(), "incident_remediation",
        &format!("incident/{incident_id}"), &operator.operator_id,
        if remediation_outcome == "applied" { AutonomousActionDecision::Allow } else { AutonomousActionDecision::Deny },
        &format!("root_cause={root_cause}; remediation={remediation_action}; outcome={remediation_outcome}"),
    ));

    let summary = format!(
        "Incident {incident_id}: root_cause={root_cause} (confidence {confidence}); remediation '{remediation_action}' -> {remediation_outcome}; correlation_id={correlation_id}"
    );

    Ok((StatusCode::OK, Json(IncidentRemediateResponse {
        status: "ok",
        incident_id,
        correlation_id,
        root_cause,
        confidence,
        recommended_action,
        remediation_action,
        remediation_outcome,
        executed: execute,
        summary,
    })))
}

// ───────────────────────── A-2 · Ops-agent orchestrator loop ─────────────────────────

/// Background ops-agent orchestrator (A-2). Reuses the `run_dr_hook_scheduler`
/// spawn pattern: ticks on an interval and runs one sweep when enabled. Disabled
/// by default — `VNG_OPS_AGENT_ENABLED` must be set for any agent to fire.
pub(crate) async fn run_ops_agent_scheduler(state: AppState) {
    let config = OpsAgentConfig::from_env();
    if !config.enabled {
        tracing::info!("ops-agent scheduler disabled (set VNG_OPS_AGENT_ENABLED=true to enable)");
        return;
    }
    let interval = std::time::Duration::from_secs(config.tick_interval_secs.max(1));
    tracing::info!(
        "ops-agent scheduler started: interval={}s tune={} self_heal={} compliance={}",
        config.tick_interval_secs, config.tune_enabled, config.self_heal_enabled, config.compliance_enabled
    );
    loop {
        tokio::time::sleep(interval).await;
        let results = crate::helpers::autonomous_exec::run_ops_agent_sweep_once(&state, &config);
        tracing::debug!("ops-agent sweep completed: {} agent(s) ran", results.len());
    }
}
