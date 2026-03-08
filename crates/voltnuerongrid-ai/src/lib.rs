#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-ai";

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomousActionDecision {
    Allow,
    Deny,
    Blocked,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutonomousActionExecutionRecord {
    pub trace_id: String,
    pub occurred_epoch_ms: u128,
    pub action: String,
    pub scope: String,
    pub requested_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    pub decision: AutonomousActionDecision,
    pub reason: String,
}

impl AutonomousActionExecutionRecord {
    pub fn new(
        trace_id: String,
        action: &str,
        scope: &str,
        requested_by: &str,
        decision: AutonomousActionDecision,
        reason: &str,
    ) -> Self {
        Self {
            trace_id,
            occurred_epoch_ms: now_epoch_millis(),
            action: action.to_string(),
            scope: scope.to_string(),
            requested_by: requested_by.to_string(),
            tenant_id: None,
            decision,
            reason: reason.to_string(),
        }
    }

    pub fn with_tenant_id(mut self, tenant_id: Option<&str>) -> Self {
        self.tenant_id = tenant_id.map(|value| value.to_string());
        self
    }
}

fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_typed_execution_record() {
        let record = AutonomousActionExecutionRecord::new(
            "trace-1".to_string(),
            "schema_change",
            "database",
            "operator",
            AutonomousActionDecision::Allow,
            "policy satisfied",
        );
        assert_eq!(record.trace_id, "trace-1");
        assert_eq!(record.action, "schema_change");
        assert_eq!(record.decision, AutonomousActionDecision::Allow);
        assert!(record.tenant_id.is_none());
    }

    #[test]
    fn record_can_be_tagged_to_tenant_scope() {
        let record = AutonomousActionExecutionRecord::new(
            "trace-2".to_string(),
            "optimize_partition",
            "tenants/acme/autonomous/records",
            "platform-admin",
            AutonomousActionDecision::Allow,
            "policy satisfied",
        )
        .with_tenant_id(Some("acme"));

        assert_eq!(record.tenant_id.as_deref(), Some("acme"));
    }
}
