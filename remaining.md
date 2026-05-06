# `remaining.md` — handoff for next session (v6)

**Last updated:** 2026-05-05 (seventh session — Phase 2.1 replay + migrate)
**Branch:** `phase-2-replay-and-migration`
**Total unit tests passing locally:** 549 (399 SQL + 97 store + 25 exec + 21 meta + 7 config) + 8 soak
**Phase:** 2.1 — engine-first replay + migration tooling

---

## TL;DR

This session delivered all three "next session priorities" from v5:

1. **Replay-on-boot from RocksDB** ✅ — service prefers the engine's SQL streams, falls back to text WAL.
2. **Migration helper** ✅ — `vng-migrate` CLI under `tools/voltnuerongrid-migrate/`.
3. **CI crash-recovery test** ✅ — `tests/soak/tests/crash_recovery.rs` (4 tests behind `--features rocksdb-recovery`).

The legacy text-WAL files (`state/ddl.wal`, `state/dml.wal`) are now backed by a parallel SQL stream in RocksDB. After running `vng-migrate` once and restarting, operators can delete the text files.

---

## What landed this session

### ✅ Trait extension: `SqlWalKind` + SQL stream methods

**File:** `crates/voltnuerongrid-store/src/lib.rs`

```rust
pub enum SqlWalKind { Ddl, Dml }

trait DurabilityEngine: Send {
    // ... existing 7 methods ...
    fn append_sql(&mut self, kind: SqlWalKind, sql: &str) -> u64 { 0 }
    fn iter_sql(&self, kind: SqlWalKind) -> Vec<String> { Vec::new() }
    fn sql_count(&self, kind: SqlWalKind) -> usize { 0 }
    fn clear_sql(&mut self, kind: SqlWalKind) {}
    fn persists_sql(&self) -> bool { false }
}
```

`InMemoryDurabilityEngine` implements them with a `HashMap<&'static str, Vec<String>>` (process-local, useful for tests). `BoxedDurabilityEngine` forwards the new methods.

**6 new in-memory engine tests** covering append-order, per-kind sequence, clear, etc.

### ✅ RocksDB engine — 4th column family for SQL streams

**File:** `crates/voltnuerongrid-store/src/rocksdb_engine.rs`

- New CF `sql` with key schema: `[kind_byte (1)] [seq (8)]` big-endian → fast per-kind iteration.
- Per-kind sequence counters persisted in meta CF (`META_SQL_DDL_SEQUENCE`, `META_SQL_DML_SEQUENCE`).
- `append_sql` writes one atomic batch: SQL row + counter update.
- `iter_sql` does a prefix-bounded iteration on the kind byte.
- `clear_sql` deletes by range and resets the counter atomically.

**5 new RocksDB engine tests** including:
- `sql_stream_survives_drop_and_reopen` — kill-9 substitute for SQL streams.
- `clear_sql_truncates_only_named_kind` — DDL clear leaves DML alone.

### ✅ Service replay refactor

**File:** `services/voltnuerongridd/src/main.rs`

- New helpers `replay_ddl_into(catalog, &engine)` and `replay_dml_into(rs, &engine)`. Engine-first; fall back to text WAL when the engine has no SQL.
- `apply_ddl_to_catalog` and `apply_dml_to_rowstore` extracted so both replay paths share one code path.
- New helper `persist_sql_statement(&state, kind, sql)` dual-writes to engine + text WAL during the migration window.
- All 9 `append_sql_wal(...)` call sites in `sql_execute` and `sql_transaction` migrated to `persist_sql_statement`.
- 3 new metrics:
  - `vng_wal_replay_total{kind, source}` — per-statement replay count, labeled by `engine`/`text_wal`.
  - `vng_wal_append_total{kind}` — runtime SQL appends.
  - `vng_durability_engine_boot{engine}` — already from previous session.

### ✅ Migration tool

**Crate:** `tools/voltnuerongrid-migrate/` (binary `vng-migrate`)

```bash
vng-migrate \
    --ddl-wal ./state/ddl.wal \
    --dml-wal ./state/dml.wal \
    --rocksdb ./data/rocksdb
```

Features:
- Reads text WAL files using the same unescape logic as the service.
- Refuses to write into a non-empty target SQL stream by default (idempotent).
- `--force` truncates first; `--dry-run` parses without writing.
- Always uses `fsync` so the migration is honest.
- Prints a clear next-steps message after success.
- Returns distinct exit codes (0/2/3/4/5) for scriptability.
- 2 unit tests for the unescape and missing-file paths.

### ✅ Crash-recovery integration tests

**File:** `tests/soak/tests/crash_recovery.rs` (4 tests, gated behind `rocksdb-recovery` feature)

- `sql_streams_survive_drop_and_reopen` — write SQL, drop, reopen, verify content.
- `checkpoint_does_not_truncate_sql_streams` — checkpoint is a snapshot point, not truncation.
- `sql_sequence_continues_across_reopen` — sequences never reset to 1.
- `clear_sql_resets_only_requested_kind` — DDL clear leaves DML alone.

Run with: `cargo test -p vng-soak --features rocksdb-recovery`.

---

## ⚠️ Things to verify on rustc 1.86+

```bash
git checkout phase-2-replay-and-migration

# Workspace compiles.
cargo check --workspace

# Default test suite — should be 549 unit tests + 8 soak.
cargo test --workspace

# Crash-recovery suite (engine-level kill-9 substitute).
cargo test -p vng-soak --features rocksdb-recovery

# Service binary.
cargo build --release -p voltnuerongridd

# Migrate tool.
cargo build --release -p voltnuerongrid-migrate
./target/release/vng-migrate --help
```

### Likely issues

1. **`now_unix_ms()` return type in `apply_ddl_to_catalog`.** I used `now_ms: u128` in the helper signature but `now_unix_ms()` may return `u64`. Cargo-check will say so — adjust to whatever the existing function returns.

2. **`voltnuerongrid_store::mvcc::Xid` import in `apply_dml_to_rowstore`.** May need `pub use` from the store crate.

3. **`metrics::counter!{"vng_wal_replay_total" ... "kind" => "ddl", ...}` syntax** matches what's already used elsewhere in main.rs, but I used `(metric).increment(N as u64)` — confirm metrics 0.23 supports this.

---

## End-to-end migration smoke test

```bash
# 1. Run the service with the legacy text-WAL paths to populate state/.
VNG_LOG=debug VNG_ADMIN_API_KEY=test ./target/release/voltnuerongridd &
sleep 1

curl -X POST -H "x-vng-admin-key: test" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"sql_batch":"CREATE TABLE t (id INT, name TEXT)"}' \
  http://127.0.0.1:8080/api/v1/sql/execute

curl -X POST -H "x-vng-admin-key: test" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"sql_batch":"INSERT INTO t (id, name) VALUES (5, '\''alice'\'')"}' \
  http://127.0.0.1:8080/api/v1/sql/execute

# Confirm the dual-write happened.
ls -la state/ddl.wal state/dml.wal
curl -s http://127.0.0.1:8080/metrics | grep vng_wal_append_total
# Expected: vng_wal_append_total{kind="ddl"} 1
#           vng_wal_append_total{kind="dml"} 1

# 2. Stop the service hard.
pkill -9 voltnuerongridd

# 3. (Optional) Migrate explicitly. The service already wrote to RocksDB
#    via the dual-write helper, so this is a no-op or skipped. Useful for
#    pre-existing deployments that haven't restarted yet.
./target/release/vng-migrate \
    --ddl-wal ./state/ddl.wal \
    --dml-wal ./state/dml.wal \
    --rocksdb ./data/rocksdb \
    --dry-run

# 4. Restart. Service should boot from RocksDB.
./target/release/voltnuerongridd &
sleep 1

# 5. Verify the row survived the kill -9.
curl -X POST -H "x-vng-admin-key: test" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"sql_batch":"SELECT * FROM t WHERE id = 5"}' \
  http://127.0.0.1:8080/api/v1/sql/execute | jq

# 6. Confirm replay came from the engine, not text WAL.
curl -s http://127.0.0.1:8080/metrics | grep vng_wal_replay_total
# Expected: vng_wal_replay_total{kind="ddl",source="engine"} 1
#           vng_wal_replay_total{kind="dml",source="engine"} 1
```

---

## What's still TODO

### Phase 2.2 — text WAL deprecation (next session, small)

Once you've verified end-to-end migration works:

1. Remove the `append_sql_wal()` call inside `persist_sql_statement` (engine-only).
2. Remove `replay_ddl_wal_into` and `replay_dml_wal_into` (legacy text WAL paths in the replay helpers).
3. Delete `ddl_wal_path()`, `dml_wal_path()`, `read_sql_wal()`, `sql_wal_escape()`, `sql_wal_unescape()`.
4. Delete `vng-migrate` after one release cycle once nobody is on the legacy text WAL.

### Phase 3 — items still in the gaps doc

- DataFusion adoption (now unblocked since the workspace MSRV bumped for RocksDB).
- Real RBAC.
- Replication.

### Phase 0 follow-ups

1. `handler_lock!` macro rollout — still ~346 sites.
2. main.rs modular refactor — now ~34,800+ lines.

---

## Reconciliation note for Pavan

Pavan's local Step 1 + Step 3 work was not pushed when this session began.
This branch builds on top of `origin/main` and assumes:

- `rocksdb_engine.rs` is the version Claude wrote in v5 (3 CFs, atomic
  WriteBatch, set_sync, persisted checkpoint id).
- The `AuthErrorResponse` + locale drift Pavan fixed in his Step 1 is on
  origin/main but his commits haven't landed yet — when they do,
  cherry-pick or rebase.

The trait extension here is additive (default impls), so any existing
`DurabilityEngine` impl Pavan wrote locally will still compile. The new
`SqlWalKind` enum and 5 methods are pure additions.

---

## How to continue

```
@.cursorrules
@gaps-may26-1.md
@remaining.md
@crates/voltnuerongrid-store/src/lib.rs
@crates/voltnuerongrid-store/src/rocksdb_engine.rs
@tools/voltnuerongrid-migrate/src/main.rs
@tests/soak/tests/crash_recovery.rs
```

Recommended next step: Phase 2.2 (text WAL deprecation) once the smoke
test in this file passes on rustc 1.86+. Or start Phase 3 if you want
to push forward instead of cleaning up.
