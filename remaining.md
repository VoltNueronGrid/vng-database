# `remaining.md` — handoff for next session (v4)

**Last updated:** 2026-05-05 (fourth session)
**Branch:** `phase-1-7-datafusion`
**Total unit tests passing locally:** 452 (399 SQL + 25 exec + 21 meta + 7 config)

---

## TL;DR

Phase 1.7 lands the **critical correctness fix** for the substring-matching
bug: `WHERE id = 5` now returns exactly the row with id 5, not rows
15, 25, 50, 51 etc. (gaps-may26-1.md §3.4).

**Important pivot from the original Phase 1.7 plan:** the new crate is named
`voltnuerongrid-exec-datafusion` but does **not** actually pull in DataFusion
yet. DataFusion 35+ has transitive deps requiring rustc 1.86+
(`edition2024`), and the sandbox is stuck at rustc 1.75. The crate name is
kept for forward compatibility — when MSRV bumps, real DataFusion can be added
behind a feature flag.

What's delivered today is a **correct, AST-driven SELECT executor** that
walks the sqlparser-rs Expr tree and evaluates it row-by-row. This delivers
the immediate correctness win without waiting for the toolchain.

---

## What this session delivered

### ✅ Phase 1.7 — correct SELECT executor

**New crate:** `crates/voltnuerongrid-exec-datafusion/` (~960 LOC, 25 tests)

- `execute_select(sql, &PagedRowStore, max_rows) -> Result<SelectOutput, ExecError>`
- `execute_parsed_select(&SelectStatement, raw_sql, ...)` for callers that
  already have the AST.

**Coverage today:**
- Equality / inequality: `=`, `!=`, `<>`
- Range: `<`, `<=`, `>`, `>=`, `BETWEEN ... AND ...`
- Set membership: `IN (...)` (literal list)
- Null tests: `IS NULL`, `IS NOT NULL`
- Boolean composition: `AND`, `OR`, `NOT`
- Pattern matching: `LIKE`, `NOT LIKE` (with `%` and `_` wildcards)
- Column projection (only listed columns returned, including `AS alias`)
- `ORDER BY` (any column, ASC / DESC, numeric or lexicographic comparison)
- `LIMIT` / `OFFSET`
- Bare aggregates without GROUP BY: `COUNT(*)`, `COUNT(col)`, `SUM(col)`,
  `AVG(col)`, `MIN(col)`, `MAX(col)`

**Returns `ExecError::Unsupported` for** (caller falls back to legacy):
- JOIN
- GROUP BY / HAVING
- Window functions
- Subqueries

**Critical regression test** (proves the §3.4 bug is fixed):
```rust
#[test]
fn where_eq_does_not_match_substrings() {
    // 5 rows with id 5, 15, 25, 50, 51
    let rows = unwrap_rows(execute_select("SELECT * FROM t WHERE id = 5", &rs, 100).unwrap());
    assert_eq!(rows.len(), 1, "WHERE id = 5 must match exactly one row");
    assert_eq!(rows[0].data.get("name").map(String::as_str), Some("Alice"));
}
```

### ✅ Service integration

**File:** `services/voltnuerongridd/src/main.rs`

- `voltnuerongrid-exec-datafusion` added as service dep.
- `execute_oltp_select()` rewritten:
  1. Try the new correct executor first (`execute_select(...)`).
  2. On `Ok(Rows)` — emit them, increment `vng_sql_select_executor_total{engine=vng_correct,outcome=ok}`.
  3. On `Err(Unsupported)` — fall back to `execute_oltp_select_legacy()`,
     emit `outcome=unsupported_fallback`.
  4. On `Ok(Aggregate)` or other errors — pass through to legacy/skip.
- Legacy substring scanner kept as `execute_oltp_select_legacy()` with a
  doc comment marking it as known-incorrect for `WHERE col = val` cases.
- New metric: `vng_sql_select_executor_total` labeled by `engine` and
  `outcome` so operators can see when the legacy path is hit.

---

## ⚠️ Things to verify on rustc 1.86+

Same constraint as last 3 sessions: the sandbox can't compile the full
`voltnuerongridd` service. The new exec crate compiles + tests cleanly on
rustc 1.75, but the integration into `main.rs` was not compiler-verified.

```bash
cargo check --workspace
cargo test -p voltnuerongrid-exec-datafusion   # 25 tests, expected pass
cargo test --workspace
cd ui/voltnuerongrid-studio && npx tsc --noEmit
```

### Likely compile issues in `voltnuerongridd`

1. **Crate name mismatch in import**: I used
   `use voltnuerongrid_exec_datafusion::{execute_select, SelectOutput, ExecError};`
   inside the new `execute_oltp_select`. Underscores are correct (Rust
   converts hyphens in package names to underscores in extern crate names).

2. **`metrics::counter!` macro syntax** — I used the v0.23 syntax matching
   what's already in the file.

3. **No new types in public API** — the new exec functions return
   `voltnuerongrid_exec_datafusion::ResultRow { key, data }` which the
   caller maps to existing `OltpRowResult { key, data }`. Same field
   names, no breakage.

---

## What's still TODO

### Phase 1.7-extended — JOIN, GROUP BY, real DataFusion

The new executor returns `Unsupported` for JOIN, GROUP BY, HAVING, window
functions, and subqueries. These currently fall through to the legacy
substring scanner — which is **also broken** for them, just differently.

Two paths from here:

**Option A — wait for MSRV bump, then adopt real DataFusion.**
Once the workspace can compile against rustc 1.86+, add DataFusion behind
a feature flag in `voltnuerongrid-exec-datafusion`. DataFusion handles
JOIN/GROUP BY/window/subquery natively. Effort: M-L (~1 week to wire,
plus the DataFusion learning curve).

**Option B — hand-roll the missing operators.** Extend the new executor
incrementally. JOIN ~3 days, GROUP BY ~2 days. Cheaper to start,
pricier long-term as features pile up.

**Recommendation: Option A.** DataFusion's correctness is battle-tested
across hundreds of OSS projects. The MSRV bump is happening anyway when
you adopt RocksDB (Phase 2).

### Phase 2 — RocksDB durable storage

Per Pavan's answer in the original session: configurable `RocksDB | VNG`
storage with RocksDB as the default. The config selector is already wired
(Phase 0). Now actually implement RocksDB.

**Plan:**
1. Add `rocksdb = "0.21"` to `voltnuerongrid-store`.
2. Create `RocksDbDurabilityEngine` implementing `DurabilityEngine` trait.
3. Drive selection via `state.runtime_config.storage.engine`.
4. Crash-recovery CI test: write rows, kill -9, restart, verify rows.
5. Migration: in-memory engine remains for tests; RocksDB for production.

The MSRV bump from RocksDB's deps is what unblocks DataFusion in Phase 1.7-extended.

### Phase 0 follow-ups (still pending)

1. **Roll out `handler_lock!` macro** to the ~346 `.lock().expect()` sites.
2. **Refactor `main.rs` into modules** — target structure in earlier
   `remaining.md` versions.
3. **Wire `vng_sql_execute_total` counter** into `sql_execute` (currently
   only the new `vng_sql_select_executor_total` increments).

---

## How to continue from a fresh Cursor session

```
@.cursorrules
@gaps-may26-1.md
@remaining.md
@crates/voltnuerongrid-config/src/lib.rs
@crates/voltnuerongrid-meta/src/lib.rs
@crates/voltnuerongrid-sql/src/sqlparser_adapter.rs
@crates/voltnuerongrid-exec-datafusion/src/lib.rs
@services/voltnuerongridd/src/resilience.rs
@services/voltnuerongridd/src/observability.rs
```

Recommended next step: **Phase 2 (RocksDB)** — the MSRV bump unblocks
everything else. Or **handler_lock! rollout** if you want a quick
maintainability win without dep churn.

---

## Smoke test for this session

```bash
# Boot server
cargo build --release
VNG_LOG=debug VNG_ADMIN_API_KEY=test ./target/release/voltnuerongridd &

# Set up test data
curl -X POST -H "x-vng-admin-key: test" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"sql_batch":"CREATE TABLE t (id INT, name TEXT)"}' \
  http://127.0.0.1:8080/api/v1/sql/execute

for i in 5 15 25 50 51; do
  curl -X POST -H "x-vng-admin-key: test" -H "x-vng-operator-id: admin" \
    -H "content-type: application/json" \
    -d "{\"sql_batch\":\"INSERT INTO t (id, name) VALUES ($i, 'row_$i')\"}" \
    http://127.0.0.1:8080/api/v1/sql/execute > /dev/null
done

# THE BIG TEST: WHERE id = 5 must return exactly one row
curl -s -X POST -H "x-vng-admin-key: test" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"sql_batch":"SELECT * FROM t WHERE id = 5"}' \
  http://127.0.0.1:8080/api/v1/sql/execute | jq '.oltp_rows'
# Expected: array with exactly one element, name = "row_5"
# Pre-Phase-1.7 behaviour: 5 rows including row_15, row_25, row_50, row_51

# Verify metrics
curl -s http://127.0.0.1:8080/metrics | grep vng_sql_select_executor_total
# Expected: vng_sql_select_executor_total{engine="vng_correct",outcome="ok"} 1+
```

---

## Sandbox & PAT note

To push from Claude's sandbox, give it the GitHub PAT in the chat.
Working directory: `/home/claude/vng-database/`.
The sandbox is ephemeral — wiped at session end.

---

## Updated next-session ordering (v5 — 2026-05-04 addendum)

1. **`cargo check --workspace` on rustc 1.86+** — verify the Phase 1.7
   `voltnuerongrid-exec-datafusion` integration in `services/voltnuerongridd/src/main.rs`
   compiles, since the previous sandbox couldn't (rustc 1.75).
2. **Phase 2 — RocksDB durable storage:**
   - Add `rocksdb = "0.21"` to `voltnuerongrid-store` (forces MSRV bump → unblocks DataFusion).
   - Implement `RocksDbDurabilityEngine` against the existing `DurabilityEngine` trait.
   - Drive selection via the Phase 0 config selector
     (`state.runtime_config.storage.engine`) — already wired, defaulting to RocksDB.
   - Add a kill -9 / restart crash-recovery CI test.
   - Keep the in-memory engine for unit tests.

### 2026-05-04 sanity-check note
`cargo check --workspace` on rustc 1.95.0 surfaced **no** Phase 1.7 wiring
issues — `voltnuerongrid-exec-datafusion` is correctly imported into
`services/voltnuerongridd/src/main.rs`. The 13 errors that did appear were
unrelated pre-existing drift: every `AuthErrorResponse` literal in the admin
database-catalog handlers (lines ~20602 onward) had to gain `locale` and
`localized_message` fields after the struct grew them. Fixed in this session.
Workspace is now clean (one unrelated unused-variable warning in
`sqlparser_adapter.rs:274`).

### 2026-05-04 — Phase 2 (RocksDB) initial landing

What landed in this session (`crates/voltnuerongrid-store`):

- `rocksdb = "0.22"` workspace dependency (transitive: librocksdb-sys 8.10).
- New module `rocksdb_engine` with `RocksDbDurabilityEngine`:
  - 3 column families: `cf_kv` (latest values), `cf_wal` (be-seq → record),
    `cf_checkpoints` (be-id → manifest).
  - `append_mutation` issues a single `WriteBatch` covering kv + wal,
    submitted with `WriteOptions::set_sync(wal_fsync_on_commit)` so
    durability matches `RuntimeConfig.storage.wal_fsync_on_commit`. **This
    closes the flush()/fsync() durability gap.**
  - `force_checkpoint`, `maybe_checkpoint`, `latest_checkpoint`, `wal_records`,
    `wal_len`, `latest_sequence`, `checkpoint_count`, `get` mirror the
    in-memory engine's API with `Result` return types.
  - 6 unit tests, including:
    - `survives_drop_and_reopen_like_sigkill` — the kill -9 substitute:
      writes 3 rows, drops the engine without graceful close, reopens at
      the same path, asserts all rows + sequence + wal records survived,
      and that the next write continues at sequence=4.
    - `checkpoint_id_persists_across_reopen` — guards against the obvious
      regression where reopen restarts checkpoint_id at 1.
- Workspace test count: 90 passing in `voltnuerongrid-store` (was 84 + 6 new).

What is **NOT** done yet (next-next session):

1. **Cut `services/voltnuerongridd/src/main.rs` over to the new engine.**
   The file holds `wal_engine: Arc<Mutex<InMemoryDurabilityEngine>>` and
   has ~80 call sites that touch `wal.wal_records()`. A direct switch to
   `Arc<Mutex<RocksDbDurabilityEngine>>` would require either:
   - Refactoring those 80 call sites to handle `Result<Vec<WalRecord>, _>`,
     **or**
   - Introducing a `BoxedDurabilityEngine` enum that `unwrap()`s rocksdb
     errors at the boundary and returns `&[WalRecord]`-shaped views.
   Recommend the second path: add `BoxedDurabilityEngine` in
   `voltnuerongrid-store/src/lib.rs` with delegating methods that match
   the in-memory API exactly, dispatch on `state.runtime_config.storage.engine`
   at boot, and migrate the 2 construction sites
   (`main.rs:5871` and `main.rs:21861`).
2. **Real kill -9 CI test** — the unit test simulates SIGKILL via drop;
   add a `tests/` integration test that spawns the binary, kills it with
   `kill -9`, restarts it, and verifies row survival end-to-end via HTTP.
3. **Remove the two `wal_engine`-touching tests** that hardcode the
   in-memory type once main.rs is migrated.
