# `remaining.md` — handoff for next session (v22)

**Last updated:** 2026-05-19 (session 21 — database isolation, RBAC, WAL persistence, UI fixes)
**Branch:** `claude/friendly-hertz-3b69fb`
**Latest commit:** `7872464`
**cargo test -p voltnuerongridd:** 749 passed, 0 failed ✓

---

## TL;DR — what landed in session 21

### ✅ Studio Issue-2 fixed: Native wire protocol toggle removed from browser UI

**`ui/voltnuerongrid-studio/src/components/ConnectionPanel/ConnectionPanel.tsx`**
- Removed the "Native wire" toggle that showed "cannot be tested from browser"
- Replaced with a single disabled informational button: "Native wire available via SDK/CLI (port 7542)"
- Programmatic SDK/CLI clients should default to the native wire protocol (port 7542)

### ✅ Studio Issues 1 & 3 fixed: Database existence check + Create dialog

**`ui/voltnuerongrid-studio/src/components/ConnectionPanel/ConnectionPanel.tsx`**
- `save()` now calls `client.listDatabases()` before saving a connection with a database name
- If the database does not exist: shows a modal dialog instead of connecting
- Modal offers two options: "Empty Database" or "Default Database (with sample schema)"
- "Default" option seeds from `samples/database/` SQLs via `client.createDatabase()` with `seed: "default"`
- `commitSave()` persists connection + navigates only after the database is confirmed to exist

**`ui/voltnuerongrid-studio/src/api/studio-client.ts`**
- Added `database?: string` field to `StudioConnection`
- `getSchemaTree(database?)` accepts optional database param and appends `?database=` query param
- All request headers now include `x-vng-database` when `conn.database` is set

**`ui/voltnuerongrid-studio/src/hooks/useSchema.ts`**
- Passes `database` from active connection to both the client constructor and `getSchemaTree()`

**`ui/voltnuerongrid-studio/src/hooks/useQuery.ts`**
- Passes `database` from active connection to the client constructor

### ✅ Studio Issue-4 fixed: Schema tree database isolation

**`services/voltnuerongridd/src/handlers/catalog.rs`**
- Added `SchemaTreeQuery { database: Option<String> }` extractor
- `admin_schema_tree` now accepts `x-vng-database` header (priority) and `?database=` query param
- Only returns DDL entries matching the requested database name (case-insensitive)
- Without a database filter returns all entries (operator-level introspection preserved)

### ✅ DatabaseCatalog WAL persistence + crash recovery

**`services/voltnuerongridd/src/handlers/admin.rs`**
- `admin_databases_create`: on success, appends `CREATE DATABASE <name>` to DDL WAL
- `admin_databases_drop`: on success, appends `DROP DATABASE <name>` to DDL WAL

**`services/voltnuerongridd/src/helpers/boot.rs`**
- New `replay_database_catalog_into(db_catalog, wal_engine)` — reads DDL WAL at startup,
  parses `CREATE DATABASE` / `DROP DATABASE` lines, replays into the catalog.
  This restores the database list across service restarts without extra storage.

**`services/voltnuerongridd/src/main.rs`**
- Calls `replay_database_catalog_into` during `AppState` initialization
- Clones `wal_engine` before the struct move to avoid borrow-after-move compile error

### ✅ RBAC per database

**`services/voltnuerongridd/src/auth.rs`**
- New `principal_has_database_access(principal, database, state) -> bool`
  - `Operator(Dba)` → always allowed
  - Other operators → check `database/<name>` or `database/*` resource with `Execute` action in RBAC matrix
  - `TenantUser` → always allowed (scoped at table prefix level, not database level)

**`services/voltnuerongridd/src/handlers/sql.rs`**
- Extracts `x-vng-database` header after authenticating principal
- Calls `principal_has_database_access` and returns HTTP 403 if access denied

### ✅ WHERE clause fix in OLTP legacy executor

**`services/voltnuerongridd/src/helpers/execution.rs`**
- `execute_oltp_select_legacy`: was using value as substring match on row key string
- Fixed to parse LHS column name and perform exact `RowData` column-value match
- Falls back to key substring match only when column is absent (backward compat)

### ✅ SQL parser keyword false-positive fix

**`crates/voltnuerongrid-sql/src/ast.rs`**
- New `keyword_outside_strings(up, keyword) -> bool` helper
- Scans the SQL string tracking single-quote regions; returns true only if the keyword
  appears outside string literals
- `has_group_by`, `has_order_by`, `has_having` detections now use this helper instead
  of plain `str::contains`, eliminating false positives like `WHERE note = 'GROUP BY x'`

### ✅ Test fix

**`services/voltnuerongridd/src/tests.rs`**
- Updated `admin_schema_tree` call to pass the new `Query(SchemaTreeQuery::default())` arg

---

## Previous sessions summary (sessions 16–20)

- Session 16: tests green, DataFusion OLAP wire-up, Raft log replication, cluster auth
- Session 17: Raft `next_index`, cluster auth handlers, DataFusion `olap_agg`
- Session 18: Raft apply loop, randomised timeouts, log compaction
- Session 19: Leader append path, snapshot transfer DTOs, dead-code audit start
- Session 20: PagedRowStore::replace_all, raft_install_snapshot, linearisable writes

---

## What's still TODO

### Tier 1 (functional correctness)

1. **Row key database prefix scoping** — `active_database` is extracted in `sql_execute`
   but row keys are still stored as `"<table>:<rowid>"` without a DB prefix.
   Full isolation requires keys be prefixed `"<db>.<table>:<rowid>"` so two databases
   with identically-named tables don't share rows in `PagedRowStore`.
   Files: `services/voltnuerongridd/src/handlers/sql.rs` (INSERT/SELECT paths),
   `services/voltnuerongridd/src/helpers/execution.rs`.

2. **Integration test for linearisable writes** — no test covers the multi-node
   quorum wait (`append_command_pending` + watch channel). Add a test that mocks two
   peers and confirms a DML write blocks until `raft_last_applied_tx.send()` fires.

3. **DataFusion wiring** — `voltnuerongrid-exec-datafusion` has a working executor,
   but some OLAP query shapes still fall through to hand-rolled paths. Audit and fill.

### Tier 2 (production quality)

4. **Database-level max_connections** — `DatabaseCatalog` has a `max_connections` field
   but it is not enforced. Add a connection semaphore per database in `AppState`.

5. **Unused-import sweep** — ~20 standalone `use` warnings in `main.rs`. Remove line
   by line; do NOT remove glob imports (`use handlers::cdc::*` etc.) that `tests.rs` depends on.

6. **replace_all unit test** — add to `crates/voltnuerongrid-store/src/mvcc.rs`.

7. **append_command_pending unit test** — add to `services/voltnuerongridd/src/raft.rs`.

### Tier 3 (RBAC completeness)

8. **Database grant management endpoints** — currently DBA has implicit access and
   other roles must be granted via the RBAC matrix config file. Add runtime endpoints
   `POST /api/v1/admin/databases/:name/grants` and `DELETE /api/v1/admin/databases/:name/grants/:role`
   so operators can grant/revoke database access without restarting.

9. **Tenant user database scoping** — currently tenant users are always allowed into any
   database (scoped only at table-prefix level). Tighten to require an explicit database
   grant for tenant users when operating on multi-database setups.

### Large deferred gaps (from gaps-may26-1.md, sessions 1–15 only)

- Gap #1: Durable storage (RocksDB backend) — partial (WAL exists, full RocksDB not yet)
- Gap #5: Connection pool management
- Gap #7: Cluster membership changes (add/remove nodes)
- Gap #8: Cross-node transactions
- Gap #10: Replication lag metrics
- Gap #11: Backup / restore endpoints
- Gap #13: Multi-region awareness

---

## How to continue

```
@remaining.md
@services/voltnuerongridd/src/handlers/sql.rs
@services/voltnuerongridd/src/helpers/execution.rs
@services/voltnuerongridd/src/auth.rs
```

Recommended next steps (in priority order):
1. **Row key DB prefix scoping** — most impactful for true data isolation
2. **Database max_connections enforcement** — complete the Tier 1 MUST requirements
3. **RBAC database grant endpoints** — runtime management without restart
4. **Integration test for linearisable writes** — correctness proof

**Environment note:** `VNG_CLUSTER_TOKEN`, `VNG_RAFT_PEERS`, `VNG_RBAC_POLICY_PATH`,
`VNG_NODE_ID` are the key env vars. All default safely for single-node dev.
