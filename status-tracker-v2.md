# VoltNueronGrid DB — Status Tracker v2 (Architecture vs Codebase)

**Purpose:** Reconcile the **design architecture** (`reference/voltnuerongrid-db-design.md`, `reference/voltnuerongrid-ws.md`) with **what is actually implemented in Rust today**, call out **gaps** honestly, and provide a **sprint-oriented backlog** by workstream.  
**Companion:** `status_tracker.md` (REQ/WS narrative + evidence links).  
**Last updated:** 2026-04-12  

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
| S2-WS2-05 | WS2 | Integration: transactions commit ↔ store visibility | **PARTIAL** | `AcidTransactionRegistry` tracks `read_snapshot_at_ms` per transaction; `PagedRowStore::read_at_snapshot(snapshot_xid)` implements the visibility protocol; 3 service-level integration tests `s2_ws2_mvcc_*` verify insert/snapshot/delete behaviour through `AppState.row_store`. Full commit-path integration (routing `COMMIT` → `PagedRowStore`) is **TODO**. |

---

### Sprint 3 — SQL: parser, planner, conformance

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S3-WS1-01 | WS1 | Statement classifier + `SqlAnalyzer` | **DONE** (partial) | Heuristic/string based |
| S3-WS1-02 | WS1 | `/sql/analyze`, `/sql/route`, `/sql/execute`, `/sql/transaction` + RBAC | **DONE** (scaffold) | `voltnuerongridd` |
| S3-WS1-03 | WS1 | UDF scaffold + contracts in execute path | **DONE** (scaffold) | Not full WASM/JS/Py sandbox in prod sense |
| S3-WS1-04 | WS1 | **Real SQL tokenizer (ANSI subset)** | **PARTIAL** | **2026-04-12:** `crates/voltnuerongrid-sql/src/tokenizer.rs` — `Token` enum (Keyword/Identifier/Number/StringLiteral/Symbol/LineComment/BlockComment/Unknown), `tokenize()` + `semantic_tokens()` + `keyword_count()` public API; handles quoted identifiers, escaped string literals, two-char symbols (`>=`, `<>`, `!=`, `::`), block + line comments, numeric literals with exponent notation; 11 unit tests in `tokenizer::tests` pass — exported from `voltnuerongrid-sql` crate root. Next step: proper recursive-descent parser producing an AST. |
| S3-WS1-05 | WS1 | **Planner/optimizer + cost model** | **TODO** | G-05 |
| S3-WS1-06 | WS1 | **ANSI conformance harness gated in CI** | **TODO** | Design checklist |

---

### Sprint 4 — HTAP routing + OLAP execution

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S4-WS3-01 | WS3 | `HtapQueryRouter` + `ws3_*` tests | **DONE** (heuristic) | `voltnuerongrid-exec`; **2026-04-12:** extended with `OVER(` (window functions) + `HAVING` clause detection as OLAP patterns; 18 `ws3_*` tests pass. |
| S4-WS3-02 | WS3 | OLTP vs OLAP **execute** separation in service | **PARTIAL** | Routed; execution simplified |
| S4-WS3-03 | WS3 | **Columnar store + vectorized operators** | **TODO** | G-03 |
| S4-WS3-04 | WS3 | **Freshness / sync from OLTP → OLAP** | **PARTIAL** | `htap_sync` scaffold exists |
| S4-WS3-05 | WS3 | Performance gates + trend artifacts | **DONE** (KPI harness) | Not same as prod scale proof |

---

### Sprint 5 — Ingestion + streaming + connectors

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S5-WS4-01 | WS4 | CSV/JSON/Parquet/Excel connectors + HTTP | **DONE** (scaffold) | Base64 for binary |
| S5-WS4-02 | WS4 | Chunked ingest HTTP + async `spawn_blocking` fan-out | **DONE** (scaffold) | Throughput vs real storage still **TODO** (S5-WS4-03) |
| S5-WS4-03 | WS4 | Ingest → **durable typed tables** (not only in-memory maps) | **TODO** | G-04 |
| S5-WS4A-01 | WS4A | Outbox + replay + WAL-backed cursors | **DONE** (scaffold) | `voltnuerongrid-ingest` |
| S5-WS4A-02 | WS4A | **Kafka/NATS/Event Hubs live e2e** | **TODO** | Adapters exist; prod hardening |
| S5-E4A-01 | Epic 4A | Connector SDK **runtime load** | **TODO** | G-08 |

---

### Sprint 6 — Security, RBAC, KMS, TLS

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S6-WS5-01 | WS5 | RBAC matrix + runtime principal enforcement | **DONE** (broad) | Across handlers |
| S6-WS5-02 | WS5 | KMS status/outage simulation endpoints | **DONE** (scaffold) | Adapters for local/cloud CLI |
| S6-WS5-03 | WS5 | **TLS/mTLS termination + cert rotation** | **TODO** | G-06 / design |
| S6-WS5-04 | WS5 | **TDE for data at rest in engine** | **TODO** | Contract vs implementation |

---

### Sprint 7 — HA/FT, distributed control plane

| ID | Workstream | Task | Status | Notes |
|----|------------|------|--------|--------|
| S7-WS6-01 | WS6 | Failover simulation + handoff report + transport abstraction | **DONE** (scaffold) | In daemon |
| S7-WS6-02 | WS6 | **Raft / quorum metadata service** | **TODO** | G-02 |
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
| S9-WS8-02 | WS8 | **Production policy + model gateway isolation** | **TODO** | |
| S9-WS8A-01 | WS8A | Audit companion CLI | **PARTIAL** | |
| S9-WS8A-02 | WS8A | **Durable tamper-evident audit chain** | **TODO** | |

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
| WS1 | Analyzer/execute/transaction/UDF scaffold | Real parser/planner |
| WS1A | Legacy agg eval + parity scripts (Ready for Validation) | Full MDAP parity operators (P0–P2) |
| WS2 | Indexes, constraints, WAL patterns | Real on-disk row store + MVCC |
| WS2A | Sync-origin scaffold | End-to-end HTAP freshness |
| WS3 | Routing heuristics + `OVER`/`HAVING` detection, perf harness | Columnar OLAP engine |
| WS4 | Multi-format ingest, chunked (Ready for Validation) | Durable table load |
| WS4A | Outbox/replay (Ready for Validation) | External broker hardening |
| WS5 | RBAC, KMS simulation | TLS/TDE production |
| WS6 | Simulated failover narrative | Real quorum/fencing |
| WS7 | Plugin manifest security | Plugin execution sandbox |
| WS8 | Autonomous API scaffold | Production AI governance |
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
**2026-04-12 progress note:** Sprint 2 (S2-WS2-04, S2-WS2-05) and Sprint 3 (S3-WS1-04) have advanced from TODO → PARTIAL with the addition of `PagedRowStore` (MVCC) and the SQL `Tokenizer` modules. Sprint 1 gap S1-GAP-01 (stub-crate decision) is now DONE. Total `cargo test` count: **170 passing** across all crates.
---

## 7) How to maintain this file

- After each sprint: flip tasks `TODO` → `DONE`, add discovered gaps.  
- Link evidence: `tests/kpi/results/...` and `cargo test -p ...` commands.  
- When stub crates are removed or implemented, update **Section 2** and **G-10**.  

---

*Generated for planning; aligns with `reference/voltnuerongrid-db-design.md` and `reference/voltnuerongrid-ws.md` checklist themes.*
