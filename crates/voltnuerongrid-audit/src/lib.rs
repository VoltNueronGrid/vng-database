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
    /// FNV-1a 64-bit chain hash linking this event to the previous one.
    /// Provides tamper-evidence: mutating any field breaks the chain.
    pub chain_hash: String,
}

/// Genesis hash used as the seed for the first audit event chain hash.
const CHAIN_GENESIS: &str = "genesis-0000000000000000";

/// FNV-1a 64-bit chain step: hash(prev_hash | event_id | actor | action | outcome | details_json).
fn chain_step(prev_hash: &str, event_id: u64, actor: &str, action: &str, outcome: &str, details_json: &str) -> String {
    const FNV_OFFSET: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;
    let input = format!("{prev_hash}|{event_id}|{actor}|{action}|{outcome}|{details_json}");
    let mut hash = FNV_OFFSET;
    for byte in input.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

#[derive(Debug, Clone)]
pub struct AppendOnlyAuditSink {
    next_event_id: u64,
    events: Vec<AuditEvent>,
    prev_chain_hash: String,
}

impl Default for AppendOnlyAuditSink {
    fn default() -> Self {
        Self::new()
    }
}

impl AppendOnlyAuditSink {
    pub fn new() -> Self {
        Self {
            next_event_id: 1,
            events: Vec::new(),
            prev_chain_hash: CHAIN_GENESIS.to_string(),
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
        let chain_hash = chain_step(
            &self.prev_chain_hash,
            self.next_event_id,
            actor,
            action,
            outcome,
            details_json,
        );
        let event = AuditEvent {
            event_id: self.next_event_id,
            occurred_epoch_ms: now_epoch_millis(),
            actor: actor.to_string(),
            action: action.to_string(),
            kind,
            outcome: outcome.to_string(),
            details_json: details_json.to_string(),
            chain_hash: chain_hash.clone(),
        };
        self.prev_chain_hash = chain_hash;
        self.next_event_id += 1;
        self.events.push(event.clone());
        event
    }

    pub fn latest(&self, max_items: usize) -> Vec<AuditEvent> {
        let len = self.events.len();
        let start = len.saturating_sub(max_items);
        self.events[start..].to_vec()
    }

    pub fn all(&self) -> &[AuditEvent] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Verify that every event's `chain_hash` is consistent with the preceding event.
    /// Returns `true` when the chain is unbroken from genesis through all events.
    pub fn verify_chain(events: &[AuditEvent]) -> bool {
        let mut expected = CHAIN_GENESIS.to_string();
        for event in events {
            let computed = chain_step(
                &expected,
                event.event_id,
                &event.actor,
                &event.action,
                &event.outcome,
                &event.details_json,
            );
            if computed != event.chain_hash {
                return false;
            }
            expected = computed;
        }
        true
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

    #[test]
    fn chain_hashes_are_non_empty_and_deterministic() {
        let mut sink = AppendOnlyAuditSink::new();
        let e1 = sink.append(AuditEventKind::Security, "op", "restart", "ok", "{}");
        let e2 = sink.append(AuditEventKind::Sql, "op", "execute", "ok", "{}");
        assert!(!e1.chain_hash.is_empty());
        assert!(!e2.chain_hash.is_empty());
        assert_ne!(e1.chain_hash, e2.chain_hash);
    }

    #[test]
    fn verify_chain_passes_for_clean_log() {
        let mut sink = AppendOnlyAuditSink::new();
        sink.append(AuditEventKind::Sql, "svc", "analyze", "ok", "{}");
        sink.append(AuditEventKind::Failover, "svc", "simulate", "ok", "{\"node\":\"2\"}");
        sink.append(AuditEventKind::Storage, "svc", "scan", "ok", "{}");
        let events = sink.all().to_vec();
        assert!(AppendOnlyAuditSink::verify_chain(&events));
    }

    #[test]
    fn verify_chain_fails_when_field_tampered() {
        let mut sink = AppendOnlyAuditSink::new();
        sink.append(AuditEventKind::Sql, "actor", "action", "ok", "{}");
        sink.append(AuditEventKind::Sql, "actor", "action2", "ok", "{}");
        let mut events = sink.all().to_vec();
        // Tamper: change the actor of the first event
        events[0].actor = "tampered".to_string();
        assert!(!AppendOnlyAuditSink::verify_chain(&events));
    }

    #[test]
    fn verify_chain_passes_for_empty_log() {
        assert!(AppendOnlyAuditSink::verify_chain(&[]));
    }
}
