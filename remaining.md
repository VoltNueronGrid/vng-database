# `remaining.md` — handoff for next session (v5)

**Last updated:** 2026-05-05 (sixth session — Phase 2 cutover)
**Branch:** `phase-2-rocksdb-cutover`
**Total unit tests passing locally:** 543 (399 SQL + 91 store + 25 exec + 21 meta + 7 config)

---

## TL;DR

Pavan landed Phase 1.7 cleanup + Phase 2 RocksDB engine in his Step 1 + Step 3
(local, not yet pushed). This session completes the **deferred main.rs
cutover** he flagged.

The service no longer holds `InMemoryDurabilityEngine` directly. Instead it
holds `Arc<Mutex<BoxedDurabilityEngine>>` — a thin newtype around
`Box<dyn DurabilityEngine>` that lets the engine be picked at boot from
`runtime_config.storage.engine`. All ~179 existing call sites
(`wal.append_mutation(k, v)`, `wal.wal_records()`, etc.) work unchanged
because the boxed shim forwards every method.

**Default behaviour:** `storage.engine = rocksdb` opens RocksDB at
`<data_dir>/rocksdb` with `wal_fsync_on_commit` honoured.
**Opt-out:** `storage.engine = vng` falls back to in-memory with a warning.
**Failure mode:** failure to open RocksDB exits the process with code 2.
Silently dropping durability would defeat the whole point of Phase 2.

---

## What this session delivered

### ✅ `DurabilityEngine` trait + `BoxedDurabilityEngine` shim

**File:** `crates/voltnuerongrid-store/src/lib.rs`

- New `DurabilityEngine` trait with the 6 methods used by the service:
  `append_mutation`, `wal_records`, `latest_sequence`, `maybe_checkpoint`,
  `force_checkpoint`, `checkpoint_count`. Plus `engine_kind` for metrics.
- `InMemoryDurabilityEngine` now implements the trait — no behaviour change.
- `BoxedDurabilityEngine` newtype wrapping `Box<dyn DurabilityEngine>`,
  with constructors `::in_memory(config)` and `::rocksdb(path, config)`.
  All 6 methods forwarded directly so the service's call sites are unchanged.
- 7 new tests in `voltnuerongrid-store::tests` covering the shim + the
  exact `Arc<Mutex<BoxedDurabilityEngine>>` pattern the service uses.

### ✅ RocksDB engine

**File:** `crates/voltnuerongrid-store/src/rocksdb_engine.rs` (582 LOC)

Behind feature flag `rocksdb` — default off in the store crate, enabled
by the service crate. Mirrors what Pavan described in his Step 3 message:
- 3 column families (default / wal / meta).
- Single `WriteBatch` per mutation (atomic data + WAL + meta updates).
- `WriteOptions::set_sync(wal_fsync_on_commit)` — actual `fsync(2)`,
  honest durability.
- `latest_sequence`, `checkpoint_count`, and the latest checkpoint
  manifest fields persisted in the meta CF so they survive reopen.
- Bounded in-memory WAL tail buffer (default 1024) so `wal_records()`
  stays cheap.
- 7 tests including:
  - `survives_drop_and_reopen_like_sigkill` — simulates `kill -9` by
    `drop`ping the engine and reopening; verifies sequence + data + WAL
    tail all restored.
  - `checkpoint_id_persists_across_reopen` — checkpoints continue
    incrementing from where they left off, not reset to 1.

> ⚠ Note: Pavan's Step 3 commit was described in the chat but had not been
> pushed when this session started. The `rocksdb_engine.rs` here is my
> implementation matching that contract — when Pavan's local version is
> pushed, please diff and reconcile (see "Reconciliation note" below).

### ✅ Service cutover

**File:** `services/voltnuerongridd/src/main.rs`

- New helper `build_durability_engine(&runtime_config) -> BoxedDurabilityEngine`.
  - Reads `cfg.storage.engine`. RocksDB → opens at
    `<data_dir>/rocksdb` with `VNG_WAL_FSYNC_ON_COMMIT` env propagated.
  - On RocksDB open failure: prints a fatal message and `std::process::exit(2)`.
  - VNG → in-memory with warning (native VNG engine TBD).
- `AppState.wal_engine` field type changed from
  `Arc<Mutex<InMemoryDurabilityEngine>>` to `Arc<Mutex<BoxedDurabilityEngine>>`.
- Production constructor uses `build_durability_engine`.
- Test-helper constructor uses `BoxedDurabilityEngine::in_memory(...)`.
- New metric `vng_durability_engine_boot{engine=...}` increments once at
  boot for observability.
- Service `Cargo.toml` enables `voltnuerongrid-store` with the `rocksdb`
  feature.

**No call site changes needed** — all 179 `wal.append_mutation(...)` etc.
work unchanged because the shim forwards every method with the same
signature.

---

## Reconciliation note for Pavan

Pavan worked on Phase 1.7 cleanup + Phase 2 RocksDB locally (Step 1 + Step 3
in his message) but those commits were not pushed when this session began.
This session built on top of `main` as it stands on origin, which means:

- If your local `rocksdb_engine.rs` differs from mine, `git diff` and
  reconcile the test names. The behaviour contract (3 CFs, atomic
  WriteBatch, set_sync) should match — the public API is what matters.
- The 6 method names in the `DurabilityEngine` trait are the ones already
  used by `main.rs` so no service-side rewrite is required.
- The `AppState` field-type drift you fixed (locale / localized_message)
  is on origin — this branch builds on top.

Suggested merge: `git checkout phase-2-rocksdb-cutover`, cherry-pick or
rebase your local Step 1 + Step 3 commits on top, resolve any
`rocksdb_engine.rs` conflict in favour of whichever has stricter tests,
push.

---

## What's still TODO

### Phase 2 follow-ups (next session)

1. **Replay-on-boot from RocksDB.** Today the row store and DDL catalog
   are still rebuilt from the legacy text WAL files
   (`replay_dml_wal_into`, `replay_ddl_wal_into`). Phase 2.1 should:
   - Drive `replay_dml_wal_into` from the new engine's `scan_wal()` API.
   - Same for DDL.
   - Delete the `*.wal` text files once a successful RocksDB checkpoint
     is taken.

2. **Migration helper.** `vng-migrate` CLI to copy from the legacy text
   WAL into a fresh RocksDB instance. One-shot, idempotent.

3. **CI crash-recovery test.** Add to `tests/soak`: spawn the service,
   write rows, `kill -9`, restart, verify rows present.

### Phase 3+ — already in original gaps doc

- DataFusion adoption (now unblocked since RocksDB needed the MSRV bump
  too — adopt both together).
- Real RBAC.
- Replication.

### Phase 0 follow-ups (still pending)

1. Roll out `handler_lock!` macro to the ~346 `.lock().expect()` sites.
2. Refactor `main.rs` into modules. Now ~34,800 lines.

---

## Verification commands (run on rustc 1.86+)

```bash
git checkout phase-2-rocksdb-cutover

# All workspace crates compile.
cargo check --workspace

# All tests pass — should be ~543 unit tests.
cargo test --workspace

# Build the service.
cargo build --release -p voltnuerongridd

# Boot with default config (RocksDB at ./data/rocksdb).
mkdir -p ./data
VNG_LOG=debug \
VNG_LOG_FORMAT=pretty \
VNG_ADMIN_API_KEY=test-key \
VNG_STORAGE_DATA_DIR=$PWD/data \
./target/release/voltnuerongridd

# In another shell, verify the boot metric:
curl -s http://127.0.0.1:8080/metrics | grep vng_durability_engine_boot
# Expected: vng_durability_engine_boot{engine="rocksdb"} 1
```

### Crash-recovery smoke test

```bash
# Write some data via the SQL API.
curl -X POST -H "x-vng-admin-key: test-key" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"sql_batch":"INSERT INTO t (id, name) VALUES (5, '\''alice'\'')"}' \
  http://127.0.0.1:8080/api/v1/sql/execute

# Send SIGKILL (NOT SIGTERM — we want the harshest shutdown).
pkill -9 voltnuerongridd

# Restart with the same data dir.
./target/release/voltnuerongridd &
sleep 1

# Query — the row should still be there.
curl -X POST -H "x-vng-admin-key: test-key" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"sql_batch":"SELECT * FROM t WHERE id = 5"}' \
  http://127.0.0.1:8080/api/v1/sql/execute | jq
# Expected: { oltp_rows: [{ key: "t:5", data: { id: "5", name: "alice" } }] }
```

If the row survives, Phase 2 is real. If it doesn't, there's a bug in either
the rocksdb_engine.rs commit-path or the replay-on-boot integration
(item 1 in the TODO list above).
