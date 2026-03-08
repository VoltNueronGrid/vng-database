#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-audit";

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKind {
    Autonomous,
    Failover,
    Ingest,
    Security,
    Sql,
    Storage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_id: u64,
    pub occurred_epoch_ms: u128,
    pub actor: String,
    pub action: String,
    pub kind: AuditEventKind,
    pub outcome: String,
    pub details_json: String,
}

#[derive(Debug, Default, Clone)]
pub struct AppendOnlyAuditSink {
    next_event_id: u64,
    events: Vec<AuditEvent>,
}

impl AppendOnlyAuditSink {
    pub fn new() -> Self {
        Self {
            next_event_id: 1,
            events: Vec::new(),
        }
    }

    pub fn append(
        &mut self,
        kind: AuditEventKind,
        actor: &str,
        action: &str,
        outcome: &str,
        details_json: &str,
    ) -> AuditEvent {
        let event = AuditEvent {
            event_id: self.next_event_id,
            occurred_epoch_ms: now_epoch_millis(),
            actor: actor.to_string(),
            action: action.to_string(),
            kind,
            outcome: outcome.to_string(),
            details_json: details_json.to_string(),
        };
        self.next_event_id += 1;
        self.events.push(event.clone());
        event
    }

    pub fn latest(&self, max_items: usize) -> Vec<AuditEvent> {
        let len = self.events.len();
        let start = len.saturating_sub(max_items);
        self.events[start..].to_vec()
    }

    pub fn len(&self) -> usize {
        self.events.len()
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
    fn appends_audit_events_with_monotonic_ids() {
        let mut sink = AppendOnlyAuditSink::new();
        let first = sink.append(
            AuditEventKind::Security,
            "operator",
            "emergency_stop",
            "ok",
            "{\"enabled\":true}",
        );
        let second = sink.append(
            AuditEventKind::Failover,
            "operator",
            "failover_simulate",
            "ok",
            "{\"new_leader\":\"node-2\"}",
        );
        assert_eq!(first.event_id, 1);
        assert_eq!(second.event_id, 2);
        assert_eq!(sink.len(), 2);
    }

    #[test]
    fn returns_latest_window() {
        let mut sink = AppendOnlyAuditSink::new();
        sink.append(AuditEventKind::Sql, "svc", "analyze", "ok", "{}");
        sink.append(AuditEventKind::Sql, "svc", "route", "ok", "{}");
        sink.append(AuditEventKind::Sql, "svc", "execute", "ok", "{}");

        let latest = sink.latest(2);
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[0].action, "route");
        assert_eq!(latest[1].action, "execute");
    }
}
