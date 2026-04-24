# E2E Scenario Pack — Prompt Requirements R-01..R-18

**Document:** S11-001  
**Status:** Draft  
**Last updated:** 2026-04-22  
**Traceability source:** `services/voltnuerongridd/reference/prompt-requirement-traceability-matrix-v3.md`

---

## Overview

This document provides a comprehensive scenario for each requirement R-01 through R-18.
Each scenario maps to at least one acceptance check that can be run locally or is
explicitly deferred to cloud/CI environments.

---

## R-01 — ANSI SQL + AI-assisted chat/extract/ingest/import/export

**Scenario name:** `R01-ANSI-SQL-basic-round-trip`

**Pre-conditions:**
- Workspace builds cleanly (`cargo build -p voltnuerongrid-sql`)
- `SqlAnalyzer` is importable from `voltnuerongrid-sql`

**Steps:**
1. Construct a `SELECT 1` SQL string.
2. Call `SqlAnalyzer::classify_statement("SELECT 1")`.
3. Verify the result is `SqlStatementKind::Query`.
4. Call `SqlAnalyzer::analyze_statement("SELECT 1")`.
5. Verify `AnalysisResult::is_read_only` is `true`.

**Expected result:** Both classification and analysis APIs return correct values for a minimal ANSI SELECT.

**Test command / acceptance check:**
```
cargo test -p voltnuerongrid-sql r01_ansi_sql_basic
```

**Status:** Pass (local)

---

## R-02 — Create DB/table/view/MV/functions

**Scenario name:** `R02-DDL-lifecycle`

**Pre-conditions:**
- `SqlAnalyzer` importable from `voltnuerongrid-sql`

**Steps:**
1. Call `classify_statement` for `CREATE TABLE`, `CREATE VIEW`, `CREATE MATERIALIZED VIEW`, `CREATE FUNCTION` each.
2. Verify all return `SqlStatementKind::Ddl`.
3. Call `analyze_statement("create materialized view mv as select 1")`.
4. Verify `AnalysisResult::object_type == "materialized_view"`.

**Expected result:** DDL classification covers all object types required by R-02.

**Test command:**
```
cargo test -p voltnuerongrid-sql
```

**Status:** Pass (local)

---

## R-03 — In-DB function languages (Rust, JS, Python)

**Scenario name:** `R03-function-language-registry`

**Pre-conditions:**
- `FunctionRegistry` importable from `voltnuerongrid-sql`

**Steps:**
1. Create a `FunctionRegistry::with_builtins()`.
2. Register a function with `FunctionLanguage::Rust`.
3. Register a function with `FunctionLanguage::JavaScript`.
4. Register a function with `FunctionLanguage::Python`.
5. Call `list()` and verify all three languages appear.

**Expected result:** Registry correctly stores and retrieves functions for each supported in-DB language.

**Test command:**
```
cargo test -p voltnuerongrid-sql
```

**Status:** Pass (local)

---

## R-04 — HA/FT/reliability/elastic/i18n/UTF-8

**Scenario name:** `R04-i18n-utf8-catalog`

**Pre-conditions:**
- `I18nCatalog` importable from `voltnuerongrid-sql`

**Steps:**
1. Call `I18nCatalog::message(SupportedLocale::En, "error.query_failed")`.
2. Verify the returned `LocalizedMessage` body is non-empty UTF-8.
3. Call `I18nCatalog::message(SupportedLocale::parse("ja"), "error.query_failed")`.
4. Verify non-empty result.
5. HA/failover: verify `voltnuerongrid-failover` crate builds and exports `LeaderElection`.

**Expected result:** i18n catalog handles multiple locales; failover crate compiles.

**Test command:**
```
cargo test -p voltnuerongrid-sql
cargo build -p voltnuerongrid-failover
```

**Status:** Pass (local)

---

## R-05 — Data files separate from engine

**Scenario name:** `R05-storage-engine-separation`

**Pre-conditions:**
- `voltnuerongrid-store` crate builds independently of `voltnuerongridd` binary

**Steps:**
1. Run `cargo build -p voltnuerongrid-store`.
2. Confirm it succeeds without the service binary.
3. Review `StorageEngine` struct: verify it holds a configurable `data_dir` path.
4. Verify no hard-coded paths appear in `store/src/`.

**Expected result:** Storage layer is a standalone crate with configurable data directory.

**Test command:**
```
cargo build -p voltnuerongrid-store
cargo test -p voltnuerongrid-store
```

**Status:** Pass (local)

---

## R-06 — CSV/Parquet/Excel ingest

**Scenario name:** `R06-format-ingestion`

**Pre-conditions:**
- `voltnuerongrid-ingest` crate builds
- Dev dependencies include `arrow-array`, `parquet`, `rust_xlsxwriter`

**Steps:**
1. Run `cargo build -p voltnuerongrid-ingest`.
2. Run `cargo test -p voltnuerongridd` (service-level ingest dev-dep tests).
3. Verify test output references CSV, Parquet, and XLSX format handlers.

**Expected result:** All three format ingest paths compile and pass unit/integration tests.

**Test command:**
```
cargo test -p voltnuerongridd --test '*ingest*'
```

**Status:** Pass (local)

---

## R-07 — Fast multi-threaded import

**Scenario name:** `R07-throughput-benchmark`

**Pre-conditions:**
- `tests/benchmarks/` directory contains throughput harness
- Tokio multi-thread runtime configured

**Steps:**
1. Run benchmark suite: `cargo bench -p voltnuerongridd`.
2. Capture rows/sec metric for bulk import.
3. Compare against baseline KPI in `tests/kpi/`.

**Expected result:** Import throughput meets or exceeds baseline KPI for multi-threaded path.

**Test command:**
```
cargo bench -p voltnuerongridd
```

**Status:** Deferred (cloud) — requires production-grade dataset; local smoke test passes

---

## R-08 — Local laptop + cloud SaaS

**Scenario name:** `R08-dual-deployment-smoke`

**Pre-conditions:**
- `deploy/local/install.sh` is executable
- `deploy/local/vng.env.example` populated

**Steps:**
1. Run `bash deploy/local/install.sh`.
2. Verify binary starts and `curl http://localhost:8080/health` returns `{"status":"ok"}`.
3. Review `deploy/cloud/README.md` for cloud deployment path.

**Expected result:** Local deployment succeeds end-to-end in under 5 minutes; cloud path documented.

**Test command:**
```
bash deploy/local/install.sh
curl http://localhost:8080/health
```

**Status:** Pass (local); Deferred (cloud)

---

## R-09 — Plugin/extensibility ecosystem

**Scenario name:** `R09-plugin-spi`

**Pre-conditions:**
- `voltnuerongrid-plugins` crate builds

**Steps:**
1. Run `cargo build -p voltnuerongrid-plugins`.
2. Verify `PluginRegistry` or equivalent SPI type is exported.
3. Register a sample no-op plugin and invoke its lifecycle hooks.

**Expected result:** Plugin SPI allows registration without modifying core engine code.

**Test command:**
```
cargo test -p voltnuerongrid-plugins
```

**Status:** Pass (local)

---

## R-10 — Trillion-row scale and fast retrieval

**Scenario name:** `R10-scale-claim`

**Pre-conditions:**
- Cloud environment with sufficient data available
- Benchmark harness in `tests/benchmarks/` ready

**Steps:**
1. Load a dataset of 10B rows minimum.
2. Execute range scans and point lookups.
3. Record P50/P99 latency.
4. Compare against targets in `tests/kpi/`.

**Expected result:** P99 latency for single-row lookup remains below target; scan throughput meets baseline.

**Test command:** Cloud-only benchmark suite.

**Status:** Deferred (cloud) — no local data generation for this scale; architecture tested at 1M rows locally

---

## R-11 — Indexes and constraints

**Scenario name:** `R11-ddl-index-constraint`

**Pre-conditions:**
- `voltnuerongrid-sql` and `voltnuerongrid-store` both build

**Steps:**
1. Parse `CREATE INDEX idx ON t(col)` through `SqlAnalyzer`.
2. Verify classified as `Ddl`.
3. Parse `ALTER TABLE t ADD CONSTRAINT pk PRIMARY KEY (id)`.
4. Verify classified as `Ddl`.
5. Verify storage layer enforces unique constraint on duplicate insert attempt.

**Expected result:** Index and constraint DDL round-trips through parser and storage layer enforces constraint semantics.

**Test command:**
```
cargo test -p voltnuerongrid-sql
cargo test -p voltnuerongrid-store
```

**Status:** Pass (local)

---

## R-12 — Full trigger model + queue sinks (Kafka/NATS)

**Scenario name:** `R12-trigger-registration`

**Pre-conditions:**
- `voltnuerongrid-store` crate builds with `TriggerRegistry` exported

**Steps:**
1. Create `TriggerRegistry::new()`.
2. Call `register()` with a `TriggerDefinition` for `AfterInsert` on table `users`.
3. Call `find_triggers("users", "public", &TriggerEvent::AfterInsert)`.
4. Verify exactly one trigger is returned.
5. Verify `find_triggers` for `AfterUpdate` returns empty.

**Expected result:** Trigger registration and lookup work correctly; queue sink integration deferred to S12.

**Test command:**
```
cargo test -p voltnuerongrid-store r12_trigger_registration
```

**Status:** Pass (local); Queue sinks (Kafka/NATS) Deferred (S12)

---

## R-13 — Retrieval optimization at extreme scale

**Scenario name:** `R13-join-paging-stress`

**Pre-conditions:**
- `voltnuerongrid-opt` crate builds
- Parity test fixtures in `tests/parity/` loaded

**Steps:**
1. Run optimizer unit tests: `cargo test -p voltnuerongrid-opt`.
2. Verify join reorder and predicate pushdown rules activate for multi-table queries.
3. Execute paginated query over 1M-row fixture and verify consistent row counts.

**Expected result:** Optimizer transforms multi-join queries; pagination returns correct, complete result sets.

**Test command:**
```
cargo test -p voltnuerongrid-opt
```

**Status:** Pass (local); Extreme-scale stress Deferred (cloud)

---

## R-14 — Seeded function parity + UDF

**Scenario name:** `R14-function-parity`

**Pre-conditions:**
- `FunctionRegistry::with_builtins()` available from `voltnuerongrid-sql`

**Steps:**
1. Create `FunctionRegistry::with_builtins()`.
2. Verify presence of common SQL builtins (e.g. `count`, `sum`, `coalesce`).
3. Register a custom UDF via `register()`.
4. Verify UDF appears in `list()`.
5. Verify `contains("my_udf")` returns `true`.

**Expected result:** Builtin catalog is populated; UDFs register alongside builtins without collision.

**Test command:**
```
cargo test -p voltnuerongrid-sql function_registry
```

**Status:** Pass (local)

---

## R-15 — Multi-user roles and authorization

**Scenario name:** `R15-rbac-integration`

**Pre-conditions:**
- `voltnuerongrid-auth` crate builds
- Admin/operator/tenant role types exported

**Steps:**
1. Build auth crate: `cargo build -p voltnuerongrid-auth`.
2. Attempt to authorize a tenant-role principal against an admin command.
3. Verify authorization returns `Denied`.
4. Authorize an admin-role principal against the same command.
5. Verify `Permitted`.

**Expected result:** RBAC guards correctly enforce role-based access at the command level.

**Test command:**
```
cargo test -p voltnuerongrid-auth
```

**Status:** Pass (local)

---

## R-16 — UI client separate from engine

**Scenario name:** `R16-client-driver-decoupling`

**Pre-conditions:**
- VSCode extension builds in `ui/ide-extensions/vscode-cursor/`
- Extension communicates via driver abstraction only

**Steps:**
1. Build extension: `cd ui/ide-extensions/vscode-cursor && npm ci && npm run compile`.
2. Verify no direct imports of `voltnuerongridd` binary internals in extension source.
3. Confirm all runtime calls go through `DriverConfig`-based HTTP or native transport.

**Expected result:** Extension has no compile-time coupling to engine internals; all access is through the driver contract.

**Test command:**
```
cd ui/ide-extensions/vscode-cursor && npm ci && npm run compile
```

**Status:** Pass (local)

---

## R-17 — Native multi-language drivers (must-have)

**Scenario name:** `R17-driver-parity`

**Pre-conditions:**
- `voltnuerongrid-driver-rust` crate builds
- `DriverConfig` with HTTP, native (`vng://`), and dual-transport configs constructable

**Steps:**
1. Construct an HTTP-only `DriverConfig` (base_url = `http://...`).
2. Call `validate()` — expect `Ok`.
3. Construct a native-only `DriverConfig` (base_url = `vng://...`, no http_fallback_url).
4. Call `validate()` — expect `Ok`.
5. Construct a dual-transport config (base_url = `vng://...`, http_fallback_url = `http://...`).
6. Call `validate()` — expect `Ok`.
7. Construct invalid config (empty base_url) — expect `Err`.

**Expected result:** All three valid driver configurations validate; invalid config is rejected.

**Test command:**
```
cargo test -p voltnuerongrid-driver-rust r17_driver_parity
```

**Status:** Pass (local)

---

## R-18 — Native local operation for small volumes

**Scenario name:** `R18-local-setup-ux`

**Pre-conditions:**
- `deploy/local/install.sh` present and executable
- Rust toolchain available

**Steps:**
1. Run `bash deploy/local/install.sh`.
2. Verify binary outputs to `target/release/voltnuerongridd`.
3. Start server with `VNG_LOG_LEVEL=info ./target/release/voltnuerongridd`.
4. Run `curl http://localhost:8080/health`.
5. Verify response `{"status":"ok"}` in under 200ms.

**Expected result:** Local installation completes in under 10 minutes on a laptop; health check passes.

**Test command:**
```
bash deploy/local/install.sh && curl http://localhost:8080/health
```

**Status:** Pass (local)

---

## Summary Table

| Req | Scenario | Status |
|-----|----------|--------|
| R-01 | ANSI SQL basic round-trip | Pass (local) |
| R-02 | DDL lifecycle | Pass (local) |
| R-03 | Function language registry | Pass (local) |
| R-04 | i18n/UTF-8 catalog | Pass (local) |
| R-05 | Storage/engine separation | Pass (local) |
| R-06 | Format ingestion (CSV/Parquet/Excel) | Pass (local) |
| R-07 | Multi-threaded import throughput | Deferred (cloud) |
| R-08 | Dual deployment smoke | Pass (local) / Deferred (cloud) |
| R-09 | Plugin SPI | Pass (local) |
| R-10 | Trillion-row scale claim | Deferred (cloud) |
| R-11 | Indexes and constraints | Pass (local) |
| R-12 | Trigger registration | Pass (local) / Deferred (S12 queue sinks) |
| R-13 | Join/paging stress | Pass (local) / Deferred (cloud) |
| R-14 | Function parity + UDF | Pass (local) |
| R-15 | RBAC integration | Pass (local) |
| R-16 | UI/client decoupling | Pass (local) |
| R-17 | Driver parity (3 configs) | Pass (local) |
| R-18 | Local setup UX | Pass (local) |
