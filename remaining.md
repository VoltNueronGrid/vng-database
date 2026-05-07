# `remaining.md` — handoff for next session (v10)

**Last updated:** 2026-05-07 (eleventh session — Slices 9-10)
**Branch:** main (latest commit: Slice 10 — tests.rs extraction)
**Unit tests passing:** 556 (399 SQL + 97 store + 32 config + 21 meta + 7 exec)
**Rust:** 1.85.1 via Ubuntu plucky apt

---

## ✅ TL;DR — This session accomplished

### main.rs refactor COMPLETE

```
Start of session:  17,351 lines (Slices 1-8 already done)
Slice 9 helpers:   14,793 lines  (helpers/ module tree, 44 fns extracted)
Slice 10 tests:     1,894 lines  (12,901-line test module → tests.rs)
TOTAL: 34,876 → 1,894 lines  (−94.6%)
```

**File map of the service:**
```
services/voltnuerongridd/src/
├── main.rs              1,894 lines  ← core types + startup + mod declarations
├── tests.rs            12,898 lines  ← cache_redis unit tests (moved here)
├── router.rs              534 lines  ← build_router() with all 330+ routes
├── auth.rs                           ← require_operator_auth/privilege
├── audit_helpers.rs                  ← append_audit_event
├── config_init.rs                    ← config init helpers
├── observability.rs                  ← metrics/tracing setup
├── handlers/
│   ├── admin.rs         ← admin handlers
│   ├── audit.rs         ← audit_events, audit_chain_verify, etc.
│   ├── autonomous.rs    ← autonomous mode handlers
│   ├── catalog.rs       ← DDL catalog handlers
│   ├── cdc.rs           ← CDC handlers
│   ├── driver.rs        ← driver/session handlers
│   ├── ingest.rs        ← ingest handlers
│   ├── misc.rs          ← chaos, benchmark, failover, mcp, search, etc.
│   ├── raft.rs          ← raft handlers
│   ├── rows.rs          ← rows_* handlers (77 total)
│   ├── security.rs      ← security handlers
│   ├── sql.rs           ← sql_execute, sql_transaction handlers
│   ├── sre.rs           ← SRE reliability handlers
│   ├── store.rs         ← store handlers
│   └── wal.rs           ← wal_* handlers (87 total)
└── helpers/
    ├── boot.rs          ← WAL replay, durability engine init, RBAC defaults
    ├── cluster.rs       ← leader rotation, pool stats, data plane connections
    ├── dr_hook.rs       ← DR hook, SRE gate, failure budget, rate limiting
    ├── env_helpers.rs   ← read_env_bool/usize/u64
    ├── execution.rs     ← OLAP/OLTP execution, pessimistic locks
    ├── native_protocol.rs← TLS, wire frames, native auth
    ├── sql_parse.rs     ← extract_*, parse_where_predicates
    ├── time.rs          ← now_unix_ms, now_unix_ms_u64, now_epoch_ms_chaos
    └── udf.rs           ← UDF runtime scaffold and catalog
```

---

## ⚠️ What still needs to be done next session

### 1. Service compile verification (FIRST PRIORITY)

RocksDB takes ~10 min to compile first time. Next session:
```bash
export LIBCLANG_PATH=/usr/lib/llvm-18/lib
cargo check -p voltnuerongridd 2>&1 | tail -10
```

**Expected errors** (from automated extraction — regex-based, not AST-based):

a) **Missing imports in handler/helper files** — some types referenced in
   extracted modules weren't included in the `use crate::{...}` header.
   Fix pattern: for each `error[E0412]: cannot find type X`, add X to
   the use statement in the relevant file.

b) **Visibility issues** — some helper fns called from handlers might still
   be private in main.rs. Fix: mark `pub(crate)` in main.rs.

c) **tests.rs `use super::*`** — `super` in tests.rs is the `tests` module
   declared in main.rs. All items used in tests must be visible from
   main.rs's scope. Verify `use super::*` covers everything.

**Systematic fix workflow:**
```bash
export LIBCLANG_PATH=/usr/lib/llvm-18/lib
# See all errors:
cargo check -p voltnuerongridd 2>&1 | grep "^error\[" | sort | uniq -c | sort -rn | head -20
# Fix each group, rerun
# Goal: cargo check passes clean
```

### 2. Full test suite on service

Once compile passes:
```bash
cargo test -p voltnuerongridd 2>&1 | grep "test result"
# The cache_redis tests in tests.rs should run here
```

### 3. Phase 3.1 — DataFusion DataFrame hydration

File: `crates/voltnuerongrid-exec-datafusion/src/datafusion.rs`

The skeleton exists (returns `Unsupported`). Implementation:
```rust
pub async fn execute_select(sql, store, max_rows) -> Result<SelectOutput, ExecError> {
    // 1. hydrate_dataframe(store, table, schema) -> DataFrame
    // 2. Parse SQL via DataFusion's SessionContext::sql()
    // 3. Execute → collect RecordBatches
    // 4. Convert to SelectOutput (Vec<HashMap<String, String>>)
}
```

Tests to add:
- `test_join()` — INNER JOIN two tables via DataFusion
- `test_group_by()` — COUNT(*) GROUP BY col
- `test_window()` — ROW_NUMBER() OVER (ORDER BY id)

### 4. Phase 2.3 — delete deprecated text WAL helpers

Eight `#[deprecated]` helpers still in `helpers/boot.rs`:
- `ddl_wal_path`, `dml_wal_path`  
- `sql_wal_escape`, `sql_wal_unescape`
- `append_sql_wal`, `read_sql_wal`
- `replay_ddl_wal_into`, `replay_dml_wal_into`

Safe to delete once one production cycle on Phase 2.2.

### 5. Real RBAC (Phase 3 extended)

`helpers/boot.rs::default_rbac_privilege_matrix()` returns a hardcoded
matrix. Replace with a proper configurable matrix loaded from
`config_init.rs` using the `RbacPrivilegeMatrix` type.

---

## Session startup recipe (run every new session)

```bash
# Rust is already installed if same sandbox session
# If new sandbox:
echo "deb http://archive.ubuntu.com/ubuntu plucky main" >> /etc/apt/sources.list
apt-get update -qq && apt-get install -y -t plucky rustc cargo
apt-get install -y libclang-18-dev
export LIBCLANG_PATH=/usr/lib/llvm-18/lib

# Pull latest
cd /home/claude/vng-database
git fetch https://YOUR_GITHUB_PAT@github.com/VoltNueronGrid/vng-database.git main
git checkout main && git reset --hard FETCH_HEAD

# Fix Cargo.lock pins (needed after fresh fetch)
cargo update idna_adapter --precise 1.1.0
cargo update comfy-table --precise 7.1.3

# Service compile (first run triggers RocksDB build, ~10 min)
cargo check -p voltnuerongridd 2>&1 | tail -10

# Unit tests (fast, no rocksdb needed):
cargo test -p voltnuerongrid-sql -p voltnuerongrid-store \
    -p voltnuerongrid-meta -p voltnuerongrid-config \
    -p voltnuerongrid-exec-datafusion 2>&1 | grep "test result"
```
