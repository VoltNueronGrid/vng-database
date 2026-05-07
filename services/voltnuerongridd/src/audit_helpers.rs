use voltnuerongrid_audit::{AuditEvent, AuditEventKind};
use serde_json::json;
use crate::{AppState, RuntimeAccessPrincipal};

pub(crate) fn append_audit_event(
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

pub(crate) fn append_runtime_audit_event(
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

pub(crate) fn audit_event_matches_tenant(event: &AuditEvent, tenant_id: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(&event.details_json)
        .ok()
        .and_then(|value| value.get("tenant_id").and_then(|v| v.as_str()).map(str::to_string))
        .map(|value| value.eq_ignore_ascii_case(tenant_id))
        .unwrap_or(false)
}

pub(crate) fn filter_audit_events_for_principal(
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
