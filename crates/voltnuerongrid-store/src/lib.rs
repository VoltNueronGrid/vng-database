#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-store";

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod htap_sync;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurabilityConfig {
    pub wal_enabled: bool,
    pub checkpoint_interval_seconds: u64,
    pub max_wal_records_before_checkpoint: usize,
}

impl Default for DurabilityConfig {
    fn default() -> Self {
        Self {
            wal_enabled: true,
            checkpoint_interval_seconds: 60,
            max_wal_records_before_checkpoint: 1_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalRecord {
    pub sequence: u64,
    pub timestamp_epoch_ms: u128,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointManifest {
    pub checkpoint_id: u64,
    pub last_sequence: u64,
    pub entry_count: usize,
}

#[derive(Debug, Default)]
pub struct InMemoryDurabilityEngine {
    config: DurabilityConfig,
    sequence: u64,
    wal: Vec<WalRecord>,
    store: HashMap<String, String>,
    checkpoints: Vec<CheckpointManifest>,
}

impl InMemoryDurabilityEngine {
    pub fn with_config(config: DurabilityConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    pub fn append_mutation(&mut self, key: &str, value: &str) -> WalRecord {
        self.sequence += 1;
        let record = WalRecord {
            sequence: self.sequence,
            timestamp_epoch_ms: now_epoch_millis(),
            key: key.to_string(),
            value: value.to_string(),
        };
        self.store.insert(record.key.clone(), record.value.clone());
        if self.config.wal_enabled {
            self.wal.push(record.clone());
        }
        record
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.store.get(key).map(String::as_str)
    }

    pub fn wal_len(&self) -> usize {
        self.wal.len()
    }

    pub fn latest_sequence(&self) -> u64 {
        self.sequence
    }

    pub fn maybe_checkpoint(&mut self) -> Option<CheckpointManifest> {
        if self.wal.len() < self.config.max_wal_records_before_checkpoint {
            return None;
        }
        Some(self.force_checkpoint())
    }

    pub fn force_checkpoint(&mut self) -> CheckpointManifest {
        let manifest = CheckpointManifest {
            checkpoint_id: self.checkpoints.len() as u64 + 1,
            last_sequence: self.sequence,
            entry_count: self.store.len(),
        };
        self.checkpoints.push(manifest.clone());
        self.wal.clear();
        manifest
    }

    pub fn latest_checkpoint(&self) -> Option<&CheckpointManifest> {
        self.checkpoints.last()
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
    fn appends_mutation_and_reads_latest_value() {
        let mut engine = InMemoryDurabilityEngine::default();
        let first = engine.append_mutation("region", "us-east-1");
        assert_eq!(first.sequence, 1);
        assert_eq!(engine.get("region"), Some("us-east-1"));
    }

    #[test]
    fn checkpoints_after_threshold() {
        let mut engine = InMemoryDurabilityEngine::with_config(DurabilityConfig {
            max_wal_records_before_checkpoint: 2,
            ..DurabilityConfig::default()
        });
        engine.append_mutation("k1", "v1");
        assert!(engine.maybe_checkpoint().is_none());

        engine.append_mutation("k2", "v2");
        let checkpoint = engine.maybe_checkpoint().expect("checkpoint expected");
        assert_eq!(checkpoint.last_sequence, 2);
        assert_eq!(engine.wal_len(), 0);
    }

    #[test]
    fn force_checkpoint_records_manifest() {
        let mut engine = InMemoryDurabilityEngine::default();
        engine.append_mutation("a", "1");
        let checkpoint = engine.force_checkpoint();
        assert_eq!(checkpoint.entry_count, 1);
        assert_eq!(
            engine
                .latest_checkpoint()
                .expect("checkpoint should exist")
                .checkpoint_id,
            1
        );
    }
}
