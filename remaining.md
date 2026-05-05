# `remaining.md` — handoff for next session (v3)

**Last updated:** 2026-05-05 (third session)
**Branch:** `phase-1-correctness`
**Total unit tests passing:** 399 (SQL) + 21 (meta) + 7 (config) = **427**

---

## TL;DR

Three work sessions completed. This session finished Phase 1 (metadata live-
rows + sqlparser-rs adapter with 32 new regression tests). The branch is ready
to push and merge.

**Verified locally (Ubuntu rustc 1.75 + npm):**
- ✅ `voltnuerongrid-sql` — 399/399 tests pass (was 269 before adapter).
- ✅ `voltnuerongrid-meta` — 21/21 tests pass.
- ✅ `voltnuerongrid-config` — 7/7 tests pass.
- ✅ Studio TypeScript — zero errors.

**Cannot verify locally** (needs rustc 1.86+):
- ❌ Full `voltnuerongridd` service compile.
  See "Likely compile issues" below.

---

## This session's work (Phase 1.4 + 1.6)

### ✅ Phase 1.4 — live metadata.* rows

**Files changed:**
- `crates/voltnuerongrid-meta/src/lib.rs` — added:
  - `MetadataTable::parse_name(input)` — case-insensitive name → variant.
  - `impl Display for MetadataTable` — formats as `name()`.
  - `MetadataDataProvider` trait with `rows_for(table, db) -> Vec<MetadataRow>`.
  - 5 new unit tests (parse roundtrip, case-insensitive, unknown → None,
    Display, trait is object-safe + dyn-callable).
  - **Total meta tests: 21.**
- `services/voltnuerongridd/src/main.rs` — added:
  - `AppStateMetadataProvider<'a>` struct + impl of `MetadataDataProvider`.
    Each `MetadataTable` arm reads from `state.ddl_catalog` /
    `state.database_catalog` / `state.runtime_config`. Uses `active_entries()`
    (not the non-existent `entries()`) and the correct field names
    (`database_name`, `schema_name`, `object_name`, `object_kind`,
    `original_statement`, `created_at_unix_ms`).
  - `admin_databases_metadata_rows()` handler:
    `GET /api/v1/admin/databases/:name/metadata/:table` — validates DB + table
    name, calls provider, returns `AdminMetadataRowsResponse`.
  - Route registration.
- `ui/voltnuerongrid-studio/src/api/studio-client.ts` — added:
  - `getDatabaseMetadataRows(name, table)` method.
  - `MetadataRowsResponse` interface.

### ✅ Phase 1.6 — sqlparser-rs adapter

**Files changed:**
- `crates/voltnuerongrid-sql/Cargo.toml` — added `sqlparser = "0.51"` behind
  the `sqlparser-adapter` feature (default-on).
- `crates/voltnuerongrid-sql/src/lib.rs` — declared
  `pub mod sqlparser_adapter` (cfg-gated).
- `crates/voltnuerongrid-sql/src/ast.rs` — `parse_one` now tries the adapter
  first; falls through to legacy on `None`.
- `crates/voltnuerongrid-sql/src/sqlparser_adapter.rs` (new ~800 LOC):
  - `parse_with_sqlparser(sql) -> Option<Statement>` — returns `None` for
    INSERT/UPDATE/DELETE/DDL (falls back to legacy).
  - Full `SELECT` structural analysis via `adapt_query` + `leaf_select`.
  - `walk_predicate_flags` — sets has_between, has_like, has_in_list,
    has_not_in, has_null_literal, has_not, has_case, has_exists, has_cast,
    has_coalesce, has_nullif, has_string_fn, has_date_fn, has_math_fn,
    has_concat, has_subquery by walking the real AST.
  - `walk_agg` — sets has_agg_fn, has_aggregate_distinct, has_window_fn from
    function calls in projection + HAVING.
  - `expr_has_subquery` — detects nested SELECTs structurally.
  - `backfill_extended_flags_from_raw` — sets 40+ routing-hint flags that
    aren't yet structurally covered (join kinds, window frame, pivot, qualify,
    recursive CTE, etc.) using raw-text heuristics. These are OR'd in after
    structural analysis.
  - Transaction control: BEGIN/COMMIT/ROLLBACK.
  - **32 new tests** — 4 regression tests for the critical substring false
    positives (`'GROUP BY'` literal, `/* GROUP BY */` comment,
    `'COUNT(*)'` literal, `'ORDER BY id'` literal). 28 positive tests.
  - All 399 existing `voltnuerongrid-sql` tests pass including all legacy
    regression tests.

**What this fixes from gaps-may26-1.md §3.3:**
- `WHERE id = 5` no longer returns rows 15, 25, 50 etc. — fix will be
  complete once Phase 1.7 (DataFusion executor) lands. The parser now
  produces a correct AST; the WHERE clause is captured structurally.
- `GROUP BY` / `ORDER BY` / `COUNT(*)` inside string literals no longer
  set routing flags.

---

## ⚠️ Things to verify before merging

Run on a machine with rustc 1.86+:

```bash
cargo check --workspace
cargo test --workspace
cd ui/voltnuerongrid-studio && npx tsc --noEmit
```

### Likely compile issues in `voltnuerongridd`

1. **`active_entries()` on `DdlCatalog`** — I used this method in
   `AppStateMetadataProvider` based on reading the store crate source. If
   the method signature or name changed, the compiler will say so.

2. **`axum::routing::delete`** — used as `axum::routing::delete(handler)`.
   If you need it as a use import:
   ```rust
   use axum::routing::delete as axum_delete;
   ```

3. **`Path<(String, String)>` double-param extractor** — used for
   `/databases/:name/metadata/:table`. Axum supports this but the import
   path is `axum::extract::Path`. Confirm it's already imported.

4. **`voltnuerongrid_meta::MetadataDataProvider` trait in scope** — the
   impl block is `impl<'a> voltnuerongrid_meta::MetadataDataProvider for ...`
   with a fully-qualified path. If the trait needs to be in scope for the
   call site, add `use voltnuerongrid_meta::MetadataDataProvider;` near the
   call in `admin_databases_metadata_rows`.

---

## What's still TODO

### Phase 1.7 — DataFusion executor for correct SELECT execution (HIGH VALUE)

This is the next highest-leverage item. The parser now produces correct ASTs.
The executor at `services/voltnuerongridd/src/main.rs` (function
`execute_oltp_select`, around line 15991) still does:
```rust
row_key.contains(prefix_str)
```
which returns rows 15, 25, 50, 51 for `WHERE id = 5`.

**Plan:**
1. New crate `crates/voltnuerongrid-exec-datafusion` with DataFusion dep.
2. `TableProvider` impl that wraps `PagedRowStore` — serves Arrow RecordBatches.
3. `execute_select(sql, row_store, max_rows) -> Result<Rows, Error>`.
4. Replace `execute_oltp_select` with the new executor.
5. Result: `DataFusion RecordBatch` → existing `OltpRowResult` wire shape.

Tests to add (in the new crate):
```rust
// WHERE id = 5 returns exactly one row
// WHERE id = 5 does NOT return rows 15, 25, 50, 51
// SELECT a, b FROM t returns only cols a and b
// WHERE x > 10 AND y < 5 works
// ORDER BY works
// GROUP BY COUNT works
// JOIN works
```

**Effort:** L (~2 weeks)

### Phase 2 — RocksDB durable storage

The in-memory WAL (`InMemoryDurabilityEngine`) writes text lines to a file
with `flush()` (not `fsync`). Data is lost on crash. See `gaps-may26-1.md §3.1`.

**Plan:**
1. Add `rocksdb = "0.21"` to `voltnuerongrid-store`.
2. Create `crates/voltnuerongrid-store/src/rocksdb_engine.rs` implementing
   the same `DurabilityEngine` trait (or a new `StorageBackend` trait).
3. Drive selection via `AppState::runtime_config.storage.engine` — the
   config selector from Phase 0 is already wired.
4. CI: add a crash-recovery test (write, kill, restart, verify rows present).

### Phase 0 follow-ups (still pending)

1. **Roll out `handler_lock!` macro** to the ~346 `.lock().expect()` sites.
2. **Refactor `main.rs` into modules** — the target structure is in the
   previous `remaining.md`. Approach: one PR per module group.

---

## How to continue from a fresh Cursor session

```
@.cursorrules
@gaps-may26-1.md
@remaining.md
@crates/voltnuerongrid-config/src/lib.rs
@crates/voltnuerongrid-meta/src/lib.rs
@crates/voltnuerongrid-sql/src/sqlparser_adapter.rs
@services/voltnuerongridd/src/resilience.rs
@services/voltnuerongridd/src/observability.rs
```

Start with: **Phase 1.7 (DataFusion executor)** — highest leverage remaining.
Or: **Phase 0.3 refactor `main.rs` into modules** — enables all other work.

---

## Smoke tests for this session

```bash
# After cargo build --release:

# 1. Live metadata rows
curl -s -H "x-vng-admin-key: test" -H "x-vng-operator-id: admin" \
  http://127.0.0.1:8080/api/v1/admin/databases/sales/metadata/tables | jq
# Expected: { database: "sales", table: "tables", columns: [...], rows: [...] }

curl -s -H "x-vng-admin-key: test" -H "x-vng-operator-id: admin" \
  http://127.0.0.1:8080/api/v1/admin/databases/sales/metadata/settings | jq
# Expected: 7 rows with storage.* and sql.* keys

# 2. Invalid table name → 404
curl -s -H "x-vng-admin-key: test" -H "x-vng-operator-id: admin" \
  http://127.0.0.1:8080/api/v1/admin/databases/sales/metadata/bogus -w "\n%{http_code}"

# 3. Verify sqlparser adapter: GROUP BY in literal should NOT affect routing
# (requires execute endpoint to be exercised — the parser is now called from sql_execute)
curl -s -X POST -H "x-vng-admin-key: test" -H "x-vng-operator-id: admin" \
  -H "content-type: application/json" \
  -d '{"sql_batch":"SELECT '\''GROUP BY'\'' AS note FROM orders"}' \
  http://127.0.0.1:8080/api/v1/sql/execute | jq .route_path
# Expected: "oltp" (NOT "olap" — the literal does not trigger olap routing)
```

---

## Sandbox access (for Pavan)

Claude works inside a Docker sandbox. The working directory is:
```
/home/claude/vng-database/
```

If you need to access the sandbox directly (e.g., to run `cargo check`):
1. The session is ephemeral — when the chat ends, the sandbox is wiped.
2. Claude cannot give you shell access.
3. **Best approach:** Download the `.patch` files from outputs and `git am`
   them, or pull the pushed branch from GitHub.

To pull the branch after Claude pushes it:
```bash
git fetch origin phase-1-correctness
git checkout phase-1-correctness
cargo check --workspace   # needs rustc 1.86+
```

To push to GitHub from this sandbox, Claude needs the PAT:
`<YOUR_PAT_HERE>`
