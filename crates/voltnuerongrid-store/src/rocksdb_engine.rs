//! RocksDB-backed durability engine. Mirrors the public surface of
//! `InMemoryDurabilityEngine` so a runtime selector can pick between them.
//!
//! Layout (3 column families):
//!   - `cf_kv`         : key → latest value (the "store")
//!   - `cf_wal`        : be(sequence) → encoded(WalRecord)  (append-only WAL)
//!   - `cf_checkpoints`: be(checkpoint_id) → encoded(CheckpointManifest)
//!
//! Durability: every `append_mutation` issues one `WriteBatch` covering
//! `cf_kv` and `cf_wal`. When `wal_fsync_on_commit` is true the write is
//! issued with `WriteOptions::set_sync(true)`, forcing fsync of RocksDB's
//! own WAL before the call returns. That closes the in-memory-flush gap.

use crate::{CheckpointManifest, DurabilityConfig, WalRecord, now_epoch_millis};
use rocksdb::{
    ColumnFamilyDescriptor, DBCompressionType, IteratorMode, Options, WriteBatch, WriteOptions, DB,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const CF_KV: &str = "cf_kv";
const CF_WAL: &str = "cf_wal";
const CF_CHECKPOINTS: &str = "cf_checkpoints";

#[derive(Debug)]
pub enum RocksdbEngineError {
    Open(String),
    Read(String),
    Write(String),
    Decode(String),
}

impl std::fmt::Display for RocksdbEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(m) => write!(f, "rocksdb open error: {m}"),
            Self::Read(m) => write!(f, "rocksdb read error: {m}"),
            Self::Write(m) => write!(f, "rocksdb write error: {m}"),
            Self::Decode(m) => write!(f, "rocksdb decode error: {m}"),
        }
    }
}

impl std::error::Error for RocksdbEngineError {}

impl From<rocksdb::Error> for RocksdbEngineError {
    fn from(e: rocksdb::Error) -> Self {
        Self::Write(e.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RocksdbEngineConfig {
    pub durability: DurabilityConfig,
    pub data_dir: PathBuf,
    pub max_background_jobs: i32,
    /// When true, every `append_mutation` fsyncs RocksDB's WAL before returning.
    pub wal_fsync_on_commit: bool,
}

impl RocksdbEngineConfig {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            durability: DurabilityConfig::default(),
            data_dir: data_dir.as_ref().to_path_buf(),
            max_background_jobs: 4,
            wal_fsync_on_commit: true,
        }
    }
}

pub struct RocksDbDurabilityEngine {
    db: DB,
    config: RocksdbEngineConfig,
    sequence: AtomicU64,
    checkpoint_id: AtomicU64,
}

impl RocksDbDurabilityEngine {
    pub fn open(config: RocksdbEngineConfig) -> Result<Self, RocksdbEngineError> {
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);
        db_opts.set_max_background_jobs(config.max_background_jobs);
        db_opts.set_compression_type(DBCompressionType::Lz4);

        let cfs = vec![
            ColumnFamilyDescriptor::new(CF_KV, Options::default()),
            ColumnFamilyDescriptor::new(CF_WAL, Options::default()),
            ColumnFamilyDescriptor::new(CF_CHECKPOINTS, Options::default()),
        ];
        let db = DB::open_cf_descriptors(&db_opts, &config.data_dir, cfs)
            .map_err(|e| RocksdbEngineError::Open(e.to_string()))?;

        let sequence = recover_latest_sequence(&db)?;
        let checkpoint_id = recover_latest_checkpoint_id(&db)?;

        Ok(Self {
            db,
            config,
            sequence: AtomicU64::new(sequence),
            checkpoint_id: AtomicU64::new(checkpoint_id),
        })
    }

    pub fn append_mutation(
        &self,
        key: &str,
        value: &str,
    ) -> Result<WalRecord, RocksdbEngineError> {
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let record = WalRecord {
            sequence: seq,
            timestamp_epoch_ms: now_epoch_millis(),
            key: key.to_string(),
            value: value.to_string(),
        };

        let cf_kv = cf(&self.db, CF_KV)?;
        let mut batch = WriteBatch::default();
        batch.put_cf(&cf_kv, key.as_bytes(), value.as_bytes());

        if self.config.durability.wal_enabled {
            let cf_wal = cf(&self.db, CF_WAL)?;
            batch.put_cf(&cf_wal, &seq.to_be_bytes(), encode_record(&record).as_bytes());
        }

        let mut wo = WriteOptions::default();
        wo.set_sync(self.config.wal_fsync_on_commit);
        self.db
            .write_opt(batch, &wo)
            .map_err(|e| RocksdbEngineError::Write(e.to_string()))?;

        Ok(record)
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, RocksdbEngineError> {
        let cf_kv = cf(&self.db, CF_KV)?;
        let value = self
            .db
            .get_cf(&cf_kv, key.as_bytes())
            .map_err(|e| RocksdbEngineError::Read(e.to_string()))?;
        match value {
            Some(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|e| RocksdbEngineError::Decode(e.to_string())),
            None => Ok(None),
        }
    }

    pub fn latest_sequence(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst)
    }

    pub fn wal_records(&self) -> Result<Vec<WalRecord>, RocksdbEngineError> {
        let cf_wal = cf(&self.db, CF_WAL)?;
        let mut out = Vec::new();
        for item in self.db.iterator_cf(&cf_wal, IteratorMode::Start) {
            let (_, v) = item.map_err(|e| RocksdbEngineError::Read(e.to_string()))?;
            let line = std::str::from_utf8(&v)
                .map_err(|e| RocksdbEngineError::Decode(e.to_string()))?;
            out.push(decode_record(line)?);
        }
        Ok(out)
    }

    pub fn wal_len(&self) -> Result<usize, RocksdbEngineError> {
        Ok(self.wal_records()?.len())
    }

    pub fn checkpoint_count(&self) -> u64 {
        self.checkpoint_id.load(Ordering::SeqCst)
    }

    pub fn force_checkpoint(&self) -> Result<CheckpointManifest, RocksdbEngineError> {
        let id = self.checkpoint_id.fetch_add(1, Ordering::SeqCst) + 1;
        let last_sequence = self.latest_sequence();
        let entry_count = count_cf(&self.db, CF_KV)?;

        let manifest = CheckpointManifest {
            checkpoint_id: id,
            last_sequence,
            entry_count,
        };

        let cf_cp = cf(&self.db, CF_CHECKPOINTS)?;
        let mut wo = WriteOptions::default();
        wo.set_sync(self.config.wal_fsync_on_commit);
        self.db
            .put_cf_opt(
                &cf_cp,
                id.to_be_bytes(),
                encode_manifest(&manifest).as_bytes(),
                &wo,
            )
            .map_err(|e| RocksdbEngineError::Write(e.to_string()))?;

        if self.config.durability.wal_enabled {
            let cf_wal = cf(&self.db, CF_WAL)?;
            let mut batch = WriteBatch::default();
            for item in self.db.iterator_cf(&cf_wal, IteratorMode::Start) {
                let (k, _) = item.map_err(|e| RocksdbEngineError::Read(e.to_string()))?;
                batch.delete_cf(&cf_wal, &k);
            }
            self.db
                .write_opt(batch, &wo)
                .map_err(|e| RocksdbEngineError::Write(e.to_string()))?;
        }

        Ok(manifest)
    }

    pub fn maybe_checkpoint(&self) -> Result<Option<CheckpointManifest>, RocksdbEngineError> {
        if self.wal_len()? < self.config.durability.max_wal_records_before_checkpoint {
            return Ok(None);
        }
        Ok(Some(self.force_checkpoint()?))
    }

    pub fn latest_checkpoint(&self) -> Result<Option<CheckpointManifest>, RocksdbEngineError> {
        let cf_cp = cf(&self.db, CF_CHECKPOINTS)?;
        let mut latest: Option<CheckpointManifest> = None;
        for item in self.db.iterator_cf(&cf_cp, IteratorMode::End) {
            let (_, v) = item.map_err(|e| RocksdbEngineError::Read(e.to_string()))?;
            let line = std::str::from_utf8(&v)
                .map_err(|e| RocksdbEngineError::Decode(e.to_string()))?;
            latest = Some(decode_manifest(line)?);
            break;
        }
        Ok(latest)
    }
}

fn cf<'a>(
    db: &'a DB,
    name: &str,
) -> Result<&'a rocksdb::ColumnFamily, RocksdbEngineError> {
    db.cf_handle(name)
        .ok_or_else(|| RocksdbEngineError::Open(format!("missing column family {name}")))
}

fn count_cf(db: &DB, name: &str) -> Result<usize, RocksdbEngineError> {
    let h = cf(db, name)?;
    let mut n = 0usize;
    for item in db.iterator_cf(h, IteratorMode::Start) {
        item.map_err(|e| RocksdbEngineError::Read(e.to_string()))?;
        n += 1;
    }
    Ok(n)
}

fn recover_latest_sequence(db: &DB) -> Result<u64, RocksdbEngineError> {
    let cf_wal = cf(db, CF_WAL)?;
    let mut it = db.iterator_cf(&cf_wal, IteratorMode::End);
    if let Some(item) = it.next() {
        let (k, _) = item.map_err(|e| RocksdbEngineError::Read(e.to_string()))?;
        if k.len() == 8 {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&k);
            return Ok(u64::from_be_bytes(buf));
        }
    }
    Ok(0)
}

fn recover_latest_checkpoint_id(db: &DB) -> Result<u64, RocksdbEngineError> {
    let cf_cp = cf(db, CF_CHECKPOINTS)?;
    let mut it = db.iterator_cf(&cf_cp, IteratorMode::End);
    if let Some(item) = it.next() {
        let (k, _) = item.map_err(|e| RocksdbEngineError::Read(e.to_string()))?;
        if k.len() == 8 {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&k);
            return Ok(u64::from_be_bytes(buf));
        }
    }
    Ok(0)
}

fn encode_record(r: &WalRecord) -> String {
    format!(
        "{}\t{}\t{}\t{}",
        r.sequence,
        r.timestamp_epoch_ms,
        escape(&r.key),
        escape(&r.value)
    )
}

fn decode_record(line: &str) -> Result<WalRecord, RocksdbEngineError> {
    let parts: Vec<&str> = line.splitn(4, '\t').collect();
    if parts.len() != 4 {
        return Err(RocksdbEngineError::Decode(line.to_string()));
    }
    let sequence: u64 = parts[0]
        .parse()
        .map_err(|_| RocksdbEngineError::Decode(line.to_string()))?;
    let timestamp_epoch_ms: u128 = parts[1]
        .parse()
        .map_err(|_| RocksdbEngineError::Decode(line.to_string()))?;
    Ok(WalRecord {
        sequence,
        timestamp_epoch_ms,
        key: unescape(parts[2]),
        value: unescape(parts[3]),
    })
}

fn encode_manifest(m: &CheckpointManifest) -> String {
    format!("{}\t{}\t{}", m.checkpoint_id, m.last_sequence, m.entry_count)
}

fn decode_manifest(line: &str) -> Result<CheckpointManifest, RocksdbEngineError> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() != 3 {
        return Err(RocksdbEngineError::Decode(line.to_string()));
    }
    Ok(CheckpointManifest {
        checkpoint_id: parts[0]
            .parse()
            .map_err(|_| RocksdbEngineError::Decode(line.to_string()))?,
        last_sequence: parts[1]
            .parse()
            .map_err(|_| RocksdbEngineError::Decode(line.to_string()))?,
        entry_count: parts[2]
            .parse()
            .map_err(|_| RocksdbEngineError::Decode(line.to_string()))?,
    })
}

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unescape(v: &str) -> String {
    let mut out = String::new();
    let mut chars = v.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn append_and_get_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let engine = RocksDbDurabilityEngine::open(RocksdbEngineConfig::new(dir.path()))
            .expect("open");

        let r = engine.append_mutation("region", "us-east-1").expect("append");
        assert_eq!(r.sequence, 1);
        assert_eq!(engine.get("region").expect("get"), Some("us-east-1".into()));
        assert_eq!(engine.latest_sequence(), 1);
    }

    #[test]
    fn wal_records_returned_in_sequence_order() {
        let dir = TempDir::new().expect("tempdir");
        let engine = RocksDbDurabilityEngine::open(RocksdbEngineConfig::new(dir.path()))
            .expect("open");
        engine.append_mutation("a", "1").unwrap();
        engine.append_mutation("b", "2").unwrap();
        engine.append_mutation("c", "3").unwrap();

        let recs = engine.wal_records().expect("wal_records");
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].sequence, 1);
        assert_eq!(recs[1].sequence, 2);
        assert_eq!(recs[2].sequence, 3);
        assert_eq!(recs[2].key, "c");
    }

    #[test]
    fn force_checkpoint_truncates_wal() {
        let dir = TempDir::new().expect("tempdir");
        let engine = RocksDbDurabilityEngine::open(RocksdbEngineConfig::new(dir.path()))
            .expect("open");
        engine.append_mutation("a", "1").unwrap();
        engine.append_mutation("b", "2").unwrap();

        let cp = engine.force_checkpoint().expect("force_checkpoint");
        assert_eq!(cp.checkpoint_id, 1);
        assert_eq!(cp.last_sequence, 2);
        assert_eq!(cp.entry_count, 2);
        assert_eq!(engine.wal_len().unwrap(), 0);
        assert_eq!(
            engine.latest_checkpoint().unwrap().map(|c| c.checkpoint_id),
            Some(1)
        );
    }

    /// "kill -9" simulation: write rows, drop the engine without any
    /// graceful shutdown call (mirroring SIGKILL — no destructors run for
    /// the process, only for the in-process resources we drop here), then
    /// reopen at the same path and verify the rows survived.
    #[test]
    fn survives_drop_and_reopen_like_sigkill() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().to_path_buf();

        {
            let engine = RocksDbDurabilityEngine::open(RocksdbEngineConfig::new(&path))
                .expect("open #1");
            engine.append_mutation("tenant", "acme").unwrap();
            engine.append_mutation("region", "us-east-1").unwrap();
            engine.append_mutation("plan", "enterprise").unwrap();
            // drop without calling any close — the next open must still see all 3 rows
        }

        let recovered = RocksDbDurabilityEngine::open(RocksdbEngineConfig::new(&path))
            .expect("open #2");
        assert_eq!(recovered.get("tenant").unwrap(), Some("acme".into()));
        assert_eq!(recovered.get("region").unwrap(), Some("us-east-1".into()));
        assert_eq!(recovered.get("plan").unwrap(), Some("enterprise".into()));
        assert_eq!(recovered.latest_sequence(), 3);
        assert_eq!(recovered.wal_records().unwrap().len(), 3);

        // A subsequent write must continue from sequence=4, not restart at 1.
        let r = recovered.append_mutation("plan", "platinum").unwrap();
        assert_eq!(r.sequence, 4);
        assert_eq!(recovered.get("plan").unwrap(), Some("platinum".into()));
    }

    #[test]
    fn checkpoint_id_persists_across_reopen() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().to_path_buf();
        {
            let engine = RocksDbDurabilityEngine::open(RocksdbEngineConfig::new(&path))
                .expect("open #1");
            engine.append_mutation("a", "1").unwrap();
            engine.force_checkpoint().unwrap();
        }
        let engine2 = RocksDbDurabilityEngine::open(RocksdbEngineConfig::new(&path))
            .expect("open #2");
        assert_eq!(engine2.checkpoint_count(), 1);
        let cp2 = engine2.force_checkpoint().unwrap();
        assert_eq!(cp2.checkpoint_id, 2, "checkpoint_id must continue, not restart");
    }

    #[test]
    fn fsync_disabled_still_works_in_unit_tests() {
        let dir = TempDir::new().expect("tempdir");
        let mut cfg = RocksdbEngineConfig::new(dir.path());
        cfg.wal_fsync_on_commit = false;
        let engine = RocksDbDurabilityEngine::open(cfg).expect("open");
        engine.append_mutation("k", "v").unwrap();
        assert_eq!(engine.get("k").unwrap(), Some("v".into()));
    }
}
