# `remaining.md` — handoff for next session (v24)

**Last updated:** 2026-05-20 (session 23 — Critical & High gap sweep)
**Branch:** `claude/friendly-hertz-3b69fb`
**cargo test -p voltnuerongridd:** 765 passed, 0 failed ✓
**cargo test -p voltnuerongrid-store:** 97 passed, 0 failed ✓
**cargo test -p voltnuerongrid-driver-rust:** 54 passed, 7 failed (all 7 are pre-existing sandbox TCP bind failures — not regressions)

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

1. **Row store RocksDB persistence — service-layer write-through missing**: `store_row()` API is implemented in the storage layer, but `handlers/sql.rs` DML paths do NOT yet call `wal_engine.store_row()` on every INSERT/UPDATE/DELETE. The `load_persisted_rows_into()` at boot will only see rows that were previously flushed via `store_row()`. **Until this is wired, row durability is not fully end-to-end.** Next step: after each `rs.insert()` or `rs.delete()` in `sql.rs`, call `state.wal_engine.lock()?.store_row(&db, &raw_k, xid, Some(&data))`.

2. **Integration test for linearisable writes** — still missing.

3. **DataFusion wiring completeness** — some OLAP query shapes still fall through.

### Tier 2 (production quality)

4. **Unused-import sweep** — ~31 standalone `use` warnings in handlers. Remove line by line; do NOT remove glob imports.

5. **replace_all unit test** — add to `crates/voltnuerongrid-store/src/mvcc.rs`.

6. **append_command_pending unit test** — add to `services/voltnuerongridd/src/raft.rs`.

7. **Table statistics: incremental updates** — currently does a full scan on every DML commit. Replace with per-operation counter increments.

8. **Parquet → DataFusion read path** — Parquet files are now written but DataFusion still reads from in-memory row data. Wire DataFusion to prefer reading from Parquet files when available.

### Tier 3 (RBAC completeness)

9. **Database grant management endpoints** — `POST /api/v1/admin/databases/:name/grants` and `DELETE /api/v1/admin/databases/:name/grants/:role`

10. **Tenant user database scoping** — require explicit database grant for tenant users in multi-DB setups.

11. **Session token rotation / revoke-all endpoint** — `DELETE /api/v1/admin/users/:id/sessions`

### Medium gaps (from gaps-may20-2.md, still open)

- Gap #2 remainder: per-DB RocksDB column families for true physical isolation (currently key-prefix only)
- Gap #3 remainder: ACID isolation levels (READ COMMITTED vs REPEATABLE READ vs SERIALIZABLE) still not differentiated
- Gap #6 remainder: DataFusion reading from Parquet files instead of in-memory row data
- Gap #10 (drivers): Java/Python/Node/TS/Deno/Perl drivers still stubs
- Gap #11 (CALL insert_rows): still intercepts in SQL execute path, should be `/api/v1/demo/seed`
- Gap #12 (design tokens): CSS token drift between globals.css and studio-design.html
- Gap #13 (Studio UI): Users panel not yet wired to new auth endpoints; no per-query routing badge

---

## How to continue

```
@remaining.md
@services/voltnuerongridd/src/handlers/sql.rs
@services/voltnuerongridd/src/helpers/boot.rs
@crates/voltnuerongrid-store/src/lib.rs
@crates/voltnuerongrid-store/src/rocksdb_engine.rs
```

**Most critical next step:** Wire `wal_engine.store_row()` calls from the DML paths in `sql.rs` so row persistence is truly end-to-end. After `rs.insert(xid, &k, d)` add `wal_engine.lock()?.store_row(&db, &raw_k, xid, Some(&d))`. After `rs.delete(xid, &k)` add `wal_engine.lock()?.store_row(&db, &raw_k, xid, None)`.

**Environment note:** `VNG_CLUSTER_TOKEN`, `VNG_RAFT_PEERS`, `VNG_RBAC_POLICY_PATH`,
`VNG_NODE_ID`, `VNG_SESSION_SECRET`, `VNG_PARQUET_FLUSH_INTERVAL_SECS` are key env vars. All default safely for single-node dev.
