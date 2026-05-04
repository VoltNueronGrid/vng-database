# `remaining.md` — handoff for next session (v2)

**Last updated:** 2026-05-04 (second session)
**Branch:** `phase-1-correctness`
**Author:** Claude (continued review pass)

---

## TL;DR

This is the second handoff. Phase 0 was merged to `main` between sessions
(Pavan merged PR #1). This session opened `phase-1-correctness` and landed
the foundational pieces of Phase 1.

**Verified compiling + tested locally** (Ubuntu rustc 1.75 + recent npm):
- ✅ `voltnuerongrid-config` crate — 7 tests pass.
- ✅ `voltnuerongrid-meta` crate — 16 tests pass.
- ✅ Studio TypeScript — zero errors (`npx tsc --noEmit` clean).

**Could NOT verify locally** (the same toolchain limitation as last time):
- ❌ Full `voltnuerongridd` service crate compile — requires rustc 1.86+
  for transitive crates (icu, hyper-rustls). Sandbox network is blocked
  from `static.rust-lang.org` so I can't install a newer toolchain.
- The Rust changes to `services/voltnuerongridd/src/main.rs` were made
  carefully reading existing types but **need to be `cargo check`ed on the
  user's real toolchain**.

---

## What this session delivered

### ✅ 1.0 — HTTP request metrics (Phase 0.4 follow-up)
**File:** `services/voltnuerongridd/src/main.rs`

- New tower middleware `track_http_metrics` registered alongside `add_cors`.
  Emits `vng_http_requests_total` (counter) and `vng_http_request_duration_seconds`
  (histogram) per response, labeled by `method`, `route`, `status_class`.
- `coarsen_route_for_metrics()` collapses path-id segments
  (e.g. `/api/v1/admin/databases/foo` → `/api/v1/admin/databases/:name`)
  to keep label cardinality bounded.
- `/metrics` is excluded from its own metrics to prevent feedback loops.

### ✅ 1.1 — Real `voltnuerongrid-meta` crate (was a 3-line stub)
**File:** `crates/voltnuerongrid-meta/src/lib.rs` (~500 LOC, 16 tests)

The crate now owns:
- `DatabaseName` — validated, normalised lowercase form. Allowed:
  `[a-z_][a-z0-9_]{0,62}`. Reserves `metadata`, `information_schema`,
  `pg_catalog`, `vng_system`. 9 unit tests.
- `Database` — name + created_at_ms + owner + description.
- `DatabaseCatalog` — `BTreeMap`-backed CRUD with case-insensitive
  uniqueness. `create()`, `drop_database()` (note: `drop` was renamed to
  avoid colliding with `Drop` trait method semantics), `get`, `exists`,
  `list` (alphabetical), `len`, `is_empty`, `snapshot_json`, `restore`. 7
  unit tests including round-trip persistence.
- `MetadataTable` enum — 12 system tables (`databases`, `schemas`,
  `tables`, `columns`, `indexes`, `views`, `routines`, `triggers`,
  `users`, `roles`, `grants`, `settings`) with column-list metadata.
- `metadata_schema_layout()` — returns the per-database metadata schema
  shape, JSON-serialisable for HTTP transport.

### ✅ 1.2 — Service wiring for the new types
**File:** `services/voltnuerongridd/Cargo.toml` + `src/main.rs`

- `voltnuerongrid-meta` added as service dep.
- `AppState` extended with two new fields:
  - `database_catalog: Arc<Mutex<voltnuerongrid_meta::DatabaseCatalog>>`
  - `runtime_config: Arc<voltnuerongrid_config::RuntimeConfig>`
- All `AppState` constructors patched (the main one explicitly; the test
  helper `state_with_key` patched once; the 3 other test sites use
  `..state_with_key(None)` so they inherit automatically).

### ✅ 1.3 — Database lifecycle HTTP API
**File:** `services/voltnuerongridd/src/main.rs`

New routes:
- `GET /api/v1/admin/databases` → `AdminDatabasesListResponse`
- `POST /api/v1/admin/databases` → 201 / 200 (idempotent if
  `if_not_exists`) / 409 / 400
- `DELETE /api/v1/admin/databases/:name?if_exists=true` → 200 / 404
- `GET /api/v1/admin/databases/:name/metadata` → `AdminMetadataLayoutResponse`
  (per-database `metadata.*` schema layout, Phase 1.4 will swap in live
  rows)
- `GET /api/v1/admin/runtime-config` → `RuntimeConfig` (read-only view of
  what was selected at boot)

All four enforce `require_admin_api_key()`. All four use match-on-Result
for mutex acquisition (no `.expect`). Each emits
`vng_database_lifecycle_total` metrics labeled by operation + status.

### ✅ 1.5 — Studio: Databases panel
**Files:**
- `ui/voltnuerongrid-studio/src/api/studio-client.ts` — added
  `listDatabases`, `createDatabase`, `dropDatabase`, `getDatabaseMetadata`,
  `getRuntimeConfig` methods + 6 new TypeScript interfaces
  (`DatabaseRecord`, `DatabasesListResponse`, `CreateDatabaseRequest/Response`,
  `DropDatabaseResponse`, `MetadataTableSpec`, `MetadataLayoutResponse`,
  `StorageConfig`, `SqlConfig`, `RuntimeConfigResponse`).
- `ui/voltnuerongrid-studio/src/api/studio-client.ts` — added private
  `del<T>()` helper for DELETE requests.
- `ui/voltnuerongrid-studio/src/store/databases.ts` (new) — zustand store
  for the databases list + selected DB. Not persisted (always fresh from
  server).
- `ui/voltnuerongrid-studio/src/components/Sidebar/DatabasesPanel.tsx`
  (new) — full CRUD UI: list, create (with inline validation matching
  server's `DatabaseName::parse` rules), drop (with browser confirm).
- `ui/voltnuerongrid-studio/src/store/ui.ts` — added `"databases"` to the
  `SidebarTab` union.
- `ui/voltnuerongrid-studio/src/components/Sidebar/Sidebar.tsx` — added
  the "DBs" tab button + panel rendering.

`npx tsc --noEmit` passes with zero errors.

---

## ⚠️ Critical things to verify after merge

Same constraint as last session: **the Rust changes to `main.rs` were not
compiled locally**. Before merging this branch, run:

```bash
cargo check --workspace
cargo test -p voltnuerongrid-meta       # 16 tests
cargo test -p voltnuerongrid-config     # 7 tests (already in main)
cargo test -p voltnuerongridd           # full service suite
cd ui/voltnuerongrid-studio
npx tsc --noEmit                        # already verified clean
```

If `cargo check -p voltnuerongridd` fails, the most likely causes are
(in order of probability):

1. **Field-init mismatch in `AppState`.** I added two fields to the struct
   and to the main + test-helper constructors. If the workspace has been
   updated between sessions to add OTHER fields to `AppState`, the
   constructor literals will be missing those.
   - Fix: `cargo check` will name the missing field; add it.

2. **Import resolution for `voltnuerongrid_meta::*` and
   `voltnuerongrid_config::*`** in `main.rs`. These are accessed via the
   crate's full path (`voltnuerongrid_meta::DatabaseCatalog`) so no `use`
   imports were added. If you prefer `use voltnuerongrid_meta::DatabaseCatalog`
   at the top, add it.

3. **`axum::routing::delete`** — used inline as
   `axum::routing::delete(admin_databases_drop)`. If your axum version
   doesn't expose it that way, replace with a regular import:
   ```rust
   use axum::routing::delete as axum_delete;
   ```
   then `.route("/api/v1/admin/databases/:name", axum_delete(admin_databases_drop))`.

4. **`Path<String>` and `Query<AdminDropDatabaseQuery>` extractors** —
   these need `axum::extract::{Path, Query}` (already imported via
   `use axum::extract::{Path, Query, State};` near the top of the file).

5. **The `metrics::counter!` macro syntax** — requires `metrics` crate
   v0.23 (which we added). If a different version is in the workspace,
   adjust the macro.

---

## Phase 1 — what's STILL TODO

This session got us through 1.0, 1.1, 1.2, 1.3, 1.5. Remaining:

### 1.4 — Wire `metadata.*` virtual tables to live data (HIGH-VALUE NEXT)

Currently `GET /api/v1/admin/databases/:name/metadata` returns the static
*schema* of the metadata tables (column lists). The next step is making
`SELECT * FROM metadata.tables` actually return rows backed by the live
`DdlCatalog` / `RbacPrivilegeMatrix` / `RuntimeConfig`.

**Approach:**
1. In `voltnuerongrid-meta`, add a `MetadataDataProvider` trait:
   ```rust
   pub trait MetadataDataProvider: Send + Sync {
       fn rows_for(&self, table: MetadataTable, db: &DatabaseName)
           -> Vec<HashMap<String, String>>;
   }
   ```
2. In the service, implement `MetadataDataProvider` over `AppState`:
   each match arm reads from `state.ddl_catalog` / `state.database_catalog`
   / `state.rbac_privilege_matrix` etc.
3. Add a route `GET /api/v1/admin/databases/:name/metadata/:table` that
   returns the rows.
4. (After Phase 1.6) `SELECT * FROM metadata.tables` parses + executes
   through DataFusion against this provider.

**Effort:** S-M (~1-2 days)

### 1.6 — sqlparser-rs adapter (the real-SQL wedge)

This is the biggest single Phase 1 item by impact. It replaces the
substring-flag parser in `voltnuerongrid-sql/src/ast.rs:309-440`.

**Plan:**

1. Add `sqlparser = "0.51"` to `crates/voltnuerongrid-sql/Cargo.toml`.
2. Create `crates/voltnuerongrid-sql/src/sqlparser_adapter.rs` that converts
   `sqlparser::ast::Statement` (output of `Parser::parse_sql`) into our
   existing `voltnuerongrid_sql::ast::Statement`. Keep our AST as the
   single contract — only swap parser internals.
3. Behind feature flag (default enabled). If the user later sets
   `VNG_SQL_ENGINE=vng`, the boot validator already rejects it (Phase 0).
4. Replace `parse_one()` body to call sqlparser, then convert. Keep the
   old function signature intact so callers don't change.
5. Delete the `up.contains("GROUP BY")` etc. heuristics — they are no
   longer needed since sqlparser gives us a real AST.

**Tests:** add at least these to `voltnuerongrid-sql`:
- A SELECT containing the literal string `'GROUP BY'` does NOT set
  `has_group_by` flag.
- A SELECT inside a comment does NOT trigger flags.
- Multi-byte UTF-8 column values parse cleanly.
- Real GROUP BY, ORDER BY, JOIN, HAVING all parse to the structured AST.

**Effort:** L (~1 week)

### 1.7 — DataFusion executor for OLTP SELECT

Once 1.6 lands, the executor at `services/voltnuerongridd/src/main.rs:15991`
(the broken `execute_oltp_select` that does `key.contains(prefix_str)`)
can go away.

**Plan:**

1. New crate `crates/voltnuerongrid-exec-datafusion`. Wraps a DataFusion
   `SessionContext` per query.
2. Build a custom `TableProvider` that wraps `PagedRowStore`. Each call
   to `scan` produces an Arrow `RecordBatch`.
3. `execute_select(ast, row_store, max_rows) -> Result<Rows, Error>`.
4. Replace `execute_oltp_select` and `execute_olap_query` callers with
   the new executor. Both paths use DataFusion now.
5. Result conversion: DataFusion `RecordBatch` → existing `OltpRowResult`
   wire format. Keep the response shape backwards-compatible.

**Tests:**
- `WHERE id = 5` returns exactly the row with id 5 (not 15, 25, 50, 51).
- `SELECT a, b FROM t` returns only columns `a` and `b`.
- `WHERE x > 10 AND y < 5` works.
- `ORDER BY` works.
- `JOIN` works.
- `GROUP BY ... HAVING` works.

**Effort:** L-XL (~2 weeks)

### 1.8 — Connection-level current-database state

Right now connections don't have a "current database" pointer. Once
multi-DB is real, every SQL statement implicitly addresses `default.public.*`.
Need:
- Add `current_database: Option<String>` to a per-session record.
- Recognise `USE <db>` / `SET DATABASE = <db>` SQL.
- Reject DDL/DML when `current_database` is None.
- RBAC checks scope by `current_database`.
- Studio: show active DB in title bar; clicking a database in the new
  panel sets it as active for the connection.

**Effort:** M (~3-4 days)

---

## Phase 0 follow-ups STILL TODO

Carried from previous handoff, none completed this session:

### 1. Roll out `handler_lock!` macro to the 346 `.lock().expect()` sites
The macro exists in `services/voltnuerongridd/src/resilience.rs`. A few
new handlers (the Phase 1.3 ones I added today) already use the
match-on-Result pattern manually; the macro itself is still unused. Mass
migration is mechanical but each touched handler must change return type
to `Result<(StatusCode, Json<X>), (StatusCode, Json<AuthErrorResponse>)>`.

**Effort:** S-M.

### 2. Refactor the 33k-line `main.rs` into modules
Per Pavan's Q5 answer ("Yes, please go ahead and refactor for modular,
clean-code, maintainable code, following reusability, OOPs, backward
compatibility and SOLID principles."), this should happen as its own PR.

**Approach (one module per PR is safest):**
```
services/voltnuerongridd/src/
  main.rs                  # ~250 lines now: bootstrap, route registration
  app_state.rs             # AppState + builders + state_with_key (test helper)
  routes/
    health.rs              # /health, /metrics
    sql/
      execute.rs           # sql_execute + try_handle_call_insert_rows_demo
      transaction.rs       # sql_transaction
      locks.rs             # sql_pessimistic_lock_*
      analyze.rs
      route.rs             # sql_route
    admin/
      schema.rs            # admin_schema_tree + types
      databases.rs         # NEW: admin_databases_list/create/drop/metadata
      runtime_config.rs    # NEW: admin_runtime_config
      cluster.rs           # admin_cluster_topology, admin_cluster_node_manage
      sql_control.rs       # admin_sql_transaction_control etc.
    sre/
      reliability.rs
      cache.rs
      driver_pool.rs
      gate.rs
      dr_hooks.rs
      failure.rs
      rate_limit.rs
    audit.rs               # audit_events, audit_chain_verify, etc.
    failover.rs            # failover_status, failover_simulate
    chaos.rs               # chaos_*
    raft_routes.rs         # raft_log, raft_heartbeat
    security.rs            # security_kms_status, security_tls_*
    autonomous.rs          # autonomous_*, authorize_autonomous_action
    ingest_routes.rs       # ingest endpoints
  helpers/
    auth.rs                # require_*_principal, require_admin_api_key,
                           # acquire_/release_sql_data_plane_connection
    audit_emit.rs          # append_runtime_audit_event
    sql_helpers.rs         # extract_insert_row_from_sql, extract_delete_*,
                           # extract_update_*, etc.
    demo.rs                # try_handle_call_insert_rows_demo,
                           # synthesize_demo_value
    middleware.rs          # add_cors, track_http_metrics,
                           # coarsen_route_for_metrics, options_preflight
  observability.rs         # already done — leave in place
  resilience.rs            # already done — leave in place
  raft.rs                  # already separate
```

**Sequence (one PR each):**
1. Extract helpers (`auth.rs`, `audit_emit.rs`, `middleware.rs`, `sql_helpers.rs`).
2. Extract route group `health`.
3. Extract `admin/databases.rs` (clean cut, recently added, easy to extract).
4. Extract `admin/runtime_config.rs`.
5. Extract `admin/schema.rs`.
6. Continue per route group...

**Effort:** L (~2-3 weeks if done carefully one PR at a time).

---

## How to continue from a fresh Cursor session

Open the new chat with these files attached:

```
@.cursorrules
@gaps-may26-1.md
@remaining.md
@vng.config.sample.json
@crates/voltnuerongrid-config/src/lib.rs
@crates/voltnuerongrid-meta/src/lib.rs
@services/voltnuerongridd/src/observability.rs
@services/voltnuerongridd/src/resilience.rs
```

Then say something like:
> Continue from `phase-1-correctness` branch in `remaining.md`. Start with
> Phase 1.4 (wire metadata.* virtual tables to live data) since 1.5 already
> landed.

Or:
> Continue with Phase 1.6 (sqlparser-rs adapter) — that's the highest-value
> remaining wedge.

---

## Smoke test for this session's work

Once `cargo check` is clean, the smoke test for Phase 1.3 + 1.5:

```bash
# 1. Build + run server
cargo build --release
VNG_LOG=debug VNG_LOG_FORMAT=pretty \
  VNG_ADMIN_API_KEY=test-admin-key \
  ./target/release/voltnuerongridd &

# 2. List databases (empty initially)
curl -s -H "x-vng-admin-key: test-admin-key" -H "x-vng-operator-id: admin" \
  http://127.0.0.1:8080/api/v1/admin/databases | jq

# 3. Create one
curl -s -X POST -H "x-vng-admin-key: test-admin-key" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"name":"sales","description":"sales data warehouse"}' \
  http://127.0.0.1:8080/api/v1/admin/databases | jq
# Expected: 201, body has the new database record.

# 4. Try duplicate (should 409)
curl -s -X POST -H "x-vng-admin-key: test-admin-key" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"name":"SALES"}' \
  http://127.0.0.1:8080/api/v1/admin/databases -w "\n%{http_code}\n"
# Expected: 409, "database \"sales\" already exists" (case-folded).

# 5. Idempotent create (should 200, already_existed=true)
curl -s -X POST -H "x-vng-admin-key: test-admin-key" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"name":"sales","if_not_exists":true}' \
  http://127.0.0.1:8080/api/v1/admin/databases | jq

# 6. Reject reserved name
curl -s -X POST -H "x-vng-admin-key: test-admin-key" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"name":"metadata"}' \
  http://127.0.0.1:8080/api/v1/admin/databases -w "\n%{http_code}\n"
# Expected: 400, error mentions reserved.

# 7. Get metadata schema layout
curl -s -H "x-vng-admin-key: test-admin-key" -H "x-vng-operator-id: admin" \
  http://127.0.0.1:8080/api/v1/admin/databases/sales/metadata | jq

# 8. Read runtime config
curl -s -H "x-vng-admin-key: test-admin-key" -H "x-vng-operator-id: admin" \
  http://127.0.0.1:8080/api/v1/admin/runtime-config | jq

# 9. Drop
curl -s -X DELETE -H "x-vng-admin-key: test-admin-key" -H "x-vng-operator-id: admin" \
  http://127.0.0.1:8080/api/v1/admin/databases/sales | jq

# 10. Verify HTTP metrics counter went up
curl -s http://127.0.0.1:8080/metrics | grep vng_database_lifecycle_total
curl -s http://127.0.0.1:8080/metrics | grep vng_http_requests_total
```

For the Studio:
```bash
cd ui/voltnuerongrid-studio
npm install
npm run dev
# Open http://localhost:1420 (Vite default)
# Connect to http://127.0.0.1:8080 with the test admin key
# Click the "DBs" tab in the sidebar
# Test: create / list / drop databases
```

---

## File inventory of this session's changes

Modified:
- `services/voltnuerongridd/Cargo.toml` (added `voltnuerongrid-meta` dep)
- `services/voltnuerongridd/src/main.rs` (~600 LOC added: middleware,
  AppState fields, 4 new HTTP handlers + types, route registration)
- `services/voltnuerongridd/src/observability.rs` (+ 2 metric descriptors)
- `crates/voltnuerongrid-meta/Cargo.toml` (added `serde`, `serde_json` deps)
- `ui/voltnuerongrid-studio/src/api/studio-client.ts` (+ 5 client methods,
  + 9 type definitions, + `del<T>` helper)
- `ui/voltnuerongrid-studio/src/components/Sidebar/Sidebar.tsx` (+ DBs tab)
- `ui/voltnuerongrid-studio/src/store/ui.ts` (+ "databases" tab type)

Added:
- `crates/voltnuerongrid-meta/src/lib.rs` (replaced 3-line stub with
  ~500 LOC, 16 tests passing)
- `ui/voltnuerongrid-studio/src/components/Sidebar/DatabasesPanel.tsx`
  (~250 LOC, type-checks clean)
- `ui/voltnuerongrid-studio/src/store/databases.ts` (zustand store)

---

*If you're picking this up from yet another session, the most leveraged
single piece of work to do next is Phase 1.6 (sqlparser-rs adapter). Once
SQL parsing is correct, Phase 1.7 (DataFusion executor) is unblocked, and
the database's correctness story closes.*
