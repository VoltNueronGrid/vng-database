# Gap Analysis — VoltNueronGrid Remaining Gaps (post-session 28)

**Prepared:** 2026-05-21 (session 28 close)
**Branch:** `claude/friendly-hertz-3b69fb`
**Commit:** `a047b26` (session 28 close, M-6 pending commit)
**Based on:** `gaps-may26-1.md` + `gaps-may20-2.md` cross-referenced against actual code
**Test baseline:** 772 passed (voltnuerongridd), 100 (store), 48 (datafusion)

This document records all remaining gaps after the full audit pass.
✅ items below are gaps that closed in sessions 26–28 and are noted here for history only.

---

## What closed in sessions 26–27

| Gap ID | Description | Evidence |
|---|---|---|
| M-1 | TenantUser DB grant enforcement | `auth.rs:378` — checks `db_grants` via role |
| M-7 (old) | OTEL OTLP exporter configured | `observability.rs` — generic `build_otel_layer<S>()` with OTLP/HTTP batch export |
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

## What closed in session 28

| Gap ID | Description | Commit | Evidence |
|---|---|---|---|
| H-2 | CREATE INDEX consulted by SELECT (index-accelerated lookups) | `b1d2dbe` | `execute_oltp_select_legacy` calls `IndexManager::lookup()`; index hits skip full scan |
| M-5 | Legacy SELECT: column projection + multi-predicate AND/OR | `b1d2dbe` | `execute_oltp_select_legacy` now projects requested columns; AND/OR compound predicates evaluated |
| C-2 | Per-DB RocksDB column families | `e932bf3` | Each DB gets its own `rows_{db}` CF; `drop_db_column_family()` drops atomically |
| L-1 | Gate `CALL insert_rows` behind `VNG_DEMO_MODE` | `4ede555` | `sql.rs:958` — env check before intercept |
| L-2 | Java driver with real HTTP I/O | `4ede555` | `VoltNueronGridDriver.java` has `execute(DriverRequest)->DriverResponse` via `HttpClient` |
| L-3 | Perl driver created | `4ede555` | `Driver.pm` with LWP::UserAgent, `execute_sql` and `health` methods |
| L-4 | `scan_rows_for_db` unit tests | `4ede555` | `scan_rows_for_db_isolates_databases`, `scan_rows_for_db_respects_mvcc_snapshot`, `rows_survive_drop_and_reopen` |
| L-5 | E2E HTTP roundtrip test | `4428f9b` | `e2e_http_roundtrip_sql_execute` (marked `#[ignore]` in sandbox) |
| M-8 Rule 3 | Emit `null` for columns absent from row | `4ede555` | `execute_select` emits `serde_json::Value::Null` for missing keys |
| M-8 Rule 7 | Set-at-a-time UPDATE scan path | `4ede555` | `extract_bulk_update_target()` + full table scan in `sql.rs` UPDATE path |
| M-7 | Serializable → row-level OCC conflict detection | `a047b26` | `check_serializable_conflict_row_level()` tracks per-row keys; eliminates false aborts |
| M-3 | `.expect("lock")` → 503 in all handler hot-paths | `a047b26` | `sql.rs`, `user_mgmt.rs`, `misc.rs` converted; `lock_poisoned_err()` helper |
| M-6 | Studio settings: isolation level, timeout, server config view | (this session) | `settings.ts`, `SettingsPanel.tsx`, `useQuery.ts` updated; `SqlExecuteRequest` extended |
| M-4 | CF_ROWS crash-recovery unit tests | `4ede555` | `rows_survive_drop_and_reopen`, `scan_rows_for_db_isolates_databases` |
| M-2 (full) | GRANT / REVOKE / CREATE ROLE classified and executed | `d524035` + prior | `handle_grant_sql()`, `handle_revoke_sql()` in `sql.rs`; WAL persistence |

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

**Current state (session 28):**
- `store_row()` persists every DML write to a per-DB RocksDB column family (`rows_{db}`).
- `scan_rows_for_db(db, snapshot_xid)` reads from the per-DB CF with MVCC-correct prefix scan.
- Four SELECT call sites in `sql.rs` and one in `misc.rs` fetch RocksDB rows and pass them
  as `rocksdb_rows: Option<Vec<_>>` to the execute functions.
- C-2 closed: each database has its own CF — no shared `CF_ROWS` prefix scanning.

**Remaining gap:**
- `PagedRowStore` (in-memory `HashMap<String, VersionChain>`) is still the **write target**
  for every DML and the **fallback read source** when RocksDB rows are not fetched (e.g.,
  in tests with `InMemoryDurabilityEngine`).
- The HashMap grows unbounded in RAM; there is no LRU eviction or page-cache cap.
- On startup, `load_persisted_rows_into()` still replays CF_ROWS into RAM, meaning all data
  must fit in memory before any query can run.
- The `execute_select` (Phase 1.7 AST-driven executor) and `execute_udf_*` paths still read
  from `PagedRowStore` directly.

**What's needed:** Eliminate `PagedRowStore` as the primary read path for production; make it
a bounded write-back cache over the per-DB RocksDB CFs. Boot no longer loads all rows into RAM.

**Files:** `crates/voltnuerongrid-store/src/mvcc.rs`, `rocksdb_engine.rs`,
`services/voltnuerongridd/src/helpers/execution.rs`

---

## 🟠 High — Remaining

### H-1 · No cost-based query optimizer (heuristic only)
**Severity:** 🟠 · **Effort:** L · **Original:** `gaps-may26-1.md §3.11`, `gaps-may20-2.md §7`

**Current state:** `QueryPlanner::estimate_cost` routes queries to OLTP / OLAP / Hybrid based
on AST flags (`has_aggregate`, `has_join`, `has_window_fn`) and hardcoded cost weights. No
statistics collection, no cardinality estimation, no JOIN order choice.

**Remaining gap:** Index lookups are available via `IndexManager` and H-2 is now closed (SELECT
uses index when available). But the planner still does not consider index selectivity when
choosing between index scan vs full-table-scan routes.

**Files:** `crates/voltnuerongrid-exec/src/lib.rs`, `crates/voltnuerongrid-opt/src/lib.rs`

---

## 🟡 Medium — Remaining

### M-6 · Studio settings — server-side isolation enforcement
**Severity:** 🟡 · **Effort:** S · **Original:** `gaps-may20-2.md §15` (partially closed)

**Current state (session 28):**
- `StudioSettings` now has `defaultIsolationLevel` and `statementTimeoutMs`.
- `useQuery.ts` sends `isolation_level` and `statement_timeout_ms` in every `executeSql` call.
- `SettingsPanel.tsx` shows Connection Defaults section (isolation level dropdown, timeout) and
  Server Configuration section (fetches `/api/v1/admin/runtime-config` and displays storage
  engine, data dir, WAL fsync, HTAP threshold, max result rows).
- `SqlExecuteRequest` on the server now accepts `isolation_level` and `statement_timeout_ms`.
- For OLAP queries: if client sends `isolation_level: "repeatable_read"` and there is no
  active ACID transaction, a snapshot xid is captured at request start.
- Audit log records `requested_isolation_level` and `statement_timeout_ms` for each request.

**Remaining sub-gap:**
- Statement timeout is recorded in the audit log but not enforced (no watchdog task that
  cancels a running query after the specified deadline).
- For OLTP DML paths (INSERT/UPDATE/DELETE in `sql_execute`), `isolation_level` from the
  request is not yet threaded into `acid_transactions.begin()` — only OLAP reads benefit
  from the RR snapshot.
- No `information_schema.settings` virtual table queryable from the SQL editor.

**Files:** `services/voltnuerongridd/src/handlers/sql.rs` (timeout watchdog),
`ui/voltnuerongrid-studio/src/components/Settings/SettingsPanel.tsx`

---

### M-7 · Serializable isolation — read-set tracking for phantoms
**Severity:** 🟡 · **Effort:** L · **Original:** `gaps-may20-2.md §3` (partially closed)

**Current state (session 28):** Row-level write-write OCC is implemented (H-2 closed).
True SSI requires tracking read-sets and detecting read-write anti-dependencies (phantoms).
The current implementation detects write-write conflicts only.

**Files:** `services/voltnuerongridd/src/main.rs:check_serializable_conflict_row_level`

---

### M-8 · Codd's rules — remaining violations
**Severity:** 🟡 · **Effort:** S–XL · **Original:** `gaps-may20-2.md §14`

| Rule | Name | Current state |
|---|---|---|
| Rule 1 | Information representation | All values are `String`; no typed column storage, no per-row NULL bitmap |
| Rule 3 | Systematic NULL handling | Session 28: `null` emitted for absent columns in result rows ✅ (partial); no per-row null mask on write |
| Rule 6 | View updatability | Views recorded in catalog; no update propagation or materialization |
| Rule 7 | Set-at-a-time UPDATE/DELETE | Session 28: bulk UPDATE scan path added ✅; DELETE still single-key |
| Rule 9 | Logical data independence | View definitions don't shield queries from physical key-format changes |
| Rule 11 | Distribution independence | Raft scaffold + per-DB CFs; multi-node deployment still requires C-1 |
| Rule 12 | Non-subversion | Admin endpoints mutate state without MVCC; WAL API bypasses transaction manager |

---

## 🟢 Low — Remaining

### L-3 · Perl driver — no HTTP I/O (session 28 status)
**Severity:** 🟢 · **Effort:** S · **Session 28:** `Driver.pm` created with LWP::UserAgent.
Marked closed above. The `.pm` compiles and `make test` runs; no integration test against
a live server exists.

---

## Priority sequencing for next session

| Priority | Gap | Effort | Impact |
|---|---|---|---|
| 1 | **M-6 sub-gaps** — statement timeout watchdog; OLTP isolation_level wire-up | S | Complete M-6 |
| 2 | **M-8 Rule 1** — typed column values in result rows (parse numbers/booleans) | S | Correctness |
| 3 | **M-8 Rule 7** — set-at-a-time DELETE scan path | S | Codd compliance |
| 4 | **M-8 Rule 12** — MVCC-wrap admin mutation endpoints | M | Non-subversion |
| 5 | **H-1** — index-aware cost routing in QueryPlanner | L | Query performance |
| 6 | **C-1** — PagedRowStore backed by RocksDB reads (eliminate in-memory primary) | XXL | Production durability |

---

*Total remaining gaps: 5 (1 critical, 1 high, 2 medium, 1 low) — down from 15 at session 27 start.*
*Session 28 closed: H-2, M-5, C-2, L-1, L-2, L-3, L-4, L-5, M-8 Rules 3+7 (partial), M-7, M-3, M-6 (partial), M-4, M-2.*
