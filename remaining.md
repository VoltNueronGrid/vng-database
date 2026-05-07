# `remaining.md` — handoff for next session (v9)

**Last updated:** 2026-05-07 (tenth session — Slices 5-8 + Rust 1.85 install)
**Branch:** `main`  
**Commit:** 9e06d00 (Slices 5-7 pushed) + Slice 8 uncommitted (router.rs)
**Total unit tests passing locally:** 556 (399 SQL + 97 store + 32 config + 21 meta + 7 exec)
**Rust version:** 1.85.1 (from Ubuntu 25.04 plucky repo via archive.ubuntu.com)

---

## TL;DR — This session

### Rust 1.85.1 installed ✅
- Solved via Ubuntu 25.04 (plucky) apt repo — NO need for sh.rustup.rs
- libclang-18-dev installed for RocksDB bindgen
- idna_adapter pinned to 1.1.0 (avoids icu_* MSRV 1.86 requirement)
- comfy-table pinned to 7.1.3 (avoids let-chain syntax requiring 1.88)
- RocksDB first-build takes ~10 min (C++ compilation)

### Install commands for next sandbox session:
```bash
echo "deb http://archive.ubuntu.com/ubuntu plucky main" >> /etc/apt/sources.list
apt-get update -qq
apt-get install -y -t plucky rustc cargo
apt-get install -y libclang-18-dev
rustc --version  # should print 1.84.1 or 1.85.1
export LIBCLANG_PATH=/usr/lib/llvm-18/lib

cd /home/claude/vng-database
cargo update idna_adapter --precise 1.1.0
cargo update comfy-table --precise 7.1.3
```

### main.rs refactor progress ✅
```
Slices 1-4 (prior sessions): 34,876 → 24,962 lines  
Slice 5 — handlers/wal.rs:    24,962 → 22,151 (87 handlers, 103 DTOs)
Slice 6 — handlers/audit.rs:  22,151 → 21,898  (6 handlers, 9 DTOs)
Slice 7 — handlers/rows.rs:   
           handlers/raft.rs:   21,898 → 17,857  (77+13+24 handlers)
           handlers/misc.rs:
Slice 8 — router.rs:          17,857 → 17,351 (508 route defs extracted)
TOTAL:    34,876 → 17,351  (−17,525 lines, −50%)
```

**Handler modules now on disk:**
admin.rs, audit.rs, autonomous.rs, catalog.rs, cdc.rs, driver.rs,
ingest.rs, misc.rs, raft.rs, rows.rs, security.rs, sql.rs, sre.rs,
store.rs, wal.rs

**router.rs** — build_router(state) fn with all 330+ routes

**main.rs remaining (~17,351 lines):**
- Core shared types (AppState, AuthErrorResponse, PoolStatsResponse, etc.)
- async fn main() + startup/boot logic
- Shared helpers (now_unix_ms, require_operator_auth, etc.)
- cache_redis_command + ~40 unit tests (kept deliberately)
- native socket helpers (native_read/write_framed)

---

## ⚠️ IMPORTANT: Slice 8 NOT committed yet

Slice 8 (router.rs extraction) was done but NOT committed/pushed before
token limit. Next session must commit first:

```bash
cd /home/claude/vng-database
git add -A
git commit -m "refactor(voltnuerongridd): Slice 8 — extract build_router() to router.rs

Extracts the 330-route axum router from async fn main() into a dedicated
fn build_router(state: AppState) -> Router in src/router.rs.
main.rs: 17,857 → 17,351 lines (−508)"
git push https://YOUR_GITHUB_PAT@github.com/VoltNueronGrid/vng-database.git main
```

Then run the service compile check:
```bash
export LIBCLANG_PATH=/usr/lib/llvm-18/lib
cargo check -p voltnuerongridd 2>&1 | tail -5
# First run takes ~10min for RocksDB C++ compile
# Subsequent runs are fast (cached)
```

---

## Likely compile errors to fix next session

The handler module extraction used automated Python (regex-based), so some
issues are expected. Common patterns from prior slices:

1. **Type not found** — a DTO used by a handler module but not imported.
   Fix: add it to the `use crate::{..}` block in the handler file.

2. **Function not visible** — helper fn needed by a handler is still private
   in main.rs. Fix: mark it `pub(crate)` in main.rs.

3. **Duplicate type** — same struct name in both main.rs and a handler module
   because the extraction missed removing the main.rs copy.
   Fix: delete the main.rs copy (keep handler module version as pub(crate)).

4. **router.rs** — `build_router` references handler functions by name.
   Those must be pub(crate) in their respective handler modules.
   All should already be pub(crate) from the visibility fix pass.

Workflow to fix compile errors:
```bash
export LIBCLANG_PATH=/usr/lib/llvm-18/lib
cargo check -p voltnuerongridd 2>&1 | grep "^error\[" | head -20
# Fix each error, then re-run
```

---

## Remaining refactor work (Slices 9+)

### Slice 9 — extract shared helpers to src/helpers.rs (low-risk, ~200 lines)

Functions that are pure utilities and can be shared from a helpers module:
- `now_unix_ms()`, `now_unix_ms_u64()`
- `failure_budget_snapshot()`, `rate_limit_policy_snapshot()`
- `evaluate_rate_limit()`, `evaluate_failure_budget_alert()`
- `build_retry_plan()`, `enqueue_dr_hook_task()`
- `latest_dr_hook_records()`, `pool_stats_response()`
- `record_transport_mutation()`

### Slice 10 — break AppState into sub-structs (~50 lines, high value)

AppState currently has ~40 fields. Group into:
- `StorageState` (wal_engine, row_store, ddl_catalog)
- `ClusterState` (raft, replication, node_runtime)  
- `ObservabilityState` (metrics, audit_sink)

### Final goal: main.rs < 5,000 lines

Current: ~17,351 | Target: < 5,000
Gap: ~12,000 lines — mostly startup logic, AppState impl, and the
cache_redis_command block (~2,000 lines with tests).

---

## Next major feature work

### Phase 3.1 — DataFusion DataFrame hydration

File: `crates/voltnuerongrid-exec-datafusion/src/datafusion.rs`

The skeleton exists (returns Unsupported). Implementation plan:
1. `hydrate_dataframe(rs: &PagedRowStore, table: &str, schema: &DdlSchema) -> DataFrame`
2. Parse SQL → DataFusion LogicalPlan
3. Execute plan → collect Arrow RecordBatches
4. Convert RecordBatches → SelectOutput (Vec<Row>)

Tests needed:
- `test_select_with_join()` — INNER JOIN two tables
- `test_select_with_group_by()` — COUNT(*) GROUP BY col
- `test_select_with_window()` — ROW_NUMBER() OVER (ORDER BY id)

### Phase 2.3 — final text WAL deletion

The 8 `#[deprecated]` helpers in main.rs can be deleted once production
has been running on Phase 2.2 for one release cycle. Names:
ddl_wal_path, dml_wal_path, sql_wal_escape, sql_wal_unescape,
append_sql_wal, read_sql_wal, replay_ddl_wal_into, replay_dml_wal_into

---

## How to start next session

```bash
# 1. Reinstall Rust (sandbox resets between sessions)
echo "deb http://archive.ubuntu.com/ubuntu plucky main" >> /etc/apt/sources.list
apt-get update -qq && apt-get install -y -t plucky rustc cargo
apt-get install -y libclang-18-dev
export LIBCLANG_PATH=/usr/lib/llvm-18/lib

# 2. Pull latest main
cd /home/claude/vng-database
git fetch https://YOUR_GITHUB_PAT@github.com/VoltNueronGrid/vng-database.git main
git checkout main && git reset --hard FETCH_HEAD

# 3. Fix Cargo.lock pins
cargo update idna_adapter --precise 1.1.0
cargo update comfy-table --precise 7.1.3

# 4. Commit Slice 8 if not already committed
git status  # check for uncommitted changes

# 5. Service compile check (takes ~10min first time)
cargo check -p voltnuerongridd 2>&1 | tail -5

# 6. Fix any compile errors, then full test suite
cargo test --workspace
```

Contexts to load:
```
@remaining.md
@services/voltnuerongridd/src/main.rs
@services/voltnuerongridd/src/router.rs
@services/voltnuerongridd/src/handlers/mod.rs
```
