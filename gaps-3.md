# Gap Analysis — VoltNueronGrid Remaining Gaps (post-session 26)

**Prepared:** 2026-05-20 (session 26 close)
**Branch:** `claude/friendly-hertz-3b69fb`
**Based on:** `gaps-may26-1.md` + `gaps-may20-2.md` cross-referenced against actual code
**Test baseline:** 770 passed (voltnuerongridd), 100 (store), 48 (datafusion), 7 sandbox failures (driver-rust)

This document records only **partially addressed** and **fully open** gaps.
Fully closed gaps are noted in `remaining.md` session summaries.

---

## Severity legend

| Icon | Meaning |
|---|---|
| 🔴 | Critical — blocks correctness or production use |
| 🟠 | High — data correctness or durability risk |
| 🟡 | Medium — scale / completeness |
| 🟢 | Low — polish / nice-to-have |

---

## 🔴 Critical — Partially Addressed

### C-1 · Row store primary path still in-memory HashMap
**Severity:** 🔴 · **Effort:** XXL · **Original:** `gaps-may20-2.md §1`

**Current state:** `store_row()` persists every DML write to RocksDB `CF_ROWS`, and
`load_persisted_rows_into()` replays those rows at boot. Crash safety: data is not lost.

**Remaining gap:** The primary read/write path is still `PagedRowStore` (an in-memory
`HashMap`). All queries scan the HashMap, not RocksDB. The HashMap grows unbounded in
memory. RocksDB is write-through persistence, not the authoritative storage engine.

**Files:** `crates/voltnuerongrid-store/src/mvcc.rs`, `rocksdb_engine.rs`

**What's needed:** Make `PagedRowStore` a thin cursor over RocksDB `CF_ROWS` so reads
go to disk and in-memory state is bounded (LRU page cache or similar).

---

### C-2 · Physical DB isolation is key-prefix only (no per-DB column families)
**Severity:** 🔴 · **Effort:** XL · **Original:** `gaps-may20-2.md §2`

**Current state:** Single shared `CF_ROWS` column family across all databases.
Database isolation is achieved via key prefix `{db}\x1f{row_key}\x1f{xid}`.

**Remaining gap:** No structural separation at the RocksDB level. A range scan for
one database can theoretically bleed into another database's key space if the prefix
comparison has a bug. `DROP DATABASE` must manually delete all matching prefixes instead
of simply dropping a CF.

**Files:** `crates/voltnuerongrid-store/src/rocksdb_engine.rs`

**What's needed:** Create one RocksDB column family per database (created lazily on
`CREATE DATABASE`, dropped atomically on `DROP DATABASE`).

---

### C-3 · ACID isolation levels — `repeatable_read` label stored but not enforced
**Severity:** 🔴 · **Effort:** XL · **Original:** `gaps-may20-2.md §3`

**Current state:**
- `"read_committed"` (default): label stored; no snapshot enforcement beyond commit ordering.
- `"repeatable_read"`: records `read_snapshot_at_ms` timestamp at `BEGIN` — **but that
  timestamp is never consulted during SELECT execution**. Reads return current committed
  state, not the snapshot at `BEGIN` time. The label is effectively a no-op.
- `"serializable"`: table-level write-write conflict detection at `COMMIT`
  (`check_serializable_conflict()` in `main.rs:369`). This is a coarse approximation
  — any two serializable transactions writing to the same table conflict, regardless of
  which rows. Not a true Serializable Snapshot Isolation (SSI) implementation.

**Files:** `services/voltnuerongridd/src/main.rs:251,369`, `src/handlers/sql.rs`

**What's needed:**
1. Hook `read_snapshot_at_ms` into the `scan_at_snapshot(xid)` call so repeatable-read
   transactions see the row-store state from their `BEGIN` time.
2. Upgrade serializable to row-level conflict detection (true SSI or OCC).

---

## 🟠 High — Open

### H-1 · No cost-based query optimizer (heuristic only)
**Severity:** 🟠 · **Effort:** L · **Original:** `gaps-may20-2.md §7`

**Current state:** `QueryPlanner::estimate_cost` uses hardcoded heuristics — `relative_cost`
on statement kind + row count threshold (`sql.htap_olap_threshold_rows`). No statistics,
no cardinality estimation, no JOIN reorder.

**Files:** `crates/voltnuerongrid-exec/src/` (optimizer/planner)

**What's needed:** Collect basic table statistics (row count, distinct values per column)
and use them to compare join orders and index access paths. Even a simple selectivity model
(histogram or top-N sketch) would be a step up.

---

## 🟡 Medium — Partially Addressed / Open

### M-1 · Tenant user DB scoping: `db_grants` exists but auth check ignores it ⚡ **QUICK WIN**
**Severity:** 🟡 · **Effort:** S · **Status:** New gap found in session 26 audit

**Current state:** Session 26 added:
- `db_grants: Arc<Mutex<HashMap<String, HashSet<String>>>>` to `AppState`
- `POST/GET/DELETE /api/v1/admin/databases/:name/grants` CRUD endpoints

But `auth.rs::principal_has_database_access()` still returns `true` unconditionally
for all `TenantUser` principals (line 378–380), ignoring `db_grants` entirely.

```rust
// auth.rs:378 — current (wrong)
crate::RuntimeAccessPrincipal::TenantUser(_) => {
    true   // ← ignores db_grants completely
}
```

**Files:** `services/voltnuerongridd/src/auth.rs:378`

**Fix (3 lines):**
```rust
crate::RuntimeAccessPrincipal::TenantUser(user) => {
    if let Ok(grants) = state.db_grants.lock() {
        grants.get(database).map_or(false, |roles| roles.contains(&user.role))
    } else {
        false  // deny on poisoned mutex rather than silently allow
    }
}
```

**Caveat:** After this fix, all tenant user requests will be denied until an admin
explicitly grants their role access to each database. The "open" default (any tenant
can access any DB) needs to be a conscious design decision — either keep a migration
flag or auto-grant `operator` role on database creation.

---

### M-2 · SQL features: parsed but not executed
**Severity:** 🟡 · **Effort:** M · **Original:** `gaps-may20-2.md §13`

**Current state:** GROUP BY, HAVING, JOINs, window functions, subqueries all execute
correctly through DataFusion (confirmed). The following **parse without error but produce
no effect at runtime**:

| Statement | Parsed? | Executed? | Notes |
|---|---|---|---|
| `ALTER TABLE … ADD/DROP COLUMN` | ✅ | ❌ | DDL catalog intercepts but no physical column migration |
| `CREATE INDEX` | ✅ | Partial | Index entry created; not used by query executor |
| SQL `GRANT/REVOKE` | ✅ | ❌ | No runtime privilege table updated |
| `MERGE` / `UPSERT` | ✅ | ❌ | No merge executor path |
| `SET TRANSACTION ISOLATION LEVEL` | ✅ | Partial | Updates label only; see C-3 |

**Priority within this gap:** `ALTER TABLE` column migration is highest value (needed
for schema evolution). `CREATE INDEX` execution (make scans use the index) second.

**Files:** `services/voltnuerongridd/src/handlers/sql.rs`, `helpers/sql_parse.rs`,
`crates/voltnuerongrid-store/src/ddl_catalog.rs`

---

### M-3 · `.expect("lock")` panic risk in handler paths
**Severity:** 🟡 · **Effort:** S · **Original:** `gaps-may20-2.md §12`

**Current state:** 31 `.expect("…lock")` calls in `sql.rs` alone. If any Rust thread
panics while holding a mutex, the mutex becomes poisoned and the next `.expect()` in any
handler panics too — bringing down the entire service. The correct pattern (already used
in `admin.rs` database handlers) is:

```rust
match state.row_store.lock() {
    Ok(rs) => rs,
    Err(_) => return Err(svc_unavailable_sql_response("row_store mutex poisoned")),
}
```

**Files:** `services/voltnuerongridd/src/handlers/sql.rs` (31 sites),
`handlers/misc.rs`, `handlers/user_mgmt.rs`

**What's needed:** Systematic replacement in DML commit paths, WAL write paths, and
session management paths. Inner closures that hold no state (short-lived sub-scopes)
are lower priority.

---

### M-4 · Crash-recovery integration test missing
**Severity:** 🟡 · **Effort:** M · **Original:** `gaps-may20-2.md §18`

**Current state:** In-process unit tests for WAL replay exist. Integration tests for
linearisable writes exist (session 25). But there is no test that:
1. Writes rows via DML
2. Stops the service abruptly (simulated crash)
3. Restarts the service
4. Verifies all committed rows are present and uncommitted rows are absent

**Files:** `services/voltnuerongridd/src/tests.rs`, `helpers/boot.rs`

---

### M-5 · SQL injection risk in legacy OLTP executor
**Severity:** 🟡 · **Effort:** M · **Original:** `gaps-may20-2.md §17`

**Current state:** DataFusion path is safe (DataFusion handles parameterisation
internally). The legacy OLTP executor (`extract_insert_row_from_sql()`,
`extract_delete_key_from_sql()`) parses raw SQL strings with no sanitisation or
prepared-statement binding. A specially crafted key value can influence which rows are
matched or overwritten.

**Files:** `services/voltnuerongridd/src/helpers/sql_parse.rs`

**What's needed:** Parameter binding layer so values in INSERT/UPDATE/DELETE are
never re-parsed as SQL syntax. Alternatively, enforce all DML through the sqlparser-rs
AST path rather than string extraction.

---

### M-6 · Studio settings panel missing
**Severity:** 🟡 · **Effort:** M · **Original:** `gaps-may20-2.md §15`

**Current state:** Per-query routing badge ✅ (exists in `ResultsPane.tsx`).
UsersPanel ✅ (wired to server in session 26). But there is no Settings panel for:
- Per-connection configuration (max rows, isolation level override)
- OLTP vs OLAP routing threshold configuration
- Server runtime config viewer (beyond the raw JSON from `/api/v1/admin/runtime-config`)

**Files:** `ui/voltnuerongrid-studio/src/components/Settings/`

---

### M-7 · OTEL: tracing spans instrumented but no exporter configured
**Severity:** 🟡 · **Effort:** S · **Original:** `gaps-may20-2.md §19` (partial)

**Current state:** `#[tracing::instrument(skip_all)]` is now on 5 key handlers
(session 26). The `tracing` crate publishes spans to subscribers. But there is no
`opentelemetry` / `tracing-opentelemetry` integration — spans are only visible
if `RUST_LOG` subscriber is configured for structured JSON output. No OTLP export.

**Files:** `services/voltnuerongridd/src/observability.rs`, `Cargo.toml`

**What's needed:** Add `opentelemetry`, `opentelemetry-otlp`, and
`tracing-opentelemetry` crate dependencies; wire an OTLP exporter (or stdout JSON
exporter) in `observability.rs::init_tracing()`.

---

### M-8 · Codd's rules — multiple not satisfied
**Severity:** 🟡 · **Effort:** S–M · **Original:** `gaps-may20-2.md §14`

| Rule | Name | Status |
|---|---|---|
| Rule 1 | Information representation | Partial — no NULL support; all values are `String` |
| Rule 2 | Guaranteed access | Partial — row key schema not always stable across schema changes |
| Rule 6 | View updatability | Not implemented — views are read-only stubs |
| Rule 9 | Logical data independence | Not implemented — view definitions don't shield queries from physical layout changes |
| Rule 12 | Non-subversion | WAL and RocksDB APIs bypass the transaction manager |

---

## 🟢 Low — Open

### L-1 · `CALL insert_rows` SQL intercept still in `sql_execute`
**Severity:** 🟢 · **Effort:** S · **Original:** `gaps-may20-2.md §11` (partial)

**Current state:** Session 26 added the dedicated `POST /api/v1/demo/seed` endpoint.
But `try_handle_call_insert_rows_demo` is still called at `sql.rs:884` — meaning
any `CALL insert_rows(...)` SQL string in production traffic gets silently intercepted
and replaced with demo data generation, bypassing the real SQL executor.

**Fix:** Remove the `try_handle_call_insert_rows_demo` call from `sql_execute`, or
gate it behind a `VNG_DEMO_MODE=true` environment variable check.

**Files:** `services/voltnuerongridd/src/handlers/sql.rs:884`

---

### L-2 · Java driver is a skeleton
**Severity:** 🟢 · **Effort:** M · **Original:** `gaps-may20-2.md §10`

**Current state:** Java driver folder has class structure and packaging but no real
HTTP/TCP transport implementation. Verified that `VoltNueronGridDriver.java` and
`VngDriver.java` are class skeletons.

**Files:** `drivers/voltnuerongrid-driver-java/`

---

### L-3 · Perl driver is a feasibility document only
**Severity:** 🟢 · **Effort:** L · **Original:** `gaps-may20-2.md §10`

**Current state:** `drivers/voltnuerongrid-driver-perl/FEASIBILITY.md` — zero
implementation. No `.pm` file, no HTTP wrapper, no native wire.

**Files:** `drivers/voltnuerongrid-driver-perl/`

---

## Priority sequencing for next sessions

| Order | Gap | Effort | Impact |
|---|---|---|---|
| 1 | **M-1** Tenant DB grant enforcement (plug in 3-line auth check) | S | Closes the security hole opened by session 26 |
| 2 | **L-1** Remove/gate `CALL insert_rows` SQL intercept | S | Production safety |
| 3 | **M-3** `.expect("lock")` → 503 in handler paths | S | Service stability under load |
| 4 | **M-7** Wire OTEL exporter | S | Observability |
| 5 | **M-2** `ALTER TABLE` column migration | M | Schema evolution |
| 6 | **M-5** SQL injection in legacy OLTP executor | M | Security |
| 7 | **M-4** Crash-recovery integration test | M | Correctness confidence |
| 8 | **C-3** Repeatable-read snapshot enforcement | XL | ACID correctness |
| 9 | **C-2** Per-DB RocksDB column families | XL | True physical isolation |
| 10 | **C-1** PagedRowStore backed by RocksDB reads | XXL | Production durability |
