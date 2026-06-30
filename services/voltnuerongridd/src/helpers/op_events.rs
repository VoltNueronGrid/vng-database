//! B-5: Operational event stream. A lightweight, in-memory ring buffer of
//! lifecycle events emitted by core subsystems (raft leader change, ingest
//! batch completion, autoscale decision, self-heal action). Distinct from the
//! tamper-evident audit log: this stream is for operational observability and
//! is queryable via `GET /api/v1/events/operational`.

use crate::AppState;
use serde::Serialize;
use std::collections::VecDeque;

/// Maximum number of operational events retained in the ring buffer.
pub(crate) const OPERATIONAL_EVENT_CAPACITY: usize = 1024;

/// One operational lifecycle event.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct OperationalEvent {
    pub(crate) event_id: u64,
    /// Emitting subsystem: `raft` | `ingest` | `autoscale` | `self_heal` | ...
    pub(crate) subsystem: String,
    /// Event kind within the subsystem (e.g. `leader_elected`, `batch_complete`).
    pub(crate) kind: String,
    pub(crate) occurred_epoch_ms: u128,
    /// Free-form JSON detail payload.
    pub(crate) detail: serde_json::Value,
}

/// In-memory operational event stream (ring buffer + monotonic id).
#[derive(Debug, Default)]
pub(crate) struct OperationalEventStream {
    next_id: u64,
    events: VecDeque<OperationalEvent>,
}

impl OperationalEventStream {
    pub(crate) fn new() -> Self {
        Self {
            next_id: 1,
            events: VecDeque::new(),
        }
    }

    /// Append an event, evicting the oldest when at capacity.
    pub(crate) fn emit(&mut self, subsystem: &str, kind: &str, detail: serde_json::Value) -> u64 {
        let event_id = self.next_id.max(1);
        self.next_id = event_id + 1;
        let event = OperationalEvent {
            event_id,
            subsystem: subsystem.to_string(),
            kind: kind.to_string(),
            occurred_epoch_ms: crate::helpers::time::now_unix_ms_u64() as u128,
            detail,
        };
        if self.events.len() >= OPERATIONAL_EVENT_CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(event);
        event_id
    }

    /// Return the most recent events (newest last), optionally filtered by
    /// subsystem, capped at `limit`.
    pub(crate) fn recent(&self, subsystem: Option<&str>, limit: usize) -> Vec<OperationalEvent> {
        let mut out: Vec<OperationalEvent> = self
            .events
            .iter()
            .filter(|e| subsystem.map(|s| e.subsystem.eq_ignore_ascii_case(s)).unwrap_or(true))
            .cloned()
            .collect();
        if out.len() > limit {
            out = out.split_off(out.len() - limit);
        }
        out
    }

    pub(crate) fn len(&self) -> usize {
        self.events.len()
    }
}

/// Emit an operational event into the shared stream. Best-effort: a poisoned
/// lock is silently ignored so observability never blocks a hot path.
pub(crate) fn emit_operational_event(
    state: &AppState,
    subsystem: &str,
    kind: &str,
    detail: serde_json::Value,
) {
    if let Ok(mut stream) = state.ops.operational_events.lock() {
        stream.emit(subsystem, kind, detail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_and_filters_by_subsystem() {
        let mut s = OperationalEventStream::new();
        s.emit("raft", "leader_elected", serde_json::json!({"term": 2}));
        s.emit("ingest", "batch_complete", serde_json::json!({"rows": 10}));
        s.emit("raft", "leader_elected", serde_json::json!({"term": 3}));
        assert_eq!(s.len(), 3);
        assert_eq!(s.recent(Some("raft"), 10).len(), 2);
        assert_eq!(s.recent(Some("ingest"), 10).len(), 1);
        assert_eq!(s.recent(None, 10).len(), 3);
    }

    #[test]
    fn ring_buffer_evicts_oldest() {
        let mut s = OperationalEventStream::new();
        for i in 0..(OPERATIONAL_EVENT_CAPACITY + 5) {
            s.emit("x", "k", serde_json::json!({ "i": i }));
        }
        assert_eq!(s.len(), OPERATIONAL_EVENT_CAPACITY);
        // Oldest evicted: first retained event id is 6.
        let first = s.recent(None, OPERATIONAL_EVENT_CAPACITY)[0].event_id;
        assert_eq!(first, 6);
    }

    #[test]
    fn recent_respects_limit() {
        let mut s = OperationalEventStream::new();
        for i in 0..10 {
            s.emit("x", "k", serde_json::json!({ "i": i }));
        }
        let last3 = s.recent(None, 3);
        assert_eq!(last3.len(), 3);
        assert_eq!(last3[2].event_id, 10);
    }
}
