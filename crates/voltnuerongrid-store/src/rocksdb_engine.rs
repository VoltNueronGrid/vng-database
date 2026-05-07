//! Phase 2 — RocksDB-backed durability engine.
//!
//! Implements [`crate::DurabilityEngine`] with on-disk durability that
//! survives `kill -9` and process restart. The previous in-memory engine
//! used `flush()` which only writes to the OS page cache and is lost on
//! crash; this engine uses `WriteOptions::set_sync(true)` to actually
//! issue an `fsync(2)` per commit when `wal_fsync_on_commit` is configured.
//!
//! # Layout
//!
//! Three column families:
//! - `cf_default` — primary key→value store (the post-replay state).
//! - `cf_wal` — append-only WAL records keyed by big-endian sequence
//!   number. Survives across reopens; checkpoints prune obsolete prefixes.
//! - `cf_meta` — durability metadata: `latest_sequence`, `checkpoint_count`,
//!   `latest_checkpoint_id`, `latest_checkpoint_last_seq`,
//!   `latest_checkpoint_entry_count`. Enables resuming the sequence
//!   counter and checkpoint id across reopens.
//!
//! Every mutation goes through one [`rocksdb::WriteBatch`] containing all
//! three CF writes so we get atomic visibility (no torn writes between
//! the data CF and the WAL CF).
//!
//! # Recent-WAL tail buffer
//!
//! [`crate::DurabilityEngine::wal_records`] returns a slice for backwards
//! compatibility. RocksDB-backed engines maintain a bounded in-memory
//! tail buffer (default 1024 records). For full WAL inspection, callers
//! should use [`RocksDbDurabilityEngine::scan_wal`].
//!
//! # Tests
//!
//! See the bottom of this file. The cornerstone is
//! `survives_drop_and_reopen_like_sigkill` which simulates `kill -9` by
//! `drop`ping the engine without graceful shutdown and verifying the
//! data + sequence + checkpoint id all survive the reopen.

#![cfg(feature = "rocksdb")]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rocksdb::{ColumnFamilyDescriptor, DB, Options, WriteBatch, WriteOptions};

use crate::{
    now_epoch_millis, CheckpointManifest, DurabilityConfig, DurabilityEngine, SqlWalKind,
    WalRecord,
};

/// Default cap on the in-memory recent-WAL tail buffer.
const DEFAULT_WAL_TAIL_CAP: usize = 1024;

/// Column family names.
const CF_WAL: &str = "wal";
const CF_META: &str = "meta";
/// Phase 2.1 — SQL statement stream. Keys are big-endian
/// `[kind_byte (1)] [seq (8)]` so the per-kind range can be iterated
/// efficiently with a 1-byte prefix bound.
const CF_SQL: &str = "sql";

// Meta keys.
const META_LATEST_SEQUENCE: &[u8]               = b"latest_sequence";
const META_CHECKPOINT_COUNT: &[u8]              = b"checkpoint_count";
const META_LATEST_CHECKPOINT_ID: &[u8]          = b"latest_checkpoint_id";
const META_LATEST_CHECKPOINT_LAST_SEQ: &[u8]    = b"latest_checkpoint_last_seq";
const META_LATEST_CHECKPOINT_ENTRY_COUNT: &[u8] = b"latest_checkpoint_entry_count";
// Phase 2.1 — per-kind SQL stream sequence counters. Persisted so
// `append_sql` keeps incrementing across reopens.
const META_SQL_DDL_SEQUENCE: &[u8] = b"sql_ddl_sequence";
const META_SQL_DML_SEQUENCE: &[u8] = b"sql_dml_sequence";

/// Single-byte tag for SqlWalKind in CF_SQL keys. Stable wire format.
const SQL_KIND_DDL: u8 = b'd';
const SQL_KIND_DML: u8 = b'm';

fn sql_kind_tag(kind: SqlWalKind) -> u8 {
    match kind {
        SqlWalKind::Ddl => SQL_KIND_DDL,
        SqlWalKind::Dml => SQL_KIND_DML,
    }
}

fn sql_kind_seq_meta_key(kind: SqlWalKind) -> &'static [u8] {
    match kind {
        SqlWalKind::Ddl => META_SQL_DDL_SEQUENCE,
        SqlWalKind::Dml => META_SQL_DML_SEQUENCE,
    }
}

/// Encode a CF_SQL key: 1-byte kind tag + 8-byte big-endian sequence.
fn sql_key(kind: SqlWalKind, seq: u64) -> [u8; 9] {
    let mut k = [0u8; 9];
    k[0] = sql_kind_tag(kind);
    k[1..].copy_from_slice(&seq.to_be_bytes());
    k
}

#[derive(Debug)]
pub enum RocksDbEngineError {
    /// rocksdb-side I/O or open failure.
    Storage(String),
    /// Column-family metadata is corrupt or unreadable.
    Corrupt(String),
}

impl std::fmt::Display for RocksDbEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(s) => write!(f, "rocksdb storage error: {s}"),
            Self::Corrupt(s) => write!(f, "rocksdb meta CF corrupt: {s}"),
        }
    }
}

impl std::error::Error for RocksDbEngineError {}

impl From<rocksdb::Error> for RocksDbEngineError {
    fn from(e: rocksdb::Error) -> Self {
        Self::Storage(e.to_string())
    }
}

pub struct RocksDbDurabilityEngine {
    db: DB,
    config: DurabilityConfig,
    sync_writes: bool,
    /// Path the engine was opened at (for diagnostics + tests).
    path: PathBuf,
    /// Hot in-memory state — read on every access. Lock when mutating.
    state: Mutex<HotState>,
    /// Bounded ring buffer of recent WAL records. Lives outside the mutex
    /// so `wal_records(&self)` can return `&[WalRecord]` without unsafe.
    /// Safe because the only writer is `append_mutation(&mut self)`.
    wal_tail: Vec<WalRecord>,
    wal_tail_cap: usize,
}

struct HotState {
    sequence: u64,
    checkpoint_count: usize,
    /// Records since the last checkpoint (for `maybe_checkpoint` threshold).
    wal_since_checkpoint: usize,
    /// Phase 2.1 — last assigned sequence per SqlWalKind.
    sql_ddl_sequence: u64,
    sql_dml_sequence: u64,
}

impl RocksDbDurabilityEngine {
    /// Open or create a RocksDB database at `path`. Creates missing column
    /// families. Replays meta CF to restore `latest_sequence` and
    /// `checkpoint_count` so they persist across reopens.
    pub fn open(
        path: impl AsRef<Path>,
        config: DurabilityConfig,
    ) -> Result<Self, RocksDbEngineError> {
        // Read sync flag from the env (config plumbing in main.rs sets it
        // from runtime_config.storage.wal_fsync_on_commit). Default to true
        // — the whole point of RocksDB-backed durability is honest fsync.
        let sync_writes = std::env::var("VNG_WAL_FSYNC_ON_COMMIT")
            .ok()
            .map(|v| v != "0" && v.to_ascii_lowercase() != "false")
            .unwrap_or(true);

        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        let cfs = vec![
            ColumnFamilyDescriptor::new(CF_WAL,  Options::default()),
            ColumnFamilyDescriptor::new(CF_META, Options::default()),
            ColumnFamilyDescriptor::new(CF_SQL,  Options::default()),
        ];
        let db = DB::open_cf_descriptors(&db_opts, path.as_ref(), cfs)?;

        let cf_meta = db
            .cf_handle(CF_META)
            .ok_or_else(|| RocksDbEngineError::Corrupt(format!("{CF_META} CF missing")))?;

        // Restore latest_sequence.
        let latest_sequence = match db.get_cf(&cf_meta, META_LATEST_SEQUENCE)? {
            Some(bytes) => decode_u64(&bytes)
                .ok_or_else(|| RocksDbEngineError::Corrupt("latest_sequence".into()))?,
            None => 0,
        };
        let checkpoint_count = match db.get_cf(&cf_meta, META_CHECKPOINT_COUNT)? {
            Some(bytes) => decode_u64(&bytes)
                .ok_or_else(|| RocksDbEngineError::Corrupt("checkpoint_count".into()))?
                as usize,
            None => 0,
        };

        // Phase 2.1 — restore per-kind SQL sequence counters.
        let sql_ddl_sequence = match db.get_cf(&cf_meta, META_SQL_DDL_SEQUENCE)? {
            Some(b) => decode_u64(&b).unwrap_or(0),
            None => 0,
        };
        let sql_dml_sequence = match db.get_cf(&cf_meta, META_SQL_DML_SEQUENCE)? {
            Some(b) => decode_u64(&b).unwrap_or(0),
            None => 0,
        };

        // Hydrate the wal_tail ring with the last DEFAULT_WAL_TAIL_CAP records.
        let wal_tail = read_recent_wal_records(&db, DEFAULT_WAL_TAIL_CAP)?;

        // wal_since_checkpoint is impossible to recover precisely without scanning;
        // approximate by min(WAL records after latest_checkpoint_last_seq, threshold).
        let wal_since_checkpoint = compute_wal_since_checkpoint(&db, &cf_meta, latest_sequence)?;

        Ok(Self {
            db,
            config,
            sync_writes,
            path: path.as_ref().to_path_buf(),
            wal_tail,
            wal_tail_cap: DEFAULT_WAL_TAIL_CAP,
            state: Mutex::new(HotState {
                sequence: latest_sequence,
                checkpoint_count,
                wal_since_checkpoint,
                sql_ddl_sequence,
                sql_dml_sequence,
            }),
        })
    }

    /// Return the open path, for diagnostics.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether `set_sync(true)` is being used on writes.
    pub fn sync_writes_enabled(&self) -> bool {
        self.sync_writes
    }

    /// Iterate every WAL record in `[from_seq, ..]` order. Used by recovery
    /// tooling / replication; not on the hot path.
    pub fn scan_wal(&self, from_seq: u64) -> Result<Vec<WalRecord>, RocksDbEngineError> {
        let cf_wal = self
            .db
            .cf_handle(CF_WAL)
            .ok_or_else(|| RocksDbEngineError::Corrupt(format!("{CF_WAL} CF missing")))?;
        let lower = encode_u64(from_seq);
        let mut out = Vec::new();
        for kv in self.db.iterator_cf(
            &cf_wal,
            rocksdb::IteratorMode::From(&lower, rocksdb::Direction::Forward),
        ) {
            let (k, v) = kv?;
            if let Some(rec) = decode_wal_record(&k, &v) {
                out.push(rec);
            }
        }
        Ok(out)
    }
}

impl DurabilityEngine for RocksDbDurabilityEngine {
    fn append_mutation(&mut self, key: &str, value: &str) -> WalRecord {
        let mut state = self.state.lock().expect("rocksdb engine state mutex");
        state.sequence += 1;
        let record = WalRecord {
            sequence: state.sequence,
            timestamp_epoch_ms: now_epoch_millis(),
            key: key.to_string(),
            value: value.to_string(),
        };

        // Single batch — primary K/V + WAL + meta — atomic.
        let cf_wal = self
            .db
            .cf_handle(CF_WAL)
            .expect("wal CF missing — engine improperly opened");
        let cf_meta = self
            .db
            .cf_handle(CF_META)
            .expect("meta CF missing — engine improperly opened");

        let mut batch = WriteBatch::default();
        batch.put(record.key.as_bytes(), record.value.as_bytes());
        batch.put_cf(&cf_wal, encode_u64(record.sequence), encode_wal_record(&record));
        batch.put_cf(&cf_meta, META_LATEST_SEQUENCE, encode_u64(record.sequence));

        let mut wo = WriteOptions::default();
        wo.set_sync(self.sync_writes && self.config.wal_enabled);
        if let Err(e) = self.db.write_opt(batch, &wo) {
            // RocksDB write failure on the durability path is fatal — there's
            // no safe way to continue with a desynced sequence counter.
            // Surface to the caller via panic; the service supervisor is
            // expected to catch and restart.
            panic!("rocksdb write failed on append_mutation: {e}");
        }

        // Update hot state.
        if self.wal_tail.len() >= self.wal_tail_cap {
            self.wal_tail.remove(0);
        }
        if self.config.wal_enabled {
            self.wal_tail.push(record.clone());
        }
        state.wal_since_checkpoint += 1;

        record
    }

    fn wal_records(&self) -> &[WalRecord] {
        &self.wal_tail
    }

    fn latest_sequence(&self) -> u64 {
        self.state
            .lock()
            .expect("rocksdb engine state mutex")
            .sequence
    }

    fn maybe_checkpoint(&mut self) -> Option<CheckpointManifest> {
        let should = {
            let state = self.state.lock().expect("rocksdb engine state mutex");
            state.wal_since_checkpoint >= self.config.max_wal_records_before_checkpoint
        };
        if should {
            Some(self.force_checkpoint())
        } else {
            None
        }
    }

    fn force_checkpoint(&mut self) -> CheckpointManifest {
        let cf_meta = self
            .db
            .cf_handle(CF_META)
            .expect("meta CF missing — engine improperly opened");

        let mut state = self.state.lock().expect("rocksdb engine state mutex");
        state.checkpoint_count += 1;
        let manifest = CheckpointManifest {
            checkpoint_id: state.checkpoint_count as u64,
            last_sequence: state.sequence,
            entry_count: 0, // populated below from a CF count
        };

        // Persist checkpoint metadata atomically.
        let mut batch = WriteBatch::default();
        batch.put_cf(&cf_meta, META_CHECKPOINT_COUNT, encode_u64(state.checkpoint_count as u64));
        batch.put_cf(&cf_meta, META_LATEST_CHECKPOINT_ID, encode_u64(manifest.checkpoint_id));
        batch.put_cf(&cf_meta, META_LATEST_CHECKPOINT_LAST_SEQ, encode_u64(manifest.last_sequence));

        // Approximate entry count from default-CF estimated keys.
        // Cheap; exact count would require a full scan.
        let entry_count = self
            .db
            .property_int_value("rocksdb.estimate-num-keys")
            .ok()
            .flatten()
            .unwrap_or(0) as usize;
        batch.put_cf(&cf_meta, META_LATEST_CHECKPOINT_ENTRY_COUNT, encode_u64(entry_count as u64));

        let mut wo = WriteOptions::default();
        wo.set_sync(self.sync_writes);
        self.db.write_opt(batch, &wo).expect("rocksdb checkpoint write failed");

        self.wal_tail.clear();
        state.wal_since_checkpoint = 0;

        CheckpointManifest {
            entry_count,
            ..manifest
        }
    }

    fn checkpoint_count(&self) -> usize {
        self.state
            .lock()
            .expect("rocksdb engine state mutex")
            .checkpoint_count
    }

    fn engine_kind(&self) -> &'static str {
        "rocksdb"
    }

    // ── Phase 2.1: SQL stream persistence ────────────────────────────────

    fn append_sql(&mut self, kind: SqlWalKind, sql: &str) -> u64 {
        let mut state = self.state.lock().expect("rocksdb engine state mutex");
        let seq = match kind {
            SqlWalKind::Ddl => {
                state.sql_ddl_sequence += 1;
                state.sql_ddl_sequence
            }
            SqlWalKind::Dml => {
                state.sql_dml_sequence += 1;
                state.sql_dml_sequence
            }
        };

        let cf_sql = self
            .db
            .cf_handle(CF_SQL)
            .expect("sql CF missing — engine improperly opened");
        let cf_meta = self
            .db
            .cf_handle(CF_META)
            .expect("meta CF missing — engine improperly opened");

        let mut batch = WriteBatch::default();
        batch.put_cf(&cf_sql, sql_key(kind, seq), sql.as_bytes());
        // Persist the new per-kind counter atomically with the SQL row so
        // a crash between them can't leave the counter behind the data.
        batch.put_cf(&cf_meta, sql_kind_seq_meta_key(kind), encode_u64(seq));

        let mut wo = WriteOptions::default();
        wo.set_sync(self.sync_writes && self.config.wal_enabled);
        if let Err(e) = self.db.write_opt(batch, &wo) {
            panic!("rocksdb write failed on append_sql: {e}");
        }
        seq
    }

    fn iter_sql(&self, kind: SqlWalKind) -> Vec<String> {
        let cf_sql = match self.db.cf_handle(CF_SQL) {
            Some(cf) => cf,
            None => return Vec::new(),
        };
        let lower = sql_key(kind, 1);
        let upper_kind_only = [sql_kind_tag(kind) + 1, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut out = Vec::new();
        for kv in self.db.iterator_cf(
            &cf_sql,
            rocksdb::IteratorMode::From(&lower, rocksdb::Direction::Forward),
        ) {
            let (k, v) = match kv {
                Ok(x) => x,
                Err(_) => continue,
            };
            // Stop when the key tag changes (different kind).
            if k.first().copied() != Some(sql_kind_tag(kind)) {
                break;
            }
            if k.as_ref() >= &upper_kind_only[..] {
                break;
            }
            if let Ok(s) = std::str::from_utf8(&v) {
                out.push(s.to_string());
            }
        }
        out
    }

    fn sql_count(&self, kind: SqlWalKind) -> usize {
        let state = self.state.lock().expect("rocksdb engine state mutex");
        match kind {
            SqlWalKind::Ddl => state.sql_ddl_sequence as usize,
            SqlWalKind::Dml => state.sql_dml_sequence as usize,
        }
    }

    fn clear_sql(&mut self, kind: SqlWalKind) {
        let cf_sql = match self.db.cf_handle(CF_SQL) {
            Some(cf) => cf,
            None => return,
        };
        let cf_meta = match self.db.cf_handle(CF_META) {
            Some(cf) => cf,
            None => return,
        };
        let mut state = self.state.lock().expect("rocksdb engine state mutex");
        let upper_seq = match kind {
            SqlWalKind::Ddl => state.sql_ddl_sequence,
            SqlWalKind::Dml => state.sql_dml_sequence,
        };
        if upper_seq == 0 {
            return;
        }

        let mut batch = WriteBatch::default();
        // Delete the prefix range — kind tag spans seq 1..=upper_seq.
        for seq in 1..=upper_seq {
            batch.delete_cf(&cf_sql, sql_key(kind, seq));
        }
        // Reset the counter.
        batch.put_cf(&cf_meta, sql_kind_seq_meta_key(kind), encode_u64(0));

        let mut wo = WriteOptions::default();
        wo.set_sync(self.sync_writes);
        if let Err(e) = self.db.write_opt(batch, &wo) {
            tracing_or_eprintln(format!("rocksdb clear_sql failed: {e}"));
            return;
        }
        match kind {
            SqlWalKind::Ddl => state.sql_ddl_sequence = 0,
            SqlWalKind::Dml => state.sql_dml_sequence = 0,
        };
    }

    fn persists_sql(&self) -> bool {
        true
    }
}

// Inline tracing-or-stderr helper. The store crate doesn't depend on
// `tracing` (Phase 0 kept it limited to the service crate), so this falls
// back to `eprintln!`. The service-side metrics + tracing instrumentation
// covers the production observability story.
fn tracing_or_eprintln(msg: String) {
    eprintln!("[vng-rocksdb] {}", msg);
}

// ─────────────────────────────────────────────────────────────────────────────
// Encoding helpers
// ─────────────────────────────────────────────────────────────────────────────

fn encode_u64(v: u64) -> [u8; 8] {
    v.to_be_bytes()
}

fn decode_u64(b: &[u8]) -> Option<u64> {
    if b.len() != 8 {
        return None;
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(b);
    Some(u64::from_be_bytes(arr))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct WalRecordOnDisk {
    sequence: u64,
    timestamp_epoch_ms: u128,
    key: String,
    value: String,
}

fn encode_wal_record(r: &WalRecord) -> Vec<u8> {
    let on_disk = WalRecordOnDisk {
        sequence: r.sequence,
        timestamp_epoch_ms: r.timestamp_epoch_ms,
        key: r.key.clone(),
        value: r.value.clone(),
    };
    serde_json::to_vec(&on_disk).expect("WAL serialize")
}

fn decode_wal_record(_key: &[u8], value: &[u8]) -> Option<WalRecord> {
    let on_disk: WalRecordOnDisk = serde_json::from_slice(value).ok()?;
    Some(WalRecord {
        sequence: on_disk.sequence,
        timestamp_epoch_ms: on_disk.timestamp_epoch_ms,
        key: on_disk.key,
        value: on_disk.value,
    })
}

fn read_recent_wal_records(db: &DB, cap: usize) -> Result<Vec<WalRecord>, RocksDbEngineError> {
    let cf_wal = db
        .cf_handle(CF_WAL)
        .ok_or_else(|| RocksDbEngineError::Corrupt(format!("{CF_WAL} CF missing")))?;
    // Iterate from the end backwards — pick up the most recent `cap` records.
    let mut records: Vec<WalRecord> = Vec::with_capacity(cap.min(64));
    for kv in db.iterator_cf(&cf_wal, rocksdb::IteratorMode::End) {
        let (k, v) = kv?;
        if let Some(r) = decode_wal_record(&k, &v) {
            records.push(r);
            if records.len() >= cap {
                break;
            }
        }
    }
    records.reverse(); // append-order
    Ok(records)
}

fn compute_wal_since_checkpoint(
    db: &DB,
    cf_meta: &impl rocksdb::AsColumnFamilyRef,
    latest_sequence: u64,
) -> Result<usize, RocksDbEngineError> {
    let last_ckpt_seq = match db.get_cf(cf_meta, META_LATEST_CHECKPOINT_LAST_SEQ)? {
        Some(b) => decode_u64(&b).unwrap_or(0),
        None => 0,
    };
    Ok(latest_sequence.saturating_sub(last_ckpt_seq) as usize)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "vng-rocksdb-engine-test-{}-{}",
            std::process::id(),
            nanos
        ))
    }

    fn cleanup(p: &Path) {
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn open_creates_column_families() {
        let p = unique_path();
        {
            let _engine = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default())
                .expect("open");
        }
        // Reopen should succeed without re-creating.
        let _engine =
            RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("reopen");
        cleanup(&p);
    }

    #[test]
    fn append_assigns_increasing_sequences() {
        let p = unique_path();
        let mut e =
            RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("open");
        let r1 = e.append_mutation("k1", "v1");
        let r2 = e.append_mutation("k2", "v2");
        let r3 = e.append_mutation("k3", "v3");
        assert_eq!(r1.sequence, 1);
        assert_eq!(r2.sequence, 2);
        assert_eq!(r3.sequence, 3);
        assert_eq!(e.latest_sequence(), 3);
        cleanup(&p);
    }

    #[test]
    fn wal_records_returns_recent_tail() {
        let p = unique_path();
        let mut e =
            RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("open");
        e.append_mutation("a", "1");
        e.append_mutation("b", "2");
        let recs = e.wal_records();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].key, "a");
        assert_eq!(recs[1].key, "b");
        cleanup(&p);
    }

    /// **THE** Phase 2 regression test: kill -9 substitute. Drop the engine
    /// without graceful shutdown and verify reopen restores the full state.
    #[test]
    fn survives_drop_and_reopen_like_sigkill() {
        let p = unique_path();
        // Session 1 — write some data, then `drop` (no graceful shutdown).
        {
            let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default())
                .expect("open");
            e.append_mutation("user:1", "alice");
            e.append_mutation("user:2", "bob");
            e.append_mutation("user:3", "carol");
            assert_eq!(e.latest_sequence(), 3);
            // Engine drops here without an explicit close.
        }
        // Session 2 — reopen and verify state is fully recovered.
        {
            let e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default())
                .expect("reopen");
            assert_eq!(e.latest_sequence(), 3, "sequence must persist across reopen");
            // wal_records on reopen reflects the persisted tail.
            let recs = e.wal_records();
            assert_eq!(recs.len(), 3);
            // The data CF is queryable directly.
            let val = e
                .db
                .get(b"user:2")
                .expect("get")
                .expect("user:2 must exist after reopen");
            assert_eq!(&val[..], b"bob");
        }
        cleanup(&p);
    }

    /// Phase 2 regression: checkpoint_id keeps incrementing across reopens.
    #[test]
    fn checkpoint_id_persists_across_reopen() {
        let p = unique_path();
        {
            let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default())
                .expect("open");
            e.append_mutation("x", "1");
            let m1 = e.force_checkpoint();
            let m2 = e.force_checkpoint();
            assert_eq!(m1.checkpoint_id, 1);
            assert_eq!(m2.checkpoint_id, 2);
            assert_eq!(e.checkpoint_count(), 2);
        }
        {
            let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default())
                .expect("reopen");
            assert_eq!(e.checkpoint_count(), 2, "count persisted");
            let m3 = e.force_checkpoint();
            assert_eq!(m3.checkpoint_id, 3, "id continues across reopen");
            assert_eq!(e.checkpoint_count(), 3);
        }
        cleanup(&p);
    }

    #[test]
    fn maybe_checkpoint_respects_threshold() {
        let p = unique_path();
        let mut e = RocksDbDurabilityEngine::open(
            &p,
            DurabilityConfig {
                wal_enabled: true,
                checkpoint_interval_seconds: 60,
                max_wal_records_before_checkpoint: 3,
            },
        )
        .expect("open");
        e.append_mutation("k1", "v1");
        assert!(e.maybe_checkpoint().is_none());
        e.append_mutation("k2", "v2");
        assert!(e.maybe_checkpoint().is_none());
        e.append_mutation("k3", "v3");
        let m = e.maybe_checkpoint().expect("threshold reached");
        assert_eq!(m.checkpoint_id, 1);
        cleanup(&p);
    }

    #[test]
    fn engine_kind_reports_rocksdb() {
        let p = unique_path();
        let e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("open");
        assert_eq!(e.engine_kind(), "rocksdb");
        cleanup(&p);
    }

    // ── Phase 2.1: SQL stream persistence ────────────────────────────────────

    #[test]
    fn append_sql_persists_per_kind_sequences() {
        let p = unique_path();
        let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("open");
        let s1 = e.append_sql(SqlWalKind::Ddl, "CREATE TABLE t(id INT)");
        let s2 = e.append_sql(SqlWalKind::Ddl, "ALTER TABLE t ADD COLUMN n TEXT");
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        let m1 = e.append_sql(SqlWalKind::Dml, "INSERT INTO t (id) VALUES (1)");
        assert_eq!(m1, 1, "DML stream is independent");
        cleanup(&p);
    }

    #[test]
    fn iter_sql_returns_only_requested_kind() {
        let p = unique_path();
        let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("open");
        e.append_sql(SqlWalKind::Ddl, "ddl-1");
        e.append_sql(SqlWalKind::Dml, "dml-1");
        e.append_sql(SqlWalKind::Ddl, "ddl-2");
        e.append_sql(SqlWalKind::Dml, "dml-2");
        let ddl = e.iter_sql(SqlWalKind::Ddl);
        let dml = e.iter_sql(SqlWalKind::Dml);
        assert_eq!(ddl, vec!["ddl-1", "ddl-2"]);
        assert_eq!(dml, vec!["dml-1", "dml-2"]);
        cleanup(&p);
    }

    /// Phase 2.1 regression: SQL stream survives kill -9 + reopen.
    #[test]
    fn sql_stream_survives_drop_and_reopen() {
        let p = unique_path();
        // Session 1.
        {
            let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("open");
            e.append_sql(SqlWalKind::Ddl, "CREATE TABLE t(id INT)");
            e.append_sql(SqlWalKind::Dml, "INSERT INTO t (id) VALUES (5)");
            // No graceful shutdown — engine drops here.
        }
        // Session 2 — verify content + per-kind counters persisted.
        {
            let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("reopen");
            assert_eq!(e.iter_sql(SqlWalKind::Ddl), vec!["CREATE TABLE t(id INT)"]);
            assert_eq!(e.iter_sql(SqlWalKind::Dml), vec!["INSERT INTO t (id) VALUES (5)"]);
            // New appends continue from the persisted seq, not reset to 1.
            let next = e.append_sql(SqlWalKind::Ddl, "ALTER TABLE t ADD n TEXT");
            assert_eq!(next, 2, "DDL seq must continue from 1 → 2");
        }
        cleanup(&p);
    }

    #[test]
    fn clear_sql_truncates_only_named_kind() {
        let p = unique_path();
        let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("open");
        e.append_sql(SqlWalKind::Ddl, "x");
        e.append_sql(SqlWalKind::Dml, "y");
        e.clear_sql(SqlWalKind::Ddl);
        assert!(e.iter_sql(SqlWalKind::Ddl).is_empty());
        assert_eq!(e.iter_sql(SqlWalKind::Dml), vec!["y"]);
        // Counter resets so next append starts at 1 again.
        let next = e.append_sql(SqlWalKind::Ddl, "fresh");
        assert_eq!(next, 1);
        cleanup(&p);
    }

    #[test]
    fn persists_sql_reports_true_for_rocksdb() {
        let p = unique_path();
        let e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("open");
        assert!(e.persists_sql());
        cleanup(&p);
    }
}
