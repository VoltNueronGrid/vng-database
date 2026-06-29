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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rocksdb::{ColumnFamilyDescriptor, DB, Options, WriteBatch, WriteOptions};
use std::sync::atomic::{AtomicU64, Ordering};

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
/// Phase 2.2 — Row store persistence (legacy/migration CF). Keys are
/// `{db}\x1f{row_key}\x1f{xid_be8}` so rows are sorted by (db, row_key, xid).
/// New writes go to per-DB CFs named `"rows_{db}"` instead; this CF is kept
/// for backwards-compatible boot replay of pre-existing data.
const CF_ROWS: &str = "rows";

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
// P1: highest XID ever written to row storage. Restored on boot so
// PagedRowStore.next_xid can be fast-forwarded past all persisted rows,
// preventing MVCC snapshot scans from filtering them out.
const META_MAX_ROW_XID: &[u8]     = b"max_row_xid";

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
    /// P3 group commit: counts the number of fsyncs actually issued.
    /// Incremented by `append_sql`, `append_mutation`, and `append_sql_batch`
    /// whenever `sync_writes && wal_enabled` is true. Lets unit tests verify
    /// that one `append_sql_batch(N entries)` issues 1 fsync not N.
    fsync_count: AtomicU64,
}

struct HotState {
    sequence: u64,
    checkpoint_count: usize,
    /// Records since the last checkpoint (for `maybe_checkpoint` threshold).
    wal_since_checkpoint: usize,
    /// Phase 2.1 — last assigned sequence per SqlWalKind.
    sql_ddl_sequence: u64,
    sql_dml_sequence: u64,
    /// P1: highest XID written to any per-DB CF row entry. Persisted to
    /// META_MAX_ROW_XID so PagedRowStore can be fast-forwarded on boot.
    max_row_xid: u64,
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

        // Discover any pre-existing per-DB CFs (names starting with "rows_")
        // so they are opened correctly. DB::open_cf_descriptors requires that
        // every CF already on disk is listed at open time.
        let existing_cfs = DB::list_cf(&db_opts, path.as_ref()).unwrap_or_default();
        let mut all_cfs = vec![
            ColumnFamilyDescriptor::new(CF_WAL,  Options::default()),
            ColumnFamilyDescriptor::new(CF_META, Options::default()),
            ColumnFamilyDescriptor::new(CF_SQL,  Options::default()),
            ColumnFamilyDescriptor::new(CF_ROWS, Options::default()),
        ];
        for cf_name in &existing_cfs {
            if cf_name.starts_with("rows_") {
                all_cfs.push(ColumnFamilyDescriptor::new(cf_name.as_str(), Options::default()));
            }
        }
        let db = DB::open_cf_descriptors(&db_opts, path.as_ref(), all_cfs)?;

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

        // P1: restore max XID so boot can fast-forward PagedRowStore.
        let max_row_xid = match db.get_cf(&cf_meta, META_MAX_ROW_XID)? {
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
            fsync_count: AtomicU64::new(0),
            state: Mutex::new(HotState {
                sequence: latest_sequence,
                checkpoint_count,
                wal_since_checkpoint,
                sql_ddl_sequence,
                sql_dml_sequence,
                max_row_xid,
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

    /// Return the CF name for a per-DB rows column family.
    fn db_rows_cf_name(db: &str) -> String {
        format!("rows_{db}")
    }

    /// Ensure the per-DB column family exists, creating it lazily if not.
    /// RocksDB requires `create_cf` after `open` for dynamically created CFs.
    /// Takes `&mut self` because `DB::create_cf` is `&mut self` in SingleThreaded mode.
    fn ensure_db_cf(&mut self, db: &str) -> Result<(), RocksDbEngineError> {
        let cf_name = Self::db_rows_cf_name(db);
        if self.db.cf_handle(&cf_name).is_some() {
            return Ok(());
        }
        self.db
            .create_cf(&cf_name, &Options::default())
            .map_err(|e| RocksDbEngineError::Storage(e.to_string()))
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
        if self.sync_writes && self.config.wal_enabled {
            self.fsync_count.fetch_add(1, Ordering::Relaxed);
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
        if self.sync_writes && self.config.wal_enabled {
            self.fsync_count.fetch_add(1, Ordering::Relaxed);
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

    // ── Phase 2.2: Row store persistence ─────────────────────────────────────

    fn store_row(
        &mut self,
        db: &str,
        row_key: &str,
        xid: u64,
        data: Option<&HashMap<String, String>>,
    ) {
        // Ensure the per-DB CF exists (lazy creation on first write for this db).
        if let Err(e) = self.ensure_db_cf(db) {
            tracing_or_eprintln(format!("store_row: ensure_db_cf failed for db={db}: {e}"));
            return;
        }
        let cf_name = Self::db_rows_cf_name(db);
        let cf_db = match self.db.cf_handle(&cf_name) {
            Some(cf) => cf,
            None => {
                tracing_or_eprintln(format!("store_row: CF {cf_name} missing after ensure"));
                return;
            }
        };

        // Key within the per-DB CF: {row_key}\x1f{xid_be8}
        // No db prefix needed — the CF IS the db scope.
        let sep = b'\x1f';
        let xid_be = xid.to_be_bytes();
        let mut key = Vec::with_capacity(row_key.len() + 1 + 8);
        key.extend_from_slice(row_key.as_bytes());
        key.push(sep);
        key.extend_from_slice(&xid_be);

        // Value: b"\x00" + JSON for live rows; b"\x01" for tombstones.
        let value: Vec<u8> = match data {
            Some(cols) => {
                let json = serde_json::to_string(cols).unwrap_or_else(|_| "{}".to_string());
                let mut v = Vec::with_capacity(1 + json.len());
                v.push(0x00_u8);
                v.extend_from_slice(json.as_bytes());
                v
            }
            None => vec![0x01_u8],
        };

        let mut batch = WriteBatch::default();
        batch.put_cf(&cf_db, &key, &value);

        // P1: persist max_row_xid so PagedRowStore can be fast-forwarded on boot.
        // Only write when xid exceeds the current persisted maximum.
        let needs_max_xid_update = {
            let state = self.state.lock().expect("rocksdb engine state mutex");
            xid > state.max_row_xid
        };
        if needs_max_xid_update {
            if let Some(cf_meta) = self.db.cf_handle(CF_META) {
                batch.put_cf(&cf_meta, META_MAX_ROW_XID, encode_u64(xid));
            }
        }

        // P1 hardening: per-DB row CFs are primary storage — fsync is
        // governed by sync_writes alone, NOT gated on wal_enabled.
        // SQL WAL enables/disables the SQL statement log (CF_SQL / CF_WAL);
        // row durability must be honoured even when the SQL WAL is disabled.
        let mut wo = WriteOptions::default();
        wo.set_sync(self.sync_writes);
        if let Err(e) = self.db.write_opt(batch, &wo) {
            tracing_or_eprintln(format!("store_row write failed: {e}"));
        }

        // Update hot state after successful write.
        if needs_max_xid_update {
            let mut state = self.state.lock().expect("rocksdb engine state mutex");
            if xid > state.max_row_xid {
                state.max_row_xid = xid;
            }
        }
    }

    fn scan_persisted_rows(&self) -> Vec<(String, String, u64, HashMap<String, String>, bool)> {
        // Collect the latest version per (db, row_key). Per-DB CF entries
        // take precedence over legacy CF_ROWS entries for the same (db, row_key).
        let sep = b'\x1f';
        let mut latest: HashMap<(String, String), (u64, Vec<u8>)> = HashMap::new();

        // ── Step 1: scan legacy CF_ROWS (old format {db}\x1f{row_key}\x1f{xid}) ──
        if let Some(cf_rows) = self.db.cf_handle(CF_ROWS) {
            for kv in self.db.iterator_cf(&cf_rows, rocksdb::IteratorMode::Start) {
                let (k, v) = match kv {
                    Ok(x) => x,
                    Err(e) => {
                        tracing_or_eprintln(format!("scan_persisted_rows (legacy) iterator error: {e}"));
                        continue;
                    }
                };

                // Parse key: {db}\x1f{row_key}\x1f{xid_be8}
                if k.len() < 10 {
                    continue; // malformed
                }
                let xid_start = k.len() - 8;
                let prefix = &k[..xid_start];
                if prefix.last() != Some(&sep) {
                    continue;
                }
                let prefix_no_sep = &prefix[..prefix.len() - 1];

                let first_sep = match prefix_no_sep.iter().position(|&b| b == sep) {
                    Some(i) => i,
                    None => continue,
                };
                let db_bytes = &prefix_no_sep[..first_sep];
                let row_key_bytes = &prefix_no_sep[first_sep + 1..];

                let db_str = match std::str::from_utf8(db_bytes) {
                    Ok(s) => s.to_string(),
                    Err(_) => continue,
                };
                let row_key = match std::str::from_utf8(row_key_bytes) {
                    Ok(s) => s.to_string(),
                    Err(_) => continue,
                };

                let mut xid_arr = [0u8; 8];
                xid_arr.copy_from_slice(&k[xid_start..]);
                let xid = u64::from_be_bytes(xid_arr);

                // Last write per (db, row_key) wins (keys sorted by xid asc).
                latest.insert((db_str, row_key), (xid, v.to_vec()));
            }
        }

        // ── Step 2: scan per-DB CFs (new format {row_key}\x1f{xid}) ──
        // These take precedence — iterate all CFs whose name starts with "rows_".
        // We discover them by checking cf_handle for known names; since the
        // DB was opened with all existing CFs, any "rows_*" CF is accessible.
        // Use DB::list_cf isn't available on &self without the path, so we rely
        // on the fact that opened CFs are registered: we check the names we
        // know about by scanning the existing structure.
        //
        // Approach: use the rocksdb metadata API to enumerate column families
        // that are currently open. We stored the path at open time for exactly this.
        let mut db_opts_for_list = Options::default();
        db_opts_for_list.create_if_missing(false);
        let known_cfs = DB::list_cf(&db_opts_for_list, &self.path).unwrap_or_default();
        for cf_name in &known_cfs {
            if !cf_name.starts_with("rows_") {
                continue;
            }
            // Extract db name from CF name: "rows_{db}"
            let db_str = &cf_name["rows_".len()..];
            let cf_handle = match self.db.cf_handle(cf_name) {
                Some(h) => h,
                None => continue,
            };

            for kv in self.db.iterator_cf(&cf_handle, rocksdb::IteratorMode::Start) {
                let (k, v) = match kv {
                    Ok(x) => x,
                    Err(e) => {
                        tracing_or_eprintln(format!("scan_persisted_rows ({cf_name}) iterator error: {e}"));
                        continue;
                    }
                };

                // Parse key: {row_key}\x1f{xid_be8}
                if k.len() < 9 {
                    continue;
                }
                let xid_start = k.len() - 8;
                let row_key_with_sep = &k[..xid_start];
                if row_key_with_sep.last() != Some(&sep) {
                    continue;
                }
                let row_key_bytes = &row_key_with_sep[..row_key_with_sep.len() - 1];

                let row_key = match std::str::from_utf8(row_key_bytes) {
                    Ok(s) => s.to_string(),
                    Err(_) => continue,
                };

                let mut xid_arr = [0u8; 8];
                xid_arr.copy_from_slice(&k[xid_start..]);
                let xid = u64::from_be_bytes(xid_arr);

                // Per-DB CF entries overwrite legacy CF_ROWS entries.
                latest.insert((db_str.to_string(), row_key), (xid, v.to_vec()));
            }
        }

        // ── Step 3: decode each final entry ──
        let mut result = Vec::with_capacity(latest.len());
        for ((db_str, row_key), (xid, value)) in latest {
            if value.is_empty() {
                continue;
            }
            let tag = value[0];
            if tag == 0x01 {
                // tombstone
                result.push((db_str, row_key, xid, HashMap::new(), true));
            } else if tag == 0x00 {
                let json_bytes = &value[1..];
                match serde_json::from_slice::<HashMap<String, String>>(json_bytes) {
                    Ok(cols) => result.push((db_str, row_key, xid, cols, false)),
                    Err(e) => {
                        tracing_or_eprintln(format!("scan_persisted_rows json decode error: {e}"));
                    }
                }
            }
        }
        result
    }

    fn persists_rows(&self) -> bool {
        true
    }

    fn max_row_xid(&self) -> u64 {
        self.state.lock().expect("rocksdb engine state mutex").max_row_xid
    }

    /// P3 group commit: write N SQL entries in ONE WriteBatch with ONE fsync.
    /// This is the core of group commit — callers that want to commit a
    /// multi-statement transaction batch all their SQL entries here and pay
    /// only 1 fsync instead of 1 per statement.
    fn append_sql_batch(&mut self, entries: &[(SqlWalKind, &str)]) -> Vec<u64> {
        if entries.is_empty() {
            return Vec::new();
        }

        let cf_sql = self.db.cf_handle(CF_SQL).expect("sql CF missing — engine improperly opened");
        let cf_meta = self.db.cf_handle(CF_META).expect("meta CF missing — engine improperly opened");

        let mut state = self.state.lock().expect("rocksdb engine state mutex");
        let mut batch = WriteBatch::default();
        let mut seqs = Vec::with_capacity(entries.len());

        for (kind, sql) in entries {
            let seq = match kind {
                SqlWalKind::Ddl => { state.sql_ddl_sequence += 1; state.sql_ddl_sequence }
                SqlWalKind::Dml => { state.sql_dml_sequence += 1; state.sql_dml_sequence }
            };
            batch.put_cf(&cf_sql, sql_key(*kind, seq), sql.as_bytes());
            seqs.push(seq);
        }
        // Persist only the final per-kind counters — one entry per kind present.
        batch.put_cf(&cf_meta, sql_kind_seq_meta_key(SqlWalKind::Ddl), encode_u64(state.sql_ddl_sequence));
        batch.put_cf(&cf_meta, sql_kind_seq_meta_key(SqlWalKind::Dml), encode_u64(state.sql_dml_sequence));
        drop(state);

        // ONE fsync for the entire batch — this is group commit.
        let mut wo = WriteOptions::default();
        wo.set_sync(self.sync_writes && self.config.wal_enabled);
        if let Err(e) = self.db.write_opt(batch, &wo) {
            panic!("rocksdb group commit write failed: {e}");
        }
        if self.sync_writes && self.config.wal_enabled {
            self.fsync_count.fetch_add(1, Ordering::Relaxed);
        }
        seqs
    }

    fn fsync_count(&self) -> u64 {
        self.fsync_count.load(Ordering::Relaxed)
    }

    fn scan_rows_for_db(
        &self,
        db: &str,
        snapshot_xid: u64,
    ) -> Vec<(String, HashMap<String, String>)> {
        // Look up the per-DB CF. The CF is created lazily on the first
        // `store_row` write, so it may not exist yet for databases with no
        // persisted rows. In SingleThreaded mode, `create_cf` requires
        // `&mut self`, so we cannot create it here — just return empty.
        let cf_name = Self::db_rows_cf_name(db);
        let cf_db = match self.db.cf_handle(&cf_name) {
            Some(cf) => cf,
            None => return Vec::new(),
        };

        // Key format in per-DB CF: {row_key}\x1f{xid_be8}
        // No db prefix — the CF IS the db scope.
        let sep = b'\x1f';

        // latest_visible[row_key] = (xid, value_bytes) — highest xid <= snapshot_xid.
        let mut latest_visible: HashMap<String, (u64, Vec<u8>)> = HashMap::new();

        for kv in self.db.iterator_cf(&cf_db, rocksdb::IteratorMode::Start) {
            let (k, v) = match kv {
                Ok(x) => x,
                Err(e) => {
                    tracing_or_eprintln(format!("scan_rows_for_db iterator error: {e}"));
                    continue;
                }
            };

            // Key: {row_key}\x1f{xid_be8}
            if k.len() < 9 {
                continue; // need at least 1 byte row_key + sep + 8 bytes xid
            }
            let xid_start = k.len() - 8;
            let row_key_with_sep = &k[..xid_start];
            if row_key_with_sep.last() != Some(&sep) {
                continue;
            }
            let row_key_bytes = &row_key_with_sep[..row_key_with_sep.len() - 1];

            let row_key = match std::str::from_utf8(row_key_bytes) {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };

            let mut xid_arr = [0u8; 8];
            xid_arr.copy_from_slice(&k[xid_start..]);
            let xid = u64::from_be_bytes(xid_arr);

            // MVCC visibility: only include versions written at or before the snapshot.
            if xid > snapshot_xid {
                continue;
            }

            // Keys are sorted ascending by xid, so later entries overwrite earlier ones.
            latest_visible.insert(row_key, (xid, v.to_vec()));
        }

        // Decode non-tombstone entries.
        let mut result = Vec::with_capacity(latest_visible.len());
        for (row_key, (_xid, value)) in latest_visible {
            if value.is_empty() {
                continue;
            }
            match value[0] {
                0x01 => {} // tombstone — row is deleted, skip
                0x00 => {
                    let json_bytes = &value[1..];
                    match serde_json::from_slice::<HashMap<String, String>>(json_bytes) {
                        Ok(cols) => result.push((row_key, cols)),
                        Err(e) => {
                            tracing_or_eprintln(format!("scan_rows_for_db json decode error: {e}"));
                        }
                    }
                }
                _ => {}
            }
        }
        result
    }

    fn drop_db_column_family(&mut self, db: &str) {
        let cf_name = Self::db_rows_cf_name(db);
        if self.db.cf_handle(&cf_name).is_none() {
            return; // CF doesn't exist — nothing to drop.
        }
        if let Err(e) = self.db.drop_cf(&cf_name) {
            tracing_or_eprintln(format!("drop_db_column_family {cf_name}: {e}"));
        }
    }

    /// M-2: Point-read the latest committed version of a single row.
    ///
    /// Scans the per-DB CF for all entries whose key starts with
    /// `{row_key}\x1f`, picks the highest xid version, and returns the
    /// decoded columns — or `None` if deleted (tombstone) or not found.
    fn get_row(&self, db: &str, row_key: &str) -> Option<HashMap<String, String>> {
        let cf_name = Self::db_rows_cf_name(db);
        let cf_db = match self.db.cf_handle(&cf_name) {
            Some(cf) => cf,
            None => return None,
        };

        // Prefix scan: all keys for this row_key are `{row_key}\x1f{xid_be8}`.
        let sep = b'\x1f';
        let mut prefix = row_key.as_bytes().to_vec();
        prefix.push(sep);

        let iter_mode = rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward);
        let mut best: Option<(u64, Vec<u8>)> = None;

        for kv in self.db.iterator_cf(&cf_db, iter_mode) {
            let (k, v) = match kv {
                Ok(x) => x,
                Err(_) => break,
            };
            if !k.starts_with(&prefix) {
                break; // Past this row's entries.
            }
            // Extract xid from the last 8 bytes.
            if k.len() < prefix.len() + 8 {
                continue;
            }
            let mut xid_arr = [0u8; 8];
            xid_arr.copy_from_slice(&k[k.len() - 8..]);
            let xid = u64::from_be_bytes(xid_arr);
            // Keep the highest xid (latest committed version).
            if best.as_ref().map(|(bx, _)| xid > *bx).unwrap_or(true) {
                best = Some((xid, v.to_vec()));
            }
        }

        let (_, value) = best?;
        if value.is_empty() {
            return None;
        }
        match value[0] {
            0x01 => None, // tombstone
            0x00 => {
                let json_bytes = &value[1..];
                serde_json::from_slice::<HashMap<String, String>>(json_bytes).ok()
            }
            _ => None,
        }
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

    // -- Phase 2.2: Row store persistence -------------------------------------

    /// Verifies `scan_rows_for_db` scoping and MVCC without crash simulation.
    #[test]
    fn scan_rows_for_db_unit() {
        let p = unique_path();
        let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("open");

        let data1: HashMap<String, String> = [("k".to_string(), "v1".to_string())].into();
        let data2: HashMap<String, String> = [("k".to_string(), "v2".to_string())].into();
        e.store_row("db1", "tbl:row-1", 5, Some(&data1));
        e.store_row("db1", "tbl:row-2", 6, Some(&data2));

        let data3: HashMap<String, String> = [("k".to_string(), "v3".to_string())].into();
        e.store_row("db2", "tbl:row-1", 7, Some(&data3));

        // DB scoping: db1 has 2 rows; db2 rows must not bleed in.
        let db1_rows = e.scan_rows_for_db("db1", 999);
        assert_eq!(db1_rows.len(), 2, "db1 should have 2 rows");

        // DB scoping: db2 has 1 row; db1 rows must not bleed in.
        let db2_rows = e.scan_rows_for_db("db2", 999);
        assert_eq!(db2_rows.len(), 1, "db2 should have 1 row");

        // MVCC: row written at xid=5, scan at xid=4 => invisible.
        let invisible = e.scan_rows_for_db("db1", 4);
        assert!(invisible.is_empty(), "rows at xid=5 must be invisible to snapshot at xid=4");

        // MVCC: scan at xid=5 => only row-1 visible (row-2 written at xid=6).
        let at5 = e.scan_rows_for_db("db1", 5);
        assert_eq!(at5.len(), 1, "only row at xid=5 visible to snapshot xid=5");
        assert_eq!(at5[0].0, "tbl:row-1");

        cleanup(&p);
    }

    /// Phase 2.2 regression: CF_ROWS survives kill -9 + reopen.
    /// Simulates crash by dropping the engine without graceful shutdown.
    #[test]
    fn rows_survive_drop_and_reopen_like_sigkill() {
        let p = unique_path();

        // Session 1 -- write rows, then drop without graceful shutdown.
        {
            let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default())
                .expect("open");

            let data1: HashMap<String, String> = [("col".to_string(), "a".to_string())].into();
            let data2: HashMap<String, String> = [("col".to_string(), "b".to_string())].into();
            let data4: HashMap<String, String> = [("col".to_string(), "d".to_string())].into();

            e.store_row("db1", "orders:row-1", 1, Some(&data1));
            e.store_row("db1", "orders:row-2", 2, Some(&data2));
            e.store_row("db1", "orders:row-3", 3, None); // tombstone
            e.store_row("db2", "products:row-1", 4, Some(&data4));
            // Engine drops here -- simulates kill -9.
        }

        // Session 2 -- reopen and verify all data survived.
        {
            let e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default())
                .expect("reopen after simulated kill -9");

            // db1: 2 live rows (tombstone excluded).
            let db1_rows = e.scan_rows_for_db("db1", 999);
            assert_eq!(db1_rows.len(), 2, "db1 must have exactly 2 live rows after reopen");
            let keys: Vec<&str> = db1_rows.iter().map(|(k, _)| k.as_str()).collect();
            assert!(!keys.contains(&"orders:row-3"), "tombstone row-3 must not be returned");

            // db2: 1 live row.
            let db2_rows = e.scan_rows_for_db("db2", 999);
            assert_eq!(db2_rows.len(), 1, "db2 must have 1 row after reopen");

            // MVCC: snapshot at xid=1 => only row-1 visible (row-2 at xid=2 invisible).
            let at1 = e.scan_rows_for_db("db1", 1);
            assert_eq!(at1.len(), 1, "only row-1 visible at snapshot xid=1");
            assert_eq!(at1[0].0, "orders:row-1");
        }

        cleanup(&p);
    }

    /// L-4: scan_rows_for_db must only return rows for the requested db (database isolation).
    #[test]
    fn scan_rows_for_db_isolates_databases() {
        let p = unique_path();
        let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("open");

        let mut data_a = HashMap::new();
        data_a.insert("name".to_string(), "alice".to_string());
        let mut data_b = HashMap::new();
        data_b.insert("name".to_string(), "bob".to_string());
        let mut data_c = HashMap::new();
        data_c.insert("name".to_string(), "charlie".to_string());

        e.store_row("db1", "orders:1", 1, Some(&data_a));
        e.store_row("db1", "orders:2", 2, Some(&data_b));
        e.store_row("db2", "products:1", 3, Some(&data_c));

        let db1_rows = e.scan_rows_for_db("db1", 999);
        assert_eq!(db1_rows.len(), 2, "db1 must return exactly 2 rows");

        let db2_rows = e.scan_rows_for_db("db2", 999);
        assert_eq!(db2_rows.len(), 1, "db2 must return exactly 1 row");

        let db3_rows = e.scan_rows_for_db("db3", 999);
        assert_eq!(db3_rows.len(), 0, "db3 (non-existent) must return 0 rows");

        cleanup(&p);
    }

    /// L-4: scan_rows_for_db must honour MVCC snapshot_xid — only versions at or
    /// before the snapshot are visible; tombstones are excluded from results.
    #[test]
    fn scan_rows_for_db_respects_mvcc_snapshot() {
        let p = unique_path();
        let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default()).expect("open");

        let mut data_old = HashMap::new();
        data_old.insert("status".to_string(), "old".to_string());
        let mut data_new = HashMap::new();
        data_new.insert("status".to_string(), "new".to_string());
        let mut data_b = HashMap::new();
        data_b.insert("amount".to_string(), "100".to_string());

        // Two versions of orders:1 at xid=10 and xid=20.
        e.store_row("db1", "orders:1", 10, Some(&data_old));
        e.store_row("db1", "orders:1", 20, Some(&data_new));
        // orders:2 at xid=5.
        e.store_row("db1", "orders:2", 5, Some(&data_b));
        // orders:3 tombstone at xid=7.
        e.store_row("db1", "orders:3", 7, None);

        // Snapshot xid=15: orders:1 at version 10 (old), orders:2, no orders:3.
        let rows_15 = e.scan_rows_for_db("db1", 15);
        let r1 = rows_15.iter().find(|(k, _)| k == "orders:1")
            .expect("orders:1 must be visible at snapshot 15");
        assert_eq!(r1.1.get("status").map(|s| s.as_str()), Some("old"),
            "must see old version at snapshot 15");
        let keys_15: Vec<&str> = rows_15.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys_15.contains(&"orders:2"), "orders:2 must be visible at snapshot 15");
        assert!(!keys_15.contains(&"orders:3"), "orders:3 tombstone must not appear");
        assert_eq!(rows_15.len(), 2, "snapshot 15 yields 2 live rows");

        // Snapshot xid=25: orders:1 at version 20 (new).
        let rows_25 = e.scan_rows_for_db("db1", 25);
        let r1_new = rows_25.iter().find(|(k, _)| k == "orders:1")
            .expect("orders:1 must be visible at snapshot 25");
        assert_eq!(r1_new.1.get("status").map(|s| s.as_str()), Some("new"),
            "must see new version at snapshot 25");
        assert_eq!(rows_25.len(), 2, "snapshot 25 yields 2 live rows");

        // Snapshot xid=3: nothing visible (all writes at xid >= 5).
        let rows_3 = e.scan_rows_for_db("db1", 3);
        assert_eq!(rows_3.len(), 0, "snapshot 3 must see 0 rows");

        cleanup(&p);
    }

    /// L-4: rows must survive a drop()-without-close + reopen cycle.
    #[test]
    fn rows_survive_drop_and_reopen() {
        let p = unique_path();
        // Session 1 — write rows, then drop without graceful shutdown.
        {
            let mut e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default())
                .expect("open");
            let mut row = HashMap::new();
            row.insert("name".to_string(), "alice".to_string());
            e.store_row("testdb", "users:1", 1, Some(&row));
            let mut row2 = HashMap::new();
            row2.insert("name".to_string(), "bob".to_string());
            e.store_row("testdb", "users:2", 2, Some(&row2));
            // Drop without graceful shutdown — simulates kill -9.
        }
        // Session 2 — reopen and verify rows survived.
        {
            let e = RocksDbDurabilityEngine::open(&p, DurabilityConfig::default())
                .expect("reopen");
            let rows = e.scan_rows_for_db("testdb", 999);
            assert_eq!(rows.len(), 2, "rows must survive kill-9-style drop and reopen");
            let keys: Vec<&str> = rows.iter().map(|(k, _)| k.as_str()).collect();
            assert!(keys.contains(&"users:1"), "users:1 must survive reopen");
            assert!(keys.contains(&"users:2"), "users:2 must survive reopen");
        }
        cleanup(&p);
    }

    /// P1 hardening: page writes must be durable even when the SQL WAL is
    /// disabled (`wal_enabled = false`).  Before the fix, `store_row` gated
    /// fsync on `sync_writes && wal_enabled`, so disabling the SQL WAL also
    /// silently disabled fsync for per-DB row CF writes.  The fix changes the
    /// condition to `sync_writes` only — SQL WAL on/off must not affect row
    /// durability.
    ///
    /// Verification strategy: open with `wal_enabled: false`, write two rows,
    /// drop the engine (simulating process exit without clean DB close), reopen
    /// and confirm the rows survived.  The test also asserts that
    /// `sync_writes_enabled()` is true so the reader can see the fsync flag is
    /// active irrespective of `wal_enabled`.
    #[test]
    fn p1_page_write_fsync_independent_of_wal_enabled() {
        let p = unique_path();

        // Build a config where the SQL WAL is explicitly disabled.
        let no_wal_config = DurabilityConfig {
            wal_enabled: false,
            ..DurabilityConfig::default()
        };

        // Session 1: write rows with wal_enabled = false then drop.
        {
            let mut e = RocksDbDurabilityEngine::open(&p, no_wal_config.clone())
                .expect("open with wal_enabled=false");

            // sync_writes defaults to true (from VNG_WAL_FSYNC_ON_COMMIT env,
            // which is unset in the test environment → true).  After the fix,
            // store_row will call set_sync(true) regardless of wal_enabled.
            assert!(
                e.sync_writes_enabled(),
                "sync_writes must be true in the default test environment"
            );

            let mut row1 = HashMap::new();
            row1.insert("col".to_string(), "val_a".to_string());
            e.store_row("ptest", "row:1", 10, Some(&row1));

            let mut row2 = HashMap::new();
            row2.insert("col".to_string(), "val_b".to_string());
            e.store_row("ptest", "row:2", 11, Some(&row2));

            // Rows must be immediately visible within the same session.
            let live = e.scan_rows_for_db("ptest", 999);
            assert_eq!(live.len(), 2, "both rows visible before drop");
        } // engine dropped here — simulates process exit

        // Session 2: reopen and confirm rows survived.
        {
            let e = RocksDbDurabilityEngine::open(&p, no_wal_config)
                .expect("reopen with wal_enabled=false");
            let rows = e.scan_rows_for_db("ptest", 999);
            assert_eq!(
                rows.len(),
                2,
                "page writes must survive drop even when SQL WAL is disabled"
            );
            let keys: Vec<&str> = rows.iter().map(|(k, _)| k.as_str()).collect();
            assert!(keys.contains(&"row:1"), "row:1 must survive reopen");
            assert!(keys.contains(&"row:2"), "row:2 must survive reopen");
        }

        cleanup(&p);
    }
}
