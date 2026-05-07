#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-store";

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod columnar;
pub mod constraints;
pub mod ddl_catalog;
pub mod htap_sync;
pub mod index;
pub mod mvcc;
// S7-001/002: trigger framework
pub mod triggers;
// S7-003: trigger emitters
pub mod trigger_emitter;
pub mod wal_adapter;
use wal_adapter::{WalAdapter, WalAdapterError};

pub use triggers::{
    DdlTriggerDefinition, TriggerDefinition, TriggerEvent, TriggerGranularity, TriggerRegistry,
};
pub use trigger_emitter::{LoggingTriggerEmitter, NoOpTriggerEmitter, TriggerEmitter};

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
    /// Phase 2.1 — persisted SQL statements per kind. Vec preserves append
    /// order; for the in-memory engine this is process-local but matches
    /// the on-disk semantics of the RocksDB engine for testing.
    sql_streams: HashMap<&'static str, Vec<String>>,
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

    pub fn append_mutation_with_adapter<A: WalAdapter>(
        &mut self,
        key: &str,
        value: &str,
        adapter: &A,
    ) -> Result<WalRecord, WalAdapterError> {
        let record = self.append_mutation(key, value);
        if self.config.wal_enabled {
            adapter.append(&record)?;
        }
        Ok(record)
    }

    pub fn recover_from_records(
        config: DurabilityConfig,
        records: &[WalRecord],
    ) -> InMemoryDurabilityEngine {
        let mut engine = InMemoryDurabilityEngine::with_config(config);
        for record in records {
            engine.sequence = engine.sequence.max(record.sequence);
            engine.store.insert(record.key.clone(), record.value.clone());
            if engine.config.wal_enabled {
                engine.wal.push(record.clone());
            }
        }
        engine
    }

    pub fn recover_from_adapter<A: WalAdapter>(
        config: DurabilityConfig,
        adapter: &A,
    ) -> Result<InMemoryDurabilityEngine, WalAdapterError> {
        let records = adapter.read_all()?;
        Ok(Self::recover_from_records(config, &records))
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

    /// Returns the current WAL record list (in append order).
    pub fn wal_records(&self) -> &[WalRecord] {
        &self.wal
    }

    /// Returns the number of checkpoints taken so far.
    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2 — DurabilityEngine trait + BoxedDurabilityEngine shim
//
// The trait abstracts the single-file in-memory engine and the new RocksDB
// engine behind one stable surface so the service can switch backends at
// boot via runtime_config without touching ~179 call sites in main.rs.
//
// Design notes:
// - All 6 methods used in main.rs are mirrored here. wal_records() returns
//   `&[WalRecord]` for backwards compatibility — RocksDB-backed engines
//   keep an in-memory tail buffer for callers that iterate the recent
//   records (introspection / metrics / tests). The buffer is bounded; for
//   long-running WAL inspection use the engine-specific scan APIs.
// - BoxedDurabilityEngine is a thin newtype around Box<dyn DurabilityEngine>
//   so the AppState field type can be Arc<Mutex<BoxedDurabilityEngine>>
//   without dyn-Trait import noise at every call site.
// ─────────────────────────────────────────────────────────────────────────────

/// Which SQL stream a statement belongs to.
///
/// Persisted as a single byte in the on-disk format (`b'd' = Ddl`,
/// `b'm' = Dml`) so the encoding is forward-compatible if more streams
/// (e.g. RBAC, replication-only) are added later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SqlWalKind {
    /// Schema-changing SQL (CREATE / DROP / ALTER) — replayed into DdlCatalog.
    Ddl,
    /// Row-changing SQL (INSERT / UPDATE / DELETE) — replayed into PagedRowStore.
    Dml,
}

impl SqlWalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ddl => "ddl",
            Self::Dml => "dml",
        }
    }
}

/// Abstraction over durability engines. Phase 2.
pub trait DurabilityEngine: Send {
    /// Append `(key, value)` as the next mutation. Allocates a new WAL
    /// record with the next sequence number, persists it (the RocksDB impl
    /// fsyncs if `wal_fsync_on_commit` is set), and returns the record so
    /// the caller can inspect/log the assigned sequence.
    fn append_mutation(&mut self, key: &str, value: &str) -> WalRecord;

    /// Recent in-memory WAL tail (for introspection/tests). The RocksDB
    /// engine bounds this buffer; for full-history scans use engine-specific
    /// APIs.
    fn wal_records(&self) -> &[WalRecord];

    /// Highest-assigned sequence number.
    fn latest_sequence(&self) -> u64;

    /// Conditionally cut a checkpoint if the in-memory WAL has grown past
    /// the configured threshold. Returns the new manifest if one was taken.
    fn maybe_checkpoint(&mut self) -> Option<CheckpointManifest>;

    /// Unconditionally cut a checkpoint and return the new manifest.
    fn force_checkpoint(&mut self) -> CheckpointManifest;

    /// Total number of checkpoints taken so far (across reopens, for
    /// engines that persist them).
    fn checkpoint_count(&self) -> usize;

    /// Engine identifier for metrics and diagnostics. Returns one of:
    /// `"in_memory"` or `"rocksdb"`.
    fn engine_kind(&self) -> &'static str;

    // ── Phase 2.1: SQL-statement persistence for boot-time replay ──────────
    //
    // The legacy text-WAL files (state/ddl.wal, state/dml.wal) are the
    // current source of truth for replay. Phase 2.1 adds parallel
    // persistence into the durability engine so RocksDB can drive replay
    // and the text files can be retired.
    //
    // Default impls do nothing for engines that don't yet persist SQL —
    // callers fall back to the text-WAL path in that case. The in-memory
    // engine implements them with a Vec, the RocksDB engine with a
    // dedicated column family.

    /// Append a SQL statement to the durable stream of `kind`. The
    /// implementation is responsible for assigning a monotonically
    /// increasing per-kind sequence number and fsyncing if configured.
    /// Returns the assigned sequence (1-indexed).
    fn append_sql(&mut self, _kind: SqlWalKind, _sql: &str) -> u64 {
        0 // default: not implemented — caller falls back to text WAL
    }

    /// Iterate every persisted SQL statement of `kind` in append order.
    /// Returns an empty vec if the engine doesn't persist SQL or has none.
    fn iter_sql(&self, _kind: SqlWalKind) -> Vec<String> {
        Vec::new()
    }

    /// Total persisted SQL statements of `kind`. Returns 0 if the engine
    /// doesn't persist SQL.
    fn sql_count(&self, _kind: SqlWalKind) -> usize {
        0
    }

    /// Clear (truncate) the SQL stream of `kind`. Used by checkpoint flows
    /// once the catalog/row store has been snapshotted to a different
    /// medium. No-op for engines that don't persist SQL.
    fn clear_sql(&mut self, _kind: SqlWalKind) {}

    /// Whether this engine actually persists SQL statements (vs the default
    /// no-op impl). Used by the service to decide whether to skip the
    /// legacy text-WAL replay path.
    fn persists_sql(&self) -> bool {
        false
    }
}

impl DurabilityEngine for InMemoryDurabilityEngine {
    fn append_mutation(&mut self, key: &str, value: &str) -> WalRecord {
        InMemoryDurabilityEngine::append_mutation(self, key, value)
    }
    fn wal_records(&self) -> &[WalRecord] {
        InMemoryDurabilityEngine::wal_records(self)
    }
    fn latest_sequence(&self) -> u64 {
        InMemoryDurabilityEngine::latest_sequence(self)
    }
    fn maybe_checkpoint(&mut self) -> Option<CheckpointManifest> {
        InMemoryDurabilityEngine::maybe_checkpoint(self)
    }
    fn force_checkpoint(&mut self) -> CheckpointManifest {
        InMemoryDurabilityEngine::force_checkpoint(self)
    }
    fn checkpoint_count(&self) -> usize {
        InMemoryDurabilityEngine::checkpoint_count(self)
    }
    fn engine_kind(&self) -> &'static str {
        "in_memory"
    }

    fn append_sql(&mut self, kind: SqlWalKind, sql: &str) -> u64 {
        let stream = self.sql_streams.entry(kind.as_str()).or_default();
        stream.push(sql.to_string());
        stream.len() as u64
    }
    fn iter_sql(&self, kind: SqlWalKind) -> Vec<String> {
        self.sql_streams
            .get(kind.as_str())
            .cloned()
            .unwrap_or_default()
    }
    fn sql_count(&self, kind: SqlWalKind) -> usize {
        self.sql_streams
            .get(kind.as_str())
            .map(Vec::len)
            .unwrap_or(0)
    }
    fn clear_sql(&mut self, kind: SqlWalKind) {
        if let Some(stream) = self.sql_streams.get_mut(kind.as_str()) {
            stream.clear();
        }
    }
    fn persists_sql(&self) -> bool {
        true
    }
}

/// Newtype wrapping `Box<dyn DurabilityEngine>`. Lets the service hold a
/// single concrete field type while picking the engine at boot. The wrapper
/// forwards every method the service uses so the call sites in main.rs read
/// exactly the same as before — `wal.append_mutation(k, v)` etc.
///
/// Construction:
///   - `BoxedDurabilityEngine::in_memory(config)` — for tests and the
///     `vng` SQL/storage selector path (until the native VNG engine ships).
///   - `BoxedDurabilityEngine::rocksdb(path, config)` — for production with
///     `storage.engine = rocksdb`. Gated behind the `rocksdb` feature flag
///     of this crate so the dep is opt-in.
pub struct BoxedDurabilityEngine {
    inner: Box<dyn DurabilityEngine>,
}

impl BoxedDurabilityEngine {
    /// Wrap any concrete `DurabilityEngine` implementation.
    pub fn new<E: DurabilityEngine + 'static>(engine: E) -> Self {
        Self { inner: Box::new(engine) }
    }

    /// In-memory engine — non-durable; for tests and dev. Loses data on crash.
    pub fn in_memory(config: DurabilityConfig) -> Self {
        Self::new(InMemoryDurabilityEngine::with_config(config))
    }

    /// RocksDB engine — durable, fsync-honest. Only available when the
    /// `rocksdb` feature is enabled at compile time. The signature is kept
    /// here even when the feature is off so callers can write portable
    /// code; without the feature, `rocksdb()` is unimplemented and the
    /// service falls back to `in_memory()` with a warning logged at boot.
    #[cfg(feature = "rocksdb")]
    pub fn rocksdb(
        path: impl AsRef<std::path::Path>,
        config: DurabilityConfig,
    ) -> Result<Self, crate::rocksdb_engine::RocksDbEngineError> {
        let engine = crate::rocksdb_engine::RocksDbDurabilityEngine::open(path, config)?;
        Ok(Self::new(engine))
    }

    // ── Forwarded API (matches the methods used in services/main.rs) ────────

    pub fn append_mutation(&mut self, key: &str, value: &str) -> WalRecord {
        self.inner.append_mutation(key, value)
    }
    pub fn wal_records(&self) -> &[WalRecord] {
        self.inner.wal_records()
    }
    pub fn latest_sequence(&self) -> u64 {
        self.inner.latest_sequence()
    }
    pub fn maybe_checkpoint(&mut self) -> Option<CheckpointManifest> {
        self.inner.maybe_checkpoint()
    }
    pub fn force_checkpoint(&mut self) -> CheckpointManifest {
        self.inner.force_checkpoint()
    }
    pub fn checkpoint_count(&self) -> usize {
        self.inner.checkpoint_count()
    }
    pub fn engine_kind(&self) -> &'static str {
        self.inner.engine_kind()
    }

    // Phase 2.1 — SQL persistence forwarding.
    pub fn append_sql(&mut self, kind: SqlWalKind, sql: &str) -> u64 {
        self.inner.append_sql(kind, sql)
    }
    pub fn iter_sql(&self, kind: SqlWalKind) -> Vec<String> {
        self.inner.iter_sql(kind)
    }
    pub fn sql_count(&self, kind: SqlWalKind) -> usize {
        self.inner.sql_count(kind)
    }
    pub fn clear_sql(&mut self, kind: SqlWalKind) {
        self.inner.clear_sql(kind)
    }
    pub fn persists_sql(&self) -> bool {
        self.inner.persists_sql()
    }
}

impl Default for BoxedDurabilityEngine {
    fn default() -> Self {
        Self::in_memory(DurabilityConfig::default())
    }
}

impl std::fmt::Debug for BoxedDurabilityEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxedDurabilityEngine")
            .field("engine_kind", &self.inner.engine_kind())
            .field("latest_sequence", &self.inner.latest_sequence())
            .field("checkpoint_count", &self.inner.checkpoint_count())
            .finish()
    }
}

#[cfg(feature = "rocksdb")]
pub mod rocksdb_engine;


fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wal_adapter::FileWalAdapter;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_wal_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "vng-durability-replay-test-{}-{}.log",
            std::process::id(),
            nanos
        ))
    }

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

    #[test]
    fn recovers_state_from_wal_adapter_records() {
        let wal_path = unique_wal_path();
        let adapter = FileWalAdapter::new(&wal_path).expect("adapter");

        let mut writer = InMemoryDurabilityEngine::default();
        writer
            .append_mutation_with_adapter("tenant", "acme", &adapter)
            .expect("append first");
        writer
            .append_mutation_with_adapter("region", "us-east-1", &adapter)
            .expect("append second");

        let recovered =
            InMemoryDurabilityEngine::recover_from_adapter(DurabilityConfig::default(), &adapter)
                .expect("recover");
        assert_eq!(recovered.get("tenant"), Some("acme"));
        assert_eq!(recovered.get("region"), Some("us-east-1"));
        assert_eq!(recovered.latest_sequence(), 2);
        assert_eq!(recovered.wal_len(), 2);

        let _ = fs::remove_file(adapter.wal_path());
    }

    // ── Phase 2: BoxedDurabilityEngine shim ─────────────────────────────────

    #[test]
    fn boxed_engine_default_is_in_memory() {
        let boxed = BoxedDurabilityEngine::default();
        assert_eq!(boxed.engine_kind(), "in_memory");
        assert_eq!(boxed.latest_sequence(), 0);
        assert_eq!(boxed.checkpoint_count(), 0);
        assert!(boxed.wal_records().is_empty());
    }

    #[test]
    fn boxed_engine_forwards_append_and_records() {
        let mut boxed = BoxedDurabilityEngine::in_memory(DurabilityConfig::default());
        let r1 = boxed.append_mutation("k1", "v1");
        let r2 = boxed.append_mutation("k2", "v2");
        assert_eq!(r1.sequence, 1);
        assert_eq!(r2.sequence, 2);
        assert_eq!(boxed.latest_sequence(), 2);
        let records = boxed.wal_records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].key, "k1");
        assert_eq!(records[1].value, "v2");
    }

    #[test]
    fn boxed_engine_force_checkpoint_increments_count() {
        let mut boxed = BoxedDurabilityEngine::in_memory(DurabilityConfig::default());
        boxed.append_mutation("k1", "v1");
        let manifest = boxed.force_checkpoint();
        assert_eq!(manifest.checkpoint_id, 1);
        assert_eq!(manifest.last_sequence, 1);
        assert_eq!(boxed.checkpoint_count(), 1);
    }

    #[test]
    fn boxed_engine_maybe_checkpoint_respects_threshold() {
        // Threshold = 3 — first 2 appends shouldn't trigger; third should.
        let mut boxed = BoxedDurabilityEngine::in_memory(DurabilityConfig {
            wal_enabled: true,
            checkpoint_interval_seconds: 60,
            max_wal_records_before_checkpoint: 3,
        });
        boxed.append_mutation("k1", "v1");
        assert!(boxed.maybe_checkpoint().is_none());
        boxed.append_mutation("k2", "v2");
        assert!(boxed.maybe_checkpoint().is_none());
        boxed.append_mutation("k3", "v3");
        let m = boxed.maybe_checkpoint().expect("threshold reached");
        assert_eq!(m.checkpoint_id, 1);
        assert_eq!(m.last_sequence, 3);
        assert_eq!(boxed.checkpoint_count(), 1);
    }

    #[test]
    fn boxed_engine_wraps_arbitrary_engine() {
        // Ensure BoxedDurabilityEngine::new() accepts any concrete engine.
        let engine = InMemoryDurabilityEngine::with_config(DurabilityConfig {
            wal_enabled: false,
            ..Default::default()
        });
        let mut boxed = BoxedDurabilityEngine::new(engine);
        boxed.append_mutation("k", "v");
        // wal_enabled = false, so wal_records stays empty even after a write.
        assert!(boxed.wal_records().is_empty());
        assert_eq!(boxed.latest_sequence(), 1);
    }

    #[test]
    fn boxed_engine_debug_is_descriptive() {
        let boxed = BoxedDurabilityEngine::in_memory(DurabilityConfig::default());
        let dbg = format!("{boxed:?}");
        assert!(dbg.contains("in_memory"), "debug should expose engine_kind: {dbg}");
        assert!(dbg.contains("latest_sequence"));
    }

    /// Phase 2 integration test — exercise the exact pattern the service
    /// uses: `Arc<Mutex<BoxedDurabilityEngine>>`. Confirms the shim is
    /// `Send` and works correctly behind the lock.
    #[test]
    fn boxed_engine_works_behind_arc_mutex() {
        use std::sync::{Arc, Mutex};
        let engine: Arc<Mutex<BoxedDurabilityEngine>> = Arc::new(Mutex::new(
            BoxedDurabilityEngine::in_memory(DurabilityConfig::default())
        ));

        // Exactly the pattern used in main.rs:
        {
            let mut wal = engine.lock().expect("lock");
            wal.append_mutation("k1", "v1");
            wal.append_mutation("k2", "v2");
        }
        {
            let wal = engine.lock().expect("lock");
            assert_eq!(wal.latest_sequence(), 2);
            assert_eq!(wal.wal_records().len(), 2);
            assert_eq!(wal.checkpoint_count(), 0);
        }
        {
            let mut wal = engine.lock().expect("lock");
            let _ = wal.maybe_checkpoint();
            let m = wal.force_checkpoint();
            assert_eq!(m.checkpoint_id, 1);
            assert_eq!(wal.checkpoint_count(), 1);
        }
    }

    // ── Phase 2.1: SQL-statement persistence ────────────────────────────────

    #[test]
    fn append_sql_returns_increasing_sequences_per_kind() {
        let mut e = BoxedDurabilityEngine::in_memory(DurabilityConfig::default());
        // DDL stream sequences.
        let s1 = e.append_sql(SqlWalKind::Ddl, "CREATE TABLE t(id INT)");
        let s2 = e.append_sql(SqlWalKind::Ddl, "ALTER TABLE t ADD COLUMN name TEXT");
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        // DML stream is independent.
        let m1 = e.append_sql(SqlWalKind::Dml, "INSERT INTO t (id) VALUES (1)");
        assert_eq!(m1, 1);
        // Counts.
        assert_eq!(e.sql_count(SqlWalKind::Ddl), 2);
        assert_eq!(e.sql_count(SqlWalKind::Dml), 1);
    }

    #[test]
    fn iter_sql_preserves_append_order() {
        let mut e = BoxedDurabilityEngine::in_memory(DurabilityConfig::default());
        e.append_sql(SqlWalKind::Ddl, "first");
        e.append_sql(SqlWalKind::Ddl, "second");
        e.append_sql(SqlWalKind::Ddl, "third");
        let stmts = e.iter_sql(SqlWalKind::Ddl);
        assert_eq!(stmts, vec!["first", "second", "third"]);
    }

    #[test]
    fn iter_sql_empty_for_unused_kind() {
        let e = BoxedDurabilityEngine::in_memory(DurabilityConfig::default());
        assert!(e.iter_sql(SqlWalKind::Ddl).is_empty());
        assert!(e.iter_sql(SqlWalKind::Dml).is_empty());
        assert_eq!(e.sql_count(SqlWalKind::Ddl), 0);
    }

    #[test]
    fn clear_sql_truncates_only_named_stream() {
        let mut e = BoxedDurabilityEngine::in_memory(DurabilityConfig::default());
        e.append_sql(SqlWalKind::Ddl, "x");
        e.append_sql(SqlWalKind::Dml, "y");
        e.clear_sql(SqlWalKind::Ddl);
        assert_eq!(e.sql_count(SqlWalKind::Ddl), 0);
        assert_eq!(e.sql_count(SqlWalKind::Dml), 1);
    }

    #[test]
    fn persists_sql_reports_true_for_in_memory() {
        let e = BoxedDurabilityEngine::in_memory(DurabilityConfig::default());
        assert!(e.persists_sql(), "in-memory engine persists SQL streams in-process");
    }

    #[test]
    fn sql_wal_kind_as_str_is_stable() {
        // The string value goes into metric labels and on-disk encoding;
        // changing it would break operators' dashboards.
        assert_eq!(SqlWalKind::Ddl.as_str(), "ddl");
        assert_eq!(SqlWalKind::Dml.as_str(), "dml");
    }
}
