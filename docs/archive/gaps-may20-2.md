# Gap Analysis — Remaining Gaps After Sessions 16–22

> ⚠️ **SUPERSEDED — 2026-06-23**
> This document covers sessions 16–22 only. It has been superseded by the living gap register:
> **`docs/gaps-4.md`** — Post-session 32, all remaining open gaps with current status.
> Do not update this file. It is retained as historical evidence only.

---

**Prepared:** 2026-05-20
**Based on:** `gaps-may26-1.md` (original 35+ gaps) cross-referenced against code through session 22
**Branch:** `claude/friendly-hertz-3b69fb`
**Test baseline:** voltnuerongridd 762 ✓ / voltnuerongrid-sql 399 ✓

---

## What Has Been Closed (sessions 16–22)

| Original gap | Section | Status |
|---|---|---|
| SQL parser false positives in string literals | §3.3 | ✅ DONE — all `up.contains()` replaced with `keyword_outside_strings` / `find_keyword_outside_strings` in `ast.rs` and `sqlparser_adapter.rs` |
| No user accounts / no real authentication | §3.7 | ✅ DONE — bcrypt cost-12 hashing, HMAC-SHA256 session tokens, `UserStore`, `SessionStore`, `SessionSigner`, login endpoint, admin create/delete user, WAL-persisted |
| Row key DB prefix scoping (multi-DB isolation) | §3.2 partial | ✅ DONE — `make_row_key`, `make_table_scan_prefix`, `db_prefix_key` helpers; all DML paths scope by `"{db}."` prefix; DataFusion strips prefix before table resolution |
| `information_schema` / `pg_catalog` virtual catalog | §3.5 partial | ✅ DONE — `is_virtual_catalog_query` + `synthesize_virtual_catalog_response` intercept in `sql.rs` (handler level); surfaces tables, columns, schemata, routines, triggers |
| SELECT WHERE correctness | §3.4 partial | ✅ IMPROVED — `execute_select` in `voltnuerongrid-exec-datafusion` correctly evaluates `=`, `<>`, `>`, `<`, `BETWEEN`, `IN`, `IS NULL`, `IS NOT NULL`, `AND`, `OR`, `NOT`; legacy substring path retained only as fallback for JOIN/GROUP BY queries |
| 33,743-line monolithic `main.rs` | §4.1 | ✅ DONE — main.rs is now **1,925 lines**; all handlers are in `src/handlers/` modules (admin, audit, autonomous, catalog, cdc, driver, ingest, misc, raft, rows, security, sql, sre, store, user_mgmt, wal) |
| No durable storage (text WAL only) | §3.1 partial | ✅ SIGNIFICANT PROGRESS — RocksDB is the **default** durability engine; `build_durability_engine` opens RocksDB from `storage.data_dir`; `VNG_WAL_FSYNC_ON_COMMIT` controls fsync; falls back to in-memory only if `storage.engine = vng` is explicitly set |
| No Prometheus `/metrics` endpoint | §7 | ✅ DONE — `metrics-exporter-prometheus` plugged in; `/metrics` route registered; common counters pre-registered; `tracing` + env-filter initialized at boot |
| No `CREATE DATABASE` / `DROP DATABASE` UI | §6.2 partial | ✅ DONE — `DatabasesPanel.tsx` (list / create / drop) + `DatabasesPanel` wired into Sidebar; `createDatabase` / `dropDatabase` calls to backend |

---

## Remaining Gaps

Gaps are ordered by severity: 🔴 Critical → 🟠 High → 🟡 Medium → 🟢 Low.

---

### 1. 🔴 XXL — Row store is still in-memory; RocksDB is only the WAL layer

**Original:** §3.1 | **Where:** `crates/voltnuerongrid-store/src/mvcc.rs`, `main.rs:AppState.row_store`

RocksDB was adopted as the durability engine for the **WAL** (DDL events, DML statements as text lines). However, the row store itself is still `PagedRowStore` — an in-memory `HashMap<String, VersionChain>`. On restart, rows are not loaded from RocksDB pages; instead the DDL WAL re-creates the schema and the DML WAL attempts to replay SQL statements to rebuild in-memory state.

Concretely:
- A crash between a COMMIT and the next checkpoint loses all rows written since the last WAL checkpoint replay.
- There is no page cache, no buffer pool, no lazy load from disk — all data lives in RAM.
- The RocksDB column-family-per-database isolation strategy (mentioned in §9 of the original analysis) has not been implemented.

**Remaining work:** Bind `PagedRowStore` reads and writes directly to RocksDB key-value pairs (one CF per database). Then remove the DML SQL-replay path from boot.

**Effort:** XL (transformative — touches all DML paths and boot sequence).

---

### 2. 🔴 XL — Physical database isolation is key-prefix only; no separate storage partitions

**Original:** §3.2 | **Where:** `services/voltnuerongridd/src/handlers/admin.rs`, `helpers/sql_parse.rs`

`CREATE DATABASE` now writes a WAL entry and stores the catalog entry. Row keys are prefixed with `"{db}."` so queries are correctly scoped by database in memory. However:

- All databases still share a single `PagedRowStore` and a single RocksDB instance. There are no column families per database — a `scan_at_snapshot()` returns rows from all databases and then filters by prefix.
- There is no per-database max_connections enforcement (the `DatabaseCatalog.max_connections` field exists but no semaphore is wired to it).
- There is no per-database user/role scope — RBAC is global. A tenant user with access to database A can query database B tables if they know the key format.
- `DROP DATABASE` does not purge rows from the row store; it only removes the catalog entry and WAL record.

**Remaining work:** Per-database connection semaphore; `DROP DATABASE` row purge; per-database RBAC grant enforcement.

**Effort:** M for semaphore + DROP purge; XL for full per-DB RocksDB CF isolation.

---

### 3. 🔴 XL — ACID is asserted but not enforced end-to-end

**Original:** §3.6 | **Where:** `crates/voltnuerongrid-store/src/mvcc.rs`, `services/voltnuerongridd/src/handlers/sql.rs`

- **Atomicity:** a multi-statement batch that fails mid-way has already written partial rows to the row store. There is no UNDO log; `ROLLBACK` cannot unwind rows that were already committed to the version chain during the batch.
- **Isolation levels:** `READ COMMITTED` / `REPEATABLE READ` / `SERIALIZABLE` are parsed but all execute with the same `scan_at_snapshot(current_xid())` behavior — no differentiation.
- **Durability:** RocksDB WAL fsync is configurable, but it only covers SQL-text WAL entries. The row store (`PagedRowStore`) is still in-memory; even with fsync on WAL writes, a crash loses uncommitted in-memory row mutations.
- **Group commit:** no batching of fsync; every WAL append is an independent flush call.

**Effort:** XL — coupled with §1 above (row store durability).

---

### 4. 🔴 L — SELECT legacy fallback still uses row-key substring matching

**Original:** §3.4 | **Where:** `services/voltnuerongridd/src/helpers/execution.rs:589`

`execute_oltp_select_legacy` (the fallback for JOIN / GROUP BY / subquery / window queries that the DataFusion path handles) still uses:

```rust
d.get(col.as_str()).map(|v| v.eq_ignore_ascii_case(val))
    .unwrap_or_else(|| k.contains(val.as_str()))  // ← substring key scan fallback
```

The `k.contains(val)` branch means a `WHERE id = 5` that reaches the legacy path can still match rows 15, 25, 50, 51. Until DataFusion handles 100% of query shapes, this bug is reachable.

Additionally, the legacy path does not handle:
- `ORDER BY` (rows returned in iteration order)
- Projection (`SELECT col1, col2` returns the full row hashmap)
- Multi-predicate `AND`/`OR` for non-DataFusion queries

**Remaining work:** Eliminate the `k.contains` fallback; always return `false` for unresolvable predicates.

**Effort:** S (short-term fix); M (full projection + ORDER BY in legacy path).

---

### 5. ✅ CLOSED — Raft background election timer + peer network I/O

**Original:** §3.9 | **Closed:** 2026-05-20 (discovered already implemented in session review)

`services/voltnuerongridd/src/helpers/raft_loop.rs` implements:
- Background `run_raft_tick_loop` tokio task spawned at startup (150 ms tick).
- Calls `RaftNode::tick()` each tick; when Follower → Candidate, runs `run_election()` which sends `RequestVote` RPCs to all peers via `reqwest::Client`.
- While Leader, sends per-peer `AppendEntries` + `InstallSnapshot` RPCs every 3 ticks (450 ms heartbeat).
- Applies committed log entries to `PagedRowStore` via `apply_committed_entries()`.
- Log compaction every 3 ticks when log exceeds `COMPACT_LOG_THRESHOLD` entries.
- `Authorization: Bearer <cluster_token>` on all outgoing RPCs when `VNG_CLUSTER_TOKEN` is set.

**Remaining:** `failover` crate is still a 3-line stub; `htap_sync` still uses `InMemoryReplicationTransport`. The raft loop is real; multi-node cluster deployment still needs storage-layer durability (§1) to be safe.

---

### 6. 🟠 L — OLAP path is still in-memory ColumnBatch; no on-disk columnar files

**Original:** §3.10, §11 | **Where:** `crates/voltnuerongrid-store/src/columnar.rs`

`ColumnVector::Int64(Vec<i64>)` etc. — no Arrow IPC layout, no null bitmaps, no compression, no zone maps, no Parquet on-disk files.

HTAP sync (reading from OLTP row store and feeding OLAP columnar engine) is `InMemoryReplicationTransport`. Materialized views are recorded in the catalog but nothing materializes them.

DataFusion is used as the executor for complex queries (GROUP BY / JOIN), but it is fed in-memory row data rather than reading Parquet files from disk. All OLAP data lives in RAM and is rebuilt per-query from a full table scan.

**Missing:**
- On-disk Parquet files written on a cadence from the OLTP row store.
- Materialized view refresh.
- Hash join / sort-merge join (DataFusion has these, but the data feeding path doesn't exploit them).
- Tiered storage (hot rows in memory, cold columns on disk/object store).
- Bulk-load fast path.
- Query parallelism (DataFusion is async but current invocation is single-threaded via `block_in_place`).

**Effort:** XL collectively. Priority wedge: Parquet write-out from row store + DataFusion reading Parquet files.

---

### 7. 🟠 L — Optimizer is still heuristic; no cost-based planning

**Original:** §3.11 | **Where:** `crates/voltnuerongrid-opt/src/lib.rs`, `crates/voltnuerongrid-exec/src/planner.rs`

OLTP/OLAP routing is based on AST flag weights (`has_aggregate`, `has_join`, `has_window_fn`). No:
- Table cardinality statistics.
- Histogram-based selectivity estimation.
- JOIN order optimization.
- Index-based predicate pushdown (IndexManager exists but is not consulted by SELECT).
- Plan caching.

**Effort:** L (or near-free if DataFusion's built-in optimizer is given accurate statistics).

---

### 8. 🟠 M — Connection pool in Rust driver is still bookkeeping-only

**Original:** §3.8 | **Where:** `drivers/voltnuerongrid-driver-rust/src/lib.rs:1851-1970`

`ConnectionPoolManager` tracks `Vec<PooledConnection>` with string IDs; it does not own real sockets or HTTP connections. The actual wire code opens a fresh `TcpStream::connect_timeout` per call. The pool is not in the call path.

**Effort:** M. Use `bb8` or `deadpool` over `tokio::net::TcpStream` for the native protocol pool; use `reqwest::Client` (built-in keep-alive) for HTTP.

---

### 9. 🟠 M — No per-database connection semaphore (max_connections not enforced)

**Original:** §3.2 item | **Where:** `services/voltnuerongridd/src/main.rs:AppState`, `crates/voltnuerongrid-store/src/ddl_catalog.rs:DatabaseCatalog`

`DatabaseCatalog` has a `max_connections` field that is set via `CREATE DATABASE ... MAX_CONNECTIONS N` or the admin API. A `Semaphore` per database should be held for the duration of each SQL request against that database. This is not wired.

**Effort:** S–M. Add `Arc<Semaphore>` per active database in `AppState.database_semaphores: HashMap<String, Arc<Semaphore>>`. Acquire before SQL execute; release after.

---

### 10. 🟡 L — Language drivers (Java, Python, Node, TypeScript, Deno, Perl) are stubs

**Original:** §3.12 | **Where:** `drivers/`

Only the Rust + C drivers are substantive. All other language driver directories need verification and likely contain just `package.json` / `pom.xml` skeletons without real wire protocol implementations.

**Effort:** M per language for a basic HTTP-over-REST driver; L per language for a full native-wire driver with connection pooling and prepared statements.

---

### 11. 🟡 S — `CALL insert_rows(...)` demo intercept still lives in the SQL execute path

**Original:** §4.3 | **Where:** `services/voltnuerongridd/src/handlers/sql.rs:793-798`, `main.rs:try_handle_call_insert_rows_demo`

The `try_handle_call_insert_rows_demo` function is called before the SQL parser in `sql_execute`. This means any user-defined stored procedure named `insert_rows` is permanently shadowed.

The function has been extracted to its own helper (no longer inline in the body) but it still intercepts in the SQL execute path rather than being a dedicated `/api/v1/demo/seed` route.

**Effort:** S. Add a `/api/v1/demo/seed` handler and remove the early-intercept call from `sql_execute`.

---

### 12. 🟡 S — `.expect("lock")` calls in handler paths (cursorrules violation)

**Original:** §4.2 | **Where:** `services/voltnuerongridd/src/handlers/sql.rs` (~17 occurrences), `handlers/user_mgmt.rs`

The `.cursorrules` file prohibits `.unwrap()` / `panic!()` in handler paths. Mutex lock `.expect("... lock")` calls will take down the entire service if any critical section panics and poisons the mutex.

**Effort:** S. Replace with `.unwrap_or_else(|_| { /* 503 */ })` pattern or a helper macro.

---

### 13. 🟡 M — SQL feature coverage: many ANSI SQL statements are parsed but not executed

**Original:** §5, §3.4 | **Where:** `services/voltnuerongridd/src/handlers/sql.rs`, `crates/voltnuerongrid-exec-datafusion/src/lib.rs`

Features that are parsed but produce no meaningful result when executed:
- `ALTER TABLE ADD / DROP / ALTER COLUMN` — catalog records the change; existing rows are not migrated.
- Real JOIN execution against the row store (DataFusion handles it if the query reaches DataFusion path).
- `GROUP BY` + `ORDER BY` for queries that fall through to the legacy path.
- `WITH` (CTEs) — flag set on AST, not executed.
- `MERGE` / `UPSERT` — no handler beyond the `CALL insert_rows` demo.
- Prepared statements with parameter binding — wire protocol carries `$1`/`?` placeholders; executor inlines them.
- Cursors / server-side pagination beyond LIMIT/OFFSET.
- Transactions with savepoints — `SAVEPOINT` is parsed; no executor state change.
- Index usage — `IndexManager` exists; SELECT does not consult it.
- `CREATE SCHEMA` / `DROP SCHEMA` — catalog records schemas; no DDL execution.
- `CREATE ROLE` / `GRANT` / `REVOKE` — RBAC is config-file-driven; no runtime SQL pathway.

**Effort:** L–XL collectively. Priority: ALTER TABLE column migration, real GRANT/REVOKE SQL.

---

### 14. 🟡 S — Codd's rules — multiple rules not yet satisfied

**Original:** §5 | Mapping from the original analysis:

| Codd rule | Current state | Remaining gap |
|---|---|---|
| 2. Guaranteed access (table+pk+col) | 🟡 Improved | DataFusion path projects columns correctly; legacy path does not. |
| 3. Systematic null handling | 🔴 Not fixed | `ColumnVector::Null(usize)` still has no per-row null mask. |
| 4. Online catalog as relations | 🟡 Partial | `information_schema` virtual tables exist but are synthesized, not SQL-queryable relations backed by the row store. |
| 6. View update | 🔴 Not fixed | Views recorded; no update propagation or materialization. |
| 7. High-level UPDATE/DELETE (set-at-a-time) | 🔴 Not fixed | UPDATE/DELETE still extract a single key from WHERE and affect only one row. |
| 9. Logical data independence | 🟡 Partial | Views not used in query routing. |
| 11. Distribution independence | 🔴 Not fixed | Raft is a scaffold; no real distribution. |

---

### 15. 🟡 M — Studio UI: missing per-query routing badges, real Users & Roles panel, settings panel

**Original:** §6.2 | **Where:** `ui/voltnuerongrid-studio/src/components/`

`DatabasesPanel.tsx` was added (create/drop databases). `UsersPanel.tsx` exists. However:
- `UsersPanel.tsx` needs to be wired to the new `/api/v1/admin/users` endpoints (session 22).
- No per-query routing badge showing `oltp` / `olap` / `datafusion` in the results pane (the backend returns `route_path` in responses; it is not surfaced in the UI).
- No server settings panel reading from the `information_schema.settings` virtual table.
- Connection dialog does not validate that the database name exists; does not prompt to create it on first connect.

**Effort:** M for UI wiring; S for the connection-dialog database validation prompt.

---

### 16. 🟡 S — Design token drift between `studio-design.html` and `globals.css`

**Original:** §6.1 | **Where:** `ui/voltnuerongrid-studio/src/styles/globals.css`, `ui/voltnuerongrid-studio/design/studio-design.html`

Not verified as fixed in sessions 16–22. The original analysis found:
- `--radius-sm` vs `--r-sm` name mismatch.
- Hex value drift on `--bg-4`, `--bg-hover`, `--border`, `--border-strong`, `--text-3`.

**Effort:** S. Rename `--r-{sm,md,lg}` → `--radius-{sm,md,lg}` and sync hex values.

---

### 17. 🟡 M — Security: admin API key is a single static secret; SQL injection risk

**Original:** §8 | **Where:** `services/voltnuerongridd/src/auth.rs:require_admin_api_key`

- Admin auth is still a single static env-var key. One leak compromises everything. No rotation, no per-user admin credentials (though session tokens were added in session 22 for DBA users, the admin API key path still exists and bypasses session auth).
- Prepared statement parameter binding is not implemented at the executor level; all parameters are inlined as strings, creating SQL injection risk if user input reaches the SQL batch string.
- mTLS enforcement path (`mtls_required` config field) has not been verified as enforced.

**Effort:** S–M per item.

---

### 18. 🟡 M — Testing: no crash-recovery integration test; KPI gates are self-graded

**Original:** §10 | **Where:** `services/voltnuerongridd/src/tests.rs`, `tests/soak/`

- All integration tests start `AppState` in-process; none exercise the HTTP socket or native wire protocol end-to-end.
- No test that inserts data, hard-kills the process, restarts, and asserts data survives.
- KPI gate JSON is produced and consumed by the same process — self-graded, not independently verified.
- Playwright tests for the Studio UI: coverage unclear; likely limited.

**Effort:** M to build a real end-to-end harness (spawned process + HTTP client); L to instrument crash-recovery once row store durability is fixed.

---

### 19. 🟡 S — OTEL distributed tracing spans are not emitted

**Original:** §7 | **Where:** `services/voltnuerongridd/src/observability.rs`

Prometheus metrics and `tracing` subscribers are now set up. However:
- No `tracing` spans (`#[instrument]`) on individual handler functions.
- No OTEL exporter configured (no Jaeger/OTLP endpoint).
- No trace context propagation from incoming `traceparent` headers.

**Effort:** S. Add `#[instrument]` to key handlers + `opentelemetry` / `tracing-opentelemetry` crates.

---

### 20. 🟢 S — Scratch `.md` files and `.DS_Store` committed to repository

**Original:** §4.4 | **Where:** repo root

`status-tracker.md`, `status-tracker-v3.md`, `status-tracker-sprintwise-v1.md`, `status_tracker.md`, `status-todo.md`, `temp.md`, `wip.md`, `understanding.md`, `.DS_Store` are committed. These were noted in the original analysis and have not been cleaned up.

**Effort:** S. Consolidate to one `STATUS.md`; move archives to `docs/archive/`; add `.DS_Store` to `.gitignore`.

---

### 21. 🟢 S — Unused import sweep (~31 warnings in main.rs)

**Original:** `remaining.md` Tier 2 | **Where:** `services/voltnuerongridd/src/main.rs`

~31 standalone `use` warnings remain. Glob imports (`use handlers::cdc::*` etc.) must NOT be removed as they feed `tests.rs`.

**Effort:** S. Remove only the standalone non-glob `use` items flagged by `cargo check`.

---

### 22. 🟢 S — Missing unit tests for `replace_all` and `append_command_pending`

**Original:** `remaining.md` Tier 2

- `replace_all` in `crates/voltnuerongrid-store/src/mvcc.rs` has no unit test.
- `append_command_pending` in `services/voltnuerongridd/src/raft.rs` has no unit test.

**Effort:** S each.

---

### 23. 🟢 S — Session token rotation / revoke-all endpoint missing

**Original:** `remaining.md` Tier 3 | **Where:** `services/voltnuerongridd/src/handlers/user_mgmt.rs`

No endpoint to:
- Invalidate all sessions for a user (e.g., after password change).
- Rotate the HMAC signer secret (requires invalidating all existing tokens).

**Effort:** S. `DELETE /api/v1/admin/users/:id/sessions` (already implemented for `admin_delete_user` — same `sessions.remove_by_user` call can be exposed on its own).

---

### 24. 🟢 S — DB grant management: runtime endpoints to grant/revoke database access

**Original:** `remaining.md` Tier 3

Currently, DBA has implicit access to all databases; other roles need an explicit entry in the RBAC matrix config file. No runtime endpoint exists to grant or revoke database access without restarting the service.

**Remaining work:** `POST /api/v1/admin/databases/:name/grants` and `DELETE /api/v1/admin/databases/:name/grants/:role`.

**Effort:** S–M.

---

## Priority Sequencing (updated for current state)

### Immediate (1–2 sessions)
1. **DB max_connections semaphore** — S–M; unblocks production safety
2. **Unused-import sweep** — S; cleans compiler warnings
3. **`.expect()` → proper error returns** — S; cursorrules compliance
4. **Session revoke-all endpoint** — S; completes user auth story
5. **DB grant management endpoints** — S–M; completes RBAC story
6. **CALL insert_rows moved to `/api/v1/demo/seed`** — S; removes production path shadowing
7. **Design token sync** — S; UI fidelity

### Short-term (2–4 sessions)
8. **Integration test: linearisable writes** — M
9. **Integration test: crash-recovery** — M (once row store durability is wired)
10. **Studio UI: wire UsersPanel to auth endpoints + per-query routing badge** — M
11. **Legacy SELECT: remove `k.contains` fallback** — S

### Medium-term (1–2 months)
12. **Row store → RocksDB backing** — XL; the single biggest correctness gap remaining
13. **Per-database RocksDB column families** — XL; true physical isolation
14. **DataFusion + Parquet for OLAP path** — XL; closes OLAP gap

### Long-term (quarter+)
15. **Real Raft / HA** — XL; adopt `openraft`
16. **ANSI SQL completeness** — XL; ALTER TABLE, GRANT/REVOKE SQL, prepared stmt binding, cursors
17. **Language drivers** — L per language
18. **Cost-based optimizer** — L

---

*Total remaining gaps: 24 across 8 categories. 9 gaps from the original 35 have been fully or substantially closed in sessions 16–22.*
