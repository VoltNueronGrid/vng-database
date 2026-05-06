//! Phase 2.1 — crash-recovery integration test.
//!
//! Drives the voltnuerongrid-store crate end-to-end through the same code
//! path the service uses: `BoxedDurabilityEngine::rocksdb(...)` →
//! `append_sql(...)` → drop without graceful shutdown → reopen → assert state.
//!
//! This is the engine-level proof that Phase 2 actually delivers durability
//! across `kill -9`. The full service-level smoke test (curl insert → kill
//! → curl select) is in `remaining.md` and runs on rustc 1.86+.
//!
//! # Why this lives in tests/soak/tests/ rather than the store crate
//!
//! Soak/integration tests can run on demand and have separate CI gating
//! from the unit-test suite — useful for tests that touch the disk and
//! take longer than a fast unit test would.

#![cfg(feature = "rocksdb-recovery")]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use voltnuerongrid_store::{BoxedDurabilityEngine, DurabilityConfig, SqlWalKind};

fn unique_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "vng-crash-recovery-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

/// Cornerstone test: write SQL through the engine, drop without graceful
/// shutdown (simulates kill -9), reopen, verify the SQL stream is intact.
#[test]
fn sql_streams_survive_drop_and_reopen() {
    let path = unique_path("sql-survive");
    std::env::set_var("VNG_WAL_FSYNC_ON_COMMIT", "1");

    // Session 1 — write some DDL + DML, drop without shutdown.
    {
        let mut engine = BoxedDurabilityEngine::rocksdb(&path, DurabilityConfig::default())
            .expect("open session 1");
        engine.append_sql(SqlWalKind::Ddl, "CREATE TABLE users (id INT, name TEXT)");
        engine.append_sql(SqlWalKind::Dml, "INSERT INTO users (id, name) VALUES (5, 'alice')");
        engine.append_sql(SqlWalKind::Dml, "INSERT INTO users (id, name) VALUES (6, 'bob')");
        // Engine drops here. RocksDB has been told to fsync on every write,
        // so no graceful shutdown is needed.
    }

    // Session 2 — reopen. Verify everything came back.
    {
        let engine = BoxedDurabilityEngine::rocksdb(&path, DurabilityConfig::default())
            .expect("open session 2");

        assert_eq!(engine.sql_count(SqlWalKind::Ddl), 1);
        assert_eq!(engine.sql_count(SqlWalKind::Dml), 2);

        let ddl = engine.iter_sql(SqlWalKind::Ddl);
        assert_eq!(ddl, vec!["CREATE TABLE users (id INT, name TEXT)"]);

        let dml = engine.iter_sql(SqlWalKind::Dml);
        assert_eq!(
            dml,
            vec![
                "INSERT INTO users (id, name) VALUES (5, 'alice')",
                "INSERT INTO users (id, name) VALUES (6, 'bob')",
            ]
        );
    }
    cleanup(&path);
}

/// After a checkpoint, the SQL streams remain intact — the checkpoint is
/// a snapshot point, not a truncation. (Truncation is the migrate tool's
/// job, not the engine's.)
#[test]
fn checkpoint_does_not_truncate_sql_streams() {
    let path = unique_path("ckpt-no-trunc");
    std::env::set_var("VNG_WAL_FSYNC_ON_COMMIT", "1");

    let mut engine = BoxedDurabilityEngine::rocksdb(&path, DurabilityConfig::default())
        .expect("open");
    engine.append_sql(SqlWalKind::Ddl, "CREATE TABLE t (id INT)");
    engine.append_sql(SqlWalKind::Dml, "INSERT INTO t VALUES (1)");
    engine.append_sql(SqlWalKind::Dml, "INSERT INTO t VALUES (2)");
    engine.force_checkpoint();
    drop(engine);

    let engine = BoxedDurabilityEngine::rocksdb(&path, DurabilityConfig::default())
        .expect("reopen after checkpoint");
    assert_eq!(engine.sql_count(SqlWalKind::Ddl), 1);
    assert_eq!(engine.sql_count(SqlWalKind::Dml), 2);
    assert_eq!(engine.checkpoint_count(), 1);
    cleanup(&path);
}

/// Sequence numbers continue across reopen — never reset to 1.
#[test]
fn sql_sequence_continues_across_reopen() {
    let path = unique_path("seq-continues");
    std::env::set_var("VNG_WAL_FSYNC_ON_COMMIT", "1");

    {
        let mut engine = BoxedDurabilityEngine::rocksdb(&path, DurabilityConfig::default())
            .expect("open");
        let s1 = engine.append_sql(SqlWalKind::Dml, "stmt-1");
        let s2 = engine.append_sql(SqlWalKind::Dml, "stmt-2");
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
    }
    {
        let mut engine = BoxedDurabilityEngine::rocksdb(&path, DurabilityConfig::default())
            .expect("reopen");
        let s3 = engine.append_sql(SqlWalKind::Dml, "stmt-3");
        assert_eq!(s3, 3, "sequence must continue, not reset");
    }
    cleanup(&path);
}

/// `clear_sql` truncates only the named kind and resets that kind's seq.
#[test]
fn clear_sql_resets_only_requested_kind() {
    let path = unique_path("clear-only-kind");
    std::env::set_var("VNG_WAL_FSYNC_ON_COMMIT", "1");

    let mut engine = BoxedDurabilityEngine::rocksdb(&path, DurabilityConfig::default())
        .expect("open");
    engine.append_sql(SqlWalKind::Ddl, "ddl-x");
    engine.append_sql(SqlWalKind::Dml, "dml-y");

    engine.clear_sql(SqlWalKind::Ddl);
    drop(engine);

    let mut engine = BoxedDurabilityEngine::rocksdb(&path, DurabilityConfig::default())
        .expect("reopen");
    assert_eq!(engine.sql_count(SqlWalKind::Ddl), 0);
    assert_eq!(engine.sql_count(SqlWalKind::Dml), 1);
    let next_ddl = engine.append_sql(SqlWalKind::Ddl, "fresh");
    assert_eq!(next_ddl, 1, "DDL counter reset to 1 after clear");
    cleanup(&path);
}
