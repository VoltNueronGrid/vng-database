# `remaining.md` — handoff for next session (v23)

**Last updated:** 2026-05-20 (session 22 — SQL parser, row-key DB isolation, user auth)
**Branch:** `claude/friendly-hertz-3b69fb`
**cargo test -p voltnuerongridd:** 762 passed, 0 failed ✓
**cargo test -p voltnuerongrid-sql:** 399 passed, 0 failed ✓

---

## TL;DR — what landed in session 22

### ✅ Gap #3 (SQL parser false-positive keywords in string literals) — COMPLETE

**`crates/voltnuerongrid-sql/src/ast.rs`**
- `keyword_outside_strings` and `find_keyword_outside_strings` promoted to `pub(crate)` so `sqlparser_adapter.rs` can import them
- All remaining `up.contains("NOT IN …")`, `up.contains("GROUPING SETS")`, `up.contains(" EXCEPT ")`, `up.contains("PARTITION BY")`, `up.contains("ORDER BY")`, `up.contains("NULLS FIRST/LAST")`, `up.contains("COLLATE")` calls in both the `"SELECT"` and `"WITH"` arms replaced with `keyword_outside_strings`
- All 10 `has_order_by_*` helpers (`has_order_by_positional`, `has_order_by_expression`, `has_order_by_function_expression`, `has_order_by_case_expression`, `has_order_by_desc_direction`, `has_order_by_asc_direction`, `has_order_by_random`, `has_order_by_random_seeded`, `has_order_by_rand_alias`, `has_order_by_multi_column`) updated to use `find_keyword_outside_strings` instead of `str::find("ORDER BY")`
- `has_group_by_rollup` and `has_group_by_cube` updated to use `keyword_outside_strings`

**`crates/voltnuerongrid-sql/src/sqlparser_adapter.rs`** (feature-gated)
- Imports `keyword_outside_strings` and `find_keyword_outside_strings` from `crate::ast`
- Fixed: CUBE/ROLLUP/GROUPING SETS, EXCEPT/INTERSECT, PARTITION BY, ORDER BY, NULLS FIRST/LAST, COLLATE, NOT IN/NOT EXISTS/IN subquery, window ORDER detection

### ✅ Gap #2 (Row key DB prefix scoping) — COMPLETE

**`services/voltnuerongridd/src/helpers/sql_parse.rs`**
- `make_row_key(db, table, row_id) -> String` — builds `"db.table:id"` when db non-empty, `"table:id"` when empty
- `make_table_scan_prefix(db, table) -> String` — for scan filtering
- `db_prefix_key(db, raw_key) -> String` — prepend db to an existing `"table:value"` key
- `extract_delete_key_from_sql` now returns `"table:where_value"` (table-prefixed, consistent with INSERT/UPDATE)

**`services/voltnuerongridd/src/helpers/execution.rs`**
- `execute_olap_query(query, max_rows, rs, db)` — new `db: &str` param; scans by db-prefixed prefix; strips prefix before DataFusion
- `execute_oltp_select(statements, rs, limit, db)` — new `db: &str` param; same scoping
- `execute_oltp_select_legacy(stmt, rs, limit, results, db)` — new `db: &str` param; filters and strips prefix

**`services/voltnuerongridd/src/handlers/sql.rs`**
- Imports `db_prefix_key`, `make_table_scan_prefix`
- Binds `let db: String = active_database.clone().unwrap_or_default()` at top of `sql_execute`
- `sql_transaction` also binds `db` from `x-vng-database` header
- All INSERT/UPDATE/DELETE DML paths apply `db_prefix_key(&db, &raw_k)` before writing to row store
- DataFusion OLAP agg path uses `make_table_scan_prefix(&db, name)` and strips prefix
- Result builder filters `all_rows` by `"{db}."` prefix and strips it for downstream matching

**`services/voltnuerongridd/src/handlers/misc.rs`**
- `olap_query` handler passes `""` as db to `execute_olap_query` (no database scope for this admin endpoint)

**`services/voltnuerongridd/src/main.rs`**
- `try_handle_call_insert_rows_demo` accepts new `db: &str` param; uses `make_row_key` and `make_table_scan_prefix`

**`services/voltnuerongridd/src/tests.rs`**
- Updated `s2_ws2_commit_flush_handles_delete_statement` to expect `"orders:o99"` (table-prefixed key, not just `"o99"`)

### ✅ Gap #7 (User accounts, password hashing, session tokens) — COMPLETE

**`services/voltnuerongridd/Cargo.toml`**
- Added: `bcrypt = "0.15"`, `hmac = "0.12"`, `sha2 = "0.10"`, `uuid = { version = "1", features = ["v4"] }`

**`services/voltnuerongridd/src/user_store.rs`** (NEW)
- `UserAccount` — username, role, tenant_id, user_id, created_ms, password_hash (bcrypt cost 12)
- `UserStore` — by_username + by_id dual index
- `SessionEntry` — user_id, username, role, tenant_id, expires_at_secs
- `SessionStore` — fingerprint → SessionEntry; TTL checked on read
- `SessionSigner` — HMAC-SHA256; token format: `base64url(user_id:expires_secs).base64url(hmac)`; `fingerprint()` uses SHA-256 for session store key
- `user_to_wal` / `user_from_wal` — tab-delimited WAL format: `CREATE USER <username>\t<role>\t<tenant>\t<user_id>\t<created_ms>\t<bcrypt_hash>`

**`services/voltnuerongridd/src/handlers/user_mgmt.rs`** (NEW)
- `POST /api/v1/admin/users` (`admin_create_user`) — DBA-only; bcrypt hash in spawn_blocking; WAL-persisted
- `DELETE /api/v1/admin/users/:id` (`admin_delete_user`) — DBA-only; invalidates sessions; WAL-persisted
- `POST /api/v1/auth/login` (`auth_login`) — bcrypt verify in spawn_blocking; issues HMAC-SHA256 session token

**`services/voltnuerongridd/src/handlers/mod.rs`**
- Added `pub(crate) mod user_mgmt;`

**`services/voltnuerongridd/src/helpers/boot.rs`**
- `replay_user_store_into(user_store, wal_engine)` — replays `CREATE USER` / `DROP USER` lines from DDL WAL at boot

**`services/voltnuerongridd/src/main.rs`**
- `pub(crate) mod user_store;`
- AppState: `user_store`, `session_store`, `session_signer` fields (all `Arc<Mutex<_>>`)
- AppState init: replays user store from WAL; signer secret from `VNG_SESSION_SECRET` or `VNG_CLUSTER_TOKEN`, 24-hour TTL
- `try_handle_call_insert_rows_demo` signature updated to accept `db: &str`

**`services/voltnuerongridd/src/auth.rs`**
- `session_identity_from_headers(headers, state) -> Option<RuntimeAccessPrincipal>` — extracts Bearer token from `Authorization` header, looks up in `SessionStore`, maps to `Operator` or `TenantUser`
- `require_runtime_principal` falls back to session token path before the tenant-user legacy path

**`services/voltnuerongridd/src/router.rs`**
- Three new routes:
  - `POST /api/v1/admin/users` → `admin_create_user`
  - `DELETE /api/v1/admin/users/:id` → `admin_delete_user`
  - `POST /api/v1/auth/login` → `auth_login`

**`services/voltnuerongridd/src/tests.rs`**
- `state_with_key` builder includes `user_store`, `session_store`, `session_signer`

---

## Previous sessions summary (sessions 16–21)

- Session 16: tests green, DataFusion OLAP wire-up, Raft log replication, cluster auth
- Session 17: Raft `next_index`, cluster auth handlers, DataFusion `olap_agg`
- Session 18: Raft apply loop, randomised timeouts, log compaction
- Session 19: Leader append path, snapshot transfer DTOs, dead-code audit start
- Session 20: PagedRowStore::replace_all, raft_install_snapshot, linearisable writes
- Session 21: DB isolation, RBAC per DB, WAL persistence, UI fixes, WHERE clause fix, SQL parser partial fix, information_schema interception

---

## What's still TODO

### Tier 1 (functional correctness)

1. **Integration test for linearisable writes** — no test covers the multi-node
   quorum wait (`append_command_pending` + watch channel). Add a test that mocks two
   peers and confirms a DML write blocks until `raft_last_applied_tx.send()` fires.

2. **DataFusion wiring** — `voltnuerongrid-exec-datafusion` has a working executor,
   but some OLAP query shapes still fall through to hand-rolled paths. Audit and fill.

### Tier 2 (production quality)

3. **Database-level max_connections** — `DatabaseCatalog` has a `max_connections` field
   but it is not enforced. Add a connection semaphore per database in `AppState`.

4. **Unused-import sweep** — ~31 standalone `use` warnings in `main.rs`. Remove line
   by line; do NOT remove glob imports (`use handlers::cdc::*` etc.) that `tests.rs` depends on.

5. **replace_all unit test** — add to `crates/voltnuerongrid-store/src/mvcc.rs`.

6. **append_command_pending unit test** — add to `services/voltnuerongridd/src/raft.rs`.

### Tier 3 (RBAC completeness)

7. **Database grant management endpoints** — currently DBA has implicit access and
   other roles must be granted via the RBAC matrix config file. Add runtime endpoints
   `POST /api/v1/admin/databases/:name/grants` and `DELETE /api/v1/admin/databases/:name/grants/:role`
   so operators can grant/revoke database access without restarting.

8. **Tenant user database scoping** — currently tenant users are always allowed into any
   database (scoped only at table-prefix level). Tighten to require an explicit database
   grant for tenant users when operating in multi-database setups.

9. **Session token rotation / revoke-all endpoint** — no endpoint to invalidate all sessions
   for a user or rotate the signer secret.

### Large deferred gaps (from gaps-may26-1.md, sessions 1–15 only)

- Gap #1: Durable storage (RocksDB backend) — partial (WAL exists, full RocksDB not yet)
- Gap #5: Connection pool management
- Gap #7 (original): Cluster membership changes (add/remove nodes)
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
@services/voltnuerongridd/src/user_store.rs
```

Recommended next steps (in priority order):
1. **Database max_connections enforcement** — complete the Tier 1 MUST requirements
2. **RBAC database grant endpoints** — runtime management without restart
3. **Integration test for linearisable writes** — correctness proof
4. **Unused-import sweep** — clean up ~31 warnings in main.rs

**Environment note:** `VNG_CLUSTER_TOKEN`, `VNG_RAFT_PEERS`, `VNG_RBAC_POLICY_PATH`,
`VNG_NODE_ID`, `VNG_SESSION_SECRET` are the key env vars. All default safely for single-node dev.
