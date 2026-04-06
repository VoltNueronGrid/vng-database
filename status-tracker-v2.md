# VoltNueronGrid DB — Status Tracker v2 (Architecture vs Codebase)

**Purpose:** Reconcile the **design architecture** (`reference/voltnuerongrid-db-design.md`, `reference/voltnuerongrid-ws.md`) with **what is actually implemented in Rust today**, call out **gaps** honestly, and provide a **sprint-oriented backlog** by workstream.  
**Companion:** `status_tracker.md` (REQ/WS narrative + evidence links).  
**Last updated:** 2026-04-05  

---

## 0) Answering “Only tests, no Rust?”

**No — development is not “tests only.”** The repository contains substantial production Rust:

| Location | Role | Notes |
|----------|------|--------|
| `services/voltnuerongridd/src/main.rs` | Axum HTTP service | **Very large** single binary: routes for SQL, ingest, store, security/KMS, SRE/DR, failover simulation, cache (Redis-compat HTTP), plugins, autonomous plane, drivers pool, benchmarks, etc. This *is* application development, not “just tests.” |
| `crates/voltnuerongrid-*` | Domain libraries | Multiple **non-trivial** crates (`auth`, `ingest`, `plugins`, `opt`, `store` modules, `sql`, `driver-rust`, …). |
| `tests/kpi/scripts/*.ps1` | Gate/smoke harness | **Integration/contract verification** around the Rust runtime; they do not replace the Rust code. |

**What is true:** much of the **data plane** described in the architecture (Raft metadata clusters, distributed OLTP/OLAP executor fleets, columnar zones, trillion-row proven scale) is **not implemented as separate long-running clusters** yet. A lot of behavior is **scaffolded** (in-memory structures, simulated handoffs, HTTP APIs, contracts) with **heavy test and gate coverage**. So: **lots of Rust + lots of tests**, but **not** “the full distributed HTAP engine as drawn in every diagram.”

---

## 1) Architecture (target) vs implementation (today) — summary

**Target (design doc):** cloud-native HTAP platform — gateways, control plane (catalog, raft, scheduler), data plane (routers, coordinators, OLTP executors, OLAP executors), storage, plugins, drivers, Studio UI, multi-cloud operator.

**Today (codebase):**

- **Implemented well (as scaffolding / single-node runtime):** HTTP API surface, RBAC matrix enforcement paths, ingest connectors (CSV/JSON/Parquet/Excel + chunked/async patterns), ingest outbox/event-bus abstractions, audit sink, plugin manifest lifecycle, distributed **cache manager** (`voltnuerongrid-opt`) + Redis-style HTTP command surface, driver Rust client contracts/pool simulation, SQL **analyze/route/execute** paths with HTAP **routing heuristics** (`HtapQueryRouter`), pessimistic lock scaffold, ACID registry/savepoints/isolation hooks (as coded in service), store **index + constraint** engines, WAL adapter + durability **engine** (in-memory + file adapters where wired), failover **simulation** and replication transport **abstractions**, many SRE endpoints, i18n catalog, benchmark endpoints, concurrency tests — all primarily **in-process** and **single-binary** oriented.

- **Thin or stub crates:** `voltnuerongrid-core`, `voltnuerongrid-meta`, `voltnuerongrid-failover` are **stubs** (crate name constants only). Failover **logic** largely lives in `voltnuerongridd` instead of the dedicated crate.

- **Major gaps vs design:** separate **metadata Raft service**, real **shard coordinators**, **vectorized OLAP execution engine** over columnar storage, durable **row-level MVCC** store matching Postgres-like semantics at scale, **native wire protocol** beyond HTTP for all clients, **Studio UI** in this repo, **production multi-node** consensus and storage, **formal ANSI conformance suite** driving pass/fail, **connector plugins** to real cloud sources beyond contracts, **TDE/TLS termination** as first-class server behavior (much is contract/simulation today), **game-day** automation tied to real clusters.

---

## 2) Crate inventory — maturity

Legend: **DONE** = fit for current scaffold scope; **PARTIAL** = real code, incomplete vs design; **STUB** = placeholder crate; **N/A** = not applicable.

| Crate / binary | Maturity | Summary |
|----------------|----------|---------|
| `voltnuerongrid-core` | **STUB** | Constant only; no shared types. **Gap:** extract shared error/types from monolith or delete stub policy. |
| `voltnuerongrid-meta` | **STUB** | Constant only. **Gap:** real catalog service or merge into store. |
| `voltnuerongrid-failover` | **STUB** | Constant only; failover logic in daemon. **Gap:** library-ize failover domain. |
| `voltnuerongrid-sql` | **PARTIAL** | Analyzer/classifier, i18n, function registry contracts, legacy aggregation eval, tests. **Gap:** real parser; planner/optimizer; conformance suite. |
| `voltnuerongrid-exec` | **PARTIAL** | `HtapQueryRouter` heuristic routing only. **Gap:** cost model; planner integration; executor integration. |
| `voltnuerongrid-store` | **PARTIAL** | WAL/checkpoint **in-memory** engine, file WAL adapter, indexes, constraints, DDL catalog module, htap sync scaffolding. **Gap:** on-disk row store + MVCC + page cache; columnar; recovery story at scale. |
| `voltnuerongrid-ingest` | **PARTIAL → strong scaffold** | CSV/JSON/Parquet/Excel, registry, event bus/replay, batch/parallel config, chunked loader (per tracker evolution). **Gap:** enterprise connectors; exactly-once to durable tables; schema evolution. |
| `voltnuerongrid-opt` | **PARTIAL** | Distributed cache manager, eviction, circuit breaker patterns. **Gap:** cluster-wide consistency; integration with real commit path. |
| `voltnuerongrid-auth` | **PARTIAL** | Security config, KMS adapters surface, RBAC matrix. **Gap:** full TLS/mTLS server stack; production KMS HSM paths. |
| `voltnuerongrid-audit` | **PARTIAL** | Append-only in-memory sink; small. **Gap:** durable audit store; tamper evidence. |
| `voltnuerongrid-plugins` | **PARTIAL** | Manifest signing, lifecycle, provenance types. **Gap:** sandboxed plugin runtime loading; connector SDK execution. |
| `voltnuerongrid-ai` | **PARTIAL** | Small serde types for autonomous records. **Gap:** model gateway, tool execution boundaries. |
| `voltnuerongrid-driver-rust` | **PARTIAL** | Client config, pool simulation. **Gap:** align with final wire protocol; non-HTTP transport. |
| `voltnuerongridd` | **PARTIAL (broad)** | Integrates almost everything via HTTP. **Gap:** split modules/crates; reduce monolith; implement real data plane. |
| `voltnuerongrid-audit-companion` | **PARTIAL** | CLI merge/report. **Gap:** parity with enterprise audit requirements. |

---

## 3) Cross-cutting gap list (prioritized)

These are **product/engine gaps**, not test gaps.

| ID | Gap | Why it matters | Suggested owner |
|----|-----|----------------|-----------------|
| G-01 | **Monolithic `main.rs`** | Hard to evolve, review, and reuse; hides “services” that design doc shows as separate. | Platform |
| G-02 | **No real distributed control plane** | Design: Raft metadata, schedulers. Code: mostly single-process HTTP. | Distributed |
| G-03 | **OLAP path not a columnar engine** | Routing can label OLAP; execution is not MPP/columnar at scale. | Query |
| G-04 | **OLTP path not durable MVCC row store** | Store crate has primitives; not a full transactional storage engine. | Storage |
| G-05 | **SQL layer is not a full parser/planner** | Analyzer/heuristics + strings; not ANSI conformance driver. | SQL |
| G-06 | **Wire protocol** | Design implies drivers + protocol; primary integration is HTTP JSON in tree. | Integrations |
| G-07 | **Studio UI not in workspace** | Design references `voltnuerongrid-studio` (separate). | UX |
| G-08 | **Connector plugins to real cloud** | Contracts exist; universal secure credential + data movement incomplete. | Ingestion |
| G-09 | **Scale proof** | “Trillion rows” needs benchmarks on real storage + cluster. | Perf |
| G-10 | **Stub crates** | `core`/`meta`/`failover` do not match modular architecture story. | Platform |

---

## 4) Sprint model (2-week sprints, design-aligned)

**Naming:** `Sprint N` = a 2-week increment; dates are **TBD** — adjust to your calendar.  
**Statuses:** `DONE`, `IN_PROGRESS`, `TODO`, `DEFERRED`.

---

### Sprint 1 — Baseline honesty + engineering hygiene

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S1-WS0-01 | WS0 | CI: `cargo check` / `cargo test` stable on Windows/Linux | **DONE** | Per `ci.yml` evolution |
| S1-WS0-02 | WS0 | Gate script conventions + HTTP helper for PS7 | **DONE** | `kpi-http-helpers.ps1`, instructions |
| S1-WS0-03 | WS0 | Document “scaffold vs engine” in tracker | **DONE** | This file + `status_tracker.md` |
| S1-GAP-01 | Platform | Decision: delete vs populate `core`/`meta`/`failover` stubs | **DONE** | G-10: Decision is **keep stubs as-is** (named-constant + `#![forbid(unsafe_code)]`) until a deliberate extraction sprint is planned; policy documented in `copilot-instructions.md` and `status-tracker-v2.md` section 2. No logic may be added to the three stub crates. |

---

### Sprint 2 — Storage engine: from scaffold toward real OLTP

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S2-WS2-01 | WS2 | B-tree index + constraint manager | **DONE** (scaffold) | `voltnuerongrid-store` + HTTP |
| S2-WS2-02 | WS2 | WAL + checkpoint **in-memory** + file adapter patterns | **PARTIAL** | Not full recovery at scale |
| S2-WS2-03 | WS2 | DDL catalog wired through `sql_execute` | **DONE** (per tracker narrative) | Verify in tree |
| S2-WS2-04 | WS2 | **Page-based row store + MVCC** | **PARTIAL** | **2026-04-12:** `crates/voltnuerongrid-store/src/mvcc.rs` — `PagedRowStore` with `RowVersion`/`MvccRow`/`StorePage` structs; snapshot-read visibility rule (`visible_at(snapshot_xid)`): latest version with `xid ≤ snapshot_xid` that is not a tombstone; `insert()` / `delete()` / `read_at_snapshot()` / `read_latest()` / `scan_at_snapshot()` / `visible_row_count()` API; automatic page split when page reaches `page_size`; `begin_xid()` + `current_xid()` for Xid allocation; 8 unit tests in `mvcc::tests` pass; `PagedRowStore` wired into `AppState.row_store` field (exposed to all handlers). Next: on-disk page serialisation + recovery. |
| S2-WS2-05 | WS2 | Integration: transactions commit ↔ store visibility | **PARTIAL** | `AcidTransactionRegistry` tracks `read_snapshot_at_ms` per transaction; `PagedRowStore::read_at_snapshot(snapshot_xid)` implements the visibility protocol; 3 service-level integration tests `s2_ws2_mvcc_*` verify insert/snapshot/delete behaviour through `AppState.row_store`. **2026-04-05:** COMMIT path in `sql_transaction` now calls `extract_insert_row_from_sql()` (uses AST tokenizer) on each statement and flushes extracted rows into `PagedRowStore` with a new `xid`; 1 integration test `s2_ws2_commit_flush_writes_inserts_to_row_store` confirms two-row flush. Remaining TODO: full DML replay (UPDATE/DELETE), conflict-aware xid management. **Session 7:** Write-intent lock table added (`write_intents: HashMap<String,Xid>` on `PagedRowStore`); `begin_write_intent()`/`release_write_intents()`/`was_modified_after()` methods; COMMIT path performs write-write conflict detection — 409 returned when `was_modified_after(key, snapshot_xid_at_begin)` is true; 3 new store unit tests + 2 integration tests `s2_ws2_05_*`. |

---

### Sprint 3 — SQL: parser, planner, conformance

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S3-WS1-01 | WS1 | Statement classifier + `SqlAnalyzer` | **DONE** (partial) | Heuristic/string based |
| S3-WS1-02 | WS1 | `/sql/analyze`, `/sql/route`, `/sql/execute`, `/sql/transaction` + RBAC | **DONE** (scaffold) | `voltnuerongridd` |
| S3-WS1-03 | WS1 | UDF scaffold + contracts in execute path | **DONE** (scaffold) | Not full WASM/JS/Py sandbox in prod sense |
| S3-WS1-04 | WS1 | **Real SQL tokenizer + AST parser (ANSI subset)** | **PARTIAL** | **2026-04-12:** Tokenizer in `crates/voltnuerongrid-sql/src/tokenizer.rs`. **2026-04-05:** Recursive-descent SQL AST parser in `crates/voltnuerongrid-sql/src/ast.rs` — `Statement` enum (Select/Insert/Update/Delete/CreateTable/Begin/Commit/Rollback/Unknown); `SelectStatement` (columns, table, where_clause, group_by, having, order_by, limit); `InsertStatement` (table, columns, multi-row values); `UpdateStatement` (table, assignments, where); `DeleteStatement` (table, where); `CreateTableStatement` (table, column defs); `OrderByClause` (column, descending). `parse_one(sql) -> Result<Statement, String>` entry point; 14 unit tests in `ast::tests` pass. All types re-exported from `voltnuerongrid-sql` crate root. Next step: planner integration / cost model on top of AST. |
| S3-WS1-05 | WS1 | **Planner/optimizer + cost model** | **PARTIAL** | **2026-04-05:** `LogicalPlan` enum (Scan/Project/Filter/Aggregate/Sort/Limit/Insert/Update/Delete/CreateTable/Begin/Commit/Rollback/Unknown) + `CostEstimate` + `QueryPlanner` created in `crates/voltnuerongrid-exec/src/planner.rs`; `plan()` converts AST → logical tree (SELECT chain: Scan→Filter→Aggregate→Sort→Limit→Project); `estimate_cost()` produces OLTP/OLAP/Hybrid routing hints; `QueryPath` enum on `AppState` routing call sites. 20 unit tests in exec crate; 2 service-level integration tests `s3_ws1_planner_*` verify OLAP/OLTP routing. **2026-04-05 (session 4):** Planner cost now wired into `sql_route` response — each `RoutedStatementResponse` gains `planner_path`, `estimated_rows`, `relative_cost`; `SqlRouteResponse` gains `batch_estimated_rows` + `batch_relative_cost`; 2 new integration tests `s3_ws1_sql_route_*` verify the aggregate→olap and filter→oltp routing. `sql_execute` response gains `planner_path: Option<String>` (dominant cost path across batch); 1 integration test `s3_ws1_sql_execute_planner_path_populated_for_aggregate` verifies. Next: wire planner into physical executor; use cost to select join/scan strategies. |
| S3-WS1-06 | WS1 | **ANSI conformance harness gated in CI** | **PARTIAL** | **2026-04-05 (session 4):** `ansi_conformance` test module added to `crates/voltnuerongrid-sql/src/ast.rs` — 17 test cases covering: SELECT with aliases/DISTINCT, multi-column GROUP BY+HAVING, ORDER BY multi-col, LIMIT, WHERE AND/OR, WHERE numeric, INSERT single/multi-row, UPDATE multi-assign/no-WHERE, DELETE with/without WHERE, CREATE TABLE types, CREATE TABLE IF NOT EXISTS (graceful Unknown), transaction control (BEGIN TRANSACTION / COMMIT WORK / ROLLBACK WORK), unsupported DDL → Unknown (ALTER/DROP/CREATE INDEX/TRUNCATE/GRANT). Gate script `tests/kpi/scripts/run-ws1-ansi-conformance-smoke.ps1` runs `cargo test -p voltnuerongrid-sql ansi_conformance` and emits `tests/kpi/results/ws1/ansi-conformance-smoke.json` (**passed**, 17 conformance tests, 81 total sql-crate tests). **2026-04-05 (session 6):** `run-ws1-gate.ps1` now includes `ws1-ansi-conformance` pack (cargo test `ansi_conformance` filter) as the second pack after `ws1-sql-core-tests` — S3-WS1-06 now gated in the CI gate orchestrator. Next: hook into `run-ws1-gate.ps1`; add SQL-99 JOIN / subquery patterns as parser improves. |

---

### Sprint 4 — HTAP routing + OLAP execution

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S4-WS3-01 | WS3 | `HtapQueryRouter` + `ws3_*` tests | **DONE** (heuristic) | `voltnuerongrid-exec`; **2026-04-12:** extended with `OVER(` (window functions) + `HAVING` clause detection as OLAP patterns; 18 `ws3_*` tests pass. |
| S4-WS3-02 | WS3 | OLTP vs OLAP **execute** separation in service | **PARTIAL** | Routed; execution simplified. **2026-04-05 (session 4):** `sql_execute` now computes dominant `planner_path` (most expensive statement wins) and returns it in `SqlExecuteResponse.planner_path`; handler uses this alongside `HtapQueryRouter` path to inform future physical executor dispatch. **2026-04-05 (session 5):** Physical OLTP executor dispatch added — when `planner_path == "oltp"`, `sql_execute` calls `execute_oltp_select()` which reads committed rows from `PagedRowStore::scan_at_snapshot()`, applies optional WHERE-clause prefix filter, caps at `max_rows` limit, and returns `oltp_rows: Option<Vec<OltpRowResult>>` in the response; `OltpRowResult { key, data }` carries actual MVCC row data; 2 integration tests `s4_ws3_sql_execute_oltp_*` verify point-read returns real rows and aggregate has no oltp_rows. Next: route to vectorized columnar executor for OLAP path. |
| S4-WS3-03 | WS3 | **Columnar store + vectorized operators** | **PARTIAL** | **2026-04-05 (session 6):** `crates/voltnuerongrid-store/src/columnar.rs` added — `ColumnVector` enum (Int64/Float64/Bool/Utf8/Null), `ColumnBatch` with named typed columns + row_keys, `ColumnBatchBuilder` with automatic type inference (i64→f64→bool→Utf8), `vectorized_scan(rows, limit) -> (ColumnBatch, VectorizedScanStats)`; `GET /api/v1/store/columnar/scan` endpoint materialises committed rows via `PagedRowStore::scan_at_snapshot()` into typed column batches and returns `ColumnarScanResponse {rows_scanned, columns_materialized, elapsed_us, columns[{name,type_hint,row_count,sample_values}]}`; 8 unit tests in `columnar::tests` (row count, type inference, limit, empty); 2 service integration tests `s4_ws3_03_*` verify typed column materialisation and empty-store handling. **Session 7:** Vectorized aggregation added — `VectorizedAggOp{Sum,Count,Avg,Min,Max}`, `AggResult{op,value,row_count}`, `aggregate_column(col, op)->AggResult`, `aggregate_batch(batch, ops)->HashMap<String,AggResult>`; 6 new unit tests in `columnar::tests`. Gap: pushdown predicates; columnar storage layout. |
| S4-WS3-04 | WS3 | **Freshness / sync from OLTP → OLAP** | **PARTIAL** | `htap_sync` scaffold exists. **2026-04-05 (session 5):** COMMIT path in `sql_transaction` now publishes each INSERT/UPDATE/DELETE mutation to `RowStoreSyncOrigin` (table=`"row_store"`, primary_key=extracted key, payload_json=raw SQL); new `POST /api/v1/store/htap/export` endpoint returns pending mutations since a given sequence via `StoreHtapExportResponse { status, since_sequence, mutation_count, checkpoint_last_sequence, mutations[] }`; 2 integration tests `s4_ws3_04_*` verify pending_len grows after COMMIT and export returns mutations with op=`"insert"`. **Session 7:** OLAP consumer side added — `olap_store: Arc<Mutex<HashMap<String,HashMap<String,String>>>>` field in AppState; `POST /api/v1/store/htap/apply` endpoint applies insert/update/delete mutations to olap_store; `GET /api/v1/store/htap/olap/scan` returns all olap_store rows; 2 integration tests `s4_ws3_04_htap_apply_*`. |
| S4-WS3-05 | WS3 | Performance gates + trend artifacts | **DONE** (KPI harness) | Not same as prod scale proof |

---

### Sprint 5 — Ingestion + streaming + connectors

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S5-WS4-01 | WS4 | CSV/JSON/Parquet/Excel connectors + HTTP | **DONE** (scaffold) | Base64 for binary |
| S5-WS4-02 | WS4 | Chunked ingest HTTP + async `spawn_blocking` fan-out | **DONE** (scaffold) | Throughput vs real storage still **TODO** (S5-WS4-03) |
| S5-WS4-03 | WS4 | Ingest → **durable typed tables** (not only in-memory maps) | **PARTIAL** | **2026-04-05:** `ingest_csv` and `ingest_json` handlers now write each `IngestRecord` into `PagedRowStore` (field `source: "csv:{connector_id}"` / `"json:{connector_id}"`, field `payload: record.payload`) before the existing in-memory map is updated; 2 integration tests `s5_ws4_row_store_receives_ingest_style_writes` confirm the pattern. **2026-04-05 (continued):** `ingest_parquet` and `ingest_excel` handlers now also write to `PagedRowStore` (same pattern, `source: "parquet:{connector_id}"` / `"excel:{connector_id}"`) — all four ingest formats now write to durable store. **2026-04-05 (session 4):** `POST /api/v1/store/rows/scan` endpoint added — `StoreRowsScanRequest` accepts `snapshot_xid`, `key_prefix`, `limit` (default 1 000, max 10 000); calls `PagedRowStore::scan_at_snapshot()`; requires `store` runtime principal; returns `StoreRowsScanResponse {status, snapshot_xid, row_count, rows}`; 3 new integration tests `s5_ws4_store_rows_scan_*` verify committed-row visibility, prefix filtering, and limit cap. |
| S5-WS4A-01 | WS4A | Outbox + replay + WAL-backed cursors | **DONE** (scaffold) | `voltnuerongrid-ingest` |
| S5-WS4A-02 | WS4A | **Kafka/NATS/Event Hubs live e2e** | **TODO** | Adapters exist; prod hardening |
| S5-E4A-01 | Epic 4A | Connector SDK **runtime load** | **TODO** | G-08 |

---

### Sprint 6 — Security, RBAC, KMS, TLS

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S6-WS5-01 | WS5 | RBAC matrix + runtime principal enforcement | **DONE** (broad) | Across handlers |
| S6-WS5-02 | WS5 | KMS status/outage simulation endpoints | **DONE** (scaffold) | Adapters for local/cloud CLI |
| S6-WS5-03 | WS5 | **TLS/mTLS termination + cert rotation** | **PARTIAL** | **2026-04-05 (session 6):** `GET /api/v1/security/tls/status` endpoint added — reads `SecurityConfigContract.tls_required` + `mtls_required`; reports `cert_source` from `VNG_TLS_CERT_PATH` env var; `cert_rotation_supported: false` (scaffold); requires `security.kms` Read privilege; 1 integration test `s6_ws5_03_tls_status_returns_contract_flags` verifies default dev config (tls=false/mtls=false). `security.kms` privilege scopes expanded to include `security/tls/status` + `security/tde/status` for DBA/Security/SRE. Gap: actual rustls/native-tls axum-server adapter; cert hot-reload via `VNG_TLS_CERT_PATH`/`VNG_TLS_KEY_PATH`. |
| S6-WS5-04 | WS5 | **TDE for data at rest in engine** | **PARTIAL** | **2026-04-05 (session 6):** `GET /api/v1/security/tde/status` endpoint added — reads `SecurityConfigContract.encryption_at_rest_required`; reports `tde_active` (true when `encryption_at_rest_required && KMS key env var resolved); reports `key_env_var` from `kms_key_ref_env`; requires `security.kms` Read privilege; 1 integration test `s6_ws5_04_tde_status_reports_encryption_at_rest_required` verifies default config (encryption_at_rest_required=true, tde_active=false in test env). Gap: actual AES-128-CTR page encryption in PagedRowStore; KMS-backed key wrapping; key rotation hooks. |

---

### Sprint 7 — HA/FT, distributed control plane

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S7-WS6-01 | WS6 | Failover simulation + handoff report + transport abstraction | **DONE** (scaffold) | In daemon |
| S7-WS6-02 | WS6 | **Raft / quorum metadata service** | **PARTIAL** | **Session 7:** `services/voltnuerongridd/src/raft.rs` created with `RaftRole{Follower,Candidate,Leader}`, `RaftLogEntry{index,term,command}`, `RaftVoteRequest/Response`, `RaftAppendRequest/Response`, `RaftStatusSnapshot`, `RaftNode` with `handle_vote_request()`/`handle_append_entries()`/`become_candidate/leader/follower()`/`status()`/`last_log_position()` methods; `raft_state: Arc<Mutex<RaftNode>>` field added to AppState; `GET /api/v1/cluster/raft/status`, `POST /api/v1/cluster/raft/vote`, `POST /api/v1/cluster/raft/append` endpoints registered; 6 unit tests in `raft::tests` + 3 integration tests `s7_ws6_02_*`. Gap: network election timer/heartbeat, actual multi-node coordination. |
| S7-WS6-03 | WS6 | **Automatic leader election + fencing** | **TODO** | Design non-negotiables |
| S7-WS6-04 | WS6 | Chaos/game-day automation vs **real** cluster | **TODO** | G-09 |

---

### Sprint 8 — Drivers, Studio, IDE, SaaS deploy

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S8-WS10-01 | WS10 | Rust driver contracts + pool behaviors | **DONE** (scaffold) | HTTP-oriented |
| S8-WS10-02 | WS10 | **Stable wire protocol + multi-language SDKs** | **TODO** | G-06 |
| S8-WS9-01 | WS9 | Studio API contract scripts | **DONE** (harness) | UI separate repo |
| S8-WS9A-01 | WS9A | IDE adapter manifests + smoke | **DONE** (harness) | |
| S8-REQ08-01 | REQ-08 | Deploy profile smoke (files/Helm) | **DONE** (per tracker) | Live cloud **DEFERRED** (credentials) |

---

### Sprint 9 — Autonomous + AI + audit hardening

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S9-WS8-01 | WS8 | Autonomous records, guardrails, emergency stop | **DONE** (scaffold) | |
| S9-WS8-02 | WS8 | **Production policy + model gateway isolation** | **PARTIAL** | **2026-04-05 (session 6):** `ModelGatewayPolicy { isolation_enabled, allowed_models, max_tokens_per_request, rate_limit_rpm }` added to `AppState` (default: isolation=true, 4096 tokens, 60 rpm); `GET /api/v1/ai/policy` returns current policy (requires `ai.governance` Read); `POST /api/v1/ai/policy/update` updates all fields atomically (requires `ai.governance` Manage); `ai.governance` resource added to privilege matrix for DBA/Security (Manage+Read) and AiOperator (Read); 2 integration tests `s9_ws8_02_*` verify default isolation=true and update persists new values. Gap: actual model-identity header enforcement on AI action endpoints; rate-limiter middleware. |
| S9-WS8A-01 | WS8A | Audit companion CLI | **PARTIAL** | |
| S9-WS8A-02 | WS8A | **Durable tamper-evident audit chain** | **PARTIAL** | **2026-04-05 (session 5):** `AuditEvent` gains `chain_hash: String`; `AppendOnlyAuditSink` gains `prev_chain_hash: String` (seeded with genesis constant) and computes `chain_hash = fnv1a_64(prev_hash|event_id|actor|action|outcome|details_json)` for each appended event; `AppendOnlyAuditSink::verify_chain(events)` static method recomputes chain from genesis and returns `false` on any tamper; `is_empty()` + `all()` accessors added; 5 new tests in `voltnuerongrid-audit` (`chain_hashes_non_empty_deterministic`, `verify_chain_clean`, `verify_chain_tampered`, `verify_chain_empty`); `GET /api/v1/audit/chain/verify` endpoint returns `{ chain_valid: bool, event_count, genesis_hash }` (requires audit Read privilege); 2 service-level integration tests `s9_ws8a_02_*` verify clean chain valid + events have non-empty hashes. **Session 7:** File-backed durability added — `audit_log_path: Option<String>` field in AppState (driven by env var `VNG_AUDIT_LOG_PATH`); `append_audit_event()` and `append_runtime_audit_event()` write JSON-line events to file when path is set; `GET /api/v1/audit/export` returns in-memory+file-backed status with `file_backed` flag and `audit_log_path`; `audit.read` resource scope added to DBA/Security privilege matrix; 1 integration test `s9_ws8a_02_audit_export_returns_buffered_events`. |

---

### Sprint 10 — Competitive / Epic 15 (optional track)

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S10-WS15-01 | WS15 | Competitive matrix + backlog scoring | **DONE** (harness) | `reference/competitive/*` |
| S10-WS15-02 | WS15 | **CDC, follower reads, vector/graph** | **TODO** | Design competitive row |

---

## 5) Workstream (WS) rollup — completion snapshot

Use this as an **executive rollup**. “DONE” here means **done for current scaffold scope**, not “done vs full design doc.”

| WS | Done in codebase today | Main remaining gap |
|----|-------------------------|--------------------|
| WS0 | CI, gates, scripts, workspace | Modularize monolith |
| WS1 | Analyzer/execute/transaction/UDF scaffold; AST parser; logical planner wired into sql_route + sql_execute; ANSI conformance now gated in `run-ws1-gate.ps1` (S3-WS1-06) | Physical executor + planner-driven scan/join strategies |
| WS1A | Legacy agg eval + parity scripts (Ready for Validation) | Full MDAP parity operators (P0–P2) |
| WS2 | Indexes, constraints, WAL patterns | Real on-disk row store + MVCC |
| WS2A | Sync-origin scaffold | End-to-end HTAP freshness |
| WS3 | Routing heuristics + `OVER`/`HAVING` detection, perf harness; physical OLTP executor dispatch (point SELECT → PagedRowStore); HTAP sync origin wired to COMMIT path; **columnar batch layout** (`ColumnBatch`/`ColumnVector`/`vectorized_scan`) + `GET /api/v1/store/columnar/scan` | Columnar aggregation operators; vectorized execution engine; OLAP consumer for sync mutations |
| WS4 | Multi-format ingest, chunked (Ready for Validation) | Durable table load |
| WS4A | Outbox/replay (Ready for Validation) | External broker hardening |
| WS5 | RBAC, KMS simulation; **TLS status endpoint** (`GET /api/v1/security/tls/status`); **TDE status endpoint** (`GET /api/v1/security/tde/status`) | Actual rustls/TDE page encryption; cert rotation |
| WS6 | Simulated failover narrative | Real quorum/fencing |
| WS7 | Plugin manifest security | Plugin execution sandbox |
| WS8 | Autonomous API scaffold; **model gateway policy** (`ModelGatewayPolicy` in AppState, `GET/POST /api/v1/ai/policy`) | Actual model-identity enforcement on action endpoints; rate-limiter middleware |
| WS8A | Audit types/companion | Durable audit |
| WS9 | API contract checks | Studio product |
| WS9A | IDE manifests | Extension features |
| WS10 | Rust driver scaffold | Wire protocol |
| WS11 | i18n endpoint/catalog | Collation/UTF-8 depth |
| WS12 | SRE/DR endpoints | Real cluster automation |
| WS13 | Cloud profile files/smoke | Live cloud |
| WS14 | Config schema gates | Central config service |
| WS15 | Matrix/backlog artifacts | Feature implementation |

---

## 6) Is “all development completed?”

**No.**

- **A large amount of *integration and control-plane scaffolding* is implemented in Rust** (especially `voltnuerongridd`) **with strong automated verification** (unit tests + KPI gates).  
- **The core database engine capabilities described in the architecture** (distributed metadata, durable MVCC row store at scale, columnar OLAP execution, universal wire protocol, full SQL conformance, production security termination, trillion-row proof) are **largely not DONE**.

Use **Section 3 (gaps)** + **Sprints 2–4** as the critical path toward a “real database” under the design doc, not more smoke tests alone.
**2026-04-05 progress note:** AST parser added (`crates/voltnuerongrid-sql/src/ast.rs`, 14 unit tests, `parse_one()` entry point, all types re-exported); S5-WS4-03 advanced TODO→PARTIAL (ingest_csv/ingest_json now write to PagedRowStore); S2-WS2-05 further advanced (COMMIT path flushes INSERT rows to PagedRowStore via `extract_insert_row_from_sql()`). Total `cargo test` count: **176 passing in voltnuerongridd** (+6 new integration tests); **48 passing in voltnuerongrid-sql** (+14 AST parser tests).

**2026-04-05 (session 6) progress note:** S4-WS3-03 advanced TODO→PARTIAL (columnar.rs module: `ColumnBatch`/`ColumnVector`/`ColumnBatchBuilder`/`vectorized_scan()`, 8 unit tests; `GET /api/v1/store/columnar/scan` endpoint, 2 service tests `s4_ws3_03_*`); S3-WS1-06 gate hookup complete (`ws1-ansi-conformance` pack added to `run-ws1-gate.ps1`); S6-WS5-03 advanced TODO→PARTIAL (`GET /api/v1/security/tls/status` endpoint + privilege matrix scopes, 1 test); S6-WS5-04 advanced TODO→PARTIAL (`GET /api/v1/security/tde/status` endpoint, 1 test); S9-WS8-02 advanced TODO→PARTIAL (`ModelGatewayPolicy` in AppState, `GET /api/v1/ai/policy` + `POST /api/v1/ai/policy/update` endpoints, `ai.governance` privilege matrix, 2 tests). Total `cargo test`: **198 passing in voltnuerongridd** (+6 new tests), **55 passing in voltnuerongrid-store** (+8 columnar tests), all other crates unchanged.

**2026-04-05 (session 5) progress note:** S4-WS3-02 advanced (physical OLTP executor dispatch: `execute_oltp_select()` reads committed rows from `PagedRowStore`, `oltp_rows` field in `SqlExecuteResponse`); S4-WS3-04 advanced TODO→PARTIAL (COMMIT publishes to `RowStoreSyncOrigin`, `POST /api/v1/store/htap/export` endpoint); S9-WS8A-02 advanced TODO→PARTIAL (FNV-1a chain hashing in `AppendOnlyAuditSink`, `verify_chain()`, `GET /api/v1/audit/chain/verify`). Total `cargo test`: **192 passing in voltnuerongridd** (+6 new tests), **81 passing in voltnuerongrid-sql** (unchanged), **20 passing in voltnuerongrid-exec** (unchanged), **6 passing in voltnuerongrid-audit** (+5 chain tests).

**2026-04-05 (session 4) progress note:** S3-WS1-05 further advanced (planner cost wired into `sql_route` per-statement response + `sql_execute` dominant path; `RoutedStatementResponse` gains `planner_path`/`estimated_rows`/`relative_cost`; `SqlRouteResponse` gains `batch_estimated_rows`/`batch_relative_cost`; `SqlExecuteResponse` gains `planner_path: Option<String>`); S3-WS1-06 advanced TODO→PARTIAL (17 ANSI conformance tests in `ast::ansi_conformance`, gate script `run-ws1-ansi-conformance-smoke.ps1` **passed**, artifact `tests/kpi/results/ws1/ansi-conformance-smoke.json`); S5-WS4-03 advanced (scan endpoint `POST /api/v1/store/rows/scan` added, 3 integration tests); S4-WS3-02 updated (planner_path in execute response). Total `cargo test` count: **186 passing in voltnuerongridd** (+6), **81 passing in voltnuerongrid-sql** (+33 conformance), **20 passing in voltnuerongrid-exec** (unchanged).

**2026-04-05 (session 3) progress note:** S3-WS1-05 advanced TODO→PARTIAL (`LogicalPlan` tree + `CostEstimate` + `QueryPlanner` in `crates/voltnuerongrid-exec/src/planner.rs`, 20 unit tests, OLTP/OLAP routing hints); S5-WS4-03 extended (ingest_parquet + ingest_excel handlers now also write to PagedRowStore — all 4 formats covered); S2-WS2-05 extended (UPDATE/DELETE DML now flushed at COMMIT via `extract_update_row_from_sql()` / `extract_delete_key_from_sql()`). 4 new integration tests in `voltnuerongridd`. Total `cargo test` count: **180 passing in voltnuerongridd** (+4), **20 passing in voltnuerongrid-exec** (+20 new crate).

**2026-04-12 progress note:** Sprint 2 (S2-WS2-04, S2-WS2-05) and Sprint 3 (S3-WS1-04) have advanced from TODO → PARTIAL with the addition of `PagedRowStore` (MVCC) and the SQL `Tokenizer` modules. Sprint 1 gap S1-GAP-01 (stub-crate decision) is now DONE. Total `cargo test` count: **170 passing** across all crates.

**2026-04-05 (session 7) progress note:** S2-WS2-05 extended (write-intent lock table: `PagedRowStore.write_intents`, `begin_write_intent()`/`release_write_intents()`/`was_modified_after()` methods; COMMIT path in `sql_transaction` now performs write-write conflict detection — returns 409 on conflict); S4-WS3-03 extended (vectorized aggregation: `VectorizedAggOp{Sum,Count,Avg,Min,Max}`, `AggResult`, `aggregate_column()`, `aggregate_batch()` added to `crates/voltnuerongrid-store/src/columnar.rs`); S4-WS3-04 extended (`POST /api/v1/store/htap/apply` + `GET /api/v1/store/htap/olap/scan` endpoints; `olap_store: Arc<Mutex<HashMap>>` field in AppState); S9-WS8A-02 extended (file-backed audit log: `audit_log_path` field in AppState driven by `VNG_AUDIT_LOG_PATH`, `append_audit_event()` writes JSON-lines; `GET /api/v1/audit/export` endpoint); S7-WS6-02 advanced TODO→PARTIAL (Raft consensus scaffold: `services/voltnuerongridd/src/raft.rs` with `RaftNode`/`RaftRole`/`RaftLogEntry`/vote+append+status RPCs; `GET /api/v1/cluster/raft/status`, `POST /api/v1/cluster/raft/vote`, `POST /api/v1/cluster/raft/append` endpoints; `raft_state: Arc<Mutex<RaftNode>>` in AppState). Store crate: **64 passing** (+9 new: 3 write-intent, 6 aggregation). Service: **212 tests total** (+14 new integration tests).
---

## 7) How to maintain this file

- After each sprint: flip tasks `TODO` → `DONE`, add discovered gaps.  
- Link evidence: `tests/kpi/results/...` and `cargo test -p ...` commands.  
- When stub crates are removed or implemented, update **Section 2** and **G-10**.  

---

*Generated for planning; aligns with `reference/voltnuerongrid-db-design.md` and `reference/voltnuerongrid-ws.md` checklist themes.*
