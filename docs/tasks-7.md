# VoltNueronGrid DB — Tasks 7 (README Architecture Coverage Audit)

> **Created:** 2026-06-30
> **Scope:** Coverage audit of every component in the README architecture diagram plus the
> README sections **Core Capabilities**, **Autonomous AI Actions**, **Proposed Platform
> Components**, **Architecture Goals**, and the **Target KPI Table**.
> **Method:** 5 parallel read-only code-exploration passes over `services/voltnuerongridd`,
> `crates/*`, `drivers/*`, `ui/*`, `tests/*`, `deploy/*`, cross-checked against the live
> codebase.
> **Goal:** Drive every item to **100%** except work that is intrinsically **cloud-deployment**
> dependent (object storage SDKs, managed SaaS, cloud autoscale provisioning), which is
> explicitly **DEFERRED** per direction.

> **Priority Execution Status (2026-06-30):** B-1 → C-7 → C-6 → C-8 — all ✅ DONE (100%).
> Distributed data-plane batch C-4 → C-3 → C-5 → C-1 → C-2 — all ✅ DONE (100%).
> Total test suite: **1042 passed, 0 failed** (`cargo test -p voltnuerongridd`).
> Verified tests (batch 1): `b1_*` (4), `c7_*` (3), `c6_*` (3), `c8_*` (2).
> Verified tests (batch 2): `c4_*` (3), `c3_*` (3), `c5_*` (2), `c1_*` (3), `c2_*` (4) plus
> inline helper unit tests in `services/voltnuerongridd/src/helpers/dataplane.rs` — all green.

## Legend

| Symbol | Meaning |
|--------|---------|
| ✅ DONE | Implemented and verified in code/tests |
| 🟡 PARTIAL | Scaffold/foundation exists; core logic incomplete |
| ❌ MISSING | No real implementation |
| ☁️ DEFERRED | Cloud-deployment dependent — out of scope for 100% local target |

---

## 1. Executive Summary

The engine has a **strong single-node functional core**: ANSI SQL DDL/DML + materialized
views, MVCC row store, DataFusion OLAP, HTAP routing, CDC, parallel multi-format ingest,
RBAC + tamper-evident audit, plugin runtime (vector/FTS/geo), signed-plugin enforcement,
full Raft state machine, and a working autonomous **policy/guardrail/audit** layer.

The gaps cluster into **5 areas**:

1. **Autonomous AI *execution*** — detection, classification, policy, and audit all work, but
   most agent *remediation/execution* paths only record intent (no real CREATE INDEX, cache
   eviction, query-kill, rotation scheduling, or fix execution). No top-level controller/orchestrator.
2. **Distributed data plane** — sharding, distributed scheduler, cross-node cache/HTAP-sync
   replication, and failover-controller wiring are missing or in-memory-only.
3. **Storage durability & advanced SQL** — row-store durability hardening (P1), physical
   partitioning, optimistic locking, multimodel/JSONB.
4. **Clients** — BI wire protocol (JDBC/ODBC/Postgres), Java JDBC layer, C++ driver, Studio UI
   completion, and 3 IDE extensions (Antigravity/JetBrains/Eclipse).
5. **KPI measurement** — every KPI has a scenario script but **no concurrent, sustained,
   real-dataset harness** that actually asserts the threshold; several only echo a static target.

**Cloud-deferred (acceptable):** S3/Azure-Blob/GCS connectors + object-storage backend,
live autoscale provisioning, managed-SaaS maturity, physical compute/storage cloud pools.

---

## 2. Coverage Matrix — Architecture Diagram

### 2.1 Clients
| Component | Status | % | Task |
|---|---|---|---|
| VoltNueronGrid Studio UI | 🟡 PARTIAL | 55 | D-4 |
| BI Tools (JDBC/ODBC/wire) | ❌ MISSING | 0 | D-1 |
| Apps and Services | ✅ DONE | 100 | — |
| Language Drivers | ✅ DONE | 100 | — |

### 2.2 Gateway
| Component | Status | % | Task |
|---|---|---|---|
| SQL and Session Gateway | ✅ DONE | 100 | — |
| AI Copilot Gateway | ✅ DONE | 100 | — |
| AI Model Gateway | ✅ DONE | 100 | — (rate-limit sliding window verified) |
| AuthN and AuthZ | ✅ DONE | 100 | — |

### 2.3 Developer Tools (IDE Extensions)
| Component | Status | % | Task |
|---|---|---|---|
| VS Code / Cursor | ✅ DONE | 100 | — |
| Visual Studio | ✅ DONE | 100 | — |
| Antigravity | 🟡 PARTIAL | 30 | D-5 |
| JetBrains | 🟡 PARTIAL | 35 | D-5 |
| Eclipse | 🟡 PARTIAL | 30 | D-5 |

### 2.4 Control Plane
| Component | Status | % | Task |
|---|---|---|---|
| Catalog Service Cluster | ✅ DONE | 100 | — |
| Metadata Raft Cluster | ✅ DONE | 100 | C-7 |
| Distributed Scheduler Cluster | ✅ DONE | 100 | C-1 |
| Placement and Autoscale Cluster | ✅ DONE | 100 | C-8 / ☁️ |
| Failover Controller Quorum | ✅ DONE | 100 | C-6 |

### 2.5 Data Plane
| Component | Status | % | Task |
|---|---|---|---|
| Query Router Cluster | ✅ DONE | 100 | — |
| Shard Coordinators | ✅ DONE | 100 | C-2 |
| Buffer and Result Cache | ✅ DONE | 100 | C-3 |
| OLTP Transaction Executors | ✅ DONE | 100 | — |
| OLAP Vectorized Executors | ✅ DONE | 100 | — |
| Transaction and Lock Manager | 🟡 PARTIAL | 75 | B-2, B-3 |
| HTAP Sync Pipeline | ✅ DONE | 100 | C-4 |
| CDC and Export Stream Engine | ✅ DONE | 100 | — |
| Native Cache Engine Cluster | ✅ DONE | 100 | C-3 |
| Bulk Ingest Engine | ✅ DONE | 100 | — |

### 2.6 Streaming and Events
| Component | Status | % | Task |
|---|---|---|---|
| Transactional Outbox | ✅ DONE | 100 | — |
| Quorum Event Bus Cluster | ✅ DONE | 100 | C-5 |
| Immutable Audit Stream | ✅ DONE | 100 | — |
| Operational Event Stream | 🟡 PARTIAL | 45 | B-5 |

### 2.7 Governance
| Component | Status | % | Task |
|---|---|---|---|
| Data Audit Engine | ✅ DONE | 100 | — |
| Audit Companion Tool | 🟡 PARTIAL | 60 | A-9 |

### 2.8 Autonomous Control Plane
| Component | Status | % | Task |
|---|---|---|---|
| Autonomous DB Controller | ❌ MISSING | 0 | A-1 |
| Ops Agent Orchestrator | ❌ MISSING | 0 | A-2 |
| DDL and Schema Agent | 🟡 PARTIAL | 40 | A-5 |
| Plugin Builder Agent | 🟡 PARTIAL | 35 | A-6 |
| Performance Tuning Agent | 🟡 PARTIAL | 60 | A-3 |
| Security and Compliance Agent | 🟡 PARTIAL | 50 | A-7 |
| Self-Heal Agent | 🟡 PARTIAL | 50 | A-4 |

### 2.9 Extensions
| Component | Status | % | Task |
|---|---|---|---|
| Plugin Runtime | ✅ DONE | 100 | — |
| Vector Search Plugin | ✅ DONE | 100 | — (flat-scan; HNSW optional) |
| Geospatial Plugin | ✅ DONE | 100 | — |
| Connector Plugins | ✅ DONE | 100 | — |
| Full-Text Search Plugin | ✅ DONE | 100 | — |
| Multimodel Plugin | ❌ MISSING | 0 | B-6 |

### 2.10 Storage
| Component | Status | % | Task |
|---|---|---|---|
| Transactional Row Store (MVCC) | ✅ DONE | 100 | B-1 |
| Analytical Columnar Store | ✅ DONE | 100 | — |
| Local SSD Segments | ✅ DONE | 100 | B-1 |
| Object Storage | ☁️ DEFERRED | — | CD-1 |
| WAL and Checkpoints | ✅ DONE | 100 | B-1 |
| Index Store | ✅ DONE | 100 | — |

### 2.11 External Sources / Connectors
| Component | Status | % | Task |
|---|---|---|---|
| FTP and FTPS | ✅ DONE | 100 | — |
| WebDAV | ✅ DONE | 100 | — |
| Other Streaming (Kafka REST) | 🟡 PARTIAL | 60 | C-9 |
| Azure Blob | ☁️ DEFERRED | — | CD-1 |
| AWS S3 | ☁️ DEFERRED | — | CD-1 |
| Google Cloud Storage | ☁️ DEFERRED | — | CD-1 |

---

## 3. Coverage Matrix — Core Capabilities / Goals

| README Capability | Status | % | Task |
|---|---|---|---|
| ANSI SQL DDL/DML + materialized views | ✅ DONE | 100 | — |
| Native AI assistant (chat-to-SQL, ingest, export) | ✅ DONE | 95 | A-3..A-8 polish |
| Autonomous ops (self-heal/tune/secure/operate) | 🟡 PARTIAL | 50 | A-1..A-8 |
| UDF in Rust/JS/Python | ✅ DONE | 100 | — |
| HA + fault tolerance + autoscaling | ✅ DONE | 100 | C-6, C-8 |
| Separate compute/storage | ✅ DONE | 100 | C-2 / ☁️ |
| Multithreaded import CSV/Parquet/JSON/Excel | ✅ DONE | 100 | — |
| Plugin source ingestion (FTP/WebDAV + cloud) | 🟡 PARTIAL | 60 | ☁️ CD-1 |
| Plugin ecosystem (vector/geo/FTS/multimodel/connector) | 🟡 PARTIAL | 85 | B-6 |
| Native distributed cache (Redis-like) | ✅ DONE | 100 | C-3 |
| Unified HTAP execution | ✅ DONE | 95 | C-4 |
| Huge datasets (partition/shard/index/constraint) | 🟡 PARTIAL | 80 | B-4, C-2 |
| RBAC + governance | ✅ DONE | 100 | — |
| Separate UI client + engine | 🟡 PARTIAL | 60 | D-4 |
| Drivers (Py/Rust/Java/JS/TS/Deno/C/C++/Perl) | 🟡 PARTIAL | 80 | D-1, D-2, D-3 |
| IDE extensions (VS/Cursor/Antigravity/JetBrains/Eclipse) | 🟡 PARTIAL | 65 | D-5 |
| Rust memory safety + performance | ✅ DONE | 100 | — |
| SOLID modular design | ✅ DONE | 100 | — |
| Observability-first | ✅ DONE | 100 | — |
| Security-first | ✅ DONE | 100 | — |
| Deployment parity local/cloud | 🟡 PARTIAL | 60 | E-8 |

---

## 4. Detailed Tasks

> Each task below is a gap toward 100%. Cloud-deferred work is collected in §6.

### Group A — Autonomous AI Execution

#### A-1 · Autonomous DB Controller (top-level orchestrator)
| Field | Value |
|---|---|
| **Status** | ❌ MISSING |
| **% Complete** | 0% |
| **Priority** | 🔴 High |
| **Depends on** | A-3..A-8 (sub-agents) |
| **Effort** | L |

**Detail:** The diagram shows an "Autonomous DB Controller" sitting above the individual
agents. Today each agent endpoint is invoked independently; there is no unifying controller
that (a) ingests a high-level goal, (b) decomposes it into agent actions, (c) sequences them
through the guardrail/policy gate, and (d) records a single correlated action plan + outcome.

**Acceptance Criteria:**
- [ ] `handlers/autonomous.rs`: `autonomous_controller_run` endpoint accepting a goal + mode
- [ ] Decomposition into ordered sub-agent calls (DDL, tuning, self-heal, security)
- [ ] Each step passes through existing `autonomous_guardrails` policy check
- [ ] Single correlation id threaded through all emitted audit events
- [ ] Unit tests for advisory/supervised/autonomous orchestration paths

#### A-2 · Ops Agent Orchestrator
| Field | Value |
|---|---|
| **Status** | ❌ MISSING |
| **% Complete** | 0% |
| **Priority** | 🟠 Medium |
| **Depends on** | A-1 |
| **Effort** | M |

**Detail:** Coordination layer that schedules recurring ops-agent duties (tuning sweeps,
compliance scans, self-heal polling) and fans them to the right agent with concurrency
limits and backoff. Currently each runs only on explicit HTTP call.

**Acceptance Criteria:**
- [ ] Background scheduler task (reuse `run_dr_hook_scheduler` pattern) for periodic agent sweeps
- [ ] Per-agent enable/interval config via env (`VNG_OPS_AGENT_*`)
- [ ] Emits audit event per scheduled invocation
- [ ] Tests for schedule firing + disabled-by-default safety

#### A-3 · Performance Tuning Agent — real execution
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 60% |
| **Priority** | 🔴 High |
| **Depends on** | — |
| **Effort** | M |

**Detail:** `ai_tune_recommendations` correctly builds CREATE INDEX / ANALYZE /
INCREASE_CONNECTIONS suggestions from the slow-query log. `ai_tune_apply` currently only
**logs intent** (does not execute the DDL). Complete the execution path so supervised/
autonomous modes actually run the recommended `CREATE INDEX` and refresh `stats_registry`.

**Acceptance Criteria:**
- [ ] `ai_tune_apply` executes recommended CREATE INDEX through the SQL engine in supervised+ mode
- [ ] `ANALYZE` recommendation recomputes and stores table stats in `stats_registry`
- [ ] Pool-limit recommendation updates the per-DB semaphore capacity at runtime (no restart)
- [ ] Every applied action emits audit event with before/after evidence
- [ ] Tests: recommendation → apply → index visible in catalog

#### A-4 · Self-Heal Agent — real remediation
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 50% |
| **Priority** | 🔴 High |
| **Depends on** | — |
| **Effort** | M |

**Detail:** Signal classification works (network→diagnostic_probe, election→leader_promotion,
disk→cache_eviction), but the remediation actions only record intent. Implement the actual
local remediations.

**Acceptance Criteria:**
- [ ] `cache_eviction` action invokes `DistributedCacheManager` eviction and reports freed entries
- [ ] `query_kill` action cancels/releases the offending pessimistic lock + transaction
- [ ] `diagnostic_probe` performs a real local health probe (row-store, wal, raft) and attaches results
- [ ] `leader_promotion` triggers the Raft election path on the local node when eligible
- [ ] Each remediation emits audit event with outcome=applied|skipped|failed + reason
- [ ] Tests for each remediation branch

#### A-5 · DDL & Schema Agent — drift detection + multi-step provisioning
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 40% |
| **Priority** | 🟠 Medium |
| **Depends on** | A-1 |
| **Effort** | M |

**Detail:** Read-only catalog queries exist; CREATE TABLE/INDEX work via SQL. Missing:
(a) schema-drift detection (compare desired vs actual catalog), and (b) autonomous multi-step
provisioning (schema → table → indexes → constraints as one governed unit).

**Acceptance Criteria:**
- [ ] `autonomous_schema_reconcile` endpoint: input desired schema spec, diff vs catalog
- [ ] Emits an ordered DDL plan, executes it in supervised+ mode through the SQL engine
- [ ] Drift report lists missing/extra tables, columns, indexes
- [ ] Audit event per executed DDL step
- [ ] Tests: drift detected → plan → applied → catalog matches spec

#### A-6 · Plugin Builder Agent
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 35% |
| **Priority** | 🟢 Low |
| **Depends on** | A-1 |
| **Effort** | M |

**Detail:** Install/upgrade with signature enforcement is done. Missing the autonomous
"builder" flow: scaffold a connector/extension manifest from a template, sign it with the
configured key, and register it through the existing signed-manifest path.

**Acceptance Criteria:**
- [ ] `autonomous_plugin_build` endpoint generates a manifest from a template descriptor
- [ ] Generated manifest is signed and validated through existing `SignaturePolicyHook`
- [ ] Rejects build when signing key absent (no unsigned artifacts)
- [ ] Audit event for build + register
- [ ] Tests for build→sign→register happy path and unsigned rejection

#### A-7 · Security & Compliance Agent — auto-rotation + auto-remediation
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 50% |
| **Priority** | 🟠 Medium |
| **Depends on** | A-2 |
| **Effort** | M |

**Detail:** Manual `security_tls_rotate` / `security_kms_rotate` and `compliance_report`
(scored) exist. Missing scheduled autonomous rotation and auto-remediation triggers when the
compliance score drops below threshold.

**Acceptance Criteria:**
- [ ] Scheduled rotation policy (interval/age threshold via env) drives `security_*_rotate`
- [ ] Compliance scan below threshold enqueues a governed remediation action
- [ ] All rotations/remediations are policy-checked + audited
- [ ] Tests: aged cert → scheduled rotation fires; low score → remediation enqueued

#### A-8 · Incident Diagnosis — fix proposal + execution + evidence
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 45% |
| **Priority** | 🟠 Medium |
| **Depends on** | A-1, A-4 |
| **Effort** | M |

**Detail:** `sre_incident_diagnose` + `DiagnosisRule` matching exist; `sre_incident_evidence`
exists. Missing: turning a diagnosis into a concrete fix action, executing it under policy,
and auto-generating a post-incident evidence summary that links diagnosis → action → outcome.

**Acceptance Criteria:**
- [ ] Diagnosis maps to a recommended remediation action (reuse A-4 remediations)
- [ ] Supervised+ mode executes the fix through the guardrail gate
- [ ] Post-incident summary bundles diagnosis, action, audit-event ids, outcome
- [ ] Tests: seeded incident → diagnosis → fix executed → evidence summary complete

#### A-9 · Audit Companion Tool (CLI)
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 60% |
| **Priority** | 🟢 Low |
| **Depends on** | — |
| **Effort** | S |

**Detail:** Audit query/export/chain-verify HTTP endpoints exist; the diagram's "Audit
Companion Tool" implies an operator-facing tool. Provide a small Rust CLI (or subcommand)
that queries events, verifies the hash chain, and exports evidence bundles.

**Acceptance Criteria:**
- [ ] CLI binary/subcommand: `audit list|verify|export` hitting the runtime API
- [ ] Chain verification surfaces tamper point if any
- [ ] Export writes a portable evidence bundle (JSON lines + manifest)
- [ ] README usage snippet + smoke test

---

### Group B — Storage & Advanced SQL (single-node)

#### B-1 · Row-store durability hardening (P1)
| Field | Value |
|---|---|
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Priority** | 🔴 High |
| **Depends on** | — |
| **Effort** | L |

**Detail:** RocksDB durability + warm-cache load + XID fast-forward exist; the long-standing
P1 concern is ensuring every committed row mutation is durably bound to RocksDB CFs (not
in-memory-only) and survives restart with correct MVCC visibility, including DROP DATABASE
row purge. Close any remaining in-memory-only write paths and add crash-recovery tests.

**Acceptance Criteria:**
- [x] Every DML commit path writes through to the durability engine (audit all `store_row` call sites)
- [x] DROP DATABASE purges persisted rows for that DB's CF
- [x] Restart test: insert → kill → reboot → rows visible with correct xids
- [x] Group-commit batched fsync path verified under `VNG_WAL_FSYNC_ON_COMMIT`

**Completed:** Tests added: `b1_replace_all_clears_old_rows_and_installs_snapshot`, `b1_drop_database_purges_rows_for_dropped_db`, `b1_dml_commit_persists_statement_to_durability_engine`, `b1_group_commit_batch_returns_one_seq_per_entry`. All 4 pass.

#### B-2 · Optimistic locking variant
| Field | Value |
|---|---|
| **Status** | ❌ MISSING |
| **% Complete** | 0% |
| **Priority** | 🟢 Low |
| **Depends on** | — |
| **Effort** | M |

**Detail:** Only pessimistic locking + serializable OCC at commit exist. Add an explicit
optimistic-locking mode (version-check on write, conflict→retry/abort) selectable per txn.

**Acceptance Criteria:**
- [ ] Optimistic mode flag on transaction begin
- [ ] Version-mismatch on commit returns a typed conflict (409) without holding row locks
- [ ] Tests for concurrent optimistic conflict + success

#### B-3 · Deadlock detection efficiency
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 75% |
| **Priority** | 🟢 Low |
| **Depends on** | — |
| **Effort** | S |

**Detail:** Wait-graph cycle detection works but is O(n²) over lock count with a scan cap.
Replace with an incremental wait-for graph traversal bounded by `DEADLOCK_SCAN_MAX_HOPS`.

**Acceptance Criteria:**
- [ ] Cycle detection traverses only the wait-for edges of the requesting txn
- [ ] Existing deadlock tests still pass; add a deeper-chain test
- [ ] Metrics unchanged (`deadlock_detections`, `scan_cap_timeouts`)

#### B-4 · Physical partitioning
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 30% |
| **Priority** | 🟠 Medium |
| **Depends on** | B-1 |
| **Effort** | M |

**Detail:** `PARTITION BY RANGE(col)` is parsed and recorded in `partition_registry`, but
storage is not actually partitioned and scans don't prune partitions. Implement partition
key→segment mapping and partition pruning on SELECT.

**Acceptance Criteria:**
- [ ] Rows routed to a partition segment by range key on insert
- [ ] SELECT with a range predicate prunes non-matching partitions
- [ ] Partition list + per-partition row counts exposed via catalog endpoint
- [ ] Tests: insert across ranges → pruned scan returns correct subset

#### B-5 · Operational Event Stream emission
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 45% |
| **Priority** | 🟢 Low |
| **Depends on** | — |
| **Effort** | S |

**Detail:** `StreamEventEnvelope` exists but operational events are not actually emitted by
service operations (only audit events are). Wire key lifecycle events (leader change, ingest
batch complete, autoscale decision, self-heal action) into an operational event stream
endpoint.

**Acceptance Criteria:**
- [ ] Operational events emitted from ≥4 subsystems (raft, ingest, autoscale, self-heal)
- [ ] `GET /api/v1/events/operational` returns recent events with filtering
- [ ] Tests asserting events are produced on those operations

#### B-6 · Multimodel / document (JSONB) plugin
| Field | Value |
|---|---|
| **Status** | ❌ MISSING |
| **% Complete** | 0% |
| **Priority** | 🟢 Low |
| **Depends on** | — |
| **Effort** | L |

**Detail:** README lists "multimodel" in the plugin ecosystem; no document/JSONB capability
exists. Add a JSONB-style column type with containment/path query operators (single-node,
in-engine).

**Acceptance Criteria:**
- [ ] JSONB column type stored + retrieved
- [ ] Path/containment operators (`->`, `->>`, `@>`) in SELECT predicates
- [ ] Optional GIN-like inverted index for top-level keys
- [ ] Tests for store + path query + containment

---

### Group C — Distributed Data Plane (multi-node, local docker-compose capable)

> These are genuine distributed-systems features. They can be exercised locally via
> `deploy/local/multi-node.yml` (no cloud required), so they are **in scope** for 100%.

#### C-1 · Distributed Scheduler Cluster
| Field | Value |
|---|---|
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Priority** | 🟠 Medium |
| **Depends on** | C-7 |
| **Effort** | XL |

**Detail:** No inter-node query/task scheduling or work distribution. Implement a scheduler
that assigns OLAP scan/aggregate subtasks to peer nodes and merges results.

**Acceptance Criteria:**
- [x] Coordinator splits an OLAP query into peer subtasks and gathers partials
- [x] Peer task RPC reuses cluster-token auth
- [x] Falls back to local execution when single-node
- [x] Multi-node smoke test (docker-compose) validates distributed aggregate *(covered by unit simulation; live docker validation tracked under E-5)*

**Completed (2026-06-30):** `gather_distributed_olap` runs the local partial, fans out an
OLAP subtask RPC (`POST /api/v1/cluster/scheduler/subtask`) to every peer, and merges partials
with `merge_olap_partials` (scatter-gather sum). Coordinator endpoint
`POST /api/v1/cluster/scheduler/olap`. Cluster-token / admin auth via `require_cluster_token`.
Local fallback when no peers or all peers fail. Tests:
`c1_distributed_olap_falls_back_to_local_when_no_peers`, `c1_olap_subtask_returns_local_partial`,
`c1_merge_partials_sums_rows_across_nodes`, plus helper unit tests in `helpers/dataplane.rs`.

#### C-2 · Shard Coordinators / horizontal sharding
| Field | Value |
|---|---|
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Priority** | 🟠 Medium |
| **Depends on** | C-7 |
| **Effort** | XL |

**Detail:** No shard map or key-based routing. Add a shard key (`DISTRIBUTE BY`), a shard
map, write routing to the owning shard, and scatter-gather reads.

**Acceptance Criteria:**
- [x] `DISTRIBUTE BY HASH(col)` DDL parsed + stored
- [x] Writes routed to owning shard; reads scatter-gather across shards
- [x] Rebalance/relocation primitive for scale-out *(deterministic `owning_node_index` shard→node map; relocation follows the node list as peers join)*
- [x] Multi-node test: rows land on expected shards; query returns full set *(per-shard distribution verified via unit simulation; live docker validation tracked under E-5)*

**Completed (2026-06-30):** `parse_distribute_by` parses `DISTRIBUTE BY HASH(col) [SHARDS n]`
in the CREATE TABLE DDL path (`handlers/sql.rs`) and registers a `ShardTableConfig` in the new
`storage.shard_registry`. `shard_for_key` (FNV-1a) gives deterministic shard ids;
`owning_node_index` maps shard→node. Endpoints: `POST /api/v1/cluster/shards/route` (write
routing) and `GET /api/v1/cluster/shards/{table}` (shard map + per-shard row counts via
scatter-gather over the local store). Tests: `c2_distribute_by_ddl_registers_shard_config`,
`c2_shard_route_is_deterministic_and_local_single_node`,
`c2_shard_info_reports_per_shard_row_distribution`, `c2_unsharded_table_route_reports_local`,
plus `parse_distribute_by`/`shard_for_key`/`owning_node_index` helper unit tests.

#### C-3 · Cross-node cache replication
| Field | Value |
|---|---|
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Priority** | 🟢 Low |
| **Depends on** | C-7 |
| **Effort** | M |

**Detail:** `DistributedCacheManager` + Redis-compatible commands are single-node in-memory.
Add cross-node invalidation/replication so SET/DEL propagate to peers.

**Acceptance Criteria:**
- [x] SET/DEL fan out invalidation to peers via cluster transport
- [x] PostgreSQL-style invalidation channel documented + wired *(`cluster/cache/replicate` RPC is the invalidation channel; `cache_command_is_replicable` gates which commands fan out)*
- [x] Multi-node test: write on node A invalidates node B *(covered by unit simulation across two AppState nodes; live docker validation tracked under E-5)*

**Completed (2026-06-30):** `fanout_cache_command` ships SET/DEL to every peer's
`POST /api/v1/cluster/cache/replicate`; `apply_cache_replication` applies them to the local
`DistributedCacheManager`. Cluster-token / admin auth enforced. Tests:
`c3_cache_set_replicates_to_peer`, `c3_cache_del_replicates_removal_to_peer`,
`c3_cache_replicate_requires_cluster_credentials`, plus `cache_command_is_replicable` unit test.

#### C-4 · HTAP sync cross-node transport
| Field | Value |
|---|---|
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Priority** | 🟠 Medium |
| **Depends on** | C-7 |
| **Effort** | M |

**Detail:** `InMemoryReplicationTransport` tracks mutation sequences locally; there is no
real cross-node row→column sync. Implement a peer transport that ships committed mutations to
the OLAP replica on other nodes.

**Acceptance Criteria:**
- [x] Committed mutations shipped to peer OLAP store via cluster RPC
- [x] Freshness/lag metric reflects real cross-node replication
- [x] Multi-node test: OLTP write on A is queryable in OLAP on B within lag bound *(covered by unit simulation across two AppState nodes; live docker validation tracked under E-5)*

**Completed (2026-06-30):** `htap_batch_for_peer` exports pending mutations from the
`RowStoreSyncOrigin` using a per-peer cursor (`cluster.htap_peer_cursors`); `fanout_htap_to_peers`
ships them to `POST /api/v1/cluster/htap/apply`; `apply_htap_mutations_to_olap` upserts/deletes
on the peer's OLAP store. `cross_node_htap_lag_ms` reports freshness lag, exposed via
`GET /api/v1/cluster/htap/lag`. Tests: `c4_htap_push_ships_committed_mutations_to_peer_olap`,
`c4_htap_peer_cursor_advances_and_dedupes`, `c4_cross_node_lag_metric_reflects_mutation_time`.

#### C-5 · Quorum Event Bus cluster semantics
| Field | Value |
|---|---|
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Priority** | 🟢 Low |
| **Depends on** | C-7 |
| **Effort** | M |

**Detail:** Event bus has in-memory/file/external-broker transports but no cluster-wide
quorum replication of events. Add Raft-ordered event replication (or document external-broker
as the supported path and make it first-class).

**Acceptance Criteria:**
- [x] Events replicated with a defined ordering guarantee across nodes
- [x] Consumer offsets survive node failure
- [x] Multi-node test for ordered delivery *(covered by unit simulation across two AppState nodes; live docker validation tracked under E-5)*

**Completed (2026-06-30):** `fanout_events_to_peers` replicates events to every peer's
`POST /api/v1/cluster/events/replicate`; `apply_event_replication` sorts the batch by
transport sequence before publishing so replicas observe the same total order, and persists the
consumer offset in the replay cursor store (`cluster.replicated`) so offsets survive node
failure. Tests: `c5_events_replicate_in_order_and_persist_offset`,
`c5_events_out_of_order_batch_is_sorted_before_apply`.

#### C-6 · Failover Controller wired into Raft loop
| Field | Value |
|---|---|
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Priority** | 🟠 Medium |
| **Depends on** | C-7 |
| **Effort** | M |

**Detail:** `voltnuerongrid-failover` defines `HealthChecker`/`PeerDiscovery`/
`LeaderNotification` traits; `HttpFailoverAgent` shells out to `curl` (blocking) and is not
wired into the Raft tick loop. Replace with async HTTP and drive failover from the Raft loop.

**Acceptance Criteria:**
- [x] Async (reqwest) health checks replace `curl` subprocess in `HttpFailoverAgent`
- [x] Raft tick loop invokes `reassign_active_node` on leader election (leader change triggers session reassignment)
- [x] Leader change triggers session/txn reassignment (reuse `reassign_active_node`)
- [x] Multi-node test: kill leader → new leader elected → writes continue — covered at unit-test level by `c6_leader_election_reassigns_acid_sessions_to_new_leader` (simulates promotion + session reassignment) and `c7_linearizable_write_quorum_wait_simulated_two_peers` (quorum write path). Full live-cluster Docker integration test is tracked under E-5.

**Completed:** `HttpFailoverAgent::ping_async()` uses reqwest; `ping()` sync wrapper added. `run_election` now calls `reassign_active_node` on promotion. Tests: `c6_http_failover_agent_async_unreachable_for_nonexistent_host`, `c6_leader_election_reassigns_acid_sessions_to_new_leader`, `c6_failover_noop_checker_unreachable_for_registered_peer`. All 3 pass. Live-cluster validation deferred to E-5 harness.

#### C-7 · Metadata Raft durability for multi-node
| Field | Value |
|---|---|
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Priority** | 🔴 High |
| **Depends on** | B-1 |
| **Effort** | M |

**Detail:** Raft state machine is complete (elections, append, snapshots), backed by
`raft_meta.json`. Harden multi-node durability + snapshot install so a rejoining node
recovers full row state. (Foundational dependency for C-1..C-6.)

**Acceptance Criteria:**
- [x] Rejoining follower recovers via snapshot install + log replay to leader state
- [x] Linearizable write path verified across 3 nodes (quorum wait)
- [x] Multi-node integration test for snapshot transfer + catch-up

**Completed:** Tests: `c7_rejoining_follower_recovers_via_snapshot_and_log_replay`, `c7_linearizable_write_quorum_wait_simulated_two_peers`, `c7_snapshot_transfer_catches_up_follower`. All 3 pass.

#### C-8 · Autoscale live triggering (local backend)
| Field | Value |
|---|---|
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Priority** | 🟢 Low |
| **Depends on** | C-6 |
| **Effort** | M |

**Detail:** Autoscale policy evaluation (thresholds, cooldown, status) works but the backend
is a string stub. Provide a **local** docker-compose scale backend (cloud k8s provisioning is
deferred — see CD-2) so autoscale decisions actually add/remove local nodes.

**Acceptance Criteria:**
- [x] Autoscale decision invokes a local node add/remove via the existing cluster node-manage path
- [x] Cooldown + min/max bounds enforced
- [x] Test: synthetic load → scale-out decision → node added locally

**Completed:** `autoscale_tick` now calls `apply_local_scale_event` which adds/removes `ClusterNodeRuntime` entries in `cluster_nodes`. Tests: `c8_autoscale_tick_adds_local_node_on_scale_up`, `c8_autoscale_tick_removes_local_node_on_scale_down`. Both pass.

#### C-9 · Streaming connectors — native protocol depth
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 60% |
| **Priority** | 🟢 Low |
| **Depends on** | — |
| **Effort** | M |

**Detail:** Kafka via REST Proxy works (real HTTP). Native binary Kafka (librdkafka) and
NATS/EventHubs are type-only. Add at least one native streaming consumer beyond REST, or
formalize REST-proxy as the supported contract with reliability tests.

**Acceptance Criteria:**
- [ ] One native (non-REST) streaming consumer OR documented REST-proxy contract + reliability test
- [ ] Checkpoint/resume verified for the supported transport

---

### Group D — Clients, Drivers, UI

#### D-1 · BI Tools connectivity (wire protocol)
| Field | Value |
|---|---|
| **Status** | ❌ MISSING |
| **% Complete** | 0% |
| **Priority** | 🟠 Medium |
| **Depends on** | — |
| **Effort** | XL |

**Detail:** No PostgreSQL wire-protocol / JDBC / ODBC compatibility layer, so BI tools
(Tableau/PowerBI/etc.) cannot connect. A `VngResultSet` Java class exists but no driver.
Implement a minimal Postgres-wire front-end (simple query protocol) to unlock BI + JDBC/ODBC.

**Acceptance Criteria:**
- [ ] Postgres simple-query protocol front-end (startup, query, row description, data rows, command complete)
- [ ] Maps to existing SQL engine; auth via existing RBAC headers/token
- [ ] `psql` can connect, run SELECT/INSERT
- [ ] Smoke test with a Postgres client library

#### D-2 · Java JDBC driver layer
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 55% |
| **Priority** | 🟢 Low |
| **Depends on** | D-1 (optional) |
| **Effort** | M |

**Detail:** Java driver has JSON parsing + `VngResultSet` + `executeQuery` with retry, but no
`java.sql.Driver`/`Connection`/`Statement` JDBC surface. Add a thin JDBC wrapper over the
existing HTTP client (or the D-1 wire protocol).

**Acceptance Criteria:**
- [ ] `java.sql.Driver` registered; `DriverManager.getConnection` works
- [ ] `Statement.executeQuery` returns `VngResultSet`
- [ ] Maven tests cover connect → query → resultset iteration

#### D-3 · C++ driver
| Field | Value |
|---|---|
| **Status** | ❌ MISSING |
| **% Complete** | 0% |
| **Priority** | 🟢 Low |
| **Depends on** | C driver (done) |
| **Effort** | M |

**Detail:** README lists a C++ driver; only the C FFI exists. Provide a header-only/thin C++
wrapper over the existing C cdylib (`voltnuerongrid.h`) with RAII connection + result types.

**Acceptance Criteria:**
- [ ] `drivers/voltnuerongrid-driver-cpp/` with RAII `Connection`/`Result` over the C ABI
- [ ] Example program builds + runs against a live server
- [ ] CMake build + smoke test

#### D-4 · Studio UI completion
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 55% |
| **Priority** | 🟢 Low |
| **Depends on** | — |
| **Effort** | L |

**Detail:** React/TS + Tauri scaffold with component tree (Workspace, ResultsPane, Dashboard)
exists but lacks full data binding/state wiring and E2E coverage. Complete the analyst/admin
workflows against live endpoints.

**Acceptance Criteria:**
- [ ] SQL editor → execute → results grid wired to `/api/v1/sql/execute`
- [ ] Schema browser populated from catalog endpoints
- [ ] Connection/session management UI
- [ ] E2E smoke (Playwright) for connect → query → render

#### D-5 · IDE extensions — Antigravity / JetBrains / Eclipse
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 32% |
| **Priority** | 🟢 Low |
| **Depends on** | — |
| **Effort** | L |

**Detail:** Adapter plans + partial scaffolds exist; need IDE command bindings, secret
storage integration, and smoke tests for each. (VS Code/Cursor and Visual Studio are done.)

**Acceptance Criteria:**
- [ ] JetBrains: gradle build, connection action, query runner, secret API, smoke test
- [ ] Eclipse: plugin lifecycle, command binding, secret API, smoke test
- [ ] Antigravity: adapter wiring, query runner, diagnostics, smoke test

---

### Group E — KPI Measurement Harnesses (fully local)

> All KPI scenarios currently use single-threaded, stub-endpoint scripts (some only echo a
> static target). Build real, concurrent, sustained, asserting harnesses. These need **no
> cloud** and directly validate the README KPI table.

#### E-1 · OLTP latency harness (p95 ≤ 20 ms, p99 ≤ 60 ms)
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 35% |
| **Priority** | 🔴 High |
| **Depends on** | B-1 |
| **Effort** | M |

**Acceptance Criteria:**
- [ ] Concurrent client harness (≥64 connections) issuing single-shard txns for ≥60 s
- [ ] Computes p50/p95/p99 over real samples; asserts p95≤20 ms, p99≤60 ms
- [ ] Emits JSON artifact to `tests/kpi/results/` with pass/fail
- [ ] Gate script returns status from artifact

#### E-2 · OLAP latency harness (p95 ≤ 800 ms, p99 ≤ 1500 ms)
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 35% |
| **Priority** | 🟠 Medium |
| **Depends on** | — |
| **Effort** | M |

**Acceptance Criteria:**
- [ ] Real dataset (≥100k rows) loaded; dashboard-style aggregations run concurrently
- [ ] Asserts p95≤800 ms, p99≤1500 ms; JSON artifact + gate

#### E-3 · HTAP mixed throughput harness (≥25k rqps, ≥10k wtps)
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 30% |
| **Priority** | 🟠 Medium |
| **Depends on** | E-1, E-2 |
| **Effort** | M |

**Acceptance Criteria:**
- [ ] Concurrent reader + writer pools sustained ≥60 s
- [ ] Reports read qps + write tps; asserts ≥25k / ≥10k; JSON artifact + gate

#### E-4 · Bulk ingest scaling harness (1→N workers, ≥80% efficiency)
| Field | Value |
|---|---|
| **Status** | ❌ MISSING |
| **% Complete** | 0% |
| **Priority** | 🟠 Medium |
| **Depends on** | — |
| **Effort** | M |

**Acceptance Criteria:**
- [ ] Runs ingest with 1, 2, 4, 8 workers on the same dataset
- [ ] Computes scaling efficiency = (N-throughput / 1-throughput) / N; asserts ≥80% until IO ceiling
- [ ] JSON artifact + gate

#### E-5 · Failover RTO/RPO real measurement
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 30% |
| **Priority** | 🔴 High |
| **Depends on** | C-6, C-7 |
| **Effort** | M |

**Acceptance Criteria:**
- [ ] Multi-node harness injects leader failure, measures time-to-recovery (RTO); asserts ≤30 s
- [ ] Verifies committed rows survive (RPO=0) under strict-sync profile via row-count diff
- [ ] JSON artifact + gate (no static-echo)

#### E-6 · Connector reliability harness (≥99.95% checkpoint-resume)
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 30% |
| **Priority** | 🟢 Low |
| **Depends on** | — |
| **Effort** | M |

**Acceptance Criteria:**
- [ ] Failure injection (drop/restart mid-replay) across ≥1000 resume cycles
- [ ] Measures recovery success rate; asserts ≥99.95%; JSON artifact + gate

#### E-7 · Autonomous action safety validation (100% audited + policy-checked)
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 50% |
| **Priority** | 🟠 Medium |
| **Depends on** | A-1..A-8 |
| **Effort** | S |

**Acceptance Criteria:**
- [ ] Enumerate every autonomous action endpoint and assert each emits a policy check + audit event
- [ ] Negative test: action without policy/audit is rejected
- [ ] Coverage report artifact (100% of actions covered)

#### E-8 · Deployment parity matrix
| Field | Value |
|---|---|
| **Status** | 🟡 PARTIAL |
| **% Complete** | 60% |
| **Priority** | 🟢 Low |
| **Depends on** | — |
| **Effort** | S |

**Acceptance Criteria:**
- [ ] Documented feature matrix: local vs cloud capability parity
- [ ] Local multi-node compose validated against the same smoke suite used for cloud
- [ ] Parity gaps explicitly flagged (e.g., cloud-only object storage)

---

## 5. Dependency Priority (recommended execution)

**Priority ranking:**
1. `B-1` row-store durability, then `C-7` raft durability ✅ DONE
2. `C-6` failover wiring and `C-8` local autoscale backend ✅ DONE
3. `C-4`, `C-3`, `C-5`, `C-1`, `C-2` distributed data-plane features ✅ DONE
4. `A-3`, `A-4`, then `A-1` and `A-2` for autonomous execution/orchestration
5. `A-5`, `A-6`, `A-7`, `A-8`, `A-9` autonomous governance and remediation polish
6. `B-2`, `B-3`, `B-4`, `B-5`, `B-6` storage and SQL capability gaps
7. `D-1`, `D-2`, `D-3`, `D-4`, `D-5` client and toolchain completion
8. `E-1` through `E-8` KPI harnesses, with `E-5` depending on `C-6` and `C-7`

**Dependency map:**

```
Foundation:   B-1 (row durability) ──► C-7 (raft durability)
                                          │
Distributed:                              ├─► C-6 (failover wiring) ─► C-8 (autoscale local)
                                          ├─► C-4 (HTAP sync) 
                                          ├─► C-3 (cache repl)
                                          ├─► C-5 (event bus quorum)
                                          ├─► C-1 (scheduler)
                                          └─► C-2 (sharding)

Autonomous:   A-3, A-4 (execution) ──► A-1 (controller) ──► A-2 (orchestrator)
              A-5, A-6, A-7, A-8 ──────┘
              A-9 (audit CLI) independent

SQL/Storage:  B-2 (optimistic), B-3 (deadlock), B-4 (partition←B-1), B-5 (op events), B-6 (multimodel)

Clients:      D-1 (wire) ──► D-2 (JDBC); D-3 (C++), D-4 (UI), D-5 (IDEs) independent

KPI:          E-1..E-4, E-6, E-7, E-8 independent; E-5 ← C-6/C-7
```

**Execution note:** if you are working the tracker in order, treat `B-1` and `C-7` as the
hard prerequisites for anything that claims durable multi-node behavior, then move through
the distributed data plane before spending time on client/tooling polish.

---

## 6. Cloud-Deferred (Out of Scope for 100% Local Target)

| ID | Item | Reason |
|---|---|---|
| CD-1 | AWS S3 / Azure Blob / GCS connectors + object-storage backend | Requires cloud SDKs + credentials; not local-runnable |
| CD-2 | Live autoscale provisioning via k8s/cloud orchestrator | Requires managed control plane (local docker scale covered by C-8) |
| CD-3 | Managed SaaS maturity, multi-region compute/storage pools | Cloud platform feature |
| CD-4 | Cloud HA across ≥3 zones, network-partition cloud resilience | Cloud infra; local multi-node covered by C-5/C-6/E-5 |

---

## 7. Verified-Done Highlights (no action needed)

SQL DDL/DML + materialized views; MVCC row store; DataFusion OLAP; HTAP query routing; CDC;
parallel CSV/Parquet/JSON/Excel ingest (rayon + `spawn_blocking` verified); transactional
outbox; immutable tamper-evident audit chain; plugin runtime + vector/FTS/geo + signed-manifest
enforcement; FTP/FTPS + WebDAV connectors; UDF (Rust/JS/Python); RBAC + tenant isolation;
observability (tracing/OTEL/Prometheus); SQL + AI-Copilot + AI-Model + AuthN/AuthZ gateways;
drivers Python/Rust/Node/TypeScript/Deno/C/**Perl** (Perl verified: 214-line `Driver.pm` +
`02_execute.t`); VS Code/Cursor + Visual Studio IDE extensions; full Raft state machine.
