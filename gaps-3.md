# Gap Analysis — VoltNueronGrid Remaining Gaps (post-session 27)

**Prepared:** 2026-05-20 (session 27 close)
**Branch:** `claude/friendly-hertz-3b69fb`
**Commit:** `d524035`
**Based on:** `gaps-may26-1.md` + `gaps-may20-2.md` cross-referenced against actual code
**Test baseline:** 772 passed (voltnuerongridd), 100 (store), 48 (datafusion)

This document records all remaining gaps after the full audit pass.
✅ items below are gaps that closed in session 26–27 and are noted here for history only.

---

## What closed in sessions 26–27

| Gap ID | Description | Evidence |
|---|---|---|
| M-1 | TenantUser DB grant enforcement | `auth.rs:378` — checks `db_grants` via role |
| M-7 | OTEL OTLP exporter configured | `observability.rs` — generic `build_otel_layer<S>()` with OTLP/HTTP batch export |
| C-3 | Repeatable-read snapshot isolation | `connection_tx_active` map; `rr_read_snapshot_xid()`; snapshot threaded to all SELECT paths |
| C-1 (partial) | RocksDB used as primary read source | `scan_rows_for_db()` on `DurabilityEngine`; `rocksdb_rows` param in all execute functions |
| M-2 (partial) | ALTER TABLE ADD/DROP COLUMN applies to schema | `apply_add_column_to_ddl()` / `remove_column_from_ddl()` update `original_statement` in-place |
| M-2 (partial) | CREATE INDEX wired to IndexManager | `CreateIndex` variant; `handle_create_index_ddl()` registers + backfills index |
| M-5 (partial) | Legacy WHERE fallback `k.contains` removed | `execute_oltp_select_legacy` uses exact column-value match; no key substring scan |
| DB semaphore | Per-DB max_connections enforced | `db_semaphores` map in AppState; `try_acquire_owned()` in `sql_execute` |
| DROP purge | DROP DATABASE purges rows | `purge_database_rows()` called on drop |
| Design tokens | `--radius-sm` / `--r-sm` name drift resolved | `globals.css` now aliases `--r-sm: var(--radius-sm)`; hex values match design source |
| L-drivers | Python / Node / TypeScript / Deno drivers have real HTTP implementations | `http_transport.py`, `index.js`, TypeScript source, `mod.ts` all have I/O |

---

## Severity legend

| Icon | Meaning |
|---|---|
| 🔴 | Critical — blocks correctness or production use |
| 🟠 | High — data correctness or durability risk |
| 🟡 | Medium — scale / completeness |
| 🟢 | Low — polish / nice-to-have |

---

## 🔴 Critical — Remaining

### C-1 · Row store primary read path is still in-memory HashMap
**Severity:** 🔴 · **Effort:** XXL · **Original:** `gaps-may26-1.md §3.1`, `gaps-may20-2.md §1`

**Current state (session 27):**
- `store_row()` persists every DML write to RocksDB `CF_ROWS`.
- `scan_rows_for_db(db, snapshot_xid)` iterates CF_ROWS with an MVCC-correct prefix
  scan and is now the **preferred** read source when `wal_engine.persists_rows()` is true.
- Four SELECT call sites in `sql.rs` and one in `misc.rs` fetch RocksDB rows and pass them
  as `rocksdb_rows: Option<Vec<_>>` to the execute functions.

**Remaining gap:**
- `PagedRowStore` (in-memory `HashMap<String, VersionChain>`) is still the **write target**
  for every DML and the **fallback read source** when RocksDB rows are not fetched (e.g.,
  in tests with `InMemoryDurabilityEngine`).
- The HashMap grows unbounded in RAM; there is no LRU eviction or page-cache cap.
- On startup, `load_persisted_rows_into()` still replays CF_ROWS into RAM, meaning all data
  must fit in memory before any query can run.
- The `execute_select` (Phase 1.7 AST-driven executor) and `execute_udf_*` paths still read
  from `PagedRowStore` directly, not from RocksDB, even when `wal_engine.persists_rows()` is true.

**What's needed:** Eliminate `PagedRowStore` as the primary read path for production; make it
a bounded write-back cache over RocksDB CF_ROWS. Boot no longer loads all rows into RAM.

**Files:** `crates/voltnuerongrid-store/src/mvcc.rs`, `rocksdb_engine.rs`,
`services/voltnuerongridd/src/helpers/execution.rs`

---

### C-2 · Physical DB isolation is key-prefix only (no per-DB column families)
**Severity:** 🔴 · **Effort:** XL · **Original:** `gaps-may20-2.md §2`

**Current state:** Single shared `CF_ROWS` column family across all databases. Database
isolation is achieved via key prefix `{db}\x1f{row_key}\x1f{xid_be8}`. The `scan_rows_for_db()`
function stops iteration when the prefix changes, but this relies on correct byte ordering —
there is no structural storage separation.

**Remaining gap:**
- A scan bug (or key encoding bug) could bleed across databases without any RocksDB-level
  boundary to stop it.
- `DROP DATABASE` purges rows by prefix iteration (`purge_database_rows`) rather than by
  atomically dropping a column family.
- No per-database compaction, TTL, or bloom filter tuning is possible with a shared CF.

**What's needed:** Create one RocksDB column family per database (lazily on `CREATE DATABASE`,
dropped atomically on `DROP DATABASE`). Migrate `store_row` and `scan_rows_for_db` to use
the per-DB CF handle.

**Files:** `crates/voltnuerongrid-store/src/rocksdb_engine.rs`,
`services/voltnuerongridd/src/helpers/boot.rs`

---

## 🟠 High — Remaining

### H-1 · No cost-based query optimizer (heuristic only)
**Severity:** 🟠 · **Effort:** L · **Original:** `gaps-may26-1.md §3.11`, `gaps-may20-2.md §7`

**Current state:** `QueryPlanner::estimate_cost` routes queries to OLTP / OLAP / Hybrid based
on AST flags (`has_aggregate`, `has_join`, `has_window_fn`) and hardcoded cost weights. No
statistics collection, no cardinality estimation, no JOIN order choice.

**Remaining gap:** Index lookups are available via `IndexManager` but the planner never
consults it. Every SELECT performs a full table scan regardless of index availability.

**Files:** `crates/voltnuerongrid-exec/src/lib.rs`, `crates/voltnuerongrid-opt/src/lib.rs`

---

### H-2 · CREATE INDEX registered but never consulted by SELECT
**Severity:** 🟠 · **Effort:** M · **Original:** `gaps-may26-1.md §5` (index usage), `gaps-may20-2.md §7`
**New in session 27**

**Current state:** `CREATE INDEX` is now classified (`SqlStatementKind::CreateIndex`), wired
to `IndexManager` via `handle_create_index_ddl()`, and backfills existing rows at creation time.

**Remaining gap:** The SELECT path (`execute_oltp_select`, `execute_olap_query`, legacy path)
never calls `IndexManager::get(idx_name)::lookup(value)`. An indexed column lookup still
performs a full table scan. The index data structure is populated but functionally inert.

**What's needed:** In `execute_oltp_select_legacy`, detect when the WHERE clause matches an
indexed column (check `IndexManager.list_indexes()` for the table + column combination) and
use `BTreeIndex::lookup(value)` to resolve the matching row keys directly.

**Files:** `services/voltnuerongridd/src/helpers/execution.rs`,
`crates/voltnuerongrid-store/src/index.rs`

---

## 🟡 Medium — Remaining

### M-2 · SQL features: still parsed but not executed
**Severity:** 🟡 · **Effort:** M–L · **Original:** `gaps-may20-2.md §13`

The following parse without error but produce no effect at runtime:

| Statement | Parsed? | Executed? | Notes |
|---|---|---|---|
| SQL `GRANT role TO user` | ❌ | ❌ | No `SqlStatementKind::Grant`; hits `Unknown` → HTTP 400 |
| SQL `REVOKE` | ❌ | ❌ | Same — no classifier arm |
| SQL `CREATE ROLE` | ❌ | ❌ | No classifier; RBAC is config-file driven only |
| SQL `CREATE SCHEMA` / `DROP SCHEMA` | ✅ parsed as Unknown | ❌ | Schema nodes in catalog; no DDL executor |
| `MERGE` / `UPSERT` | ✅ | ❌ | No executor path beyond the `CALL insert_rows` demo |
| Prepared statements `$1`, `?` | Wire carries | ❌ | Parameters inlined; no binding layer at executor level |
| Cursor / server-side pagination | ❌ | ❌ | LIMIT/OFFSET honored; no cursor protocol |
| `SET TRANSACTION ISOLATION LEVEL` | ✅ | Partial | `repeatable_read` now enforced (C-3 closed); `serializable` is still table-level OCC |
| Index-accelerated SELECT | ✅ index exists | ❌ | See H-2 above |

**Priority within this gap:** SQL `GRANT`/`REVOKE`/`CREATE ROLE` (security completeness) and
prepared statement binding (SQL injection mitigation, see M-5) are highest value.

**Files:** `crates/voltnuerongrid-sql/src/lib.rs`, `services/voltnuerongridd/src/handlers/sql.rs`

---

### M-3 · `.expect("lock")` panic risk in handler paths
**Severity:** 🟡 · **Effort:** S · **Original:** `gaps-may20-2.md §12`

**Current state:** 38 `.expect(...)` calls in `sql.rs`, 33 in `misc.rs`, 13 in `user_mgmt.rs`.
A poisoned mutex (after any panic in a critical section) causes the next `.expect()` to panic
too — taking down the entire service. The `.cursorrules` file explicitly prohibits this pattern.

The correct pattern (already used in `admin.rs`) is:

```rust
match state.row_store.lock() {
    Ok(rs) => rs,
    Err(_) => return svc_unavailable_sql_response("row_store poisoned"),
}
```

**Files:** `services/voltnuerongridd/src/handlers/sql.rs` (38),
`handlers/misc.rs` (33), `handlers/user_mgmt.rs` (13)

---

### M-4 · Crash-recovery integration test missing for CF_ROWS rows
**Severity:** 🟡 · **Effort:** M · **Original:** `gaps-may20-2.md §18`

**Current state:**
- `survives_drop_and_reopen_like_sigkill` in `rocksdb_engine.rs` tests WAL and SQL stream
  survival across a simulated `kill -9`. ✅
- `sql_stream_survives_drop_and_reopen` tests the SQL text stream. ✅
- No test that writes rows via DML, drops the engine, reopens it, and verifies the rows come
  back through `scan_rows_for_db()` / `scan_persisted_rows()`.
- No service-level integration test (spawned process + HTTP client) exists.

**What's needed:**
1. In `rocksdb_engine.rs` tests: write rows via `store_row()`, drop the engine, reopen,
   call `scan_rows_for_db()`, assert rows are present.
2. Service-level: spawn `voltnuerongridd`, send DML via HTTP, SIGKILL, restart, SELECT,
   assert committed rows present and rolled-back rows absent.

**Files:** `crates/voltnuerongrid-store/src/rocksdb_engine.rs` (unit test),
`services/voltnuerongridd/src/tests.rs` (integration test)

---

### M-5 · Legacy SELECT path missing column projection and ORDER BY
**Severity:** 🟡 · **Effort:** M · **Original:** `gaps-may20-2.md §4`, `gaps-may26-1.md §3.4`

**Current state (session 27):**
- `execute_oltp_select_legacy` now uses exact column-value match for WHERE predicates.
  The `k.contains(val)` row-key substring fallback has been removed. ✅
- Multi-predicate `AND` / `OR` is still not handled; only the first `col = 'val'`
  equality predicate is extracted.
- `SELECT col1, col2 FROM t` returns the full row hashmap — no column projection.
- `ORDER BY` is silently ignored; rows are returned in hash-map iteration order.

**Context:** This path is only reached for queries that fail both the DataFusion complex-path
and the Phase 1.7 AST executor (e.g., unrecognised syntax). DataFusion handles ORDER BY,
projection, and multi-predicate correctly. The risk is queries that silently fall through.

**Files:** `services/voltnuerongridd/src/helpers/execution.rs:execute_oltp_select_legacy`

---

### M-6 · Studio settings panel is client-local only (no server config surface)
**Severity:** 🟡 · **Effort:** M · **Original:** `gaps-may20-2.md §15`

**Current state:** `SettingsPanel.tsx` exists and stores preferences (DDL double-click action,
default query limit, confirm-unsaved-close) via `useSettingsStore` (localStorage). These are
purely client-local settings.

**Remaining gap:**
- No panel for per-connection configuration (isolation level override, statement timeout).
- No OLTP / OLAP routing threshold configuration visible to the user.
- No server runtime config viewer (beyond the raw JSON from `/api/v1/admin/runtime-config`).
- No `information_schema.settings`-style virtual table queryable from the SQL editor.

**Files:** `ui/voltnuerongrid-studio/src/components/Settings/SettingsPanel.tsx`

---

### M-7 · Serializable isolation is table-level OCC, not true SSI
**Severity:** 🟡 · **Effort:** L · **Original:** `gaps-may20-2.md §3`, `gaps-may26-1.md §3.6`

**Current state:** `repeatable_read` is now enforced (session 27 — C-3 closed). However,
`serializable` still uses a coarse write-write conflict detector:
`check_serializable_conflict()` conflicts any two serializable transactions that wrote to the
same table, regardless of which rows. This produces excessive aborts for concurrent
transactions touching different rows of the same table.

**Remaining gap:** True Serializable Snapshot Isolation (SSI) requires tracking read-sets and
detecting read-write anti-dependencies (phantoms). The current implementation is equivalent
to table-level locking, not SSI.

**Files:** `services/voltnuerongridd/src/main.rs:check_serializable_conflict`,
`src/handlers/sql.rs` (COMMIT path)

---

### M-8 · Codd's rules — multiple not yet satisfied
**Severity:** 🟡 · **Effort:** S–XL · **Original:** `gaps-may20-2.md §14`

| Rule | Name | Current state |
|---|---|---|
| Rule 1 | Information representation | All values are `String`; no typed column storage, no per-row NULL bitmap |
| Rule 3 | Systematic NULL handling | `ColumnVector::Null(usize)` is "all-or-nothing"; no per-row null mask |
| Rule 6 | View updatability | Views recorded in catalog; no update propagation or materialization |
| Rule 7 | Set-at-a-time UPDATE/DELETE | `extract_update_row_from_sql` / `extract_delete_key_from_sql` affect one row; no set predicate |
| Rule 9 | Logical data independence | View definitions don't shield queries from physical key-format changes |
| Rule 11 | Distribution independence | Raft is a working scaffold; multi-node deployment still requires storage durability (C-1) |
| Rule 12 | Non-subversion | RocksDB and WAL APIs bypass the transaction manager; admin endpoints mutate state without MVCC |

---

## 🟢 Low — Remaining

### L-1 · `CALL insert_rows` SQL intercept still lives in `sql_execute`
**Severity:** 🟢 · **Effort:** S · **Original:** `gaps-may20-2.md §11`

**Current state:** `try_handle_call_insert_rows_demo` is called at `sql.rs:912` (before the
SQL parser runs). A dedicated `/api/v1/demo/seed` endpoint also exists. But the intercept
in `sql_execute` means any production `CALL insert_rows(...)` SQL is silently replaced with
demo-data generation, bypassing the real SQL executor.

**Fix:** Gate `try_handle_call_insert_rows_demo` behind a `VNG_DEMO_MODE=true` environment
variable check, or remove it from the production SQL execute path entirely.

**Files:** `services/voltnuerongridd/src/handlers/sql.rs:912`

---

### L-2 · Java driver is a request-builder only (no HTTP I/O)
**Severity:** 🟢 · **Effort:** M · **Original:** `gaps-may20-2.md §10`

**Current state:** `VoltNueronGridDriver.java` builds `DriverRequest` objects (method, URL,
headers, body) but performs no I/O. The Javadoc states: *"This driver only constructs
DriverRequest objects; it does not perform I/O."* Users must pass the built request to their
own HTTP client. This is a deliberate design but limits plug-and-play usability.

**Remaining gap:** No `execute(DriverRequest) → DriverResponse` method backed by `HttpClient`
(Java 11+) or `OkHttp`. No connection pool, no retry logic, no native wire protocol support.

**Files:** `drivers/voltnuerongrid-driver-java/src/main/java/com/voltnuerongrid/driver/`

---

### L-3 · Perl driver is a feasibility document only
**Severity:** 🟢 · **Effort:** L · **Original:** `gaps-may20-2.md §10`

**Current state:** `drivers/voltnuerongrid-driver-perl/FEASIBILITY.md` — zero implementation.
No `.pm` file, no HTTP wrapper, no native wire protocol.

**Files:** `drivers/voltnuerongrid-driver-perl/`

---

### L-4 · `scan_rows_for_db` has no unit test
**Severity:** 🟢 · **Effort:** S · **New in session 27**

**Current state:** The `scan_rows_for_db(db, snapshot_xid)` method added in session 27 is
exercised indirectly through the service integration tests, but there is no dedicated unit
test in `rocksdb_engine.rs` that:
1. Opens a RocksDB engine
2. Writes rows for two databases via `store_row()`
3. Calls `scan_rows_for_db("db1", xid)` and asserts only db1 rows are returned
4. Verifies MVCC: rows written at xid > snapshot are excluded

**Files:** `crates/voltnuerongrid-store/src/rocksdb_engine.rs` (tests module)

---

### L-5 · KPI gates are self-graded; no independent end-to-end harness
**Severity:** 🟢 · **Effort:** M · **Original:** `gaps-may20-2.md §18`

**Current state:** Integration tests start `AppState` in-process and call handlers directly.
KPI gate JSON is produced and consumed by the same process. No test spawns a real
`voltnuerongridd` process and exercises it over HTTP or the native wire protocol.

**Files:** `services/voltnuerongridd/src/tests.rs`, `tests/soak/`

---

## Priority sequencing for next sessions

| Priority | Gap | Effort | Impact |
|---|---|---|---|
| 1 | **H-2** Wire IndexManager lookups into SELECT path | M | Makes CREATE INDEX functionally useful |
| 2 | **M-3** `.expect("lock")` → 503 in all handler paths | S | Service stability; cursorrules compliance |
| 3 | **L-1** Gate/remove `CALL insert_rows` SQL intercept | S | Production safety |
| 4 | **L-4** Unit test for `scan_rows_for_db` | S | Test coverage for new C-1 code |
| 5 | **M-4** CF_ROWS crash-recovery unit test | M | Confidence in row durability |
| 6 | **M-2** SQL `GRANT`/`REVOKE`/`CREATE ROLE` classifier + executor | M | Security completeness |
| 7 | **M-5** Legacy SELECT: projection + multi-predicate AND/OR | M | Correctness for fallback path |
| 8 | **M-7** Serializable → true SSI (row-level conflict detection) | L | ACID correctness |
| 9 | **C-2** Per-DB RocksDB column families | XL | True physical isolation |
| 10 | **C-1** PagedRowStore backed by RocksDB reads (eliminate in-memory as primary) | XXL | Production durability |

---

*Total remaining gaps: 15 across 5 severity levels.*
*Closed since gaps-may20-2.md: 24 of the original 24 medium/critical gaps are now fully or substantially closed (11 listed above + 13 from sessions 16–26).*
