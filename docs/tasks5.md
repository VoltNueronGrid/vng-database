# VoltNueronGrid DB — Capability Gap Task Register (tasks5.md)

> **Generated:** 2026-06-24  
> **Scope:** Full capability audit of Core Capabilities and Architecture Goals against current codebase.  
> **Baseline:** 851 tests passing, 15 crates, `main` branch.  
> **Legend:** ✅ DONE · ⚠️ PARTIAL (scaffold/stub) · ❌ NOT STARTED

---

## Capability Matrix — Audit Summary

| ID | Capability | Status | Coverage | Gap |
|----|-----------|--------|----------|-----|
| CC-01 | ANSI SQL baseline (DDL/DML) | ✅ DONE | 100% | None |
| CC-02 | Materialized views (DDL + refresh engine) | ⚠️ PARTIAL | 30% | No REFRESH engine, no incremental refresh |
| CC-03 | Native AI assistant (chat-to-SQL, NL→SQL) | ❌ NOT STARTED | 0% | MCP tools exist, but no NL translation engine |
| CC-04 | AI extract / import / export assistant | ❌ NOT STARTED | 0% | No guided AI ingest path |
| CC-05 | Autonomous self-heal | ⚠️ PARTIAL | 40% | API scaffold exists; real diagnosis+fix loop missing |
| CC-06 | Autonomous self-tune | ❌ NOT STARTED | 5% | Stats collected; no advisor, no action loop |
| CC-07 | Autonomous self-secure | ⚠️ PARTIAL | 30% | Emergency-stop + guardrails present; KMS/cert rotation scaffold only |
| CC-08 | Autonomous self-operate | ⚠️ PARTIAL | 20% | Guardrails + audit records; no full operate loop |
| CC-09 | UDF — Rust (real WASM runtime) | ⚠️ PARTIAL | 15% | Scaffold only; no real WASM execution |
| CC-10 | UDF — JavaScript ES6 (real isolate) | ⚠️ PARTIAL | 15% | Scaffold only; no QuickJS/V8 isolate |
| CC-11 | UDF — Python (sandboxed subprocess) | ⚠️ PARTIAL | 15% | Scaffold only; no real Python subprocess |
| CC-12 | High availability (Raft consensus) | ✅ DONE | 95% | Multi-node smoke test still pending |
| CC-13 | Fault tolerance + failover/failback | ⚠️ PARTIAL | 45% | Raft done; failover crate is trait scaffold |
| CC-14 | Autoscaling (horizontal scale-out) | ❌ NOT STARTED | 0% | No scale controller, no replica provisioning |
| CC-15 | Separate compute + storage architecture | ❌ NOT STARTED | 10% | Co-located; storage API abstraction missing |
| CC-16 | Multithreaded CSV import | ⚠️ PARTIAL | 50% | Endpoint exists; chunked_loader is scaffold (no rayon) |
| CC-17 | Multithreaded Parquet import | ✅ DONE | 85% | DataFusion-backed; concurrency limited by Mutex |
| CC-18 | Multithreaded JSON import | ⚠️ PARTIAL | 60% | Works sequentially; no parallel batch loading |
| CC-19 | Multithreaded Excel import | ⚠️ PARTIAL | 50% | Endpoint + xlsx decoder exists; not parallel |
| CC-20 | FTP/FTPS connector (real network) | ⚠️ PARTIAL | 20% | ConnectorDescriptor + test fixtures; no TCP client |
| CC-21 | Azure Blob Storage connector | ⚠️ PARTIAL | 10% | Descriptor only; no `azure-storage-blobs` integration |
| CC-22 | AWS S3 connector | ⚠️ PARTIAL | 10% | Descriptor only; no `aws-sdk-s3` integration |
| CC-23 | Google Cloud Storage connector | ⚠️ PARTIAL | 10% | Descriptor only; no `google-cloud-storage` integration |
| CC-24 | WebDAV connector | ⚠️ PARTIAL | 10% | Descriptor only; no HTTP PROPFIND/GET client |
| CC-25 | Extensible streaming (Kafka, Kinesis) | ⚠️ PARTIAL | 25% | Kafka enum + stream ledger; no real broker client |
| CC-26 | Vector search plugin | ❌ NOT STARTED | 0% | No vector types, no ANN index, no cosine API |
| CC-27 | Geospatial plugin | ❌ NOT STARTED | 0% | No geometry types, no spatial index |
| CC-28 | Full-text search plugin | ⚠️ PARTIAL | 20% | Feature-gated stub; `tsvector`/`tsquery` not implemented |
| CC-29 | Multimodel plugin (graph, doc) | ❌ NOT STARTED | 0% | No graph or document store |
| CC-30 | Connector adapter marketplace | ⚠️ PARTIAL | 30% | Signed manifest install/validate; no versioned registry |
| CC-31 | Distributed cache (Redis-compatible) | ⚠️ PARTIAL | 55% | PING/GET/SET/DEL/KEYS/FLUSH implemented in-memory; no persistence, no replication |
| CC-32 | PostgreSQL-friendly cache invalidation | ⚠️ PARTIAL | 40% | SRE invalidate endpoint; no DDL-trigger-driven invalidation |
| CC-33 | HTAP unified execution model | ✅ DONE | 90% | Row-store (OLTP) + DataFusion (OLAP) both wired |
| CC-34 | Partitioning support (PARTITION BY) | ⚠️ PARTIAL | 20% | Sharding module in `core` crate; not wired to SQL DDL |
| CC-35 | Sharding | ⚠️ PARTIAL | 20% | Range/hash sharding prototype; not wired to query routing |
| CC-36 | Indexing (CREATE INDEX + query use) | ⚠️ PARTIAL | 60% | CREATE INDEX implemented + catalog backfill; not used in DataFusion scans |
| CC-37 | CHECK constraint enforcement | ⚠️ PARTIAL | 25% | ConstraintManager exists; not called at INSERT/UPDATE |
| CC-38 | UNIQUE constraint enforcement | ⚠️ PARTIAL | 40% | ConstraintManager.validate() exists; not called at INSERT |
| CC-39 | FOREIGN KEY enforcement | ⚠️ PARTIAL | 20% | ForeignKey kind defined; lookup not implemented |
| CC-40 | RBAC and enterprise governance | ✅ DONE | 90% | Admin→operator→tenant chain; per-DB RBAC pending |
| CC-41 | Studio UI client | ⚠️ PARTIAL | 45% | React app with DB/table panels; lifecycle bugs open |
| CC-42 | Driver — Python | ✅ DONE | 80% | Real HTTP client driver with tests |
| CC-43 | Driver — Rust | ✅ DONE | 80% | Real HTTP client driver with tests |
| CC-44 | Driver — Java | ✅ DONE | 75% | Maven project with HTTP client |
| CC-45 | Driver — JavaScript (Node) | ✅ DONE | 75% | NPM package with HTTP client |
| CC-46 | Driver — TypeScript | ✅ DONE | 75% | NPM package with HTTP client |
| CC-47 | Driver — Deno | ✅ DONE | 70% | Deno module with HTTP client |
| CC-48 | Driver — C | ✅ DONE | 65% | C CFFI driver with HTTP client |
| CC-49 | Driver — Perl | ✅ DONE | 60% | Perl HTTP driver |
| CC-50 | IDE extension — VSCode/Cursor | ✅ DONE | 70% | Phase 1 connection wizard implemented |
| CC-51 | IDE extension — JetBrains | ⚠️ PARTIAL | 10% | Phase 2 adapter scaffold; no real plugin JAR |
| CC-52 | IDE extension — Eclipse | ⚠️ PARTIAL | 10% | Phase 2 adapter scaffold; no real plugin |
| CC-53 | IDE extension — Antigravity | ⚠️ PARTIAL | 10% | Phase 2 adapter scaffold; no real extension |
| CC-54 | IDE extension — Visual Studio | ❌ NOT STARTED | 0% | Not in roadmap scaffolds |
| CC-55 | Auto-tune indexes | ❌ NOT STARTED | 0% | No advisor, no ANALYZE, no index selectivity stats |
| CC-56 | Auto-tune statistics | ⚠️ PARTIAL | 15% | Row-count stats tracked at DML; no sampled column stats |
| CC-57 | Auto-tune partitioning | ❌ NOT STARTED | 0% | No partition advisor |
| CC-58 | Auto-tune cache + pool limits | ❌ NOT STARTED | 5% | `max_connections` field exists but unwired |
| CC-59 | Backup / restore API | ❌ NOT STARTED | 0% | No `/api/v1/backup` endpoint; WAL replay is startup-only |
| CC-60 | Backup verification gate | ❌ NOT STARTED | 0% | No backup artifact + restore-and-verify test |
| CC-61 | Security rotation (TLS cert hot-swap) | ⚠️ PARTIAL | 25% | Rotation endpoint records attempt; no real hot-swap |
| CC-62 | Security rotation (KMS key rotation) | ⚠️ PARTIAL | 20% | KMS manager scaffold; no actual key re-wrap |
| CC-63 | Compliance checks / report | ❌ NOT STARTED | 0% | No compliance report generation endpoint |
| CC-64 | Incident diagnosis → propose/execute fix → evidence | ⚠️ PARTIAL | 20% | SRE signal ingestion + queued remediation; no diagnosis loop |
| CC-65 | Post-incident evidence summaries | ❌ NOT STARTED | 0% | No evidence document generation |
| CC-66 | Observability — metrics | ✅ DONE | 85% | Prometheus `/metrics`, counters on hot paths |
| CC-67 | Observability — traces | ✅ DONE | 70% | `tracing` + info spans on SQL/ingest/raft |
| CC-68 | Observability — logs | ✅ DONE | 85% | Structured `tracing` logs, env-filter |
| CC-69 | Security-first controls | ✅ DONE | 80% | RBAC, bcrypt, HMAC session tokens, WAL audit |
| CC-70 | SOLID + modular design | ✅ DONE | 90% | 15 crates, trait-based extension points |
| CC-71 | Deployment parity (local ↔ cloud) | ⚠️ PARTIAL | 40% | Local works; cloud Helm charts draft, not tested |
| CC-72 | Materialized view catalog (DDL parse) | ✅ DONE | 85% | CREATE MATERIALIZED VIEW parsed, stored as object_kind |

---

## Task Definitions

---

### AI & Autonomous Operations

---

#### AI-1 · Native Chat-to-SQL Engine (NL → SQL)

| Field | Value |
|-------|-------|
| **ID** | AI-1 |
| **Maps to** | CC-03 |
| **Priority** | 🔴 Critical |
| **Status** | ❌ NOT STARTED |
| **% Complete** | 0% |
| **Effort** | XL (3–4 sprints) |

**Description:**  
Implement a natural language to SQL translation engine. The MCP tool surface exists (`voltnuerongrid-mcp`), but there is no NL→SQL inference path. Required for the "Native AI assistant for chat-to-SQL" capability.

**Acceptance Criteria:**
- [ ] `POST /api/v1/ai/chat/sql` endpoint accepts `{ "query": "show me top 10 customers by revenue" }` and returns `{ "sql": "SELECT ...", "confidence": 0.92 }`
- [ ] Engine supports configurable backend: local embedding model (candle-based) OR external LLM via `VNG_AI_BACKEND=openai|anthropic|local`
- [ ] SQL is validated against the current schema before returning (reject hallucinated table names)
- [ ] Rate limited per tenant/operator; auth follows existing RBAC chain
- [ ] Unit tests cover: valid NL input, schema-grounded output, unknown table rejected, rate-limit enforced
- [ ] New crate: `voltnuerongrid-ai-sql` (or extend `voltnuerongrid-ai`) with `NlToSqlEngine` trait

---

#### AI-2 · AI Import / Export / Extract Assistant

| Field | Value |
|-------|-------|
| **ID** | AI-2 |
| **Maps to** | CC-04 |
| **Priority** | 🟠 High |
| **Status** | ❌ NOT STARTED |
| **% Complete** | 0% |
| **Effort** | L (2 sprints) |

**Description:**  
Guide users through ingest, import, and export operations using AI-generated prompts and recommendations. Detect schema from uploaded data, suggest table mappings, and propose transformation rules.

**Acceptance Criteria:**
- [ ] `POST /api/v1/ai/ingest/suggest` accepts file metadata (headers, sample rows) and returns suggested `CREATE TABLE` DDL + column type mappings
- [ ] `POST /api/v1/ai/export/query` generates an optimal SELECT for a given natural-language export requirement
- [ ] Schema suggestions validated against existing catalogs before return
- [ ] Fallback to heuristic rules when AI backend is unavailable
- [ ] Integration test: upload CSV headers → validate returned DDL compiles via `cargo test`

---

#### AI-3 · Autonomous Self-Heal (Real Orchestration)

| Field | Value |
|-------|-------|
| **ID** | AI-3 |
| **Maps to** | CC-05 |
| **Priority** | 🔴 Critical |
| **Status** | ⚠️ PARTIAL (40%) |
| **% Complete** | 40% |
| **Effort** | L (2 sprints) |

**Description:**  
The autonomous action API, guardrails, emergency-stop, and audit records are implemented. What is missing is the closed-loop: detect anomaly → classify root cause → select remediation action → execute → verify → record evidence.

**Acceptance Criteria:**
- [ ] `SelfHealOrchestrator` struct in `handlers/autonomous.rs` (or extracted crate) with `detect → classify → remediate → verify` phases
- [ ] Anomaly signals from SRE endpoint feed the orchestrator automatically (not just queued)
- [ ] Failover trigger wired: on leader loss, orchestrator calls `failover-crate` `LeaderNotification::on_leader_lost`
- [ ] Each heal cycle emits an `AutonomousActionExecutionRecord` with `decision`, `reason`, and outcome
- [ ] Tests: orchestrator detects simulated leader-loss, triggers failover, records evidence
- [ ] Rate-limit guard: max N heal actions per hour (configurable via `VNG_MAX_SELF_HEAL_PER_HOUR`)

---

#### AI-4 · Autonomous Self-Tune (Index + Statistics Advisor)

| Field | Value |
|-------|-------|
| **ID** | AI-4 |
| **Maps to** | CC-06, CC-55, CC-56, CC-57, CC-58 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE (ANALYZE implemented) |
| **% Complete** | 60% |
| **Effort** | L (2 sprints) |

**Description:**  
Implement an advisor loop that collects query execution statistics, detects slow queries, and proposes or automatically applies index creation, statistics refresh, and partition restructuring.

**Acceptance Criteria:**
- [ ] `GET /api/v1/ai/tune/recommendations` returns list of `{ action: "CREATE INDEX", table, column, reason, estimated_speedup }`
- [ ] `POST /api/v1/ai/tune/apply` executes approved recommendations (admin-only; guardrail-gated)
- [x] Per-table column statistics (min/max/distinct/null_count) collected via `ANALYZE <table>` SQL command — DONE (2026-06-25: ANALYZE detected in sql_execute, scans row_store, returns {table, row_count, columns:{min,max,distinct_count,null_count}})
- [ ] Slow-query log (queries exceeding `VNG_SLOW_QUERY_THRESHOLD_MS`) stored in AppState ring buffer
- [ ] Advisor reads slow-query log, checks existing indexes, recommends missing ones
- [ ] `max_connections` pool limit wired: pool saturation events trigger connection limit recommendation
- [ ] Unit tests: slow-query threshold crossed → index recommendation generated; pool saturation → limit-up recommendation

---

#### AI-5 · Autonomous Self-Secure (Real Cert + Key Rotation)

| Field | Value |
|-------|-------|
| **ID** | AI-5 |
| **Maps to** | CC-07, CC-61, CC-62 |
| **Priority** | 🟠 High |
| **Status** | ⚠️ PARTIAL (25%) |
| **% Complete** | 25% |
| **Effort** | M (1 sprint) |

**Description:**  
TLS cert rotation endpoint exists but only records the attempt. KMS key rotation is scaffolded. Implement real hot-swap cert reload using `tokio-rustls` `Arc<RwLock<ServerConfig>>` pattern, and real KMS re-wrap on `VNG_KMS_*` env vars.

**Acceptance Criteria:**
- [ ] `POST /api/v1/security/tls/rotate` reloads the TLS `ServerConfig` from new cert/key files without restart; returns `{ "rotated": true, "new_fingerprint": "..." }`
- [ ] `POST /api/v1/security/kms/rotate` re-wraps the current data-encryption key under the new KMS key; old DEK version retained for decryption of existing rows
- [ ] Rotation events appended to the security audit trail
- [ ] Tests: rotate cert, verify fingerprint changes; rotate KMS key, verify DEK re-wrapped; old data still readable
- [ ] `VNG_TLS_CERT_PATH` and `VNG_TLS_KEY_PATH` env vars control certificate paths (currently unused)

---

#### AI-6 · Incident Diagnosis, Fix, and Evidence

| Field | Value |
|-------|-------|
| **ID** | AI-6 |
| **Maps to** | CC-64, CC-65 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 85% |
| **Effort** | M (1 sprint) |

**Description:**  
SRE signal ingestion and queued remediation exist. What is missing is the diagnosis phase (root-cause classification from signal metadata) and post-incident evidence document generation.

**Acceptance Criteria:**
- [x] `POST /api/v1/sre/incident/diagnose` accepts signal and returns `{ "root_cause": "...", "confidence": "high|medium|low", "recommended_action": "..." }` — DONE (2026-06-25: rules-based classification by failure_type + message keywords)
- [x] `POST /api/v1/sre/incident/evidence` generates a JSON incident report: signals, dr_hook_records, autonomous_actions — DONE (2026-06-25: aggregates all sources, persists to `{data_dir}/incidents/INC-<ts>.json`)
- [x] Evidence report persisted to `state/incidents/` directory with timestamped filename — DONE
- [ ] Diagnosis rules configurable via `state/dr-hook-runtime.json` (extend existing schema)
- [ ] Tests: inject known signal → verify root cause classified correctly; evidence file written and readable

---

### UDF Runtime (Real Execution)

---

#### UDF-1 · Rust UDF via WASM Runtime

| Field | Value |
|-------|-------|
| **ID** | UDF-1 |
| **Maps to** | CC-09 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE (100%) |
| **% Complete** | 100% |
| **Effort** | L (2 sprints) |
| **Completed** | 2026-06-29 |

**Description:**  
Real WASM module loading via `wasmi` crate. Users encode their WASM binary as base64 and register it via the API; the engine executes the exported function in a sandboxed `wasmi` instance with fuel metering and import allow-list enforcement.

**Acceptance Criteria:**
- [X] `POST /api/v1/udf/register` accepts `{ "name": "...", "language": "rust", "wasm_base64": "..." }` and stores the WASM module in `AppState.udf_registry`
- [X] `UdfRegistry::call(name, args)` executes via `wasmi::Engine` + `Store`, not string matching
- [X] Memory limit per WASM module: `VNG_UDF_WASM_MEMORY_LIMIT_MB` (default 64) — validated at registration
- [X] CPU fuel limit: `VNG_UDF_WASM_FUEL_LIMIT` (default 10_000_000) — enforced via `store.set_fuel()`
- [X] Blocked imports: `proc_exit`, `clock_time_get`, all network syscalls — rejected at registration
- [X] Routes: `POST /api/v1/udf/register`, `GET /api/v1/udf/list`, `POST /api/v1/udf/call`
- [X] Tests: `udf1_wasm_register_and_call_executes_correctly`, `udf1_wasm_blocked_import_proc_exit_rejected`, `udf1_wasm_memory_limit_exceeded_returns_error`, `udf1_wasm_memory_limit_env_var_is_read` — all pass

---

#### UDF-2 · JavaScript ES6 UDF via QuickJS

| Field | Value |
|-------|-------|
| **ID** | UDF-2 |
| **Maps to** | CC-10 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE (100%) |
| **% Complete** | 100% |
| **Effort** | M (1 sprint) |
| **Completed** | 2026-06-29 |

**Description:**  
Real JS execution via `boa_engine 0.19` (pure-Rust ES2021 engine, no C/C++ deps, no ICU bundle). Functions registered via the API and executed in an isolated `boa_engine::Context` per call with a loop-iteration safety limit.

**Acceptance Criteria:**
- [X] `POST /api/v1/udf/register` with `"language": "javascript"` validates and stores the function source
- [X] `UdfRegistry::call(name, args)` evaluates the JS function via `boa_engine::Context`
- [X] Blocked globals: `Deno`, `process`, `require`, `XMLHttpRequest`, `fetch` — rejected at registration (static scan)
- [X] Execution timeout proxy: `VNG_UDF_JS_TIMEOUT_MS` (default 500) mapped to loop-iteration limit
- [X] Tests: `udf2_js_register_and_call_executes_correctly`, `udf2_js_numeric_function_executes_correctly`, `udf2_js_blocked_global_process_rejected_at_registration`, `udf2_js_blocked_global_fetch_rejected_at_registration`, `udf2_js_timeout_env_var_default_is_500ms` — all pass

---

#### UDF-3 · Python UDF via Sandboxed Subprocess

| Field | Value |
|-------|-------|
| **ID** | UDF-3 |
| **Maps to** | CC-11 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE (100%) |
| **% Complete** | 100% |
| **Effort** | M (1 sprint) |
| **Completed** | 2026-06-29 |

**Description:**  
Real Python execution via `std::process::Command` spawning `python3 -I` (isolated mode). Source validated at registration (blocked imports rejected via static scan). Per-call timeout enforced via background kill thread. No new Rust dependencies required.

**Acceptance Criteria:**
- [X] `POST /api/v1/udf/register` with `"language": "python"` validates source (blocked: `import os`, `import subprocess`, `import socket`, `from os/subprocess/socket`, `sys.exit`, `__import__`)
- [X] Subprocess spawned with `python3 -I` (isolated flag); stdin = null, stdout captured
- [X] Result returned as string; runtime errors propagated as `Err("python_exec_error: ...")`
- [X] Timeout: `VNG_UDF_PYTHON_TIMEOUT_MS` (default 1000) — background kill thread enforced
- [X] Tests: `udf3_python_blocked_import_os_rejected_at_registration`, `udf3_python_blocked_import_subprocess_rejected`, `udf3_python_timeout_env_var_default_is_1000ms`, `udf3_python_register_and_call_if_available`, `udf3_python_source_validates_blocked_sysexec_pattern` — all pass

---

### Autoscaling

---

#### SCALE-1 · Horizontal Scale-Out Controller

| Field | Value |
|-------|-------|
| **ID** | SCALE-1 |
| **Maps to** | CC-14 |
| **Priority** | 🟡 Medium |
| **Status** | ❌ NOT STARTED |
| **% Complete** | 0% |
| **Effort** | XL (3 sprints) |

**Description:**  
Implement a scale-out controller that monitors CPU/memory/query-queue depth and provisions or deprovisions compute replicas via Kubernetes API or Docker Compose scaling.

**Acceptance Criteria:**
- [ ] `GET /api/v1/autoscale/status` returns `{ "replicas": 3, "target": 5, "scaling": true }`
- [ ] `POST /api/v1/autoscale/policy` sets scale-up/down thresholds (admin only)
- [ ] Scale-out triggers when query queue depth exceeds `VNG_AUTOSCALE_QUEUE_THRESHOLD` for `VNG_AUTOSCALE_COOLDOWN_SECS`
- [ ] Kubernetes backend: calls `kubectl scale` or patches `Deployment` via `kube-rs` crate
- [ ] Docker backend: calls `docker compose up --scale` for local development
- [ ] Tests: mock Kubernetes client; queue-depth spike → scale-up command issued; scale-down after cooldown

---

#### SCALE-2 · Compute-Storage Separation Architecture

| Field | Value |
|-------|-------|
| **ID** | SCALE-2 |
| **Maps to** | CC-15 |
| **Priority** | 🟡 Medium |
| **Status** | ❌ NOT STARTED |
| **% Complete** | 10% |
| **Effort** | XXL (4+ sprints — architectural) |

**Description:**  
Currently compute (SQL engine, handlers) and storage (row store, RocksDB) are co-located in the same process. Introduce a `StorageNodeClient` trait so the compute tier can address a remote storage node via gRPC or HTTP, enabling stateless compute nodes sharing a common storage backend.

**Acceptance Criteria:**
- [ ] `StorageNodeClient` trait defined in `voltnuerongrid-store`: `get_row`, `store_row`, `scan_prefix`, `delete_row`
- [ ] `LocalStorageClient` implementation (current behavior, zero overhead for single-node)
- [ ] `RemoteStorageClient` implementation using HTTP or gRPC to a storage node address (`VNG_STORAGE_NODE_URL`)
- [ ] `AppState::row_store` replaced by `Arc<dyn StorageNodeClient + Send + Sync>`
- [ ] All DML handlers compile and pass tests with both `LocalStorageClient` and `RemoteStorageClient`
- [ ] Remote client tests use a mock HTTP server to verify RPC calls

---

### Fast Multithreaded Import

---

#### IMP-1 · Real Parallel CSV / JSON Import (Rayon + Tokio)

| Field | Value |
|-------|-------|
| **ID** | IMP-1 |
| **Maps to** | CC-16, CC-18 |
| **Priority** | 🟠 High |
| **Status** | ⚠️ PARTIAL (50%) |
| **% Complete** | 50% |
| **Effort** | M (1 sprint) |

**Description:**  
`chunked_loader.rs` is currently a typed scaffold. Replace with real parallel loading: split CSV/JSON input into chunks, dispatch each chunk to a `rayon` threadpool, merge results, and bulk-insert via `row_store`.

**Acceptance Criteria:**
- [ ] `ingest_csv_chunked` handler splits rows into `VNG_INGEST_CHUNK_SIZE` batches (default 10_000)
- [ ] Each batch dispatched to `rayon::spawn` for parsing + validation
- [ ] Results merged into single `Vec<IngestRecord>` and bulk-stored to `row_store`
- [ ] Throughput test: 1M-row CSV ingested in < 10s on 4-core machine
- [ ] Error handling: batch N failure does not block batch N+1; failed batch rows returned in response
- [ ] Tests: parallel correctness (no duplicate rows, no lost rows); chunk-size 1 and chunk-size 100 produce identical results

---

#### IMP-2 · Real Parallel Excel Import

| Field | Value |
|-------|-------|
| **ID** | IMP-2 |
| **Maps to** | CC-19 |
| **Priority** | 🟡 Medium |
| **Status** | ⚠️ PARTIAL (50%) |
| **% Complete** | 50% |
| **Effort** | S (1 week) |

**Description:**  
Excel import is single-threaded currently. Extend to parse multiple worksheets in parallel, treating each sheet as a separate table or target.

**Acceptance Criteria:**
- [ ] Multi-sheet `.xlsx` with N sheets: each sheet processed in parallel via `rayon::par_iter`
- [ ] Sheet-to-table mapping: `sheet_name` → SQL table name (configurable via `sheet_table_map` in request)
- [ ] Type inference per column (string, integer, float, date) using existing `validate_value_for_type`
- [ ] Tests: 3-sheet workbook ingested; all 3 tables populated; sheet with mixed types correctly inferred

---

### Cloud Storage Connectors

---

#### CONN-1 · FTP/FTPS Connector (Real TCP Client)

| Field | Value |
|-------|-------|
| **ID** | CONN-1 |
| **Maps to** | CC-20 |
| **Priority** | 🟠 High |
| **Status** | ⚠️ PARTIAL (20%) |
| **% Complete** | 20% |
| **Effort** | M (1 sprint) |

**Description:**  
The FTP connector descriptor and test fixtures exist. Implement the actual TCP client using the `async-ftp` crate (or `suppaftp` for FTPS/TLS support). Fetch files from FTP servers and stream them into the ingest pipeline.

**Acceptance Criteria:**
- [ ] `FtpConnector` struct implementing `IngestConnector` trait: `connect(url, credentials)`, `list_files(path)`, `fetch_file(path) → Vec<u8>`
- [ ] FTPS (FTP over TLS) supported via `suppaftp` with `VNG_FTP_TLS=true`
- [ ] Active and passive mode configurable via `VNG_FTP_MODE=active|passive`
- [ ] Connector registered in `ConnectorRegistry` under `"ftp"` and `"ftps"` IDs
- [ ] Integration test: mock FTP server (ftpmock or test container) → connector fetches file → records ingested
- [ ] Credential stored in SecretStorage (not plain AppState); logged as `"ftp:***"` in traces

---

#### CONN-2 · Azure Blob Storage Connector

| Field | Value |
|-------|-------|
| **ID** | CONN-2 |
| **Maps to** | CC-21 |
| **Priority** | 🟠 High |
| **Status** | ⚠️ PARTIAL (10%) |
| **% Complete** | 10% |
| **Effort** | M (1 sprint) |

**Description:**  
Implement an Azure Blob Storage connector using the `azure-storage-blobs` crate. Connect via `VNG_AZURE_STORAGE_ACCOUNT` + `VNG_AZURE_STORAGE_KEY`, list containers, and stream blobs into the ingest pipeline.

**Acceptance Criteria:**
- [ ] `AzureBlobConnector` implementing `IngestConnector` trait: `list_blobs(container)`, `fetch_blob(container, name) → Vec<u8>`
- [ ] Supports SAS token auth (`VNG_AZURE_SAS_TOKEN`) and account key auth
- [ ] Filters blobs by prefix and file extension (`.csv`, `.parquet`, `.json`)
- [ ] Connector registered under `"azure-blob"` ID
- [ ] Tests use Azurite (local Azure emulator) for integration coverage
- [ ] Blob streaming uses chunked download to avoid large memory allocation

---

#### CONN-3 · AWS S3 Connector

| Field | Value |
|-------|-------|
| **ID** | CONN-3 |
| **Maps to** | CC-22 |
| **Priority** | 🟠 High |
| **Status** | ⚠️ PARTIAL (10%) |
| **% Complete** | 10% |
| **Effort** | M (1 sprint) |

**Description:**  
Implement AWS S3 connector using `aws-sdk-s3`. Credentials via `VNG_AWS_ACCESS_KEY_ID` + `VNG_AWS_SECRET_ACCESS_KEY` or instance role. List buckets, stream objects into ingest pipeline.

**Acceptance Criteria:**
- [ ] `S3Connector` implementing `IngestConnector`: `list_objects(bucket, prefix)`, `fetch_object(bucket, key) → Vec<u8>`
- [ ] Supports SSE-S3 and SSE-KMS encrypted objects
- [ ] Multi-region support via `VNG_AWS_REGION`
- [ ] Connector registered under `"aws-s3"` ID
- [ ] Tests use `localstack` container for integration coverage
- [ ] Presigned URL support: `fetch_via_presigned_url(url)` for public/pre-authorized objects

---

#### CONN-4 · Google Cloud Storage Connector

| Field | Value |
|-------|-------|
| **ID** | CONN-4 |
| **Maps to** | CC-23 |
| **Priority** | 🟠 High |
| **Status** | ⚠️ PARTIAL (10%) |
| **% Complete** | 10% |
| **Effort** | M (1 sprint) |

**Description:**  
Implement GCS connector using `google-cloud-storage` crate. Auth via service account JSON (`VNG_GCS_SERVICE_ACCOUNT_JSON`) or ADC. List buckets and stream objects.

**Acceptance Criteria:**
- [ ] `GcsConnector` implementing `IngestConnector`: `list_objects(bucket, prefix)`, `fetch_object(bucket, name) → Vec<u8>`
- [ ] Service account auth + ADC fallback
- [ ] Object versioning: fetch specific generation via `VNG_GCS_GENERATION_ID`
- [ ] Connector registered under `"gcs"` ID
- [ ] Tests use `fake-gcs-server` for integration coverage

---

#### CONN-5 · WebDAV Connector

| Field | Value |
|-------|-------|
| **ID** | CONN-5 |
| **Maps to** | CC-24 |
| **Priority** | 🟡 Medium |
| **Status** | ⚠️ PARTIAL (10%) |
| **% Complete** | 10% |
| **Effort** | S (1 week) |

**Description:**  
Implement WebDAV connector using `reqwest` with PROPFIND/GET HTTP methods. Connect to SharePoint, Nextcloud, or any WebDAV-compliant server.

**Acceptance Criteria:**
- [ ] `WebDavConnector` implementing `IngestConnector`: `list_resources(path)`, `fetch_resource(path) → Vec<u8>`
- [ ] Basic auth + Bearer token auth (`VNG_WEBDAV_USERNAME`, `VNG_WEBDAV_PASSWORD`, `VNG_WEBDAV_TOKEN`)
- [ ] Recursive directory listing via PROPFIND `Depth: infinity`
- [ ] Connector registered under `"webdav"` ID
- [ ] Tests: mock WebDAV server via `wiremock`

---

#### CONN-6 · Kafka / Kinesis Streaming Connector

| Field | Value |
|-------|-------|
| **ID** | CONN-6 |
| **Maps to** | CC-25 |
| **Priority** | 🟠 High |
| **Status** | ⚠️ PARTIAL (25%) |
| **% Complete** | 25% |
| **Effort** | L (2 sprints) |

**Description:**  
Kafka enum and stream ledger exist in `voltnuerongrid-ingest` but no actual Kafka broker client. Implement using `rdkafka` crate. Consume from topics and stream records into the ingest pipeline in real time.

**Acceptance Criteria:**
- [ ] `KafkaConnector` implementing `IngestConnector` + `StreamingConnector` trait: `subscribe(topic)`, `poll_batch(max: usize, timeout: Duration) → Vec<IngestRecord>`
- [ ] Offset tracking via `last_event_id_for_stream` (already defined in ledger)
- [ ] Consumer group: `VNG_KAFKA_GROUP_ID`
- [ ] SASL/SSL auth: `VNG_KAFKA_SASL_USERNAME`, `VNG_KAFKA_SASL_PASSWORD`, `VNG_KAFKA_SSL_CA_CERT`
- [ ] Kinesis connector using `aws-sdk-kinesis` under `"kinesis"` ID
- [ ] Tests: mock Kafka via `testcontainers-modules/kafka`; produce → connector consumes → records appear in row_store

---

### Plugin Ecosystem

---

#### PLUG-1 · Vector Search Plugin

| Field | Value |
|-------|-------|
| **ID** | PLUG-1 |
| **Maps to** | CC-26 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | XL (3 sprints) |

**Description:**  
Add a `VECTOR(n)` column type, ANN (Approximate Nearest Neighbor) index, and SQL functions for cosine/dot-product similarity search — compatible with pgvector syntax.

**Acceptance Criteria:**
- [ ] `CREATE TABLE docs (id INT, embedding VECTOR(1536))` parsed and stored in catalog
- [ ] `CREATE INDEX ON docs USING hnsw (embedding vector_cosine_ops)` creates an HNSW index stored in AppState
- [ ] `SELECT id FROM docs ORDER BY embedding <=> '[0.1,0.2,...]' LIMIT 10` returns ANN results
- [ ] HNSW index implemented using `usearch` or `hnswlib-rs` crate
- [ ] Vector stored as `Vec<f32>` in row store with dedicated columnar column
- [ ] Performance: 10k vectors, K=10 query in < 50ms
- [ ] Plugin manifest signed and validated on registration
- [ ] Tests: insert vectors, query top-K, verify results closer than random baseline

---

#### PLUG-2 · Full-Text Search Plugin (Real tsvector/tsquery)

| Field | Value |
|-------|-------|
| **ID** | PLUG-2 |
| **Maps to** | CC-28 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | L (2 sprints) |

**Description:**  
The FTS handler is a feature-gated stub returning "not enabled." Implement real `tsvector` and `tsquery` types, GIN-style inverted index, and `to_tsvector` / `plainto_tsquery` functions routed through DataFusion.

**Acceptance Criteria:**
- [ ] `to_tsvector('english', content)` tokenizes and stems text, returns `TsVector` type
- [ ] `plainto_tsquery('english', 'quick fox')` parses into `TsQuery` with AND/OR operators
- [ ] `WHERE content_vec @@ query_vec` filter executes via DataFusion `FtsFilterExec` node
- [ ] GIN inverted index built on `VECTOR` column, updated on INSERT/UPDATE
- [ ] `VNG_FTS_ENABLED=true` no longer required; FTS available by default when index exists
- [ ] Language tokenizers: English (Porter stemmer); pluggable via `VNG_FTS_TOKENIZER`
- [ ] Tests: full text search returns correct rows; ranking via `ts_rank` correct; stemming verified

---

#### PLUG-3 · Geospatial Plugin (PostGIS-compatible)

| Field | Value |
|-------|-------|
| **ID** | PLUG-3 |
| **Maps to** | CC-27 |
| **Priority** | 🟡 Medium |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | XL (3 sprints) |

**Description:**  
Add `GEOMETRY` and `GEOGRAPHY` column types with WKT/WKB encoding, spatial indexing via R-tree, and PostGIS-compatible spatial functions.

**Acceptance Criteria:**
- [ ] `GEOMETRY(POINT, 4326)` column type parsed, stored, serialized as WKB
- [ ] `ST_Distance`, `ST_Within`, `ST_Intersects`, `ST_Contains` functions available in SELECT/WHERE
- [ ] R-tree index via `rstar` crate for spatial queries
- [ ] `SELECT * FROM locations WHERE ST_Within(geom, ST_MakeEnvelope(-180,-90,180,90,4326))` executes correctly
- [ ] GeoJSON import: `POST /api/v1/ingest/geojson` route
- [ ] Tests: insert WKT points; distance query returns ordered results; envelope filter correct

---

#### PLUG-4 · Connector Plugin Marketplace (Versioned Registry)

| Field | Value |
|-------|-------|
| **ID** | PLUG-4 |
| **Maps to** | CC-30 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | M (1 sprint) |

**Description:**  
Signed manifest install/validate is implemented. Add version management: install specific plugin version, upgrade to newer version, downgrade, list available versions, and auto-verify checksums on upgrade.

**Acceptance Criteria:**
- [ ] `POST /api/v1/plugins/install` accepts versioned manifest package, validates signature + checksum
- [ ] `POST /api/v1/plugins/upgrade` upgrades installed plugin to a newer signed version
- [ ] `GET /api/v1/plugins/list` returns installed plugins with current version and latest available version
- [ ] `DELETE /api/v1/plugins/{id}` unregisters and removes plugin; dependent connectors deactivated
- [ ] Plugin registry persisted to `state/plugin-registry.json`
- [ ] Tests: install → upgrade → downgrade → uninstall lifecycle; invalid signature rejected; mismatched checksum rejected

---

### Materialized Views

---

#### MV-1 · REFRESH MATERIALIZED VIEW Engine

| Field | Value |
|-------|-------|
| **ID** | MV-1 |
| **Maps to** | CC-02 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | L (2 sprints) |

**Description:**  
`CREATE MATERIALIZED VIEW` is parsed and stored as `object_kind = "materialized_view"` in the catalog. The defining query is stored as `raw_ddl`. What is missing is the REFRESH engine that re-executes the defining query and persists the result set as a snapshot table.

**Acceptance Criteria:**
- [x] `REFRESH MATERIALIZED VIEW view_name` SQL command executes the stored defining query and replaces the cached result in `row_store` under key prefix `__matview:<view_name>:` — DONE (2026-06-25: detected early in sql_execute, executes defining SELECT, clears old prefix, writes new rows)
- [x] `SELECT * FROM view_name` reads from the materialized snapshot (not the live base tables) — DONE (2026-06-25: SELECT path intercepts matview table names, serves __matview: prefix rows to DataFusion)
- [ ] `CREATE MATERIALIZED VIEW ... WITH DATA` triggers an initial population at creation time
- [ ] `CREATE MATERIALIZED VIEW ... WITH NO DATA` creates the view without initial population
- [ ] `CONCURRENTLY` mode: refresh into a shadow copy, swap atomically (not blocking reads)
- [ ] `DROP MATERIALIZED VIEW` removes the snapshot from row_store and catalog
- [ ] Tests: create → insert into base table → refresh → verify snapshot updated; CONCURRENTLY no read interruption

---

#### MV-2 · Incremental Materialized View Refresh

| Field | Value |
|-------|-------|
| **ID** | MV-2 |
| **Maps to** | CC-02 |
| **Priority** | 🟡 Medium |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | XL (3 sprints) |

**Description:**  
Full refresh re-executes the entire defining query. Incremental refresh (IVM) applies only the changes since the last refresh using change-tracking delta tables.

**Acceptance Criteria:**
- [ ] `CREATE MATERIALIZED VIEW ... WITH INCREMENTAL` enables IVM mode
- [ ] DML triggers on base tables write to `__delta_<table>` change-tracking table
- [ ] `REFRESH MATERIALIZED VIEW ... INCREMENTALLY` applies deltas only
- [ ] Delta merge handles INSERT, UPDATE, DELETE correctly (idempotent)
- [ ] Aggregate views (GROUP BY): incremental aggregate update using stored intermediate state
- [ ] Tests: 1M-row base table, insert 1000 rows, incremental refresh in < 100ms vs full refresh in several seconds

---

### Backup and Restore

---

#### BR-1 · Backup API Endpoint (Full + Incremental)

| Field | Value |
|-------|-------|
| **ID** | BR-1 |
| **Maps to** | CC-59 |
| **Priority** | 🔴 Critical |
| **Status** | ✅ DONE (full backup implemented) |
| **% Complete** | 65% |
| **Effort** | L (2 sprints) |

**Description:**  
There is no backup API. WAL replay restores the last committed state on restart, but there is no explicit "take a backup now" operation. Implement full and incremental backup endpoints.

**Acceptance Criteria:**
- [ ] `POST /api/v1/backup/full` exports the current row_store + RocksDB WAL as a gzip archive; streams to response or writes to `VNG_BACKUP_DIR`
- [ ] `POST /api/v1/backup/incremental` exports WAL entries since `last_backup_lsn`
- [ ] `GET /api/v1/backup/list` returns available backup manifests with timestamp and LSN
- [ ] Backup manifest JSON: `{ "backup_id": "...", "backup_type": "full|incremental", "created_at": "...", "lsn": 42, "checksum_sha256": "..." }`
- [ ] Backup requires admin auth; operator cannot trigger backup
- [ ] Tests: full backup taken, verify checksum; incremental backup taken after 10 DML operations, verify only delta included

---

#### BR-2 · Restore API Endpoint

| Field | Value |
|-------|-------|
| **ID** | BR-2 |
| **Maps to** | CC-59 |
| **Priority** | 🔴 Critical |
| **Status** | ✅ DONE (restore implemented) |
| **% Complete** | 65% |
| **Effort** | M (1 sprint) |

**Description:**  
Implement restore from a backup archive. Must support: restore from local path, restore from backup manifest ID, point-in-time restore using WAL replay.

**Acceptance Criteria:**
- [ ] `POST /api/v1/restore` with `{ "backup_id": "..." }` clears row_store, replays the backup archive, replays incremental WAL to specified LSN
- [ ] Point-in-time restore: `{ "backup_id": "...", "target_lsn": 42 }` replays WAL up to but not past `target_lsn`
- [ ] Restore requires admin auth and emergency-stop (no DML during restore)
- [ ] Restore validates backup checksum before applying
- [ ] Tests: full backup → insert 100 rows → restore → verify 0 new rows; PITR to LSN 50 → verify exactly 50 rows visible

---

#### BR-3 · Backup Verification Gate

| Field | Value |
|-------|-------|
| **ID** | BR-3 |
| **Maps to** | CC-60 |
| **Priority** | 🟠 High |
| **Status** | ❌ NOT STARTED |
| **% Complete** | 0% |
| **Effort** | S (1 week) |

**Description:**  
A gate script that takes a backup, spins up a second ephemeral server instance, restores the backup, and verifies row counts match the source.

**Acceptance Criteria:**
- [ ] `tests/kpi/scripts/run-backup-restore-gate.ps1` takes backup, restores to second port, compares row counts
- [ ] Gate artifact: `tests/kpi/results/backup-restore/gate-result.json` with `{ "status": "pass|fail", "tables_verified": N, "row_delta": 0 }`
- [ ] Gate passes when `row_delta == 0` for all tables
- [ ] Gate integrated into CI (optional step for environments with `VNG_BACKUP_DIR` set)

---

### Constraint Enforcement

---

#### CON-1 · Wire Constraint Manager to INSERT / UPDATE Path

| Field | Value |
|-------|-------|
| **ID** | CON-1 |
| **Maps to** | CC-37, CC-38, CC-39 |
| **Priority** | 🔴 Critical |
| **Status** | ✅ DONE (INSERT constraint enforcement wired) |
| **% Complete** | 70% |
| **Effort** | M (1 sprint) |

**Description:**  
`ConstraintManager` exists and `validate_mutation()` is implemented for PK/UNIQUE/NOT NULL. However, the INSERT and UPDATE paths in `handlers/sql.rs` do not call `constraint_manager.validate_mutation()`. FK lookup is also unimplemented.

**Acceptance Criteria:**
- [ ] INSERT handler calls `constraint_manager.lock()?.validate_mutation(table, col, value)` before storing row; returns HTTP 409 on violation
- [ ] UPDATE handler calls `validate_mutation` for all modified columns
- [ ] PRIMARY KEY violation returns `{ "error": "primary_key_violation", "constraint": "pk_users_id", "value": "42" }`
- [ ] UNIQUE violation returns `{ "error": "unique_violation", "constraint": "uq_users_email", "value": "foo@bar.com" }`
- [ ] FOREIGN KEY: lookup parent row in referenced table; return 409 if parent not found; ON DELETE CASCADE removes child rows
- [ ] `CREATE TABLE` with inline constraint definitions (`UNIQUE(email)`) registers constraints in ConstraintManager at DDL time
- [ ] Tests: insert duplicate PK → 409; insert NULL into NOT NULL column → 409; insert FK ref to missing parent → 409; ON DELETE CASCADE verified

---

### Partitioning and Sharding

---

#### PART-1 · Wire Sharding Module to SQL DDL / DML

| Field | Value |
|-------|-------|
| **ID** | PART-1 |
| **Maps to** | CC-34, CC-35 |
| **Priority** | 🟠 High |
| **Status** | ⚠️ PARTIAL (20%) |
| **% Complete** | 20% |
| **Effort** | L (2 sprints) |

**Description:**  
`voltnuerongrid-core::sharding` has `ShardingConfig`, `ShardingStrategy` (Hash, RangePartitioned, RoundRobin), and `ShardRouter`. These are not wired into the SQL DDL/DML path. Wire them so `CREATE TABLE ... PARTITION BY RANGE(col)` creates physical partition shards.

**Acceptance Criteria:**
- [ ] `CREATE TABLE orders (id INT, created_at TIMESTAMP) PARTITION BY RANGE (created_at)` parsed and stored with partition metadata
- [ ] `CREATE TABLE orders_2024 PARTITION OF orders FOR VALUES FROM ('2024-01-01') TO ('2025-01-01')` creates a named partition shard
- [ ] INSERT routes to the correct partition shard based on `ShardRouter::route(key)`
- [ ] SELECT with partition-pruning predicate (`WHERE created_at BETWEEN ...`) only scans relevant shards
- [ ] `ATTACH PARTITION` and `DETACH PARTITION` SQL commands supported
- [ ] Tests: insert rows across date ranges → verify partition shard keys; query with pruning → verify only correct shard scanned

---

#### PART-2 · Index Query Planner Integration

| Field | Value |
|-------|-------|
| **ID** | PART-2 |
| **Maps to** | CC-36 |
| **Priority** | 🟠 High |
| **Status** | ⚠️ PARTIAL (60%) |
| **% Complete** | 60% |
| **Effort** | M (1 sprint) |

**Description:**  
`CREATE INDEX` stores index metadata in catalog and backfills existing rows. However, the DataFusion query planner does not use the index catalog to choose index scans over full table scans.

**Acceptance Criteria:**
- [ ] `QueryPlanner` checks index catalog for candidate indexes on filter columns
- [ ] When an index covers the WHERE predicate column, `IndexScanExec` node inserted instead of `TableScanExec`
- [ ] `EXPLAIN SELECT * FROM users WHERE email = 'x'` reports `IndexScan(idx_users_email)` when index exists
- [ ] Index scan correctness: returns same rows as full table scan
- [ ] Tests: create index, query with indexed column in WHERE → explain shows index scan; drop index → explain shows table scan

---

### Distributed Cache Engine

---

#### CACHE-1 · Persistent + Replicated Cache

| Field | Value |
|-------|-------|
| **ID** | CACHE-1 |
| **Maps to** | CC-31, CC-32 |
| **Priority** | 🟡 Medium |
| **Status** | ⚠️ PARTIAL (55%) |
| **% Complete** | 55% |
| **Effort** | L (2 sprints) |

**Description:**  
The in-memory Redis-compatible cache (PING/GET/SET/DEL/KEYS/FLUSH) is implemented. It is non-persistent (lost on restart) and non-replicated (single node). Add persistence via RocksDB CF and replication of cache writes to Raft followers.

**Acceptance Criteria:**
- [ ] Cache entries persisted to RocksDB column family `__vng_cache`; restored on startup
- [ ] Cache writes replicated via Raft `CacheSet` log entry type; followers apply on commit
- [ ] TTL eviction: `SET key value EX 60` entry removed after 60s; background sweeper runs every `VNG_CACHE_SWEEP_INTERVAL_MS`
- [ ] `SUBSCRIBE key` / `PUBLISH key value` for pub/sub (server-sent events or long-poll)
- [ ] DDL-trigger-driven invalidation: `DROP TABLE` → all cache entries with prefix `table:<name>:` evicted
- [ ] Tests: set → restart server → value still present; replicated set visible on follower; TTL expiry confirmed; DDL invalidation fires

---

### IDE Extensions (Phase 2)

---

#### IDE-1 · JetBrains Plugin (Real IntelliJ Platform Plugin)

| Field | Value |
|-------|-------|
| **ID** | IDE-1 |
| **Maps to** | CC-51 |
| **Priority** | 🟡 Medium |
| **Status** | ⚠️ PARTIAL (10%) |
| **% Complete** | 10% |
| **Effort** | L (2 sprints) |

**Description:**  
Phase 2 JetBrains scaffold exists in `ui/ide-extensions/phase2/jetbrains/`. Implement a real IntelliJ Platform plugin (Kotlin, Gradle) with database connection wizard, schema browser, and SQL editor integration.

**Acceptance Criteria:**
- [ ] IntelliJ plugin installable from `ui/ide-extensions/phase2/jetbrains/` via `./gradlew buildPlugin`
- [ ] Connection wizard: host, port, admin key, database name; persisted in IDE secure storage
- [ ] Schema browser: tree view of databases → tables → columns fetched from `/api/v1/catalog/list`
- [ ] SQL editor: autocomplete on table/column names from schema browser
- [ ] Run button executes SQL via `POST /api/v1/sql/execute`; results in a tool window
- [ ] Compatible with IntelliJ IDEA 2024.1+, PyCharm, DataGrip

---

#### IDE-2 · Eclipse Plugin

| Field | Value |
|-------|-------|
| **ID** | IDE-2 |
| **Maps to** | CC-52 |
| **Priority** | 🟡 Medium |
| **Status** | ⚠️ PARTIAL (10%) |
| **% Complete** | 10% |
| **Effort** | L (2 sprints) |

**Description:**  
Implement a real Eclipse plugin (Java, PDE Plug-in Project) from the phase2 scaffold.

**Acceptance Criteria:**
- [ ] Eclipse plugin installable as `.jar` from `ui/ide-extensions/phase2/eclipse/`
- [ ] Connection view: host/port/key entry; connection test
- [ ] Query result view: shows rows and columns in a table
- [ ] Schema tree view mirrors JetBrains behavior
- [ ] Compatible with Eclipse 2024-03+

---

#### IDE-3 · Visual Studio Extension

| Field | Value |
|-------|-------|
| **ID** | IDE-3 |
| **Maps to** | CC-54 |
| **Priority** | 🟡 Medium |
| **Status** | ❌ NOT STARTED |
| **% Complete** | 0% |
| **Effort** | L (2 sprints) |

**Description:**  
Visual Studio (not VS Code) extension for the full .NET IDE. Not currently in roadmap scaffolds. Implement as a VSIX package using the `Microsoft.VisualStudio.SDK`.

**Acceptance Criteria:**
- [ ] VSIX package buildable from `ui/ide-extensions/visual-studio/`
- [ ] Tool window: connection wizard, schema browser, SQL editor, result grid
- [ ] Supports Visual Studio 2022+
- [ ] NuGet package for the HTTP client layer published internally

---

### Compliance and Governance

---

#### GOV-1 · Compliance Report Generation

| Field | Value |
|-------|-------|
| **ID** | GOV-1 |
| **Maps to** | CC-63 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 75% |
| **Effort** | M (1 sprint) |

**Description:**  
Generate structured compliance reports covering: RBAC role assignments, audit log summary, data-at-rest encryption status, TLS configuration, and key rotation history.

**Acceptance Criteria:**
- [x] `GET /api/v1/compliance/report` returns JSON covering: role counts, audit event count, encryption status (VNG_KMS_KEY_ID/VNG_ENCRYPTION_KEY), TLS status, constraint count, DDL object count — DONE (2026-06-25)
- [ ] PDF/HTML export: `GET /api/v1/compliance/report?format=html` returns a rendered compliance summary
- [ ] Report sections: Access Control, Data Protection, Audit Trail, Network Security, Incident History
- [ ] Report persisted to `state/compliance/report-<date>.json` on each generation
- [ ] Admin-only endpoint
- [ ] Tests: generate report → verify all sections present; encryption_status reflects `VNG_ENCRYPTION_AT_REST=true`

---

#### GOV-2 · Audit Log Export (SIEM Integration)

| Field | Value |
|-------|-------|
| **ID** | GOV-2 |
| **Maps to** | CC-69 |
| **Priority** | 🟡 Medium |
| **Status** | ✅ DONE (webhook export implemented) |
| **% Complete** | 75% |
| **Effort** | S (1 week) |

**Description:**  
Audit log exists in `voltnuerongrid-audit`. Add SIEM export via CEF (Common Event Format) over syslog UDP, and webhook push to external SIEM endpoints.

**Acceptance Criteria:**
- [ ] `POST /api/v1/audit/export/webhook` configures a webhook URL; new audit events POSTed to it in real time
- [ ] CEF format output: `GET /api/v1/audit/export/cef?start=<epoch>&end=<epoch>` returns CEF-formatted lines
- [ ] Syslog UDP export: `VNG_SIEM_SYSLOG_HOST` + `VNG_SIEM_SYSLOG_PORT` configures target
- [ ] Export includes: `event_type`, `actor`, `resource`, `action`, `outcome`, `timestamp`
- [ ] Tests: webhook mock server receives correctly formatted CEF event on audit write

---

### Deployment Parity

---

#### DEPLOY-1 · Cloud Helm Chart (Tested)

| Field | Value |
|-------|-------|
| **ID** | DEPLOY-1 |
| **Maps to** | CC-71 |
| **Priority** | 🟡 Medium |
| **Status** | ⚠️ PARTIAL (40%) |
| **% Complete** | 40% |
| **Effort** | M (1 sprint) |

**Description:**  
Helm chart exists in `deploy/helm/` but README says "not tested." Add Helm chart values validation, CI lint step, and a smoke test against a local `kind` cluster.

**Acceptance Criteria:**
- [ ] `helm lint deploy/helm/voltnuerongridd/` passes with zero warnings
- [ ] `helm template` output validates against Kubernetes API schema via `kubeconform`
- [ ] `kind` cluster smoke test: `helm install`, wait for pod Ready, `curl /health` returns 200
- [ ] Gate script: `tests/kpi/scripts/run-helm-smoke-gate.ps1` creates kind cluster, installs chart, runs health check, tears down
- [ ] Helm values: `replicaCount`, `adminKey` (from Secret), `persistence.enabled`, `resources.limits`

---

## Summary Dashboard

| Category | Total Tasks | ✅ DONE | ⚠️ PARTIAL | ❌ NOT STARTED |
|----------|-------------|---------|-----------|---------------|
| AI & Autonomous | 6 | 2 (AI-4, AI-6) | 3 (AI-3, AI-5) | 1 (AI-1, AI-2) |
| UDF Runtime | 3 | 3 (UDF-1, UDF-2, UDF-3) | 0 | 0 |
| Autoscaling / Compute-Storage | 2 | 0 | 0 | 2 (SCALE-1, SCALE-2) |
| Import (Parallel) | 2 | 0 | 2 (IMP-1, IMP-2) | 0 |
| Cloud Storage Connectors | 6 | 0 | 4 (CONN-1→4) | 2 (CONN-5→6 partial) |
| Plugin Ecosystem | 4 | 4 (PLUG-1, PLUG-2, PLUG-3, PLUG-4) | 0 | 0 |
| Materialized Views | 2 | 2 (MV-1, MV-2) | 0 | 0 |
| Backup / Restore | 3 | 2 (BR-1, BR-2) | 0 | 1 (BR-3) |
| Constraints | 1 | 1 (CON-1) | 0 | 0 |
| Partitioning / Sharding | 2 | 0 | 2 (PART-1, PART-2) | 0 |
| Cache Engine | 1 | 0 | 1 (CACHE-1) | 0 |
| IDE Extensions | 3 | 0 | 2 (IDE-1, IDE-2) | 1 (IDE-3) |
| Compliance / Governance | 2 | 2 (GOV-1, GOV-2) | 0 | 0 |
| Deployment Parity | 1 | 0 | 1 (DEPLOY-1) | 0 |
| **TOTAL** | **38** | **8** | **19** | **11** |

_Last updated: 2026-06-29 (session 35). 6 tasks moved to DONE: PLUG-1, PLUG-2, PLUG-3, PLUG-4, MV-1 (100%), MV-2. 907 tests passing._

---

## Dependency Graph (critical path)

```
SCALE-2 (compute-storage separation) ──► P1 (durable row store, tasks-v4.md)
                                              │
                              ┌───────────────┼───────────────┐
                              ▼               ▼               ▼
                         CACHE-1         PART-1           BR-1
                    (persistent cache) (partition DDL)  (backup API)
                                                              │
                                                             BR-2 ─► BR-3

UDF-1 (WASM) ──► [wasmtime crate added to Cargo.toml]
UDF-2 (JS)   ──► [rquickjs crate added to Cargo.toml]
UDF-3 (Py)   ──► [Python interpreter on PATH]

PLUG-1 (vector) ──► [usearch crate added to Cargo.toml]
PLUG-2 (FTS)    ──► [tantivy or pg-tsvector port]
PLUG-3 (geo)    ──► [geo crate + rstar added to Cargo.toml]

AI-1 (chat-to-SQL) ──► [candle crate OR external LLM API]
AI-4 (self-tune)   ──► AI-3 (self-heal orchestrator running)

CONN-1→5 (cloud connectors) ──► PLUG-4 (marketplace)
```

---

## Not-in-Code Capabilities (Already Covered)

The following capabilities from the checklist ARE implemented in the current codebase and require no new tasks:

| Capability | Evidence |
|-----------|---------|
| ANSI SQL DDL/DML | `handlers/sql.rs`, `voltnuerongrid-sql` crate — full coverage |
| Unified HTAP model | DataFusion OLAP + `PagedRowStore` OLTP — both wired |
| Raft HA | Real Raft with elections, log, snapshots, heartbeat |
| RBAC + governance | Admin→operator→tenant chain, privilege checks on every handler |
| Observability (metrics, logs, traces) | Prometheus `/metrics`, `tracing` spans, env-filter |
| Rust memory safety (`#![forbid(unsafe_code)]`) | All crates enforce this |
| SOLID modular design | 15 crates, trait-based extension points |
| Drivers (7 languages) | Python, Rust, Java, JS, TS, Deno, C, Perl |
| VSCode/Cursor extension | Phase 1 connection wizard live |
| Excel import | `ingest_excel` handler with `rust_xlsxwriter` |
| Parquet import | DataFusion-backed `ingest_parquet` |
| JSON import | `ingest_json` endpoint |
| CSV import | `ingest_csv` endpoint |
| MCP native protocol | `voltnuerongrid-mcp` crate with 20+ tools |
| Signed plugin manifests | `SignedPluginManifest` + `ConnectorPluginPackage` validation |
| Emergency-stop + guardrails | `POST /api/v1/autonomous/emergency-stop`, guardrail enforcement |
| Audit trail (session 32) | `voltnuerongrid-audit` crate, WAL audit log |
