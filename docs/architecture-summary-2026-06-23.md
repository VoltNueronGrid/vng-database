# VoltNueronGrid DB — Architecture, Constitution & Spec Summary

**Date:** 2026-06-23  
**Branch:** `main`  
**Artifacts covered:** Constitution v1.0.0 · Scenario View · Logical View · Process View · Development View · Physical View · Architecture Synthesis  
**Gap documents:** `gaps-may20-2.md` (sessions 16–22) · `gaps-4.md` (session 32)  
**Test baseline at session 32:** 807 passed (voltnuerongridd), 0 failed

---

## 1. High-Level Overview

### What VoltNueronGrid DB Is

VoltNueronGrid DB (`voltnuerongridd`) is a Rust-first distributed HTAP (Hybrid Transactional/Analytical Processing) database engine. It delivers a unified SQL surface for OLTP and OLAP workloads through:

- A modular Rust workspace with 15+ crates covering SQL, execution, storage, auth, audit, ingest, failover, plugins, AI/autonomous governance, and MCP.
- An axum HTTP service binary exposing 330+ route groups for SQL, admin, security, Raft consensus, WAL, HTAP, ingest, and operational control.
- A Studio desktop/web UI (React + TypeScript + Vite + Tauri) for database management, schema inspection, and query execution.
- Language drivers (Rust, TypeScript/JS, Python, Java, Node, Perl, Deno, CFFI) over a native protocol and HTTP transport.
- MCP/IDE tools for AI-assisted database operations.
- Gate scripts and CI pipelines that produce evidence artifacts as the source of release truth.

### Architecture Model

The 4+1 architecture is organized as:

| View | Purpose |
|------|---------|
| **Scenario** | UC-producing use cases, actors, acceptance semantics, and identified gaps |
| **Logical** | Capability boundaries, domain objects, states, and invariants |
| **Process** | Runtime handoffs, approvals, receipts, failure closure, and collaboration paths |
| **Development** | Architecture-level components, package boundaries, contracts, dependency rules |
| **Physical** | Deployment/hosting units, external systems, fact sources, observability boundaries |
| **Architecture synthesis** | Cross-view SSOT with primary tradeoffs, stable boundaries, anti-patterns, and open risks |

### Central Design Forces

1. **Runtime authority is the source of truth.** Studio, drivers, MCP, and admin tooling must derive database catalog, schema, and security state from the runtime — never from local mock/cached state.
2. **A connection must resolve to an authorized, existing or explicitly created database.** Phantom connections, empty workspace displays without selected databases, and implicit resources violate the database product contract.
3. **Correctness and evidence beat performance claims and deployment ambition.** Cloud, trillion-row scale, and zero-loss crash recovery are goals, not current architecture conclusions.
4. **Native protocol is first-class.** Browser-only limitations do not remove the native listener and language driver product surface.
5. **Evidence artifacts control release status.** Trackers and release summaries must match current gate/test artifact truth.

### Governance: Constitution v1.0.0

Seven non-negotiable principles ratified 2026-06-22:

| # | Principle |
|---|-----------|
| I | Durable HTAP correctness — persisted state, recovery, ACID, and HTAP freshness must be defined before implementation |
| II | Security, RBAC, and tenant isolation first — auth before domain, 401/403 semantics, no secret leakage |
| III | Performance claims require reproducible evidence — benchmarks, smoke, soak, or gate artifacts |
| IV | Modular Rust-first architecture with reuse — SOLID, `voltnuerongrid-{name}` crate naming, `state_with_key()` helper |
| V | Native interfaces and tooling are product surface — drivers, Studio, MCP, IDE extensions all first-class |
| VI | Autonomous and plugin actions must be governed — plan/simulate/apply/audit, signed manifests, resource limits |
| VII | Evidence-backed delivery and tracker truth — gate JSON status fields, 90% coverage target, no unproven claims |

---

## 2. Changes Required for a Production-Level Scalable Product

These are the architecture-significant work areas that must be completed before any production or SaaS readiness claim is defensible.

### 2a. Durable Row Store (Critical)

**Current state:** RocksDB is the WAL (DDL events and DML SQL text). The actual row store is `PagedRowStore`, an in-memory `HashMap`. On crash, rows not yet checkpointed are lost; boot replays WAL SQL to rebuild in-memory state.

**Required change:** Bind `PagedRowStore` reads and writes directly to RocksDB key-value pairs with one column family per database. Remove the DML SQL-replay boot path. This is an XL effort touching all DML paths and the boot sequence.

**Why it matters for production:** Any acknowledged COMMIT that survives WAL flush but not a page flush is a silent data loss. This is a fundamental correctness blocker.

### 2b. Full Physical Database Isolation (Critical)

**Current state:** Databases share one `PagedRowStore` and one RocksDB instance. Isolation is by row-key prefix (`"{db}."`). `DROP DATABASE` does not purge rows. No per-database connection semaphore. RBAC is global.

**Required change:** Per-database RocksDB column families; `DROP DATABASE` with full row/metadata purge; per-database connection semaphores; per-database role/privilege grant enforcement.

**Why it matters for production:** Tenant isolation and multi-tenancy require hard storage partitions. Key-prefix filtering is a logic gap, not a security boundary.

### 2c. Full ACID Enforcement (Critical)

**Current state:** Multi-statement batches can partially commit. `ROLLBACK` cannot unwind already-written version chains. Isolation levels (`READ COMMITTED`, `REPEATABLE READ`, `SERIALIZABLE`) are parsed but all execute identically. Group commit is not batched.

**Required change:** UNDO log for rollback; differentiated isolation level semantics; write-set persistence tied to the durable row store (partially started in session 32: `acid_write_sets.json`); batched WAL fsync.

**Why it matters for production:** ACID is a baseline contract. Partial-write exposure and phantom isolation levels break data integrity guarantees.

### 2d. HTAP Routing and Freshness Completeness (High)

**Current state:** Raft apply loop, DataFusion OLAP crate, and basic query routing exist. Full end-to-end proof of automatic transactional/analytical/hybrid routing for representative query shapes is incomplete. Freshness semantics are not surfaced to callers.

**Required change:** Evidence artifacts for routing across point-lookup (→ OLTP), aggregation (→ OLAP), and mixed (→ hybrid) shapes; freshness receipts on analytical query results; HTAP sync transport beyond `InMemoryReplicationTransport`.

**Why it matters for production:** The HTAP value proposition requires routing to be automatic and correct. Stale analytical data without freshness disclosure breaks enterprise trust.

### 2e. Multi-Node Raft Cluster Durability (High)

**Current state:** Raft background loop, elections, append, heartbeat, snapshot transfer, and cluster token auth are implemented and partially tested. However, the row store behind Raft is still in-memory. The `failover` crate is a stub.

**Required change:** Row store durability (§2a above) must be completed first. Then multi-node smoke testing with actual data-loss proofs and RTO/RPO evidence.

**Why it matters for production:** A Raft implementation backed by an in-memory store provides leader election but not distributed data durability.

### 2f. Crash Recovery Evidence (High)

**Current state:** WAL fsync-on-commit is configurable. RocksDB WAL covers SQL text. There is no executed gate artifact proving latest-transaction recovery across a full database's row/metadata/WAL state.

**Required change:** A repeatable recovery gate that writes N rows, crashes/restarts, and proves all N rows are present and correct.

**Why it matters for production:** Zero-data-loss claims require executed proof. Configuration of WAL fsync is not the same as recovery evidence.

### 2g. Cloud Deployment Hardening (Medium/Deferred)

**Current state:** `deploy/cloud/` README explicitly says draft/not tested. Helm and cloud profiles exist but have not been smoke or load tested.

**Required change:** Production smoke, load, failover, and security tests against cloud deployment profile. Cloud must not be cited in architecture conclusions until this evidence exists.

### 2h. Native Driver and Studio Protocol Completeness (Medium)

**Current state:** Rust driver and HTTP-based drivers exist. Native protocol listener is implemented. Driver contract (`driver-core-contract-v1.md`) is HTTP-only with a placeholder for native parity. Full language driver conformance (Python, Java, TypeScript, Node, Perl, Deno) against a conformance gate is incomplete.

**Required change:** Conformance gate with per-language test results; native protocol framing spec aligned with runtime behavior; Studio native path validated through desktop bridge or native client rather than browser fetch.

### 2i. Security Hardening for Production (High)

**Current state:** RBAC order is implemented. `bcrypt` cost-12 password hashing and HMAC-SHA256 session tokens are in place. TLS configuration exists. KMS references use env vars.

**Required change:** Full security gate evidence covering TLS certificate lifecycle, KMS key rotation, signed plugin manifest loading, per-database role enforcement, audit trail completeness, and session token rotation endpoint. Security checklist artifacts should be current.

---

## 3. Status: Pending vs Achieved

### ✅ Achieved / Already Present

| Area | Evidence |
|------|---------|
| SQL parser false positives | `keyword_outside_strings` / `find_keyword_outside_strings` everywhere in `ast.rs` and `sqlparser_adapter.rs` |
| User accounts / bcrypt / session tokens | bcrypt cost-12, HMAC-SHA256 session tokens, `UserStore`, `SessionStore`, login, admin user endpoints, WAL-persisted |
| Row key DB prefix scoping | `make_row_key`, `make_table_scan_prefix`, `db_prefix_key`; DML paths scope by `"{db}."` prefix |
| `information_schema` / `pg_catalog` virtual catalog | Handler-level interception for tables, columns, schemata, routines, triggers, `information_schema.settings` |
| SELECT WHERE correctness | DataFusion path handles `=`, `<>`, `>`, `<`, `BETWEEN`, `IN`, `IS NULL/NOT NULL`, `AND`, `OR`, `NOT` |
| `main.rs` de-monolithization | 33,743 lines → 1,925 lines; 16+ handler modules extracted |
| RocksDB WAL durability | RocksDB default durability engine with configurable `VNG_WAL_FSYNC_ON_COMMIT` |
| Prometheus `/metrics` and tracing | `metrics-exporter-prometheus`, `/metrics` route, counters, `tracing` + env-filter |
| Studio Databases UI | `DatabasesPanel.tsx` with create/drop wired into Sidebar |
| Raft background loop | 150ms tick, elections, `AppendEntries`, `InstallSnapshot`, heartbeat fanout, committed-entry apply, log compaction |
| Raft log persistence across restarts | `persist_raft_state()` / `load_raft_state()` via `raft_meta.json` with atomic write |
| Correct `prev_log_term` in AppendEntries | `RaftNode::term_at()` used in fanout |
| Raft db-prefix threading | `__vng_db:<name>\n` prefix in `RaftLogEntry.command`; `apply_dml_command` unpacks db |
| RocksDB read-miss fallback | `DurabilityEngine::get_row` trait, `read_latest_with_rocksdb_fallback` on all before-image reads |
| Write-set persistence | `acid_write_sets.json` persisted on commit, restored on boot |
| Statement timeout watchdog | `tokio::time::timeout` wrapping DataFusion call sites; `ExecError::Timeout` variant |
| Leader reads (linearisable SELECT) | `VNG_REQUIRE_LEADER_READS` guard; non-leaders return 503 in multi-node clusters |
| Column type validation at INSERT | `validate_value_for_type` / `validate_row_against_ddl` in `sql_parse.rs` |
| View expansion improvements | Word-boundary matching, schema-qualifier stripping in `sql_parse.rs` |
| `information_schema.settings` virtual table | Exposes all 7 runtime config fields |
| Modular crate structure | 15 crates: sql, exec, datafusion, store, auth, audit, ingest, plugins, ai, config, meta, core, failover, mcp, opt |
| RBAC auth order in handlers | Admin key → operator identity → tenant/user headers; 401/403 responses |
| DataFusion OLAP integration | `voltnuerongrid-exec-datafusion` for analytical query execution |
| 4+1 Architecture artifacts | Scenario, Logical, Process, Development, Physical, Synthesis views complete |
| Constitution v1.0.0 | Seven principles ratified; plan/spec/tasks templates aligned |
| Gate/KPI scripts | `tests/kpi/scripts/` PowerShell gate framework with JSON artifact output |
| Test count | 807 tests passing (session 32 baseline) |

---

### 🔲 Pending

| Area | Severity | Notes |
|------|----------|-------|
| Row store → RocksDB pages (one CF per DB) | 🔴 Critical | XL effort; blocking durability and ACID |
| UNDO log for multi-statement rollback | 🔴 Critical | Linked to row store overhaul |
| Per-database RocksDB column-family isolation | 🔴 Critical | Required for hard tenant isolation |
| `DROP DATABASE` row/metadata purge | 🔴 Critical | Currently only removes catalog entry |
| Full isolation level semantics (RC/RR/SERIALIZABLE) | 🔴 Critical | Currently parsed but not differentiated |
| SELECT legacy fallback — `k.contains()` substring bug | 🔴 Critical | Reachable for JOIN/GROUP BY/subquery shapes |
| Per-database connection semaphore | 🟠 High | `max_connections` field exists but unwired |
| HTAP route/freshness evidence across query shapes | 🟠 High | Route evidence incomplete |
| End-to-end crash recovery gate | 🟠 High | No executed recovery artifact |
| Multi-node cluster smoke test (with durable store) | 🟠 High | Raft is real; backing store is not yet durable |
| `failover` crate implementation | 🟠 High | Currently a stub |
| `htap_sync` beyond `InMemoryReplicationTransport` | 🟠 High | Needed for real OLAP freshness sync |
| Studio connection/database lifecycle fix | 🟠 High | Reported broken; architecture-significant |
| Full per-database RBAC grant enforcement | 🟠 High | Currently global RBAC |
| Native driver conformance gate | 🟠 High | Traceability matrix marks as critical gap |
| Native protocol Studio validation path | 🟠 High | Browser-only test is insufficient |
| Driver core contract native parity | 🟠 High | `driver-core-contract-v1.md` is HTTP-only |
| Security gate artifacts (TLS, KMS, plugin signing) | 🟠 High | Items from security checklist |
| Cloud deployment smoke/load/failover | 🟡 Medium/Deferred | Draft; README says not tested |
| `CALL insert_rows` in SQL path | 🟡 Medium | CALL statement routing incomplete |
| ALTER TABLE DDL | 🟡 Medium | SQL feature gap |
| GRANT/REVOKE via SQL syntax | 🟡 Medium | Admin-only endpoint today |
| JOIN execution via DataFusion for all shapes | 🟡 Medium | Some shapes still reach legacy path |
| Codd's rules completeness | 🟡 Medium | Partial coverage |
| `.expect()` panic removal | 🟡 Medium | Prod-unsafe unwrap patterns remain |
| OTEL spans on hot paths | 🟡 Medium | Tracing infrastructure present; spans incomplete |
| Design token drift (Studio) | 🟡 Medium | UI design consistency |
| Session token rotation endpoint | 🟢 Low | Feature gap, not safety blocker |
| Scratch files / unused import cleanup | 🟢 Low | Code hygiene |
| Missing focused unit tests for new helpers | 🟢 Low | Coverage gaps on newer modules |

---

## 4. Refactoring Code Changes Required

These are changes to existing code structures required by the architecture — distinct from new feature work.

### 4a. Row Store Bind to RocksDB (XL)

**Files:** `crates/voltnuerongrid-store/src/mvcc.rs`, `crates/voltnuerongrid-store/src/rocksdb_engine.rs`, `services/voltnuerongridd/src/helpers/boot.rs`, all DML handlers

**Change:** Replace in-memory `HashMap<String, VersionChain>` writes in `PagedRowStore::store()` with direct RocksDB CF puts. Replace `scan_at_snapshot()` with CF-scoped RocksDB range scans. Remove DML SQL text replay from `boot.rs`. Add per-DB CF creation in `CREATE DATABASE` and CF deletion in `DROP DATABASE`.

### 4b. Eliminate `k.contains(val)` Legacy SELECT Fallback

**Files:** `services/voltnuerongridd/src/helpers/execution.rs` (~line 589)

**Change:** Replace the `k.contains(val.as_str())` fallback with `false` (predicate cannot be resolved → no match). Expand DataFusion coverage to cover JOIN/GROUP BY/subquery so the legacy path is only reached for unsupported syntax, not correctness-impacting fallbacks.

### 4c. Per-Database RBAC Scope

**Files:** `crates/voltnuerongrid-auth/`, `services/voltnuerongridd/src/auth/`, privilege check call sites in handlers

**Change:** Pass database name as a scope parameter into privilege-check helpers. Privilege tables must be keyed by `(user, database, object)`. Handlers reject cross-database actions even when the user has a global role.

### 4d. `DROP DATABASE` Row Purge

**Files:** `services/voltnuerongridd/src/handlers/admin.rs`

**Change:** After removing the catalog entry and WAL record, issue a delete scan over all row-store keys with the `"{db}."` prefix (or CF drop when per-DB CFs are implemented).

### 4e. Isolation Level Differentiation

**Files:** `crates/voltnuerongrid-store/src/mvcc.rs`, `services/voltnuerongridd/src/handlers/sql.rs`

**Change:** Track transaction isolation level in the session/transaction context. `REPEATABLE READ` must snapshot at transaction start. `SERIALIZABLE` must add write-set conflict detection at commit. `READ COMMITTED` is the current default and can remain as-is.

### 4f. UNDO Log for Multi-Statement Rollback

**Files:** `crates/voltnuerongrid-store/src/mvcc.rs`, `services/voltnuerongridd/src/handlers/sql.rs` (`sql_transaction`)

**Change:** Track before-images (already partially started with `read_latest_with_rocksdb_fallback`) in a per-transaction undo buffer. On `ROLLBACK`, walk the undo buffer in reverse and revert each `VersionChain` entry.

### 4g. Failover Crate Implementation

**Files:** `crates/voltnuerongrid-failover/src/lib.rs`

**Change:** Implement the health-check, leader-election-notification, and peer-discovery interfaces declared in the `failover` crate. Currently a 3-line stub. This is separate from the Raft loop logic in `raft_loop.rs`.

### 4h. Remove `.expect()` / Unwrap Panics on Hot Paths

**Files:** Distributed across handler files and crate code

**Change:** Audit with `grep -r '\.expect\|unwrap()' services/voltnuerongridd/src/handlers/` and `crates/`. Replace with `?`/`Result` propagation or explicit error responses. `.expect()` on external inputs (locks, channels, user data) is a production availability risk.

### 4i. Studio Connection Flow Refactor

**Files:** `ui/voltnuerongrid-studio/src/` — connection state management, connection form component

**Change:** Connection state must not become `Active` until the runtime validates the target database. Add a `Pending` state that shows a prompt: "Database `{name}` does not exist. Create empty / Create with samples / Select different database." This is an architecture-driven UI refactor, not just a bug fix.

### 4j. HTAP Sync Transport Replacement

**Files:** `crates/voltnuerongrid-ingest/`, relevant sync pathway

**Change:** Replace `InMemoryReplicationTransport` with a durable, network-capable replication channel that works across Raft followers for OLAP freshness synchronization.

---

## 5. Critical, High, Medium, and Low Risks

### 🔴 Critical Risks

| Risk | Architecture Evidence | Consequence if Unaddressed |
|------|----------------------|---------------------------|
| **Row store data loss on crash** | Physical view: "WAL route presence does not prove crash recovery." | Any acknowledged COMMIT is vulnerable to loss on OOM/crash. |
| **Phantom connections in Studio** | Scenario view: "No DB exists + user enters name → must prompt before opening workspace." | Users interact with a connection that has no database scope, leading to confusing or incorrect schema displays. |
| **Cross-database data leakage via key-prefix scan** | Logical view: "A connection references exactly one active database scope." | `scan_at_snapshot()` returns all-DB rows filtered in code, not by storage boundary. Tenant isolation is not enforced at the storage layer. |
| **Multi-statement partial commit** | Process view: "Crash/restart recovery gap" failure branch. | Batched SQL that fails mid-way leaves partial row state. ROLLBACK has no effect on already-written version chains. |
| **Legacy SELECT substring false matches** | `gaps-may20-2.md §4` | JOIN/GROUP BY queries reaching the fallback path return incorrect rows (e.g., `WHERE id = 5` matches row 15, 25, 50). |

### 🟠 High Risks

| Risk | Architecture Evidence | Consequence if Unaddressed |
|------|----------------------|---------------------------|
| **Raft durability backed by in-memory store** | Architecture synthesis: "Multi-node operational claims remain review triggers." | Raft provides leader election and log replication, but followers can diverge from durable state after restart. |
| **No executed crash recovery gate** | Process view gap: "End-to-end latest-transaction crash recovery evidence is missing." | Zero-data-loss claim is not defensible. |
| **HTAP freshness unproven** | Scenario view gap: "Full automatic HTAP query routing is not proven for all query shapes." | Analytical results may be stale without a freshness receipt, violating the HTAP product promise. |
| **Global RBAC — no per-database privilege enforcement** | Logical view: "Authorization precedes protected operations" invariant not fully implemented. | A user with any role can access tables in any database. |
| **Native driver parity gap** | Development view gap: "Full driver parity is a critical gap in traceability." | Native protocol is a documented product surface. Incomplete conformance gate leaves it unverifiable. |
| **Studio native protocol dead-end** | Physical view gap: "Native protocol validation path in Studio is unclear." | Browser fetch cannot validate native connections. Users get an unusable UI path rather than an accurate capability scope. |
| **Security checklist items incomplete** | Physical view: "Production-facing security settings must not rely on plaintext secrets or missing TLS decisions." | TLS lifecycle, KMS rotation, signed plugin loading, and session rotation are not yet gated. |

### 🟡 Medium Risks

| Risk | Architecture Evidence | Consequence if Unaddressed |
|------|----------------------|---------------------------|
| `failover` crate is a stub | Development view: "Database Core Capabilities — owns failover." | Failover capability boundary exists in the architecture but has no implementation. HA claims cannot be made. |
| `.expect()` panics on hot paths | Constitution principle IV — modular Rust | Server process terminates on any unexpected value in production. |
| JOIN/GROUP BY still reaching legacy path | `gaps-may20-2.md §4` | Intermediate correctness risk until DataFusion covers 100% of query shapes. |
| Cloud profiles untested | Physical view: "Cloud assets remain draft until tested." | Cloud deployment cited in docs but cannot be used for production without smoke/load evidence. |
| OTEL span coverage incomplete | Constitution principle III — evidence | Performance profiling and SLO enforcement are limited without tracing on hot paths. |
| ALTER TABLE / GRANT/REVOKE via SQL | Scenario view use cases | DDL and privilege management are only partially available via SQL; some only via admin HTTP endpoints. |
| `CALL insert_rows` SQL path | `gaps-may20-2.md` | CALL statement routing incomplete. |

### 🟢 Low Risks

| Risk | Architecture Evidence | Consequence if Unaddressed |
|------|----------------------|---------------------------|
| Scratch files and unused imports | Constitution principle IV — clean codebase | Minor developer experience and review friction. |
| Session token rotation endpoint missing | Constitution principle II — no secret leakage | Long-lived tokens increase exposure window but are not currently a leakage vector. |
| Design token drift in Studio | Principle V — native interfaces are product surface | UI inconsistency across panels. |
| Missing unit tests for some newer helpers | Constitution principle VII — 90% coverage target | Some session 31–32 additions have partial coverage. |

---

## Architecture Reference Summary

| Document | Location | Status |
|----------|----------|--------|
| Constitution v1.0.0 | `.specify/memory/constitution.md` | Ratified 2026-06-22 |
| Architecture Synthesis | `.specify/memory/architecture.md` | Generated project SSOT |
| Scenario View | `.specify/memory/architecture-scenario-view.md` | Generated project SSOT |
| Logical View | `.specify/memory/architecture-logical-view.md` | Generated from scenario view |
| Process View | `.specify/memory/architecture-process-view.md` | Generated from scenario + logical views |
| Development View | `.specify/memory/architecture-development-view.md` | Generated from logical + process views |
| Physical View | `.specify/memory/architecture-physical-view.md` | Generated from process + development views |
| Gap analysis (sessions 16–22) | `docs/archive/gaps-may20-2.md` | 24 gaps remaining at session 22 |
| Gap analysis (session 32) | `docs/gaps-4.md` | 0 critical/high/medium remaining at session 32 |
| Spec templates | `.specify/templates/` | Aligned to constitution v1.0.0 |
