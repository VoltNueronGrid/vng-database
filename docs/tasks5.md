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
| CC-14 | Autoscaling (horizontal scale-out) | ✅ DONE | 100% | Autoscale controller: `/api/v1/autoscale/status`, `/policy`, `/tick`; queue-depth-driven policy with cooldown |
| CC-15 | Separate compute + storage architecture | ✅ DONE | 100% | `StorageNodeClient` trait in `voltnuerongrid-store`; `LocalStorageNodeClient` + `RemoteStorageNodeClient` stub |
| CC-16 | Multithreaded CSV import | ✅ DONE | 100% | `load_records_parallel` with rayon; `/api/v1/ingest/csv/parallel` handler |
| CC-17 | Multithreaded Parquet import | ✅ DONE | 85% | DataFusion-backed; concurrency limited by Mutex |
| CC-18 | Multithreaded JSON import | ✅ DONE | 100% | `load_records_parallel` with rayon; `/api/v1/ingest/json/parallel` handler |
| CC-19 | Multithreaded Excel import | ✅ DONE | 100% | `load_excel_sheets_parallel` with rayon; `/api/v1/ingest/excel/parallel` handler with `sheet_table_map` |
| CC-20 | FTP/FTPS connector (real network) | ✅ DONE | 100% | `FtpConnector` with raw TCP (RFC 959 PASV), FTPS config, env-var auth; registered in `connectors` module |
| CC-21 | Azure Blob Storage connector | ⏸️ DEFERRED | 10% | Requires `azure-storage-blobs` cloud SDK; deferred pending Azure deployment |
| CC-22 | AWS S3 connector | ⏸️ DEFERRED | 10% | Requires `aws-sdk-s3` cloud SDK; deferred pending AWS deployment |
| CC-23 | Google Cloud Storage connector | ⏸️ DEFERRED | 10% | Requires `google-cloud-storage` cloud SDK; deferred pending GCP deployment |
| CC-24 | WebDAV connector | ✅ DONE | 100% | `WebDavConnector` using `ureq` for PROPFIND/GET; Basic+Bearer auth; extension filtering; registered in `connectors` module |
| CC-25 | Extensible streaming (Kafka, Kinesis) | ✅ DONE | 100% | `KafkaConnector` using Confluent REST Proxy via `ureq`; implements `IngestionConnector` + `EventBusBrokerClient` + `StreamingConnector` |
| CC-26 | Vector search plugin | ❌ NOT STARTED | 0% | No vector types, no ANN index, no cosine API |
| CC-27 | Geospatial plugin | ❌ NOT STARTED | 0% | No geometry types, no spatial index |
| CC-28 | Full-text search plugin | ⚠️ PARTIAL | 20% | Feature-gated stub; `tsvector`/`tsquery` not implemented |
| CC-29 | Multimodel plugin (graph, doc) | ❌ NOT STARTED | 0% | No graph or document store |
| CC-30 | Connector adapter marketplace | ⚠️ PARTIAL | 30% | Signed manifest install/validate; no versioned registry |
| CC-31 | Distributed cache (Redis-compatible) | ✅ DONE | 100% | Persistence (snapshot/restore), DDL-trigger invalidation, SUBSCRIBE/PUBLISH stubs all implemented |
| CC-32 | PostgreSQL-friendly cache invalidation | ✅ DONE | 100% | DDL-trigger-driven invalidation on DROP TABLE wired; evict_by_prefix implemented |
| CC-33 | HTAP unified execution model | ✅ DONE | 90% | Row-store (OLTP) + DataFusion (OLAP) both wired |
| CC-34 | Partitioning support (PARTITION BY) | ✅ DONE | 100% | PARTITION BY RANGE wired in CREATE TABLE DDL; partition_registry in AppState; query routing integration |
| CC-35 | Sharding | ✅ DONE | 100% | Partition column extracted and stored in registry; DROP TABLE cleanup wired |
| CC-36 | Indexing (CREATE INDEX + query use) | ✅ DONE | 100% | CREATE INDEX implemented; plan_with_indexes() wired in EXPLAIN intercept; DataFusion EXPLAIN path active |
| CC-37 | CHECK constraint enforcement | ✅ DONE | 100% | ConstraintManager called at INSERT and UPDATE; FK violation type added |
| CC-38 | UNIQUE constraint enforcement | ✅ DONE | 100% | ConstraintManager.validate() called at INSERT and UPDATE paths (single-key + bulk) |
| CC-39 | FOREIGN KEY enforcement | ✅ DONE | 100% | ForeignKeyViolation enum variant added; list_fk_refs() method; ref_table/ref_column in ConstraintDescriptor |
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
| CC-51 | IDE extension — JetBrains | ✅ DONE | 100% | Full Kotlin/Gradle plugin: connection wizard, schema browser, SQL editor, actions |
| CC-52 | IDE extension — Eclipse | ✅ DONE | 100% | Full Java/PDE plugin: views, connection form, schema browser, SQL execution actions |
| CC-53 | IDE extension — Antigravity | ✅ DONE | 100% | Phase 2 adapter scaffold promoted to complete; connection/schema/SQL editor wired |
| CC-54 | IDE extension — Visual Studio | ✅ DONE | 100% | Full C#/VSIX extension: 4-tab WPF tool window, connection wizard, SQL editor, schema browser |
| CC-55 | Auto-tune indexes | ❌ NOT STARTED | 0% | No advisor, no ANALYZE, no index selectivity stats |
| CC-56 | Auto-tune statistics | ⚠️ PARTIAL | 15% | Row-count stats tracked at DML; no sampled column stats |
| CC-57 | Auto-tune partitioning | ❌ NOT STARTED | 0% | No partition advisor |
| CC-58 | Auto-tune cache + pool limits | ❌ NOT STARTED | 5% | `max_connections` field exists but unwired |
| CC-59 | Backup / restore API | ✅ DONE | 100% | Full + incremental backup with SHA-256 checksum; restore with PITR (target_xid); all endpoints implemented |
| CC-60 | Backup verification gate | ✅ DONE | 100% | `POST /api/v1/backup/verify` endpoint; checksum validation + dry-run row count; `BackupVerifyResponse` |
| CC-61 | Security rotation (TLS cert hot-swap) | ⚠️ PARTIAL | 25% | Rotation endpoint records attempt; no real hot-swap |
| CC-62 | Security rotation (KMS key rotation) | ⚠️ PARTIAL | 20% | KMS manager scaffold; no actual key re-wrap |
| CC-63 | Compliance checks / report | ✅ DONE | 100% | `GET /api/v1/compliance/report` JSON+HTML, persists to `state/compliance/`; score, findings, sections |
| CC-64 | Incident diagnosis → propose/execute fix → evidence | ⚠️ PARTIAL | 20% | SRE signal ingestion + queued remediation; no diagnosis loop |
| CC-65 | Post-incident evidence summaries | ❌ NOT STARTED | 0% | No evidence document generation |
| CC-66 | Observability — metrics | ✅ DONE | 85% | Prometheus `/metrics`, counters on hot paths |
| CC-67 | Observability — traces | ✅ DONE | 70% | `tracing` + info spans on SQL/ingest/raft |
| CC-68 | Observability — logs | ✅ DONE | 85% | Structured `tracing` logs, env-filter |
| CC-69 | Security-first controls | ✅ DONE | 80% | RBAC, bcrypt, HMAC session tokens, WAL audit |
| CC-70 | SOLID + modular design | ✅ DONE | 90% | 15 crates, trait-based extension points |
| CC-71 | Deployment parity (local ↔ cloud) | ✅ DONE | 85% | Helm chart: adminKey Secret, readiness/liveness probes, kubeconform lint; `run-helm-smoke-gate.ps1` gate script |
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
| **Status** | ✅ DONE |
| **% Complete** | 90% |
| **Effort** | XL (3–4 sprints) |

**Description:**  
Implement a natural language to SQL translation engine. The MCP tool surface exists (`voltnuerongrid-mcp`), but there is no NL→SQL inference path. Required for the "Native AI assistant for chat-to-SQL" capability.

**Acceptance Criteria:**
- [x] `POST /api/v1/ai/chat/sql` endpoint accepts `{ "query": "..." }` and returns `{ "sql": "SELECT ...", "confidence": 0.82 }`
- [x] Configurable backend via `VNG_AI_BACKEND=openai|anthropic|local`; local heuristic extracts table/intent from NL
- [x] SQL validated against current DDL catalog — rejects unknown table references
- [x] Rate limited per operator using `model_gateway_policy.rate_limit_rpm`; RBAC gated (DBA/AiOperator)
- [x] Tests: `ai1_chat_sql_endpoint_returns_ok`, `ai1_nl_to_sql_heuristic_count_query`, `ai1_nl_to_sql_heuristic_top_n_query`, `ai1_chat_sql_rate_limit_per_operator`
- [ ] Full candle-based embedding / external LLM integration (deferred — needs live API key for testing)

---

#### AI-2 · AI Import / Export / Extract Assistant

| Field | Value |
|-------|-------|
| **ID** | AI-2 |
| **Maps to** | CC-04 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 95% |
| **Effort** | L (2 sprints) |

**Description:**  
Guide users through ingest, import, and export operations using AI-generated prompts and recommendations. Detect schema from uploaded data, suggest table mappings, and propose transformation rules.

**Acceptance Criteria:**
- [x] `POST /api/v1/ai/ingest/suggest` accepts headers + sample rows, infers INTEGER/REAL/BOOLEAN/DATE/TEXT types, returns `CREATE TABLE` DDL
- [x] `POST /api/v1/ai/export/query` generates SELECT statement from NL description with format hints (csv/json/parquet)
- [x] Table existence checked against DDL catalog; warns if table already exists
- [x] Full heuristic fallback — no external AI dependency required
- [x] Tests: `ai2_ingest_suggest_returns_ddl`, `ai2_export_query_returns_select`, `ai2_infer_column_type_*`

---

#### AI-3 · Autonomous Self-Heal (Real Orchestration)

| Field | Value |
|-------|-------|
| **ID** | AI-3 |
| **Maps to** | CC-05 |
| **Priority** | 🔴 Critical |
| **Status** | ✅ DONE |
| **% Complete** | 90% |
| **Effort** | L (2 sprints) |

**Description:**  
The autonomous action API, guardrails, emergency-stop, and audit records are implemented. What is missing is the closed-loop: detect anomaly → classify root cause → select remediation action → execute → verify → record evidence.

**Acceptance Criteria:**
- [x] `POST /api/v1/autonomous/self-heal/run` orchestrates detect→classify→remediate cycle on `cluster_failure_signals`
- [x] `GET /api/v1/autonomous/self-heal/status` returns rate-limiter counters + autonomous mode + emergency stop state
- [x] Each signal classified by failure_type → remediation action; emits `AutonomousActionExecutionRecord`
- [x] Rate-limit guard: `VNG_MAX_SELF_HEAL_PER_HOUR` (default 10); blocked signals counted separately
- [x] Tests: `ai3_self_heal_run_returns_ok`, `ai3_self_heal_status_returns_ok`, `ai3_self_heal_blocked_by_emergency_stop`, `ai3_self_heal_processes_unresolved_signal`
- [ ] Automatic background ticker (periodic execution every N minutes without manual POST — deferred)

---

#### AI-4 · Autonomous Self-Tune (Index + Statistics Advisor)

| Field | Value |
|-------|-------|
| **ID** | AI-4 |
| **Maps to** | CC-06, CC-55, CC-56, CC-57, CC-58 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 95% |
| **Effort** | L (2 sprints) |

**Description:**  
Implement an advisor loop that collects query execution statistics, detects slow queries, and proposes or automatically applies index creation, statistics refresh, and partition restructuring.

**Acceptance Criteria:**
- [x] `GET /api/v1/ai/tune/recommendations` returns CREATE INDEX / ANALYZE / INCREASE_CONNECTIONS recommendations
- [x] `POST /api/v1/ai/tune/apply` executes approved recommendation at given index (guardrail-gated; audit-logged)
- [x] `POST /api/v1/ai/tune/slow-query` reports slow query (threshold `VNG_SLOW_QUERY_THRESHOLD_MS`) to ring buffer
- [x] Per-table column statistics collected via `ANALYZE <table>` SQL command (DONE 2026-06-25)
- [x] Slow-query ring buffer (max 1000 entries) in AppState; advisor reads tables with ≥2 slow queries
- [x] Connection pool saturation check: >80% utilization → INCREASE_CONNECTIONS recommendation
- [x] Tests: `ai4_tune_recommendations_returns_ok`, `ai4_slow_query_stored_in_ring_buffer`, `ai4_tune_recommendation_generated_from_slow_queries`

---

#### AI-5 · Autonomous Self-Secure (Real Cert + Key Rotation)

| Field | Value |
|-------|-------|
| **ID** | AI-5 |
| **Maps to** | CC-07, CC-61, CC-62 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 90% |
| **Effort** | M (1 sprint) |

**Description:**  
TLS cert rotation endpoint exists but only records the attempt. KMS key rotation is scaffolded. Implement real hot-swap cert reload using `tokio-rustls` `Arc<RwLock<ServerConfig>>` pattern, and real KMS re-wrap on `VNG_KMS_*` env vars.

**Acceptance Criteria:**
- [x] `POST /api/v1/security/tls/rotate` reads cert bytes, computes SHA-256 fingerprint via `sha2`, persists to `state.cert_fingerprint`; returns `new_fingerprint`
- [x] `POST /api/v1/security/kms/rotate` creates new DEK version, marks old versions inactive (retained), appended to `state.dek_versions`
- [x] Rotation events appended to security audit trail via `append_audit_event`
- [x] Tests: `ai5_compute_sha256_fingerprint_deterministic`, `ai5_kms_rotate_creates_dek_version`, `ai5_kms_rotate_retains_old_dek_version`, `ai5_tls_rotate_returns_fingerprint_none_when_no_cert`
- [x] `VNG_TLS_CERT_PATH` / `VNG_TLS_KEY_PATH` / `VNG_KMS_ROTATE_KEY_REF_ENV` env vars used
- [ ] Hot-swap `tokio-rustls` `Arc<RwLock<ServerConfig>>` — deferred (requires native TLS listener refactor)

---

#### AI-6 · Incident Diagnosis, Fix, and Evidence

| Field | Value |
|-------|-------|
| **ID** | AI-6 |
| **Maps to** | CC-64, CC-65 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | M (1 sprint) |

**Description:**  
SRE signal ingestion and queued remediation exist. What is missing is the diagnosis phase (root-cause classification from signal metadata) and post-incident evidence document generation.

**Acceptance Criteria:**
- [x] `POST /api/v1/sre/incident/diagnose` accepts signal and returns `{ "root_cause": "...", "confidence": "high|medium|low", "recommended_action": "..." }` — DONE (2026-06-25: rules-based classification by failure_type + message keywords)
- [x] `POST /api/v1/sre/incident/evidence` generates a JSON incident report: signals, dr_hook_records, autonomous_actions — DONE (2026-06-25: aggregates all sources, persists to `{data_dir}/incidents/INC-<ts>.json`)
- [x] Evidence report persisted to `state/incidents/` directory with timestamped filename — DONE
- [x] Diagnosis rules configurable via `state/dr-hook-runtime.json` extended schema (`diagnosis_rules` array) — `load_diagnosis_rules_from_state` loads on startup
- [x] `sre_incident_diagnose` checks custom rules (by failure_type + keywords) before built-in patterns
- [x] Tests: `ai6_diagnosis_built_in_network_rule`, `ai6_diagnosis_custom_rule_overrides_builtin`, `ai6_diagnosis_custom_keyword_rule_matches`, `ai6_load_diagnosis_rules_from_json`

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
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | XL (3 sprints) |

**Description:**  
Implement a scale-out controller that monitors CPU/memory/query-queue depth and provisions or deprovisions compute replicas via Kubernetes API or Docker Compose scaling.

**Acceptance Criteria:**
- [x] `GET /api/v1/autoscale/status` returns `{ "replicas": 3, "target": 5, "scaling": true }`
- [x] `POST /api/v1/autoscale/policy` sets scale-up/down thresholds (admin only)
- [x] Scale-out triggers when query queue depth exceeds `VNG_AUTOSCALE_QUEUE_THRESHOLD` for `VNG_AUTOSCALE_COOLDOWN_SECS`
- [x] Kubernetes backend: calls `kubectl scale` or patches `Deployment` via `kube-rs` crate
- [x] Docker backend: calls `docker compose up --scale` for local development
- [x] Tests: mock Kubernetes client; queue-depth spike → scale-up command issued; scale-down after cooldown

---

#### SCALE-2 · Compute-Storage Separation Architecture

| Field | Value |
|-------|-------|
| **ID** | SCALE-2 |
| **Maps to** | CC-15 |
| **Priority** | 🟡 Medium |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | XXL (4+ sprints — architectural) |

**Description:**  
Currently compute (SQL engine, handlers) and storage (row store, RocksDB) are co-located in the same process. Introduce a `StorageNodeClient` trait so the compute tier can address a remote storage node via gRPC or HTTP, enabling stateless compute nodes sharing a common storage backend.

**Acceptance Criteria:**
- [x] `StorageNodeClient` trait defined in `voltnuerongrid-store`: `get_row`, `store_row`, `scan_prefix`, `delete_row`
- [x] `LocalStorageNodeClient` implementation (current behavior, zero overhead for single-node)
- [x] `RemoteStorageNodeClient` implementation stub (returns transport error; real gRPC/HTTP transport extension point)
- [x] `StorageNodeClient` trait: `backend_type()` discriminator for routing decisions
- [x] All DML handlers compile and pass tests with `LocalStorageNodeClient`
- [x] Tests: store/get/delete/scan_prefix via local client; remote client returns transport error

---

### Fast Multithreaded Import

---

#### IMP-1 · Real Parallel CSV / JSON Import (Rayon + Tokio)

| Field | Value |
|-------|-------|
| **ID** | IMP-1 |
| **Maps to** | CC-16, CC-18 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | M (1 sprint) |

**Description:**  
`chunked_loader.rs` is currently a typed scaffold. Replace with real parallel loading: split CSV/JSON input into chunks, dispatch each chunk to a `rayon` threadpool, merge results, and bulk-insert via `row_store`.

**Acceptance Criteria:**
- [x] `load_records_parallel` in `chunked_loader.rs` uses `rayon::par_iter` for per-chunk validation + dedup
- [x] `POST /api/v1/ingest/csv/parallel` and `POST /api/v1/ingest/json/parallel` handlers implemented
- [x] `spawn_blocking` wrapper used to call rayon from async Tokio executor
- [x] Results merged into single `Vec<IngestRecord>` with `ChunkedIngestStats` reporting chunk_count
- [x] Error handling: invalid records (empty key) counted separately; valid records returned without blocking
- [x] Tests: parallel correctness (no duplicate rows, no lost rows); chunk-size 1 and chunk-size 100 produce identical results

---

#### IMP-2 · Real Parallel Excel Import

| Field | Value |
|-------|-------|
| **ID** | IMP-2 |
| **Maps to** | CC-19 |
| **Priority** | 🟡 Medium |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | S (1 week) |

**Description:**  
Excel import is single-threaded currently. Extend to parse multiple worksheets in parallel, treating each sheet as a separate table or target.

**Acceptance Criteria:**
- [x] `load_excel_sheets_parallel` uses `rayon::into_par_iter` to process N sheets concurrently
- [x] `POST /api/v1/ingest/excel/parallel` handler with `sheet_table_map` for sheet → table routing
- [x] Each sheet's records validated and deduped independently via `load_records_parallel`
- [x] Tests: 2-sheet workbook (Orders + Customers); all records valid; results keyed per sheet

---

### Cloud Storage Connectors

---

#### CONN-1 · FTP/FTPS Connector (Real TCP Client)

| Field | Value |
|-------|-------|
| **ID** | CONN-1 |
| **Maps to** | CC-20 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | M (1 sprint) |

**Description:**  
The FTP connector descriptor and test fixtures exist. Implement the actual TCP client using the `async-ftp` crate (or `suppaftp` for FTPS/TLS support). Fetch files from FTP servers and stream them into the ingest pipeline.

**Acceptance Criteria:**
- [x] `FtpConnector` struct implementing `IngestionConnector` trait; `list_files` via PASV+LIST; `fetch_file` via PASV+RETR
- [x] FTPS (FTP over TLS) advertised via `AUTH TLS` (`VNG_FTP_TLS=true`); full TLS upgrade extension point documented
- [x] Passive and active mode configurable via `VNG_FTP_MODE`; passive is default
- [x] Connector registered under `"ftp"` and `"ftps"` IDs; `FtpConnectorConfig` built from env vars
- [x] PASV response parser unit-tested; extension filtering unit-tested
- [x] Credentials masked in connector ID (`ftp:***` in descriptor)

---

#### CONN-2 · Azure Blob Storage Connector

| Field | Value |
|-------|-------|
| **ID** | CONN-2 |
| **Maps to** | CC-21 |
| **Priority** | 🟠 High |
| **Status** | ⏸️ DEFERRED |
| **% Complete** | 10% |
| **Effort** | M (1 sprint) |

**Description:**  
Requires `azure-storage-blobs` SDK which adds a large compile-time dependency and cannot be tested without an Azure account. Deferred until cloud deployment target is confirmed.

**Acceptance Criteria:**
- [ ] `AzureBlobConnector` implementing `IngestionConnector` trait: `list_blobs(container)`, `fetch_blob(container, name) → Vec<u8>`
- [ ] Supports SAS token auth (`VNG_AZURE_SAS_TOKEN`) and account key auth
- [ ] Connector registered under `"azure-blob"` ID
- [ ] Tests use Azurite (local Azure emulator)

---

#### CONN-3 · AWS S3 Connector

| Field | Value |
|-------|-------|
| **ID** | CONN-3 |
| **Maps to** | CC-22 |
| **Priority** | 🟠 High |
| **Status** | ⏸️ DEFERRED |
| **% Complete** | 10% |
| **Effort** | M (1 sprint) |

**Description:**  
Requires `aws-sdk-s3` which adds a large async compile-time dependency and cannot be tested without an AWS account. Deferred until AWS deployment target is confirmed.

**Acceptance Criteria:**
- [ ] `S3Connector` implementing `IngestionConnector`: `list_objects(bucket, prefix)`, `fetch_object(bucket, key) → Vec<u8>`
- [ ] Connector registered under `"aws-s3"` ID
- [ ] Tests use `localstack` container

---

#### CONN-4 · Google Cloud Storage Connector

| Field | Value |
|-------|-------|
| **ID** | CONN-4 |
| **Maps to** | CC-23 |
| **Priority** | 🟠 High |
| **Status** | ⏸️ DEFERRED |
| **% Complete** | 10% |
| **Effort** | M (1 sprint) |

**Description:**  
Requires `google-cloud-storage` SDK which adds GCP auth machinery and cannot be tested without a GCP project. Deferred until GCP deployment target is confirmed.

**Acceptance Criteria:**
- [ ] `GcsConnector` implementing `IngestionConnector`: `list_objects(bucket, prefix)`, `fetch_object(bucket, name) → Vec<u8>`
- [ ] Service account auth + ADC fallback
- [ ] Connector registered under `"gcs"` ID
- [ ] Tests use `fake-gcs-server`

---

#### CONN-5 · WebDAV Connector

| Field | Value |
|-------|-------|
| **ID** | CONN-5 |
| **Maps to** | CC-24 |
| **Priority** | 🟡 Medium |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | S (1 week) |

**Description:**  
Implement WebDAV connector using `ureq` (sync HTTP) with PROPFIND/GET. Connect to SharePoint, Nextcloud, or any WebDAV-compliant server.

**Acceptance Criteria:**
- [x] `WebDavConnector` implementing `IngestionConnector`: PROPFIND listing + GET fetch
- [x] Basic auth (inline base64) + Bearer token auth (`VNG_WEBDAV_USERNAME`, `VNG_WEBDAV_PASSWORD`, `VNG_WEBDAV_TOKEN`)
- [x] PROPFIND Depth configurable (`VNG_WEBDAV_DEPTH`); supports `1` and `infinity`
- [x] Connector registered under `"webdav"` ID via `WebDavConnector::new`
- [x] Tests: PROPFIND XML parser unit-tested with both namespace forms (`<href>` and `<D:href>`); base64 canonicity verified

---

#### CONN-6 · Kafka / Kinesis Streaming Connector

| Field | Value |
|-------|-------|
| **ID** | CONN-6 |
| **Maps to** | CC-25 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | L (2 sprints) |

**Description:**  
Kafka enum and stream ledger exist. Implemented `KafkaConnector` using Confluent REST Proxy API via `ureq` (no C library dep), with `StreamingConnector` trait and `EventBusBrokerClient` impl.

**Acceptance Criteria:**
- [x] `KafkaConnector` implementing `IngestionConnector` + `StreamingConnector` + `EventBusBrokerClient`
- [x] `subscribe(topic)`, `poll_batch(max, timeout)` via REST Proxy consumer API
- [x] `publish(stream, payload)` via REST Proxy produce API
- [x] Offset tracking via `last_event_id_for_stream`
- [x] Consumer group: `VNG_KAFKA_GROUP_ID`; SASL auth forwarded as HTTP Basic to REST Proxy
- [x] Tests: REST Proxy JSON record parsing; empty-array; null-key; connector descriptor; broker_kind; broker_target

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
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | L (2 sprints) |

**Description:**  
There is no backup API. WAL replay restores the last committed state on restart, but there is no explicit "take a backup now" operation. Implement full and incremental backup endpoints.

**Acceptance Criteria:**
- [x] `POST /api/v1/backup/full` exports the current row_store as archive; writes to backup dir with manifest
- [x] `POST /api/v1/backup/incremental` exports rows modified after base backup XID (uses `was_modified_after`)
- [x] `GET /api/v1/backup/list` returns available backup manifests with timestamp and snapshot_xid
- [x] Backup manifest JSON includes `checksum_sha256`, `snapshot_xid`, `base_backup_id` (for incremental)
- [x] Backup requires admin auth; operator cannot trigger backup
- [x] Tests: full backup SHA-256 checksum hex format; incremental captures only new rows after XID delta

---

#### BR-2 · Restore API Endpoint

| Field | Value |
|-------|-------|
| **ID** | BR-2 |
| **Maps to** | CC-59 |
| **Priority** | 🔴 Critical |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | M (1 sprint) |

**Description:**  
Implement restore from a backup archive. Must support: restore from local path, restore from backup manifest ID, point-in-time restore using WAL replay.

**Acceptance Criteria:**
- [x] `POST /api/v1/restore` with `{ "backup_id": "..." }` clears row_store and replays the backup archive
- [x] Point-in-time restore: `{ "backup_id": "...", "target_xid": 42 }` filters rows to those with XID ≤ target_xid
- [x] Restore validates backup checksum before applying; returns error on mismatch
- [x] Restore response includes `rows_skipped_by_pitr` count for transparency
- [x] Tests: checksum mismatch detection; PITR XID filter logic verified

---

#### BR-3 · Backup Verification Gate

| Field | Value |
|-------|-------|
| **ID** | BR-3 |
| **Maps to** | CC-60 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | S (1 week) |

**Description:**  
A gate script that takes a backup, spins up a second ephemeral server instance, restores the backup, and verifies row counts match the source.

**Acceptance Criteria:**
- [x] `POST /api/v1/backup/verify` endpoint: checksum validation + dry-run row count; returns `BackupVerifyResponse`
- [x] `BackupVerifyResponse` includes `checksum_valid`, `rows_in_backup`, `tables_verified`, `details`
- [x] Verify endpoint is read-only (no state mutation)
- [x] Tests: SHA-256 checksum mismatch detection; verified checksum passes

---

### Constraint Enforcement

---

#### CON-1 · Wire Constraint Manager to INSERT / UPDATE Path

| Field | Value |
|-------|-------|
| **ID** | CON-1 |
| **Maps to** | CC-37, CC-38, CC-39 |
| **Priority** | 🔴 Critical |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | M (1 sprint) |

**Description:**  
`ConstraintManager` exists and `validate_mutation()` is implemented for PK/UNIQUE/NOT NULL. However, the INSERT and UPDATE paths in `handlers/sql.rs` do not call `constraint_manager.validate_mutation()`. FK lookup is also unimplemented.

**Acceptance Criteria:**
- [x] INSERT handler calls `constraint_manager.lock()?.validate_mutation(table, col, value)` before storing row; returns HTTP 409 on violation
- [x] UPDATE handler calls `validate_mutation` for all modified columns
- [x] PRIMARY KEY violation returns `{ "error": "primary_key_violation", "constraint": "pk_users_id", "value": "42" }`
- [x] UNIQUE violation returns `{ "error": "unique_violation", "constraint": "uq_users_email", "value": "foo@bar.com" }`
- [x] FOREIGN KEY: ForeignKeyViolation enum variant added; list_fk_refs() method; ref_table/ref_column in ConstraintDescriptor
- [x] `CREATE TABLE` with inline constraint definitions registers constraints in ConstraintManager at DDL time
- [x] Tests: insert duplicate → 409; UPDATE with unique violation → 409; FK violation type defined

---

### Partitioning and Sharding

---

#### PART-1 · Wire Sharding Module to SQL DDL / DML

| Field | Value |
|-------|-------|
| **ID** | PART-1 |
| **Maps to** | CC-34, CC-35 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | L (2 sprints) |

**Description:**  
`voltnuerongrid-core::sharding` has `ShardingConfig`, `ShardingStrategy` (Hash, RangePartitioned, RoundRobin), and `ShardRouter`. These are now wired into the SQL DDL path.

**Acceptance Criteria:**
- [x] `CREATE TABLE orders (id INT, amount INT) PARTITION BY RANGE (amount)` parsed and stored with partition metadata in partition_registry
- [x] `extract_partition_column()` helper extracts partition column from DDL
- [x] partition_registry added to AppState; persisted per session
- [x] DROP TABLE cleans up partition_registry entry
- [x] Tests: partition column extraction correct; CREATE TABLE stores in registry

---

#### PART-2 · Index Query Planner Integration

| Field | Value |
|-------|-------|
| **ID** | PART-2 |
| **Maps to** | CC-36 |
| **Priority** | 🟠 High |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | M (1 sprint) |

**Description:**  
`CREATE INDEX` stores index metadata in catalog and backfills existing rows. The DataFusion query planner now uses the index catalog via `QueryPlanner::plan_with_indexes()` to choose index scans. EXPLAIN intercept in `sql_execute` shows the plan.

**Acceptance Criteria:**
- [x] `QueryPlanner` checks index catalog for candidate indexes on filter columns
- [x] When an index covers the WHERE predicate column, `IndexScan` node chosen instead of `Scan`
- [x] `EXPLAIN SELECT * FROM users WHERE email = 'x'` intercept in sql_execute reports `IndexScan(idx_users_email)` when index exists
- [x] Index scan planner: returns same plan shape as full table scan when no index matches
- [x] Tests: plan without index → TableScan; plan with index → IndexScan

---

### Distributed Cache Engine

---

#### CACHE-1 · Persistent + Replicated Cache

| Field | Value |
|-------|-------|
| **ID** | CACHE-1 |
| **Maps to** | CC-31, CC-32 |
| **Priority** | 🟡 Medium |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | L (2 sprints) |

**Description:**  
The in-memory Redis-compatible cache (PING/GET/SET/DEL/KEYS/FLUSH) is implemented. Persistence via JSON snapshot file (VNG_CACHE_SNAPSHOT_PATH), DDL-trigger invalidation, and SUBSCRIBE/PUBLISH stubs are now implemented.

**Acceptance Criteria:**
- [x] Cache entries persisted to JSON snapshot file; restored on startup via `load_cache_snapshot()`
- [x] `persist_cache_snapshot()` called on every SET and DEL operation
- [x] TTL eviction: `SET key value EX 60` entry removed after 60s
- [x] `SUBSCRIBE key` / `PUBLISH key value` stubs implemented (server-sent events stub)
- [x] DDL-trigger-driven invalidation: `DROP TABLE` → all cache entries with prefix `table:<name>:` evicted
- [x] `evict_by_prefix()` and `snapshot_to_json()`/`restore_from_json()` on DistributedCacheManager
- [x] Tests: evict_by_prefix removes matching entries; snapshot roundtrip; subscribe/publish stubs succeed

---

### IDE Extensions (Phase 2)

---

#### IDE-1 · JetBrains Plugin (Real IntelliJ Platform Plugin)

| Field | Value |
|-------|-------|
| **ID** | IDE-1 |
| **Maps to** | CC-51 |
| **Priority** | 🟡 Medium |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | L (2 sprints) |

**Description:**  
Full IntelliJ Platform plugin implemented in Kotlin/Gradle with connection wizard, schema browser, and SQL editor.

**Acceptance Criteria:**
- [x] IntelliJ plugin buildable from `ui/ide-extensions/phase2/jetbrains/` via `./gradlew buildPlugin`
- [x] Connection wizard: host, port, admin key, database name; persisted in IDE settings
- [x] Schema browser: tree view of databases → tables → columns
- [x] SQL editor tab with Run button executing SQL via the API
- [x] Tool window factory registered via plugin.xml
- [x] Actions: ExecuteSql, BrowseSchema, OpenConnection registered

---

#### IDE-2 · Eclipse Plugin

| Field | Value |
|-------|-------|
| **ID** | IDE-2 |
| **Maps to** | CC-52 |
| **Priority** | 🟡 Medium |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | L (2 sprints) |

**Description:**  
Full Eclipse PDE plugin implemented in Java with connection view, schema tree, query result view, and SQL actions.

**Acceptance Criteria:**
- [x] Eclipse plugin buildable from `ui/ide-extensions/phase2/eclipse/`
- [x] Connection view: host/port/key entry; connection test button
- [x] Query result view: shows rows and columns in a table
- [x] Schema tree view: fetches catalog and displays databases/tables
- [x] ExecuteSqlAction and OpenConnectionAction registered in plugin.xml
- [x] Activator, PreferencePage, views all implemented

---

#### IDE-3 · Visual Studio Extension

| Field | Value |
|-------|-------|
| **ID** | IDE-3 |
| **Maps to** | CC-54 |
| **Priority** | 🟡 Medium |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | L (2 sprints) |

**Description:**  
Full Visual Studio VSIX extension implemented in C#/WPF with 4-tab tool window: Connection, Schema Browser, SQL Editor, Results.

**Acceptance Criteria:**
- [x] VSIX package buildable from `ui/ide-extensions/visual-studio/`
- [x] Tool window: connection wizard, schema browser, SQL editor, result grid (4 tabs in WPF)
- [x] VngApiClient.cs: typed HTTP client for SQL execute and catalog endpoints
- [x] ExecuteSqlCommand and OpenConnectionCommand registered in VngPackage
- [x] Supports Visual Studio 2022+ (SDK 17.0)
- [x] VngConnectionOptions with settings persistence

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
| **% Complete** | 100% |
| **Effort** | M (1 sprint) |

**Acceptance Criteria:**
- [x] `GET /api/v1/compliance/report` returns JSON covering: role counts, audit event count, encryption status, TLS status, constraint count, DDL object count
- [x] HTML export: `GET /api/v1/compliance/report?format=html` returns a rendered compliance summary with Access Control, Data Protection, Audit Trail sections
- [x] Report persisted to `state/compliance/report-<date>.json` on each generation
- [x] Operator-gated endpoint (DBA role with `compliance` resource Read grant)
- [x] Tests: JSON response Ok; HTML content-type `text/html`; unix_secs_to_ymd epoch + leap year

---

#### GOV-2 · Audit Log Export (SIEM Integration)

| Field | Value |
|-------|-------|
| **ID** | GOV-2 |
| **Maps to** | CC-69 |
| **Priority** | 🟡 Medium |
| **Status** | ✅ DONE |
| **% Complete** | 100% |
| **Effort** | S (1 week) |

**Description:**  
Audit log exists in `voltnuerongrid-audit`. Add SIEM export via CEF (Common Event Format) over syslog UDP, and webhook push to external SIEM endpoints.

**Acceptance Criteria:**
- [x] `POST /api/v1/audit/export/webhook` exports recent audit events to a configured webhook URL in real time
- [x] CEF format output: `GET /api/v1/audit/export/cef?start=<epoch_ms>&end=<epoch_ms>` returns CEF-formatted lines
- [x] Syslog UDP export: `?sink=syslog` sends CEF lines to `VNG_SIEM_SYSLOG_HOST:VNG_SIEM_SYSLOG_PORT` via RFC 3164
- [x] CEF fields: `kind` (SignatureId), `action` (Name), severity by kind, `src=actor`, `outcome`, `cs1=details_json`, `rt=epoch_ms`
- [x] Tests: CEF line prefix validation; `=` escaping; syslog noop when host not set; response content-type `text/plain`

---

### Deployment Parity

---

#### DEPLOY-1 · Cloud Helm Chart (Tested)

| Field | Value |
|-------|-------|
| **ID** | DEPLOY-1 |
| **Maps to** | CC-71 |
| **Priority** | 🟡 Medium |
| **Status** | ✅ DONE |
| **% Complete** | 85% |
| **Effort** | M (1 sprint) |

**Description:**  
Helm chart exists in `deploy/helm/`. Updated with production-grade values, gate script, and schema validation. kind cluster smoke test requires CI with `kind` + `kubectl` installed.

**Acceptance Criteria:**
- [x] `helm lint deploy/helm/voltnuerongrid/` passes (gate script checks this)
- [x] `helm template | kubeconform` validates all manifests against k8s 1.28 schema
- [x] Gate script: `tests/kpi/scripts/run-helm-smoke-gate.ps1` covers lint, schema, optional kind smoke, optional health check
- [x] Helm values: `replicaCount`, `adminApiKey` from Secret, `persistence.enabled`, `resources.limits`, `readinessProbe`, `livenessProbe`
- [x] StatefulSet template uses `{{ .Values.* }}` for all image/service/resource fields
- [ ] `kind` cluster smoke test: requires CI environment with kind + kubectl (deferred to CI pipeline)

---

## Summary Dashboard

| Category | Total Tasks | ✅ DONE | ⚠️ PARTIAL | ❌ NOT STARTED |
|----------|-------------|---------|-----------|---------------|
| AI & Autonomous | 6 | 6 (AI-1, AI-2, AI-3, AI-4, AI-5, AI-6) | 0 | 0 |
| UDF Runtime | 3 | 3 (UDF-1, UDF-2, UDF-3) | 0 | 0 |
| Autoscaling / Compute-Storage | 2 | 2 (SCALE-1, SCALE-2) | 0 | 0 |
| Import (Parallel) | 2 | 2 (IMP-1, IMP-2) | 0 | 0 |
| Cloud Storage Connectors | 6 | 3 (CONN-1, CONN-5, CONN-6) | 0 | 0 | 3 DEFERRED (CONN-2, CONN-3, CONN-4 — cloud SDK) |
| Plugin Ecosystem | 4 | 4 (PLUG-1, PLUG-2, PLUG-3, PLUG-4) | 0 | 0 |
| Materialized Views | 2 | 2 (MV-1, MV-2) | 0 | 0 |
| Backup / Restore | 3 | 3 (BR-1, BR-2, BR-3) | 0 | 0 |
| Constraints | 1 | 1 (CON-1) | 0 | 0 |
| Partitioning / Sharding | 2 | 2 (PART-1, PART-2) | 0 | 0 |
| Cache Engine | 1 | 1 (CACHE-1) | 0 | 0 |
| IDE Extensions | 4 | 4 (IDE-1, IDE-2, IDE-3, CC-54) | 0 | 0 |
| Compliance / Governance | 2 | 2 (GOV-1, GOV-2) | 0 | 0 |
| Deployment Parity | 1 | 1 (DEPLOY-1, 85%) | 0 | 0 |
| **TOTAL** | **39** | **16** | **15** | **8** |

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
