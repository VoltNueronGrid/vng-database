# `remaining.md` — handoff for next session

**Author of this file:** Claude (review pass on `phase-0-hygiene` branch)
**Created:** 2026-05-04
**For:** the next operator (you, in a fresh Cursor / new chat)
**Read first:** `gaps-may26-1.md` (full gap analysis), then this file.

---

## TL;DR

A `phase-0-hygiene` branch has been created with one commit that lands all of
Phase 0 (UI tokens synced, demo intercept extracted, observability scaffold,
config selector for storage / SQL engines, doc cleanup). The next operator
should:

1. **Verify the branch builds** on a real Rust toolchain (sandbox here only had
   rustc 1.75 which is too old). If it does, push and merge.
2. **Pick up Phase 1** (correctness wedge) from the sequencing in
   `gaps-may26-1.md` §13.

The user's open questions have been answered (they want **configurable
RocksDB | VNG** storage and **DataFusion+sqlparser-rs | VNG** SQL engine, with
VNG being a placeholder). The new `crates/voltnuerongrid-config` crate
implements that selector. Phase 1 work should call into it from the runtime.

---

## Branch & commits

```
phase-0-hygiene
└── (one commit) Phase 0: hygiene + observability + config selector
```

Push command:
```
git push -u origin phase-0-hygiene
# then open a PR against main
```

---

## What was actually delivered in Phase 0

### ✅ 0.1 — Studio design tokens synced
- `ui/voltnuerongrid-studio/src/styles/globals.css` — dark `:root` block now
  matches `studio-design.html` exactly (verified by python diff: 0 value
  differences).
- Added `--radius-{sm,md,lg}` so the design source-of-truth name works, and
  kept `--r-*` as `var(--radius-*)` aliases so existing 15 component-level
  references compile.
- `ui/voltnuerongrid-studio/design/studio-design.html` updated with the two
  extension tokens that were code-only (`--brand-cyan-low`, `--right-panel-w`).

### ✅ 0.2 — `CALL insert_rows` demo intercept extracted
- Moved out of `sql_execute` (was lines 7558-7682, now a single helper call).
- New helper `try_handle_call_insert_rows_demo(...)` lives at
  `services/voltnuerongridd/src/main.rs` near the other extract_* helpers.
- New helpers `synthesize_demo_value()` (pure, unit-testable) and
  `svc_unavailable_sql_response()` (graceful 503).
- The helper uses `match state.row_store.lock()` instead of `.expect(...)`,
  so a poisoned mutex returns 503 instead of taking the request down.

### ✅ 0.3 — Resilience helper module
- New file `services/voltnuerongridd/src/resilience.rs` with:
  - `LockOutcome` enum (`Held(MutexGuard)` | `Poisoned { resource }`).
  - `lock_or_unavailable(&Mutex<T>, &'static str) -> LockOutcome<'_, T>`.
  - `handler_lock!` macro that returns a 503 JSON envelope.
  - Two unit tests (one for the happy path, one for poisoned recovery).
- **Not yet rolled out to the 346 existing call sites.** See "Next session
  task — finish the .expect() migration" below.

### ✅ 0.4 — Observability
- New file `services/voltnuerongridd/src/observability.rs`:
  - `init_observability()` — idempotent, called from `main()`.
  - `tracing_subscriber` with `EnvFilter` (env: `VNG_LOG`, default
    `info,voltnuerongridd=info`) and pretty / json output (env:
    `VNG_LOG_FORMAT=json|pretty`).
  - `metrics-exporter-prometheus` recorder installed at startup.
  - `render_metrics()` returns the current Prometheus text-format output.
- New route `GET /metrics` returns the Prometheus exposition with
  `Content-Type: text/plain; version=0.0.4; charset=utf-8`.
- Pre-described counters: `vng_http_requests_total`,
  `vng_sql_execute_total`, `vng_handler_errors_total`,
  `vng_sql_execute_duration_ms`. **Not yet incremented anywhere** — that's
  follow-up wiring in handlers (cheap, ~1 line per route).
- Env var `VNG_METRICS_DISABLED=1` skips recorder install for tests.
- Crate deps added: `tracing 0.1`, `tracing-subscriber 0.3 (env-filter, fmt,
  json)`, `metrics 0.23`, `metrics-exporter-prometheus 0.15
  (http-listener)`.

### ✅ 0.5 — Cleanup
- 15 superseded `.md` files moved to `docs/archive/` (with a README
  explaining why each is there). Root went from 23 `.md` files to 8.
- `.gitignore` adds `.DS_Store` (recursive), editor scratch (`*.swp`,
  `.idea/`, `*.iml`), `Cargo.lock.bak`.
- 4 tracked `.DS_Store` files removed.

### ✅ Configuration selector for backends (the user's first two open questions)
- New crate `crates/voltnuerongrid-config` (registered in workspace).
- `StorageEngine::Rocksdb` (default, supported) | `StorageEngine::Vng`
  (rejected at validate with friendly message pointing to
  `gaps-may26-1.md §3.1`).
- `SqlEngine::Datafusion` (default, supported) | `SqlEngine::Vng`
  (rejected at validate).
- `RuntimeConfig::from_env_and_file(env, json_text)` — loads defaults, then
  overlays JSON file, then env vars.
- Env vars: `VNG_STORAGE_ENGINE`, `VNG_SQL_ENGINE`, `VNG_DATA_DIR`,
  `VNG_STORAGE_BACKGROUND_JOBS`, `VNG_WAL_FSYNC_ON_COMMIT`,
  `VNG_HTAP_OLAP_THRESHOLD_ROWS`, `VNG_MAX_RESULT_ROWS`, `VNG_CONFIG_PATH`.
- `EnvProvider` trait + `MemEnv` for clean unit tests (no process-env mutation).
- 6 unit tests covering: defaults validate, env overrides apply, vng engine
  rejected at validate (storage + sql), unknown values error at parse, file
  + env merge, malformed JSON errors.
- Service `main()` loads config first thing, exits 2 with a logged error on
  invalid config.
- Sample at `vng.config.sample.json`.

---

## ⚠️ What COULDN'T be verified locally

The sandbox where Phase 0 was authored had **only Ubuntu apt's `rustc 1.75`**,
and the repo's actual MSRV is higher (some transitive crates need
`edition2024`, requiring rustc 1.86+). I could not run `cargo check`.

**Therefore:** before pushing, the next operator should run:

```bash
cargo check --workspace
cargo test -p voltnuerongrid-config        # 6 tests, all should pass
cargo test -p voltnuerongridd resilience  # 2 tests in resilience.rs
cargo test -p voltnuerongridd              # full service test suite
```

I'm reasonably confident the changes are correct because:

1. The Rust changes are **additive** — new modules, new functions, new fields
   in `Cargo.toml`. The only edits to existing functions are:
   - `sql_execute`: replaced an inline 124-line block with a single helper
     call. Same control flow; same return type.
   - `main()`: prepended ~30 lines of init/config code. Nothing existing was
     touched.
2. Type signatures match what was already in `main.rs` (I read
   `RuntimeAccessPrincipal`, `AppState`, `SqlExecuteRequest`,
   `SqlExecuteResponse`, `acquire_sql_data_plane_connection`, etc. before
   writing helpers).
3. The new crate has no unusual deps (just `serde` + `serde_json`).

Things to watch for in the build:

- **Imports of `tracing` macros** — the new code uses `tracing::info!`,
  `tracing::error!`. If the `tracing` crate isn't in scope at the call sites
  in `main.rs`, add `use tracing;` at the top.
- **`metrics::counter!` / `metrics::describe_counter!`** are used in
  `observability.rs` — should be fine since `metrics` is a fresh dep.
- **`axum::http::HeaderName`** in `metrics_handler` return type — if axum's
  re-exports are stricter, change the tuple to use a `[(http::HeaderName, &str); 1]`
  with explicit imports.
- **The `pub mod resilience;` and `pub mod observability;` declarations**
  must come after `mod raft;` and at top level of `main.rs`.

---

## Phase 0 follow-ups still TODO

These are small Phase 0 items that didn't fit in the first commit:

### 1. Roll out `handler_lock!` to the 346 existing `.lock().expect()` sites
**File:** `services/voltnuerongridd/src/main.rs` (mostly).
**Effort:** S (mechanical, but every site needs the surrounding handler to
return `Result<(StatusCode, Json<X>), ...>` instead of `Json<X>`).
**Approach:**
```bash
# Inventory:
grep -nE '\.lock\(\)\.expect\(' services/voltnuerongridd/src/main.rs

# Migrate in groups by handler. Each touched handler returns a Result.
# Use the macro:
let rs = handler_lock!(state.row_store, "row_store");
```
**Priority:** SQL data-plane first (`sql_execute`, `sql_transaction`,
`sql_pessimistic_lock_*`), then admin endpoints, then SRE / chaos endpoints
last (those legitimately can fail-fast).

### 2. Wire metrics counters into hot handlers
**Currently** `vng_http_requests_total` etc. are described but never
incremented. One-line per handler:
```rust
metrics::counter!("vng_http_requests_total", "route" => "/api/v1/sql/execute", "status" => "ok").increment(1);
```
**Better:** add `tower_http::trace::TraceLayer` and a small middleware that
emits the counter automatically. ~30 lines.

### 3. Studio: surface the runtime config in the Settings panel
**File:** `ui/voltnuerongrid-studio/src/components/Settings/SettingsPanel.tsx`
Add a read-only "Server" section that fetches `/api/v1/admin/runtime-config`
(new endpoint to add) and shows the current storage/SQL engine. Editing
should be Phase 1 (needs persistence into the metadata schema).

### 4. Refactor `main.rs` into modules
The user explicitly approved this in their answer (Q5). Plan:
```
services/voltnuerongridd/src/
  main.rs                  # ~200 lines: bootstrap, route registration
  app_state.rs             # AppState + builders
  routes/
    health.rs
    sql/
      execute.rs
      transaction.rs
      locks.rs
      analyze.rs
    admin/
      schema.rs
      cluster.rs
      databases.rs         # NEW for Phase 1
    sre/
      reliability.rs
      cache.rs
      driver_pool.rs
      ...
    audit.rs
    failover.rs
    chaos.rs
    raft_routes.rs
    security.rs
    autonomous.rs
  helpers/
    auth.rs                # require_*_principal, acquire_*_connection
    audit_emit.rs
    sql_helpers.rs         # extract_insert_row_from_sql, extract_delete_*, etc.
    demo.rs                # try_handle_call_insert_rows_demo, synthesize_demo_value
  observability.rs         # already done
  resilience.rs            # already done
  raft.rs                  # already separate
```
**Effort:** L (mechanical, but 33k lines). Approach: extract one module per
PR, run `cargo check` after each, keep reviews tight. Don't try to split
everything in one commit — that's unreviewable.

### 5. Move the Studio's hardcoded backend URL to the new config
The Studio currently uses `http://127.0.0.1:8080` as default base URL. Should
read from `VNG_HTTP_BIND` (server) so the Studio knows where to connect, OR
emit the URL in `/health` for the studio to discover. Low priority but
relevant once we have multi-node.

---

## Phase 1 plan — start here next session

Per `gaps-may26-1.md` §13 + your priority ranking
(durable storage → real SQL → UI → drivers → multi-DB → OLAP), Phase 1 is
the **correctness wedge** that has to land before durable storage matters.

### 1.1 Adopt `sqlparser-rs` for parsing (replaces substring-flag parser)
- **Add dep:** `sqlparser = "0.51"` (or latest) to
  `crates/voltnuerongrid-sql/Cargo.toml`.
- **Build adapter:** `crates/voltnuerongrid-sql/src/sqlparser_adapter.rs`
  that converts `sqlparser::ast::Statement` into our existing
  `voltnuerongrid_sql::ast::Statement`. The existing AST is already richly
  typed and used everywhere — keep that as the contract; replace just the
  *parser* internals.
- **Behind feature flag:** `default-features = ["sqlparser"]`. The "vng"
  parser path stays for backward compat but emits a deprecation warning.
- **Drive selection from config:** `cfg.sql.engine == SqlEngine::Datafusion`
  → use sqlparser; `SqlEngine::Vng` → already errors out at validate, never
  reached.

### 1.2 Adopt DataFusion as the executor
- **New crate:** `crates/voltnuerongrid-exec-datafusion`. Wraps a DataFusion
  `SessionContext`. Receives a parsed AST + a resolver that can fetch row
  batches from the row store, and returns a `RecordBatch`.
- **HtapQueryRouter:** keep its routing logic (OLTP vs OLAP) but the executor
  it dispatches to becomes DataFusion in both cases — DataFusion handles both
  paths well and we can later split if a real columnar OLAP store goes in.
- **Replace `execute_oltp_select`:** delete the broken row-key-substring code
  in `main.rs:15991-16038`. Wire `sql_execute` to call the new executor.

### 1.3 `CREATE DATABASE` end-to-end
- **Catalog change:** add a `DatabaseCatalog` (separate from `DdlCatalog`).
  Each database has a unique name (rejected on conflict), a creation
  timestamp, an owner, and a reference to its own `DdlCatalog` instance.
- **SQL:** `CREATE DATABASE <name> [IF NOT EXISTS]`,
  `DROP DATABASE <name> [CASCADE]`, `ALTER DATABASE <name> RENAME TO ...`.
- **Routes:** `POST /api/v1/admin/databases`,
  `GET /api/v1/admin/databases`, `DELETE /api/v1/admin/databases/{name}`.
  Existing `/api/v1/admin/schema/tree` already iterates per-database — wire
  it to read from `DatabaseCatalog`.
- **Connection state:** add `current_database: Option<String>` to the
  connection. SQL like `USE <db>` or `\c <db>` switches it. RBAC checks must
  scope by database.
- **UI:** add a "Databases" pane in the sidebar (above "Schemas"). Modal for
  Create / Drop. Active database shown in the title bar.

### 1.4 Metadata schema per database
- Each database gets a `metadata` schema auto-populated on creation, with
  views over the catalog: `metadata.tables`, `metadata.columns`,
  `metadata.schemas`, `metadata.routines`, `metadata.indexes`,
  `metadata.users`, `metadata.roles`, `metadata.settings`.
- Mirror Postgres' `information_schema` and `pg_catalog` for familiarity.
- Settings table is **read-write**: the Studio's settings panel writes here,
  the DB reads on hot-reload (or restart).

### 1.5 Studio: per-DB metadata browser
- Right-panel addition showing the active database's `metadata.*` tables.
- "Settings" tab queries `metadata.settings`.

**Phase 1 sequencing inside Phase 1:** 1.1 → 1.2 → 1.3 → 1.4 → 1.5 (each
unblocks the next).

**Phase 1 estimated effort:** 4-6 weeks for a competent solo developer
following this plan; 2-3 weeks with two engineers in parallel (one on
parser+exec, one on multi-DB+UI).

---

## Phase 2+ — at-a-glance

| Phase | Focus | Gating | Effort |
|---|---|---|---|
| 2 | Durable storage on RocksDB | Phase 1 done so we know what tables look like | 4-6 weeks |
| 3 | Users + auth (per-DB) | Phase 1 (tables for users) + Phase 2 (durable users) | 3-4 weeks |
| 4 | OLAP path (Parquet snapshots, DataFusion-on-Parquet) | Phases 1-3 | 6-8 weeks |
| 5 | Drivers (Python, TS bring-up) | Phase 1 (real SQL) | 2-3 weeks each |

---

## Open items the user might come back to

1. **The 311 HTTP routes** — the user said "go ahead" on pruning. I left them
   as-is for Phase 0 (out of scope), but during the Phase 0.4 main.rs
   refactor, take a hard look at:
   - SRE/chaos endpoints used only by the KPI gate scripts.
   - Audit / autonomous-action endpoints that look gate-driven.
   - Check what the Studio actually calls (`grep -r "/api/v1" ui/`); anything
     unused by the Studio AND not exercised by drivers AND not in active
     KPI gates is a deletion candidate.

2. **The `voltnuerongrid-meta` crate** — currently a 3-line stub. Phase 1.4
   work should populate this with the per-database metadata schema impl.

3. **The `voltnuerongrid-failover` crate** — also a 3-line stub. Defer to
   Phase 5+.

4. **The `voltnuerongrid-core` crate** — currently has only `sharding.rs`.
   Should host the cross-crate types (Database, Schema, Connection identity)
   that emerge in Phase 1.

5. **MCP and AI crates** — substantial code (~1500 LOC + 600 LOC); not
   reviewed in this pass for production-readiness. Should review separately
   once Phase 1 lands.

6. **Driver languages other than Rust/C** — at next major checkpoint:
   - `drivers/voltnuerongrid-driver-python/` — verify it's a real wheel
   - `drivers/voltnuerongrid-driver-typescript/` and `node/` — verify build
   - `drivers/voltnuerongrid-driver-java/` — verify Maven build
   - `drivers/voltnuerongrid-driver-deno/` and `perl/` — these are likely
     skeletons; deprioritize.

7. **Playwright tests** — the user mentioned exhaustive Playwright coverage.
   Current test count and coverage need a separate audit. Estimate ~15-20
   existing tests; will need ~80-100 to cover everything Phase 1 introduces
   (DB CRUD, users, settings, metadata browse, query routing visibility).

---

## Cursor / Copilot context for the next session

When you open this in a fresh Cursor session, prime the AI with:

```
@.cursorrules
@gaps-may26-1.md
@remaining.md
@vng.config.sample.json
@crates/voltnuerongrid-config/src/lib.rs
@services/voltnuerongridd/src/observability.rs
@services/voltnuerongridd/src/resilience.rs
```

**Key invariants to preserve:**
- Every existing HTTP route's response shape must remain backward compatible
  unless explicitly changed in a documented PR. The user has external code
  (drivers, Studio, KPI scripts) reading these.
- `.cursorrules` says: NO `unwrap()` / NO `panic!` in handler paths, RBAC
  checks in order (admin → operator → tenant), never log API key header
  values.
- Workspace stays Rust 2021 edition for now (until MSRV bump is intentional).
- `forbid(unsafe_code)` is workspace-wide — keep it that way.
- Every public crate has at least one unit test in `lib.rs`.

---

## Smoke test (post-merge)

After this branch lands, run:

```bash
# 1. Build clean
cargo build --workspace --release

# 2. Unit tests
cargo test --workspace

# 3. Boot smoke test
VNG_LOG=debug VNG_LOG_FORMAT=pretty \
  VNG_ADMIN_API_KEY=test-admin-key \
  ./target/release/voltnuerongridd &

# 4. Hit /health and /metrics
curl -s http://127.0.0.1:8080/health
curl -s http://127.0.0.1:8080/metrics | head -30

# 5. Try invalid config — should exit 2
VNG_STORAGE_ENGINE=vng VNG_ADMIN_API_KEY=k ./target/release/voltnuerongridd
# Expected: exits with code 2 and a clear error message.

# 6. Try invalid SQL engine — should exit 2
VNG_SQL_ENGINE=vng VNG_ADMIN_API_KEY=k ./target/release/voltnuerongridd
# Expected: exits with code 2.
```

---

## Token-budget note for whoever reads this in Claude

I (Claude) wrote this on a working branch with no remote push privilege.
Everything I did is in `phase-0-hygiene` locally inside the sandbox. The
session that ran out is the one that produced this file. If you're reading
this in a fresh chat, the user (Pavan) will have pushed `phase-0-hygiene`
already — start by `git log --oneline -5` and confirming the Phase 0 commit
is at HEAD before continuing.

If `cargo check` fails on the new code, the most likely fix is import paths
in `main.rs` for `tracing::*` and `axum::http::HeaderName`. Both are minor.
The new crate (`voltnuerongrid-config`) is self-contained and should compile
in isolation: `cargo check -p voltnuerongrid-config`.

Good luck. The repo is in good shape architecturally; the gap is in execution
depth. Phase 1 is where the real work begins.
