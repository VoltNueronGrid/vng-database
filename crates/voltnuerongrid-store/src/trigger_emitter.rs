// S7-003: Trigger → queue emitters.
//
// The `TriggerEmitter` trait abstracts the delivery of trigger fire events to
// downstream sinks (logging, Kafka, NATS, …).  Kafka and NATS adapters are
// planned for a future sprint; this module ships `LoggingTriggerEmitter` (for
// development and integration tests) and `NoOpTriggerEmitter` (for unit tests).

use crate::triggers::TriggerDefinition;

// ─── Core trait ───────────────────────────────────────────────────────────────

pub trait TriggerEmitter: Send + Sync {
    /// Called when a trigger fires.
    ///
    /// `trigger`       — the matching trigger definition.
    /// `event_payload` — serialised JSON string describing the operation context
    ///                   (old row, new row, operation type, timestamp, …).
    fn emit(
        &self,
        trigger: &TriggerDefinition,
        event_payload: &str,
    ) -> Result<(), String>;
}

// ─── LoggingTriggerEmitter ────────────────────────────────────────────────────

/// Emitter that writes a structured line to stderr.  Useful during development
/// and integration tests where a real message broker is unavailable.
pub struct LoggingTriggerEmitter;

impl TriggerEmitter for LoggingTriggerEmitter {
    fn emit(
        &self,
        trigger: &TriggerDefinition,
        event_payload: &str,
    ) -> Result<(), String> {
        eprintln!(
            r#"{{"component":"trigger_emitter","trigger_name":"{}","table":"{}","schema":"{}","event":"{}","payload":{}}}"#,
            trigger.name,
            trigger.table,
            trigger.schema,
            trigger.event.as_str(),
            event_payload,
        );
        Ok(())
    }
}

// ─── NoOpTriggerEmitter ───────────────────────────────────────────────────────

/// Emitter that silently discards all events.  Intended for unit tests where
/// side-effects must be suppressed.
pub struct NoOpTriggerEmitter;

impl TriggerEmitter for NoOpTriggerEmitter {
    fn emit(
        &self,
        _trigger: &TriggerDefinition,
        _event_payload: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

// ─── RecordingTriggerEmitter (Q-3) ────────────────────────────────────────────

/// Emitter that records every fired trigger name into a shared vector and bumps
/// a shared counter.  Intended for integration tests that assert a trigger
/// fired on a given DML operation.
#[derive(Clone, Default)]
pub struct RecordingTriggerEmitter {
    pub fired: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    pub count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl RecordingTriggerEmitter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of times any trigger fired.
    pub fn fire_count(&self) -> usize {
        self.count.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Snapshot of all fired trigger names, in fire order.
    pub fn fired_names(&self) -> Vec<String> {
        self.fired.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

impl TriggerEmitter for RecordingTriggerEmitter {
    fn emit(
        &self,
        trigger: &TriggerDefinition,
        _event_payload: &str,
    ) -> Result<(), String> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut g) = self.fired.lock() {
            g.push(trigger.name.clone());
        }
        Ok(())
    }
}

// ─── Future adapters (planned) ────────────────────────────────────────────────
//
// KafkaTriggerEmitter  — publishes to a Kafka topic per trigger/table.
//                        Planned for NT-S7 / queue-sink integration sprint.
//
// NatsTriggerEmitter   — publishes to a NATS subject.
//                        Planned for NT-S7 / queue-sink integration sprint.

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::triggers::{TriggerDefinition, TriggerEvent, TriggerGranularity};

    fn sample_trigger() -> TriggerDefinition {
        TriggerDefinition::new(
            "audit_insert",
            "orders",
            "public",
            TriggerEvent::AfterInsert,
            TriggerGranularity::Row,
            "EXECUTE FUNCTION audit_fn()",
        )
    }

    #[test]
    fn noop_emitter_always_ok() {
        let emitter = NoOpTriggerEmitter;
        assert!(emitter.emit(&sample_trigger(), r#"{"new_row":{"id":1}}"#).is_ok());
    }

    #[test]
    fn logging_emitter_returns_ok() {
        let emitter = LoggingTriggerEmitter;
        assert!(emitter.emit(&sample_trigger(), r#"{"new_row":{"id":42}}"#).is_ok());
    }
}
