//! `vng-migrate` — copy legacy text WAL files into a RocksDB durability engine.
//!
//! Phase 2.1 migration tool. Run once per deployment to lift the SQL streams
//! out of the on-disk text files (`state/ddl.wal`, `state/dml.wal`) and into
//! the new RocksDB-backed engine. After a successful run, the service can
//! boot cleanly from RocksDB and the text files can be deleted.
//!
//! # Usage
//!
//! ```bash
//! # Default paths: ./state/ddl.wal, ./state/dml.wal → ./data/rocksdb
//! vng-migrate
//!
//! # Explicit paths.
//! vng-migrate \
//!     --ddl-wal ./state/ddl.wal \
//!     --dml-wal ./state/dml.wal \
//!     --rocksdb ./data/rocksdb
//!
//! # Dry-run: parse and count, do not write.
//! vng-migrate --dry-run
//! ```
//!
//! # Idempotency
//!
//! The tool refuses to write into a non-empty SQL stream by default. This
//! means running it twice is safe — the second run sees existing data and
//! exits with a clear message. Pass `--force` to truncate before writing.
//!
//! # Exit codes
//!
//! - `0` — success (or dry-run completed).
//! - `2` — bad arguments / config.
//! - `3` — RocksDB open failure.
//! - `4` — non-empty target SQL stream and `--force` not given.
//! - `5` — text WAL parse failure.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use voltnuerongrid_store::{
    BoxedDurabilityEngine, DurabilityConfig, SqlWalKind,
};

#[derive(Debug)]
struct CliArgs {
    ddl_wal: PathBuf,
    dml_wal: PathBuf,
    rocksdb_path: PathBuf,
    dry_run: bool,
    force: bool,
}

impl CliArgs {
    fn parse() -> Result<Self, String> {
        let mut ddl_wal = PathBuf::from("./state/ddl.wal");
        let mut dml_wal = PathBuf::from("./state/dml.wal");
        let mut rocksdb_path = PathBuf::from("./data/rocksdb");
        let mut dry_run = false;
        let mut force = false;

        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--ddl-wal" => {
                    i += 1;
                    ddl_wal = PathBuf::from(args.get(i).ok_or("--ddl-wal requires value")?);
                }
                "--dml-wal" => {
                    i += 1;
                    dml_wal = PathBuf::from(args.get(i).ok_or("--dml-wal requires value")?);
                }
                "--rocksdb" => {
                    i += 1;
                    rocksdb_path = PathBuf::from(args.get(i).ok_or("--rocksdb requires value")?);
                }
                "--dry-run" => dry_run = true,
                "--force" => force = true,
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                other => return Err(format!("unknown argument: {other}")),
            }
            i += 1;
        }

        Ok(Self {
            ddl_wal,
            dml_wal,
            rocksdb_path,
            dry_run,
            force,
        })
    }
}

fn print_help() {
    println!(
        "vng-migrate — copy text WAL → RocksDB durability engine\n\
         \n\
         USAGE:\n    vng-migrate [OPTIONS]\n\
         \n\
         OPTIONS:\n\
             --ddl-wal <path>   Source DDL text WAL (default: ./state/ddl.wal)\n\
             --dml-wal <path>   Source DML text WAL (default: ./state/dml.wal)\n\
             --rocksdb <path>   Target RocksDB directory (default: ./data/rocksdb)\n\
             --dry-run          Parse and count statements without writing\n\
             --force            Overwrite a non-empty target stream (DANGER)\n\
             -h, --help         Show this help"
    );
}

/// Read a single text WAL file. Mirrors the unescape logic from the service's
/// `sql_wal_unescape` so output here matches what the service replayer reads.
fn read_text_wal(path: &Path) -> Result<Vec<String>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let f = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => return Err(format!("read {}: {e}", path.display())),
        };
        if line.is_empty() {
            continue;
        }
        out.push(unescape_wal_line(&line));
    }
    Ok(out)
}

fn unescape_wal_line(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n')  => result.push('\n'),
                Some('r')  => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some(other) => { result.push('\\'); result.push(other); }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn run() -> Result<(), (i32, String)> {
    let args = CliArgs::parse().map_err(|e| (2, e))?;

    eprintln!("vng-migrate");
    eprintln!("  ddl-wal:  {}", args.ddl_wal.display());
    eprintln!("  dml-wal:  {}", args.dml_wal.display());
    eprintln!("  rocksdb:  {}", args.rocksdb_path.display());
    eprintln!("  dry-run:  {}", args.dry_run);
    eprintln!("  force:    {}", args.force);

    // Read source WAL files.
    let ddl_stmts = read_text_wal(&args.ddl_wal).map_err(|e| (5, e))?;
    let dml_stmts = read_text_wal(&args.dml_wal).map_err(|e| (5, e))?;

    eprintln!(
        "found: {} DDL + {} DML statement(s) to migrate",
        ddl_stmts.len(),
        dml_stmts.len()
    );

    if args.dry_run {
        eprintln!("--dry-run set, exiting without writing.");
        return Ok(());
    }

    if ddl_stmts.is_empty() && dml_stmts.is_empty() {
        eprintln!("nothing to migrate — both text WAL files are empty.");
        return Ok(());
    }

    // Open target RocksDB. The migrate tool ALWAYS uses fsync to keep the
    // migration honest — there is no "fast import" mode. This matches the
    // service's default WAL_FSYNC_ON_COMMIT=1 behaviour.
    std::env::set_var("VNG_WAL_FSYNC_ON_COMMIT", "1");
    let mut engine = BoxedDurabilityEngine::rocksdb(
        &args.rocksdb_path,
        DurabilityConfig::default(),
    )
    .map_err(|e| (3, format!("RocksDB open failed: {e}")))?;

    // Idempotency check.
    let existing_ddl = engine.sql_count(SqlWalKind::Ddl);
    let existing_dml = engine.sql_count(SqlWalKind::Dml);
    if (existing_ddl > 0 || existing_dml > 0) && !args.force {
        return Err((
            4,
            format!(
                "target RocksDB already has {existing_ddl} DDL + {existing_dml} DML statements. \
                 Pass --force to overwrite, or migrate to a fresh path."
            ),
        ));
    }
    if args.force && (existing_ddl > 0 || existing_dml > 0) {
        eprintln!(
            "--force set: clearing existing {existing_ddl} DDL + {existing_dml} DML rows"
        );
        engine.clear_sql(SqlWalKind::Ddl);
        engine.clear_sql(SqlWalKind::Dml);
    }

    // Copy.
    let mut ddl_written = 0u64;
    for sql in &ddl_stmts {
        engine.append_sql(SqlWalKind::Ddl, sql);
        ddl_written += 1;
    }
    let mut dml_written = 0u64;
    for sql in &dml_stmts {
        engine.append_sql(SqlWalKind::Dml, sql);
        dml_written += 1;
    }

    eprintln!(
        "migrated {ddl_written} DDL + {dml_written} DML statement(s) to {}",
        args.rocksdb_path.display()
    );

    // Force a checkpoint at the end so the meta CF reflects the migration.
    let manifest = engine.force_checkpoint();
    eprintln!(
        "checkpoint #{} taken (last_sequence = {})",
        manifest.checkpoint_id, manifest.last_sequence
    );

    eprintln!(
        "\nMigration complete. Next steps:\n\
         1. Restart the service. It will boot from RocksDB and emit\n\
            `vng_wal_replay_total{{source=\"engine\"}}` for each replayed statement.\n\
         2. Verify the row count matches expectations via the SQL API.\n\
         3. Once verified, you can safely delete:\n\
            - {}\n\
            - {}\n",
        args.ddl_wal.display(),
        args.dml_wal.display()
    );

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, msg)) => {
            eprintln!("error: {msg}");
            ExitCode::from(code as u8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_round_trips_common_sequences() {
        assert_eq!(unescape_wal_line("hello"), "hello");
        assert_eq!(unescape_wal_line("a\\nb"), "a\nb");
        assert_eq!(unescape_wal_line("a\\rb"), "a\rb");
        assert_eq!(unescape_wal_line("a\\\\b"), "a\\b");
        // Unknown escapes preserve the backslash.
        assert_eq!(unescape_wal_line("a\\xb"), "a\\xb");
        // Trailing backslash preserved.
        assert_eq!(unescape_wal_line("trail\\"), "trail\\");
    }

    #[test]
    fn read_text_wal_returns_empty_for_missing_file() {
        let path = std::path::PathBuf::from("/tmp/vng-migrate-does-not-exist-xyz.wal");
        let r = read_text_wal(&path).expect("missing file is ok");
        assert!(r.is_empty());
    }
}
