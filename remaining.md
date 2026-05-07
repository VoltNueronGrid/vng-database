# `remaining.md` — handoff for next session (v7)

**Last updated:** 2026-05-05 (eighth session — Phase 2.2 text WAL deprecation)
**Branch:** `phase-2-2-text-wal-deprecation`
**Total unit tests passing locally:** 562 (399 SQL + 97 store + 25 exec + 21 meta + 12 config + 8 soak)
**Phase:** 2.2 — text WAL deprecation + auto-migrate-on-boot

---

## TL;DR

This session deprecated the legacy text WAL files. Going forward:

- **Engine is canonical.** `storage.legacy_text_wal` defaults to `false`. The dual-write helper now skips the text WAL by default.
- **Auto-migrate-on-boot.** If RocksDB has no SQL but `state/ddl.wal` / `state/dml.wal` exist, the service migrates them automatically before serving requests. Operators upgrading from Phase 2.0/2.1 don't need to run `vng-migrate` manually — but it's still there if they want explicit control.
- **All legacy text WAL helpers marked `#[deprecated]`** (since 0.2.0, removal in 0.3.0). Compiler warns at every call site that isn't `#[allow(deprecated)]`.

---

## ⚠️ Sandbox network status

The two domains needed to install latest Rust (`sh.rustup.rs`,
`static.rust-lang.org`) are **still blocked** as of this session — `403 Host
not in allowlist` from the egress proxy. Even some previously-allowed
domains (`crates.io`, `raw.githubusercontent.com`, `static.crates.io`) now
also return 403. Worth confirming the allowlist actually got updated.

This means I'm still on rustc 1.75 in the sandbox and still cannot
compile-test the full service. All 562 unit tests pass on what I CAN
compile (the small isolated crates).

---

## What landed this session

### ✅ Config: `legacy_text_wal` flag

**File:** `crates/voltnuerongrid-config/src/lib.rs`

```rust
pub struct StorageConfig {
    // ... existing fields ...
    /// Phase 2.2 — opt-in to ALSO write to the legacy text WAL files.
    /// Defaults to `false`. The next release will remove this entirely.
    #[serde(default)]
    pub legacy_text_wal: bool,
}
```

- Default `false`.
- Env var: `VNG_LEGACY_TEXT_WAL=1` to enable.
- JSON: `storage.legacy_text_wal: true`.
- 5 new unit tests covering default, env enable/disable, JSON load,
  back-compat with old config files (omitted key → false).

### ✅ Service: dual-write becomes engine-only by default

**File:** `services/voltnuerongridd/src/main.rs::persist_sql_statement`

The previous unconditional `append_sql_wal(&path, sql)` is now gated:
```rust
if state.runtime_config.storage.legacy_text_wal {
    append_sql_wal(&path, sql);
}
```

The metric `vng_wal_append_total` gained a `text_wal=yes|no` label so
operators can confirm at a glance whether the legacy path is actually
being used in production.

### ✅ Auto-migrate-on-boot

**File:** `services/voltnuerongridd/src/main.rs::auto_migrate_text_to_engine`

Called from `replay_ddl_into` and `replay_dml_into` before the engine-vs-text
fallback decision. Behaviour:

1. If a `<wal_path>.migrated` marker exists → skip (already migrated).
2. If the text WAL file doesn't exist → skip (nothing to migrate).
3. If the engine doesn't persist SQL (in-memory) → skip.
4. If the engine already has SQL of this kind → mark and skip.
5. Otherwise: read text WAL, append every statement to the engine, write
   `<wal_path>.migrated` marker.

After migration, the next replay step finds the data in the engine and
emits `vng_wal_replay_total{source=engine}` as before. The original
`state/*.wal` files are left untouched (operators may want them as a
sanity-check backup).

New metric `vng_wal_auto_migrate_total{kind}` counts how many statements
were lifted on first boot.

### ✅ Deprecation markers

**File:** `services/voltnuerongridd/src/main.rs`

Eight functions now carry:
```rust
#[deprecated(since = "0.2.0", note = "Phase 2.2: replaced by durability engine SQL streams. Will be removed in 0.3.0.")]
```

- `ddl_wal_path`, `dml_wal_path`
- `sql_wal_escape`, `sql_wal_unescape`
- `append_sql_wal`, `read_sql_wal`
- `replay_ddl_wal_into`, `replay_dml_wal_into`

Their authorised callers (`persist_sql_statement`,
`auto_migrate_text_to_engine`, `replay_*_into`) are tagged
`#[allow(deprecated)]` so the warnings only appear if a future PR adds a
new caller.

### ✅ Studio + sample config

- `ui/voltnuerongrid-studio/src/api/studio-client.ts` — added optional
  `legacy_text_wal?: boolean` to the `StorageConfig` interface.
- `vng.config.sample.json` — documented the new flag with a comment
  explaining when (rarely) to enable it.

---

## ⚠️ Things to verify on rustc 1.86+ (when it's available)

```bash
git checkout phase-2-2-text-wal-deprecation
cargo check --workspace            # should compile clean
cargo test --workspace              # 562 unit tests + 8 soak
cargo build --release -p voltnuerongridd

# Smoke test: upgrade path from a Phase-2.1 deployment
mkdir -p state data
echo "CREATE TABLE t (id INT, name TEXT)" > state/ddl.wal
echo "INSERT INTO t (id, name) VALUES (5, 'alice')" > state/dml.wal

VNG_LOG=info VNG_ADMIN_API_KEY=test ./target/release/voltnuerongridd &
sleep 1

# 1. Auto-migrate metric should report the lift.
curl -s http://127.0.0.1:8080/metrics | grep vng_wal_auto_migrate_total
# Expected: vng_wal_auto_migrate_total{kind="ddl"} 1
#           vng_wal_auto_migrate_total{kind="dml"} 1

# 2. Marker files should exist now.
ls -la state/*.migrated
# Expected: state/ddl.wal.migrated, state/dml.wal.migrated

# 3. Replay should report engine source.
# (Restart the service first to trigger replay)
pkill voltnuerongridd; sleep 1
./target/release/voltnuerongridd &
sleep 1
curl -s http://127.0.0.1:8080/metrics | grep vng_wal_replay_total
# Expected: vng_wal_replay_total{kind="ddl",source="engine"} 1
#           vng_wal_replay_total{kind="dml",source="engine"} 1

# 4. Confirm new writes go ONLY to the engine.
curl -X POST -H "x-vng-admin-key: test" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"sql_batch":"INSERT INTO t (id, name) VALUES (6, '\''bob'\'')"}' \
  http://127.0.0.1:8080/api/v1/sql/execute

curl -s http://127.0.0.1:8080/metrics | grep vng_wal_append_total
# Expected: vng_wal_append_total{kind="dml",text_wal="no"} 1

# 5. With legacy_text_wal=true, dual-write resumes.
pkill voltnuerongridd; sleep 1
VNG_LEGACY_TEXT_WAL=1 ./target/release/voltnuerongridd &
sleep 1
curl -X POST -H "x-vng-admin-key: test" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"sql_batch":"INSERT INTO t (id, name) VALUES (7, '\''carol'\'')"}' \
  http://127.0.0.1:8080/api/v1/sql/execute

curl -s http://127.0.0.1:8080/metrics | grep vng_wal_append_total
# Expected: vng_wal_append_total{kind="dml",text_wal="yes"} 1
# AND state/dml.wal grew by one line.
```

### Likely compile issues

1. **`#[allow(deprecated)]` placement.** I put it as an outer attribute
   above each authorised caller fn. If rustc complains it should be
   inside the fn body, move it to `#![allow(deprecated)]` at the top of
   the relevant `mod` instead.

2. **`now_unix_ms()` return type.** Phase 2.1 had this same caveat —
   confirm the helper returns whatever `apply_ddl_to_catalog` expects
   (currently `u128`).

---

## What's still TODO

### Phase 2.3 — actual deletion (next session, after one production cycle)

Once production has run on Phase 2.2 successfully and nobody is on
`legacy_text_wal=true`, mechanically delete:

1. The 8 deprecated functions.
2. The `legacy_text_wal` config field (or keep it as a deprecated no-op
   for compatibility).
3. The `text_wal` label from `vng_wal_append_total` (always `no` after
   removal).
4. The `vng-migrate` CLI under `tools/voltnuerongrid-migrate/` — keep one
   release cycle for operators with stale text WAL backups, then remove.

### Phase 3 — items still in the gaps doc

These are the next big rocks:

1. **DataFusion adoption** — the executor crate is named for it but
   currently uses a hand-rolled std-only evaluator. Once MSRV bumps for
   RocksDB anyway, swap in real DataFusion behind a feature flag for
   JOIN / GROUP BY / window / subquery support.
2. **Real RBAC** — the current matrix is hardcoded.
3. **Replication** — leader/follower with the existing Raft skeleton.

### Phase 0 follow-ups

1. `handler_lock!` macro rollout — still ~346 sites of `.lock().expect()`.
2. main.rs modular refactor — now ~34,900+ lines.

---

## How to continue

```
@.cursorrules
@gaps-may26-1.md
@remaining.md
@crates/voltnuerongrid-config/src/lib.rs
@crates/voltnuerongrid-store/src/lib.rs
@crates/voltnuerongrid-store/src/rocksdb_engine.rs
@services/voltnuerongridd/src/main.rs        # replay + persist_sql_statement
@tools/voltnuerongrid-migrate/src/main.rs
```

Recommended next step (your choice):

- **Phase 2.3 cleanup.** Lowest-risk; lets you remove ~300 lines from main.rs.
- **Phase 3 DataFusion.** Biggest functional unlock — JOIN / GROUP BY actually work.
- **main.rs modular refactor.** Maintainability win, no behaviour change.

Or unblock the network so I can install rustc 1.87 and compile the full
service end-to-end. The two domains needed:

- `static.rust-lang.org`
- `sh.rustup.rs`
