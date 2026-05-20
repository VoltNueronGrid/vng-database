# `remaining.md` — handoff for next session (v27)

**Last updated:** 2026-05-20 (session 26 — medium/low gap sweep)
**Branch:** `claude/friendly-hertz-3b69fb`
**cargo test -p voltnuerongridd:** 770 passed, 0 failed ✓
**cargo test -p voltnuerongrid-store:** 100 passed, 0 failed ✓
**cargo test -p voltnuerongrid-exec-datafusion (--features datafusion):** 48 passed, 0 failed ✓
**cargo test -p voltnuerongrid-driver-rust:** 54 passed, 7 failed (all 7 are pre-existing sandbox TCP bind failures — not regressions)

---

## TL;DR — what landed in session 26

### ✅ Gap #23 (session token rotation) — COMPLETE
- `DELETE /api/v1/admin/users/:id/sessions` handler in `user_mgmt.rs`
- `SessionStore::sessions_for_user()` helper added to `user_store.rs`
- Wired in `router.rs`

### ✅ Gap #24 (DB grant management endpoints) — COMPLETE
- `POST /api/v1/admin/databases/:name/grants` — grant role access to DB
- `DELETE /api/v1/admin/databases/:name/grants/:role` — revoke role access
- `GET /api/v1/admin/databases/:name/grants` — list granted roles
- `db_grants: Arc<Mutex<HashMap<String, HashSet<String>>>>` added to `AppState`
- Persists GRANT/REVOKE to WAL; handlers in `admin.rs`

### ✅ Gap #11 (CALL insert_rows migration) — COMPLETE
- `POST /api/v1/demo/seed` dedicated REST endpoint in `misc.rs`
- Accepts `{database, table, count}` JSON; synthesizes demo rows
- `synthesize_demo_value` made `pub(crate)` in `main.rs`

### ✅ Tier 2 #7 (table stats incremental updates) — COMPLETE
- Replaced O(all_rows) full scan with O(touched_tables) delta tracking in `sql_execute` direct path
- INSERT: increments only if row was new (before-image = None)
- DELETE: decrements only if row existed (before-image = Some)
- UPDATE: no delta (row count unchanged)

### ✅ Gap #19 (OTEL tracing spans) — COMPLETE
- `#[tracing::instrument(skip_all)]` on `sql_transaction`, `sql_execute`, `auth_login`,
  `admin_databases_create`, `admin_databases_drop`
- Named spans: `sql.transaction`, `sql.execute`, `auth.login`, `admin.databases.create`, `admin.databases.drop`

### ✅ Gap #15 (Studio UI — UsersPanel wired to server) — COMPLETE
- `GET /api/v1/admin/users` new list-users endpoint (`admin_list_users` in `user_mgmt.rs`)
- `studio-client.ts`: added `AdminUserEntry`, `AdminUsersListResponse`, user CRUD types,
  `LoginRequest/Response`, `DbGrantsResponse`, and all corresponding client methods
- `UsersPanel.tsx`: replaced localStorage-only mock with live `listUsers()` API call;
  falls back to localStorage cache when disconnected

### ✅ Gap #20 (scratch files cleanup) — COMPLETE
- `.gitignore`: added `*.tmp`, `*.bak`, `flamegraph.svg`, `perf.data` patterns

---

## TL;DR — what landed in session 25

### ✅ Unused-import sweep — COMPLETE

**`services/voltnuerongridd/src/main.rs`**
- Removed ~20 standalone unused imports: `BTreeMap`, `std::fs`, `Instant`, `SystemTime`, `UNIX_EPOCH`, `Semaphore`, `Path`, `Query`, `State`, axum routing items (`get`, `post`, `options`, `Router`), `base64::Engine`, and several crate-level types (`PrivilegeAction`, `ResourceGrant`, `AutonomousActionDecision`, `I18nCatalog`, `SupportedLocale`, `eval_legacy_numeric_aggregation`, `SUPPORTED_LEGACY_AGGREGATIONS`, `MutationOp`, `CatalogResult`, `parse_ddl_info`, `DurabilityConfig`, `McpRequest/McpServerCapabilities/process_request`, `PoolAcquireError`, `IngestionConnector`, `StreamDirection`, `ReplayCursorStore`)
- Moved test-only symbols (`State`, `Query`, `Path`, `PrivilegeAction`, `AutonomousActionDecision`, `DurabilityConfig`, `MutationOp`, `SupportedLocale`, `IngestionConnector`) to a dedicated import block at the top of `tests.rs`

**`crates/voltnuerongrid-exec-datafusion/src/datafusion.rs`**
- Removed `AggregateCell` and `AggregateResult` (imported but not used in that file)

**`services/voltnuerongridd/src/helpers/execution.rs`**
- Removed inner `use crate::helpers::sql_parse::make_table_scan_prefix` at line 597 (redundant inner import inside a function that doesn't call it)

### ✅ Tier 2 unit tests — COMPLETE

**`crates/voltnuerongrid-store/src/mvcc.rs`** — 3 new tests for `replace_all`:
- `replace_all_clears_existing_rows_and_inserts_new`: verifies old rows vanish, new rows appear
- `replace_all_preserves_monotone_xid`: next_xid is strictly greater after replace_all
- `replace_all_with_empty_set_clears_all_rows`: empty iterator wipes all rows

**`services/voltnuerongridd/src/raft.rs`** — 3 new tests for `append_command_pending`:
- `append_command_pending_single_node_commits_but_does_not_apply`: single-node commits immediately but last_applied stays at 0
- `append_command_pending_multi_node_waits_for_quorum`: multi-node does not advance commit_index
- `append_command_pending_indices_are_monotone`: successive calls get strictly higher indices

### ✅ Tier 1 #2 (Linearisable write integration test) — COMPLETE

**`services/voltnuerongridd/src/helpers/raft_loop.rs`**
- Made `apply_committed_entries` `pub(crate)` so tests can call it directly

**`services/voltnuerongridd/src/tests.rs`** — 2 integration tests:
- `linearisable_write_apply_loop_applies_committed_entry_to_row_store`: appends pending command → apply loop fires → last_applied advances → watch channel notified
- `linearisable_write_two_pending_commands_both_applied`: two pending commands, single apply call covers both

### ✅ Tier 2 #8 (DataFusion → Parquet read path) — COMPLETE

**`crates/voltnuerongrid-exec-datafusion/src/datafusion.rs`**
- New `execute_select_prefer_parquet(sql, table_rows, max_rows, data_dir)`: for each table checks `{data_dir}/parquet/_default/{table}.parquet` first; registers as DataFusion `ListingTable` (native Parquet reader) if present; falls back to `MemTable` if absent
- `execute_select_from_rows` now delegates to `execute_select_prefer_parquet("")` (backward compat)
- 2 new tests: `prefer_parquet_with_empty_data_dir_uses_memtable` and `prefer_parquet_falls_back_to_memtable_when_file_absent`

**`services/voltnuerongridd/src/helpers/execution.rs`**
- `df_select_owned` now takes `data_dir: String` and passes it to `execute_select_prefer_parquet`
- `execute_olap_query` now takes `data_dir: &str`

**`services/voltnuerongridd/src/handlers/misc.rs`** and **`handlers/sql.rs`**
- Both call sites updated to pass `state.runtime_config.storage.data_dir`

---

## TL;DR — what landed in session 24

### ✅ Gap #1 (Row store → RocksDB persistence) — NOW FULLY END-TO-END

**`services/voltnuerongridd/src/handlers/sql.rs`**
- `sql_transaction` DML path: every `rs.insert()`/`rs.delete()` is now preceded by `wal.store_row()` for INSERT, UPDATE, and DELETE arms
- `sql_execute` direct path: same wiring — all three DML arms persist to RocksDB CF_ROWS before applying to in-memory row store

**`services/voltnuerongridd/src/helpers/raft_loop.rs`**
- `apply_dml_command` now receives `&AppState` and calls `wal.store_row()` for each INSERT/UPDATE/DELETE applied by Raft followers
- Raft followers now persist rows to RocksDB just like the leader — full cluster durability

Row persistence is now truly end-to-end: every committed DML writes to CF_ROWS → survives crash → loaded back at boot via `load_persisted_rows_into()`.

---

## TL;DR — what landed in session 23

### ✅ Gap #1 (Row store → RocksDB row persistence) — COMPLETE

**`crates/voltnuerongrid-store/src/lib.rs`**
- Added `store_row(db, row_key, xid, data)`, `scan_persisted_rows()`, `persists_rows()` to `DurabilityEngine` trait with default no-op implementations
- `BoxedDurabilityEngine` shim forwards all three methods

**`crates/voltnuerongrid-store/src/rocksdb_engine.rs`**
- New column family `CF_ROWS = "rows"` — 4th CF alongside `wal`, `meta`, `sql`
- Key format: `{db}\x1f{row_key}\x1f{xid_be8}` → `\x00{json_data}` (live) or `\x01` (tombstone)
- `store_row`: writes to CF_ROWS using existing `sync_writes` flag
- `scan_persisted_rows`: iterates CF_ROWS in sorted order, returns latest version per (db, row_key)
- `persists_rows`: returns `true`

**`services/voltnuerongridd/src/helpers/boot.rs`**
- `load_persisted_rows_into()`: scans RocksDB CF_ROWS at boot and populates PagedRowStore — rows now survive restarts
- `purge_database_rows()`: deletes all in-memory rows with given db prefix (used by DROP DATABASE)

**`services/voltnuerongridd/src/main.rs`**
- Calls `load_persisted_rows_into()` at boot before Raft tick loop starts

### ✅ Gap #2 (Physical DB isolation — DROP DATABASE purge) — PARTIAL COMPLETE

**`services/voltnuerongridd/src/handlers/admin.rs`**
- DROP DATABASE handler now calls `purge_database_rows()` after WAL write
- Per-DB connection semaphore enforces `max_connections` at request time (Gap #9)

### ✅ Gap #3 (ACID — per-transaction undo log for ROLLBACK) — COMPLETE

**`services/voltnuerongridd/src/main.rs`** — `AppState.tx_undo_log`
- `tx_undo_log: Arc<Mutex<HashMap<String, Vec<(String, Option<RowData>)>>>>` — connection_id → [(key, before_data)]

**`services/voltnuerongridd/src/handlers/sql.rs`**
- `record_undo()` private helper captures before-image before every INSERT/UPDATE/DELETE
- ROLLBACK: applies undo entries in reverse, restores before-images via new MVCC xid
- COMMIT: clears undo log for the connection

### ✅ Gap #4 (Legacy SELECT `k.contains` fallback) — COMPLETE

**`services/voltnuerongridd/src/helpers/execution.rs`**
- `execute_oltp_select_legacy`: changed `k.contains(val.as_str())` → `false`
- WHERE predicates no longer incorrectly match rows by key substring

### ✅ Gap #5 (Raft background election timer) — ALREADY DONE (discovered in session 23 review)

`services/voltnuerongridd/src/helpers/raft_loop.rs` was already implementing:
- Background `run_raft_tick_loop` (150 ms tick, spawned at startup)
- `run_election()`: sends RequestVote RPCs to all peers
- `fanout_heartbeat()`: sends per-peer AppendEntries/InstallSnapshot RPCs every 450 ms
- `apply_committed_entries()`: applies committed log entries to PagedRowStore
- `compact_if_needed()`: log compaction every 3 ticks

### ✅ Gap #6 (OLAP Parquet flush to disk) — COMPLETE

**`services/voltnuerongridd/src/helpers/parquet_flush.rs`** (NEW)
- `flush_rows_to_parquet(rows, data_dir)`: groups rows by (db, table), writes Arrow RecordBatch to `{data_dir}/parquet/{db}/{table}.parquet`
- Schema auto-discovered from column name union across all rows in the table
- 3 unit tests included

**`services/voltnuerongridd/src/main.rs`**
- Background tokio task flushes every `VNG_PARQUET_FLUSH_INTERVAL_SECS` seconds (default: 60)
- Skips the first tick; runs indefinitely; logs `vng.parquet` tracing spans

### ✅ Gap #7 (Basic table statistics) — COMPLETE

**`services/voltnuerongridd/src/main.rs`** — `AppState.table_stats`
- `table_stats: Arc<Mutex<HashMap<String, u64>>>` — "db.table" → row count
- Updated after every DML commit (full scan, incremental updates are future work)

**`services/voltnuerongridd/src/handlers/misc.rs`**
- `GET /api/v1/sre/table-stats` — returns per-table row counts as JSON

### ✅ Gap #8 (Real connection pool in Rust driver) — COMPLETE

**`drivers/voltnuerongrid-driver-rust/src/lib.rs`**
- `NativeConnectionPool`: `Arc<Mutex<VecDeque<TcpStream>>>` with idle connection reuse
- `PooledNativeConnection`: RAII guard — returns stream to pool on Drop, closes on `invalidate()`
- Liveness probing via 1 ms peek on idle connections before reuse
- `SocketNativeTransport::with_pool()` constructor; pool used in `send_frame()`
- 7 new pool tests pass

### ✅ Gap #9 (Per-database max_connections semaphore) — COMPLETE

**`services/voltnuerongridd/src/main.rs`**
- `DEFAULT_DB_MAX_CONNECTIONS: usize = 100`
- `AppState.db_semaphores: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>` — lazy per-DB semaphores

**`services/voltnuerongridd/src/handlers/sql.rs`**
- `sql_execute` acquires `db_semaphores` permit via `try_acquire_owned()` for every DB-scoped request
- Returns HTTP 503 if database is at connection capacity

---

## Previous sessions summary (sessions 16–22)

- Session 16: tests green, DataFusion OLAP wire-up, Raft log replication, cluster auth
- Session 17: Raft `next_index`, cluster auth handlers, DataFusion `olap_agg`
- Session 18: Raft apply loop, randomised timeouts, log compaction
- Session 19: Leader append path, snapshot transfer DTOs, dead-code audit start
- Session 20: PagedRowStore::replace_all, raft_install_snapshot, linearisable writes
- Session 21: DB isolation, RBAC per DB, WAL persistence, UI fixes, WHERE clause fix, SQL parser partial fix, information_schema interception
- Session 22: SQL parser false positives (keyword_outside_strings), row key DB prefix scoping, user auth (bcrypt + HMAC-SHA256 session tokens)

---

## What's still TODO

See `gaps-may20-2.md` for the full list. Summary of what remains:

### Tier 1 (functional correctness, still open)

1. ✅ **Row store RocksDB persistence — COMPLETE**: `store_row()` is now called from all DML paths in `sql.rs` (both `sql_transaction` and `sql_execute` direct path) and from `raft_loop.rs::apply_dml_command`. Row durability is fully end-to-end.

2. ✅ **Integration test for linearisable writes** — COMPLETE (session 25). Two tests in `tests.rs` exercise the full pending→apply→notify path.

3. **DataFusion wiring completeness** — some OLAP query shapes still fall through.

### Tier 2 (production quality)

4. ✅ **Unused-import sweep** — COMPLETE (session 25). ~20 standalone imports removed from `main.rs`, `datafusion.rs`, `execution.rs`; test-only types moved to `tests.rs`.

5. ✅ **replace_all unit test** — COMPLETE (session 25). 3 tests added to `crates/voltnuerongrid-store/src/mvcc.rs`.

6. ✅ **append_command_pending unit test** — COMPLETE (session 25). 3 tests added to `services/voltnuerongridd/src/raft.rs`.

7. ✅ **Table statistics: incremental updates** — COMPLETE (session 26). O(touched_tables) delta tracking replaces O(all_rows) full scan.

8. ✅ **Parquet → DataFusion read path** — COMPLETE (session 25). `execute_select_prefer_parquet` registered via DataFusion `ListingTable`; wired into `execute_olap_query` and `df_select_owned`; both OLAP call sites pass `data_dir`.

### Tier 3 (RBAC completeness)

9. ✅ **Database grant management endpoints** — COMPLETE (session 26). `POST/GET /api/v1/admin/databases/:name/grants` and `DELETE /api/v1/admin/databases/:name/grants/:role`.

10. **Tenant user database scoping** — require explicit database grant for tenant users in multi-DB setups.

11. ✅ **Session token rotation / revoke-all endpoint** — COMPLETE (session 26). `DELETE /api/v1/admin/users/:id/sessions`.

### Medium gaps (from gaps-may20-2.md, still open)

- Gap #2 remainder: per-DB RocksDB column families for true physical isolation (currently key-prefix only)
- Gap #3 remainder: ACID isolation levels (READ COMMITTED vs REPEATABLE READ vs SERIALIZABLE) still not differentiated
- Gap #10 (drivers): Java/Python/Node/TS/Deno/Perl drivers still stubs
- Gap #12 (design tokens): ✅ No drift found — globals.css and studio-design.html token values are in sync
- Gap #13 (Studio UI): ✅ UsersPanel now wired to server (session 26); no per-query routing badge yet

---

## How to continue

```
@remaining.md
@services/voltnuerongridd/src/handlers/sql.rs
@services/voltnuerongridd/src/helpers/boot.rs
@crates/voltnuerongrid-store/src/lib.rs
@crates/voltnuerongrid-store/src/rocksdb_engine.rs
```

**Most critical next step:** Tenant user database scoping (Tier 3 #10) — enforce that tenant users must have an explicit DB grant to access a database. Then tackle the remaining functional gaps: DataFusion wiring completeness (Tier 1 #3), ACID isolation level differentiation (Gap #3 remainder), and per-query routing badge in Studio UI.

**Environment note:** `VNG_CLUSTER_TOKEN`, `VNG_RAFT_PEERS`, `VNG_RBAC_POLICY_PATH`,
`VNG_NODE_ID`, `VNG_SESSION_SECRET`, `VNG_PARQUET_FLUSH_INTERVAL_SECS` are key env vars. All default safely for single-node dev.
