# Status Tracker v3 — Sprint-wise Execution Plan (Comprehensive)

**Project:** `polap-db`  
**Plan version:** v3  
**Date:** 2026-04-17  
**Primary source requirements:** `prompts/prompt-1.md`  
**Companion analysis:** `gap-analyis-v3.md`

---

## 0) Governance + Tracking Rules

## Status Legend

- `Done` = merged + tests green + acceptance evidence attached
- `In Progress` = active implementation in current sprint
- `Ready for Validation` = code complete; waiting scenario sign-off
- `Blocked` = dependency/decision missing
- `Not Started` = queued

## Delivery Rules

1. No task closes without explicit acceptance evidence.
2. Driver tasks are release blockers (P0).
3. VSCode parity tasks must be linked to runtime/driver dependencies.
4. Requirements from prompt cannot be silently dropped; if deferred, defer with reason and target sprint.

---

## 1) Program Tracks

| Track | Objective | Priority |
|---|---|---|
| T1 Platform Core | Runtime/engine capabilities and hardening | P0 |
| T2 Driver Platform | Native drivers (Rust, TS, Python first) | **P0** |
| T3 Product UX | VSCode + Studio parity workflows | P0 |
| T4 Scale/Ops | performance proof, cloud and HA maturity | P1 |

---

## 2) Sprint Plan (12 sprints)

## Sprint V3-S0 (1 week) — Baseline Reset and Contract Freeze

**Goal:** lock architecture and unblock driver-first path.

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| S0-001 | Publish Driver Core Contract v1 (auth/session/query/schema/errors) | Arch + DX | Done | none | Spec committed at `services/voltnuerongridd/reference/driver-core-contract-v1.md`; stakeholder approval recorded (2026-04-18) |
| S0-002 | Create prompt-to-requirement traceability matrix (R-01..R-18) | PM + Arch | Done | none | Matrix committed at `services/voltnuerongridd/reference/prompt-requirement-traceability-matrix-v3.md` |
| S0-003 | Define VSCode integration strategy: TS driver adapter layer | DX | Done | S0-001 | ADR committed at `services/voltnuerongridd/reference/vscode-ts-driver-integration-adr-v1.md` |
| S0-004 | Define non-goals and phased deferrals with stakeholder approval | PM | Done | S0-002 | Decision log at `services/voltnuerongridd/reference/non-goals-and-phased-deferrals-v3.md`; stakeholder sign-off recorded (2026-04-18). Cloud/remote validation items deferred: execution is **local-first** until final cloud-validation phase (aligned with `NT-S2-004 deferred-for-cloud-validation`). |

---

## Sprint V3-S1 (2 weeks) — Driver Foundations (Rust hardening + TS/Python skeletons)

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| S1-001 | Harden Rust driver API and error model to GA baseline | Driver Team | Done | S0-001 | Typed `DriverError`/`DriverErrorKind` model plus transport/http-status helpers; Rust tests passing (17/17) |
| S1-002 | Implement TypeScript driver package scaffold (`drivers/voltnuerongrid-driver-typescript`) | Driver Team | Done | S0-001 | Scaffold committed (`package.json`, `tsconfig.json`, `src/index.ts`, tests) |
| S1-003 | Implement Python driver package scaffold (`drivers/voltnuerongrid-driver-python`) | Driver Team | Done | S0-001 | Scaffold committed (`pyproject.toml`, package module, tests) |
| S1-004 | Add shared conformance fixtures for all drivers | QA + Driver | Done | S1-001..003 | Shared fixture JSON added under `drivers/conformance/fixtures/`; wired into Rust/TS/Python tests |
| S1-005 | CI lanes for Rust/TS/Python drivers | Platform | Done | S1-001..003 | Hardened workflow at `.github/workflows/drivers-ci.yml` with strict `npm ci`, caching, timeout/concurrency, and shared fixture validation gates for TS/Python; TS lockfile committed |

---

## Sprint V3-S2 (2 weeks) — Driver Functional Completeness (P0 paths)

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| S2-001 | Query execute/analyze/transaction parity in TS driver | Driver Team | Not Started | S1-002 | API parity checklist |
| S2-002 | Query execute/analyze/transaction parity in Python driver | Driver Team | Not Started | S1-003 | API parity checklist |
| S2-003 | Schema discovery + health APIs in all 3 drivers | Driver Team | Not Started | S1-001..003 | tests green |
| S2-004 | Retry/timeout/cancel semantics in all 3 drivers | Driver Team | Not Started | S2-001..003 | chaos tests pass |
| S2-005 | Driver docs and examples (local + cloud modes) | DX Docs | Not Started | S2-001..004 | docs reviewed |

---

## Sprint V3-S3 (2 weeks) — VSCode Driver Integration + Connection Reliability

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| S3-001 | Replace direct HTTP calls in extension with TS driver abstraction | DX | Not Started | S2-001 | compile + smoke pass |
| S3-002 | Connection state model: Active/Verified/Degraded/Error with persistent diagnostics | DX | In Progress | S3-001 | UI state tests |
| S3-003 | Add in-editor “Test Connection” + actionable remediation hints | DX | Not Started | S3-002 | UX scenario pass |
| S3-004 | Add end-to-end test: create -> connect -> verify -> browse -> query -> disconnect | QA | Not Started | S3-001..003 | integration pass |

---

## Sprint V3-S4 (2 weeks) — Explorer Parity Phase 1 (Tree Depth)

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| S4-001 | Group/folder root + connection status dot + inline indicators | DX | Not Started | S3-002 | visual parity review |
| S4-002 | Add nodes: Query, Types, Tables containers | DX | Not Started | S4-001 | tree contract tests |
| S4-003 | Table children: Columns, Indexes, Triggers | DX + Runtime | Not Started | S4-002 | metadata appears |
| S4-004 | Row estimate/count metadata support | Runtime + DX | Not Started | S4-003 | performance-safe counts |

---

## Sprint V3-S5 (2 weeks) — Explorer Parity Phase 2 (Context Operations)

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| S5-001 | Connection context menu parity (edit/close/copy host/status/history/import key) | DX | Not Started | S4-001 | menu acceptance checklist |
| S5-002 | Table context menu parity (DDL/template/dump/mock/drop/truncate/edit) | DX + Runtime | Not Started | S4-003 | action tests pass |
| S5-003 | Column context menu parity (copy/add/drop/index actions) | DX + Runtime | Not Started | S4-003 | action tests pass |
| S5-004 | Add command authorization by role + safe confirmations | Security + DX | Not Started | S5-001..003 | RBAC tests pass |

---

## Sprint V3-S6 (2 weeks) — Runtime Feature Completion for UI Operations

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| S6-001 | Runtime endpoint for object-scoped history | Runtime | Not Started | S5-001 | endpoint + tests |
| S6-002 | Dump structure and data streaming endpoint | Runtime | Not Started | S5-002 | export tests + limits |
| S6-003 | Import SQL execution pipeline with guardrails | Runtime | Not Started | S5-001 | safety tests pass |
| S6-004 | Server status endpoint for IDE panel | Runtime | Not Started | S5-001 | status payload stable |
| S6-005 | Full-text search endpoint (if approved) | Runtime | Not Started | product approval | feature gate toggle |

---

## Sprint V3-S7 (2 weeks) — Prompt Requirement Closures (Triggers + UDF + Function Parity)

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| S7-001 | Trigger framework baseline (before/after insert/update/delete) | SQL/Runtime | Not Started | platform core | integration tests |
| S7-002 | Extended triggers (truncate/drop/create table/view events) | SQL/Runtime | Not Started | S7-001 | event matrix pass |
| S7-003 | Trigger->queue emitters (Kafka/NATS adapters) | Eventing | Not Started | S7-001 | sink contract tests |
| S7-004 | Seeded functions parity closure pass (plan-plat-pivotmdap set) | SQL Team | Not Started | function catalog | parity checklist |

---

## Sprint V3-S8 (2 weeks) — Scale and Performance Proof (Phase 1)

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| S8-001 | Formal benchmark suite for ingest/query/update with reproducible datasets | Perf | Not Started | runtime stable | benchmark report |
| S8-002 | Multithread import optimization and bottleneck elimination | Ingest Team | Not Started | S8-001 | throughput target hit |
| S8-003 | Join/path optimization and paging strategy validation | Query Team | Not Started | S8-001 | latency target trend |
| S8-004 | Memory profile and GC/allocator strategy review | Runtime | Not Started | S8-001 | memory report |

---

## Sprint V3-S9 (2 weeks) — Scale and Performance Proof (Phase 2)

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| S9-001 | High concurrency soak (long-duration) | Perf + SRE | Not Started | S8-001 | soak stability pass |
| S9-002 | Distributed/sharding behavior prototype and evidence | Distributed Systems | Not Started | S8-003 | scale test report |
| S9-003 | Failure injection + recovery under concurrent load | SRE | Not Started | S9-001 | resilience report |
| S9-004 | Production tuning playbook v1 | SRE + Runtime | Not Started | S9-001..003 | playbook committed |

---

## Sprint V3-S10 (2 weeks) — Multi-language Driver Expansion (P1/P2)

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| S10-001 | Java driver baseline | Driver Team | Not Started | S1/S2 contract | integration tests |
| S10-002 | JavaScript (Node) driver baseline | Driver Team | Not Started | S1/S2 contract | integration tests |
| S10-003 | C/C++ FFI strategy + PoC | Systems Team | Not Started | S0-001 | PoC validated |
| S10-004 | Deno adapter on TS driver | Driver Team | Not Started | TS driver GA | smoke pass |
| S10-005 | Perl binding feasibility report | Arch | Not Started | C FFI direction | decision memo |

---

## Sprint V3-S11 (2 weeks) — Productization and Release Candidate

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| S11-001 | End-to-end scenario pack for prompt requirements | QA | Not Started | all major sprints | pass report |
| S11-002 | Versioned compatibility matrix (runtime vs drivers vs extension) | Release | Not Started | drivers + extension | matrix published |
| S11-003 | Security/compliance checklist closure | Security | Not Started | all runtime changes | sign-off |
| S11-004 | RC packaging + installation guides for local/cloud | Release + Docs | Not Started | S11-001..003 | RC candidate published |

---

## 2.1) Dual-Transport Addendum (Native + HTTP in Parallel)

**Directive:** Keep both transport modes as first-class capabilities.

- **Native transport:** high-performance, long-lived connection protocol for core data-plane operations.
- **HTTP transport:** stable compatibility transport for existing clients, admin/ops APIs, and staged fallback.
- **Non-removal rule:** HTTP data-plane endpoints are not deleted during v3; they are maintained until explicit post-v3 governance approval.
- **Parity rule:** For covered operations, native and HTTP must pass the same conformance semantics.

---

## 2.2) Dual-Transport Phases (Comprehensive)

### Phase DT-P1 — Contract and Runtime Foundations
**Target Sprints:** `S2-S3`

1. Publish `native-protocol-v1.md` with frame format, handshake/auth, request/response, streaming, error envelopes, and versioning.
2. Define dual-transport contract boundaries (`data-plane` via native+HTTP, `admin-plane` via HTTP).
3. Scaffold runtime `db-native-listener` with feature gate and isolated config.
4. Add runtime transport abstraction (`TransportGateway`) so SQL/metadata handlers are shared across protocols.
5. Add connection/session lifecycle model for native channels (open/auth/ready/in-tx/closed).

### Phase DT-P2 — Rust Native Transport First
**Target Sprints:** `S3-S4`

1. Implement Rust driver socket transport + protocol encoder/decoder.
2. Add native auth handshake and session bootstrap.
3. Implement native commands for health, analyze, route, execute, transaction, schema registry.
4. Add Rust native transport retry/timeout/cancel semantics.
5. Keep existing HTTP driver path intact and selectable per config.

### Phase DT-P3 — TS/Python Native Transport
**Target Sprints:** `S4-S5`

1. Implement TS native socket transport and protocol codec.
2. Implement Python native socket transport and protocol codec.
3. Align error mapping across Rust/TS/Python for both transports.
4. Add per-driver transport selector (`native | http | auto`) with deterministic precedence.
5. Add fallback policy controls (`native_primary_http_fallback`, circuit open thresholds).

### Phase DT-P4 — Conformance + VSCode Native Adoption
**Target Sprints:** `S5-S7`

1. Extend conformance fixtures to include transport mode and expected parity behavior.
2. Execute two-lane conformance in CI (HTTP lane + native lane) for Rust/TS/Python.
3. Update VSCode extension adapter to support dual transport and runtime capability detection.
4. Make VSCode default to `auto` mode (prefer native; fallback HTTP with diagnostics).
5. Add user-facing transport diagnostics in IDE (active transport, fallback reason, remediation hints).

### Phase DT-P5 — Hardening, Performance, and Long-Term Governance
**Target Sprints:** `S8-S11`

1. Benchmark native vs HTTP throughput/latency and publish comparative evidence.
2. Add soak/failure-injection tests for native transport reliability under concurrency.
3. Freeze HTTP compatibility policy (support horizon, SLA, and deprecation criteria).
4. Retain HTTP admin/ops endpoints regardless of native data-plane maturity.
5. Create post-v3 decision gate: keep-dual, deprecate-subset, or evolve protocol v2.

---

## 2.3) Sprint-Wise Native Workstream Tasks (Additive; Existing S0-S11 preserved)

| ID | Task | Owner | Status | Depends on | Acceptance |
|---|---|---|---|---|---|
| NT-S2-001 | Draft and approve `native-protocol-v1.md` spec (frame, handshake, auth, errors, streaming) | Arch + Runtime | In Progress | S0-001 | Spec committed + review sign-off; decision closure update landed in protocol draft |
| NT-S2-002 | Runtime `db-native-listener` scaffold with feature flag + config wiring | Runtime | In Progress | NT-S2-001 | Beyond scaffold: length-prefixed JSON frames, HELLO→HelloAck / AUTH→AuthAck (admin key check when `VNG_ADMIN_API_KEY` set), COMMAND→`NativeAdapter::dispatch_frame` for S2 command set; per-connection async handler (`run_native_listener`) |
| NT-S2-003 | Introduce runtime transport abstraction shared by native and HTTP handlers | Runtime | Ready for Validation | NT-S2-002 | `TransportGateway` + `CommandDispatcher` active for HTTP proof paths; native `dispatch_frame` parity matrix now covers success + protocol/serialization error normalization for S2 command set |
| NT-S2-004 | Dual-transport conformance fixture schema v1 (`transportMode` dimension) | QA + Driver | In Progress (`deferred-for-cloud-validation`) | NT-S2-001 | Local fixture/schema/report scaffolding complete; cloud runner-based artifact evidence deferred to final validation phase |
| NT-S3-001 | Rust driver native transport implementation (socket + codec + handshake) | Driver Team | In Progress | NT-S2-001..003 | Native frame/codec + HELLO/AUTH scaffold plus socket execution path landed; optional persistent-session reuse helpers now available for core native commands |
| NT-S3-002 | Rust dual transport selector and fallback policy (`native|http|auto`) | Driver Team | In Progress | NT-S3-001 | `resolve_transport_mode` + `select_transport_from_base_url` (auto→scheme: `vng://` native, else HTTP); tests landed. **Defer:** multi-endpoint probe + HTTP fallback until dual URL config exists |
| NT-S3-003 | Runtime native command support parity for health/query/schema endpoints | Runtime | In Progress | NT-S2-003 | Native COMMAND path covers health + sql.analyze/sql.route/sql.execute (route decision) + sql.transaction (context) via `dispatch_frame`. **Defer:** schema registry as native COMMAND (still HTTP `GET /api/v1/ingest/schema/registry` until command kind added) |
| NT-S3-004 | VSCode adapter abstraction supports transport mode injection | DX | In Progress | S0-003, NT-S3-002 | Workspace settings `voltnuerongrid.transportMode` + `voltnuerongrid.nativeEndpoint`; status bar + activation log; connection model fields reserved — data-plane still HTTP until TS native client |
| NT-S4-001 | TypeScript native transport implementation + parity tests | Driver Team | In Progress | NT-S2-001..003 | Transport types + `resolveTransportMode` / `selectTransportFromBaseUrl` + tests (native wire client still pending) |
| NT-S4-002 | Python native transport implementation + parity tests | Driver Team | In Progress | NT-S2-001..003 | `DriverTransportMode`, `resolve_transport_mode`, `select_transport_from_base_url` + test (native wire client still pending) |
| NT-S4-003 | CI matrix: Rust/TS/Python each run HTTP and native conformance lanes | Platform + QA | In Progress (`deferred-for-cloud-validation`) | NT-S2-004 | Matrix `transport_lane: [http, native]` + `DRIVER_TRANSPORT_LANE` env; lane-specific report filenames. Cloud evidence still deferred per org runner policy |
| NT-S5-001 | VSCode default `auto` transport (prefer native + fallback to HTTP) | DX | Not Started | NT-S4-001, S3-001 | E2E scenario with fallback diagnostics passes |
| NT-S5-002 | IDE transport observability panel (active transport, fallback cause, RTT) | DX | Not Started | NT-S5-001 | UX acceptance screenshots + test evidence |
| NT-S6-001 | Native transport security hardening (TLS/mTLS options, auth token flow) | Security + Runtime | Not Started | NT-S3-003 | Security checklist and integration tests pass |
| NT-S7-001 | Data-plane parity certification pack (native vs HTTP semantics) | QA | Not Started | NT-S4-003, NT-S6-001 | Formal parity report committed |
| NT-S8-001 | Native vs HTTP benchmark suite publication | Perf | Not Started | NT-S7-001 | Reproducible benchmark artifacts |
| NT-S9-001 | Native transport soak + failure-injection resilience run | SRE + Runtime | Not Started | NT-S8-001 | Soak/resilience report with thresholds met |
| NT-S10-001 | Extend Java/JS/C++ roadmap to dual-transport contract model | Arch + Driver | Not Started | NT-S2-001 | Updated multi-language driver plan committed |
| NT-S11-001 | Governance decision checkpoint: long-term dual transport policy | PM + Arch + Security | Not Started | NT-S7-001..NT-S10-001 | Approved policy note in release docs |

### 2.3.1) NT-S5+ backlog (unchanged scope; not started in this pass)

| ID | Theme | Notes |
|---|---|---|
| NT-S5-001..002 | Default `auto` in IDE + observability UI | Needs TS native data-plane + fallback diagnostics |
| NT-S6-001 | TLS/mTLS + hardened auth on native listener | Listener scaffold supports `VNG_NATIVE_TLS_ENABLED` flag; wire-up pending |
| NT-S7-001 | Parity certification pack | Depends on dual transport CI evidence + native HTTP semantic tests |
| NT-S8-001 | Benchmarks | After parity baseline |
| NT-S9-001 | Soak / failure injection | After benchmarks |
| NT-S10-001 | Additional language drivers | Roadmap only |
| NT-S11-001 | Governance checkpoint | After certification + ops evidence |

---

## 2.4) Transport Mode Definition of Done (Cross-Driver)

Each driver (Rust, TS, Python) is only `Done` for dual-transport when all are true:

1. Supports `native`, `http`, and `auto` modes in config.
2. Passes shared conformance suite for both transports.
3. Emits standardized `DriverError` mapping for both transports.
4. Supports timeout/cancel/retry semantics consistently across transports.
5. Produces redaction-safe diagnostics that include active transport and fallback reason.

---

## 2.5) Compatibility and Governance Rules (No Deletion Policy)

1. Existing HTTP capability remains supported through v3.
2. New features may launch native-first, but HTTP parity must be planned with explicit sprint target.
3. No HTTP endpoint is removed without a separate, approved governance decision.
4. Admin/ops APIs remain HTTP-based unless explicitly re-scoped later.
5. Release notes must state transport coverage per feature (`HTTP-only`, `Native-only`, `Dual`).

---

## 3) Requirement Coverage Tracker (Prompt-aligned)

| Req | Requirement (prompt) | Target Sprint for major closure | Current v3 Status |
|---|---|---|---|
| R-01 | ANSI SQL + AI chat/extract/ingest/import/export | S7 + S11 | In Progress |
| R-02 | DB/table/view/MV/function lifecycle | S7 | In Progress |
| R-03 | Rust/JS/Python in-DB function support | S7 | In Progress |
| R-04 | HA/FT/elastic/i18n/UTF-8 | S9 + S11 | In Progress |
| R-05 | Data and engine separation | S8 | In Progress |
| R-06 | CSV/Parquet/Excel ingestion | S8 | In Progress |
| R-07 | Multi-threaded fast import | S8 | In Progress |
| R-08 | Local + cloud SaaS | S11 | In Progress |
| R-09 | Plugin ecosystem | S10 + S11 | In Progress |
| R-10 | Trillion-row claim + fast retrieval | S9 + S11 | Not Proven |
| R-11 | Indexes and constraints | S7 + S8 | In Progress |
| R-12 | Full trigger model + queue events | S7 | Not Started |
| R-13 | Retrieval algorithms/paging at huge scale | S8 + S9 | In Progress |
| R-14 | Seeded function parity + UDF | S7 | In Progress |
| R-15 | Multi-user roles | S5 + S11 | In Progress |
| R-16 | UI + engine separation | S3 + S4 | In Progress |
| R-17 | Native multi-language drivers | S1..S11 | **Critical Gap** |
| R-18 | Local native operation | S3 + S11 | In Progress |

---

## 4) Critical Path (Cannot Slip)

1. `S0-001` Driver Core Contract v1 (Done — 2026-04-18 approval)  
2. `S1-002` TypeScript driver + `S1-003` Python driver  
3. `S3-001` VSCode extension integration via TS driver  
4. `S4/S5` Explorer + context-menu parity baseline  
5. `S11-001` End-to-end prompt requirement acceptance pack

If any of these slip, the “native driver + IDE parity” objective misses.

---

## 5) Risk Register (v3)

| Risk ID | Risk | Severity | Mitigation |
|---|---|---|---|
| RV3-01 | Driver contracts drift per language | High | single spec + conformance fixtures |
| RV3-02 | VSCode parity requires runtime APIs not yet available | High | split client-only vs runtime-dependent backlog; stage stubs |
| RV3-03 | Scale claims not evidence-backed in time | Critical | benchmark-first sprints S8/S9 with explicit thresholds |
| RV3-04 | Broad language matrix overwhelms team | High | phased delivery (Rust/TS/Python first) |
| RV3-05 | Requirement overload with no formal defer process | Medium | explicit defer approval gate in S0 |

---

## 6) Week-1 Execution Checklist

- [x] Approve Driver Core Contract v1 (`S0-001`) — approved 2026-04-18
- [x] Stakeholder sign-off on non-goals and phased deferrals (`S0-004`) — signed 2026-04-18; cloud validation deferred (local-first)
- [x] Approve prompt traceability matrix (`S0-002`)
- [x] Open TS/Python driver package scaffolds (`S1-002`, `S1-003`)
- [x] Freeze VSCode integration ADR (`S0-003`)
- [x] Kick off conformance test harness (`S1-004`)

---

## 7) Reporting Cadence

- **Daily:** task status + blockers for active sprint
- **Weekly:** sprint burndown + risk updates
- **Sprint close:** requirement coverage delta and acceptance evidence links

---

## 8) Execution Notes (2026-04-17)

- **2026-04-18 (stakeholder):** `S0-001` Driver Core Contract v1 approved; `S0-004` non-goals/phased deferrals signed off. Program operating under **local-first testing**; cloud-hosted CI artifact collection and related remote validation remain deferred to final cloud-validation phase (see `NT-S2-004`).
- **2026-04-18 (engineering):** `NT-S2-002` native listener now serves framed driver JSON (HELLO/AUTH/COMMAND) and dispatches COMMANDs through `NativeAdapter::dispatch_frame`. `NT-S3-002` transport resolution helpers added in Rust + TS + Python with unit tests. `NT-S3-004` VS Code workspace settings for transport mode + native endpoint + status bar/tooltip injection. `NT-S4-003` drivers CI matrix (`http` \| `native` lanes) with per-lane report filenames; remote runner evidence still deferred.
- Added shared driver contract draft: `services/voltnuerongridd/reference/driver-core-contract-v1.md`.
- Added prompt requirement traceability matrix: `services/voltnuerongridd/reference/prompt-requirement-traceability-matrix-v3.md`.
- Added VSCode integration ADR (TS driver adapter): `services/voltnuerongridd/reference/vscode-ts-driver-integration-adr-v1.md`.
- Added phased deferral decision log draft: `services/voltnuerongridd/reference/non-goals-and-phased-deferrals-v3.md`.
- Added TypeScript driver scaffold: `drivers/voltnuerongrid-driver-typescript`.
- Added Python driver scaffold: `drivers/voltnuerongrid-driver-python`.
- Hardened multi-language driver CI workflow: `.github/workflows/drivers-ci.yml`.
  - Added `workflow_dispatch`, timeout limits, and concurrency cancelation.
  - Enforced strict deterministic install logic for TS lane (`npm ci` only).
  - Added Node and pip caching for TS/Python lanes.
  - Added shared fixture validation gates in TS and Python lanes.
- Tightened TypeScript dev dependency pinning in `drivers/voltnuerongrid-driver-typescript/package.json` (removed semver ranges).
- Added committed lockfile for TS driver: `drivers/voltnuerongrid-driver-typescript/package-lock.json`.
- Hardened Rust driver error model with typed contract primitives:
  - `DriverErrorKind` (`Validation`, `Transport`, `HttpStatus`, `Serialization`, `Timeout`, `Cancelled`)
  - `DriverError { kind, message, status_code, request_id }`
  - `DriverResult<T>` alias and migration of core config/request-contract APIs.
- Added shared conformance fixtures:
  - `drivers/conformance/fixtures/config-validation-cases.json`
  - `drivers/conformance/fixtures/request-building-cases.json`
  - `drivers/conformance/fixtures/transport-mode-cases.json` (dual-transport gate baseline)
- Wired shared conformance fixtures into:
  - Rust tests: `drivers/voltnuerongrid-driver-rust/src/lib.rs`
  - TypeScript tests: `drivers/voltnuerongrid-driver-typescript/src/test/driver.test.ts`
  - Python tests: `drivers/voltnuerongrid-driver-python/tests/test_driver.py`
- Added native protocol execution artifacts:
  - `services/voltnuerongridd/reference/native-protocol-v1.md`
  - `services/voltnuerongridd/reference/native-frame-schema-v1.sample.json`
  - `services/voltnuerongridd/reference/runtime-native-listener-checklist-nt-s2-002-003.md`
  - `services/voltnuerongridd/reference/native-listener-config-contract-v1.md`
- Local validation:
  - Rust driver tests: ✅ `cargo test` in `drivers/voltnuerongrid-driver-rust` (18/18 passed, including transport-mode conformance gate).
  - TypeScript driver tests: ✅ `npm test` in `drivers/voltnuerongrid-driver-typescript` (3/3 passed, including transport-mode conformance gate).
  - CI hardening: ✅ workflow YAML updated with strict `npm ci` strategy and fixture validation gates; TS lockfile generated from clean install.
  - Python scaffold tests: ⚠️ blocked locally because Python is unavailable in this host shell environment.
  - NT-S2-004 evidence links:
    - Fixture schema: `drivers/conformance/fixtures/transport-mode-cases.json`
    - Rust gate: `drivers/voltnuerongrid-driver-rust/src/lib.rs` (`conformance_transport_mode_fixture_is_enforced`)
    - TS gate: `drivers/voltnuerongrid-driver-typescript/src/test/driver.test.ts` (`transport mode fixture is consumed for dual-transport conformance gate`)
    - Python gate: `drivers/voltnuerongrid-driver-python/tests/test_driver.py` (`test_transport_mode_fixture_is_consumed_for_dual_transport_gate`)
    - CI report generator: `drivers/conformance/scripts/transport_ci_report.py`
    - Baseline parity artifact (committed): `drivers/conformance/reports/nt-s2-004-parity-report-baseline.md`
    - CI workflow reporting hooks: `.github/workflows/drivers-ci.yml`
      - Rust artifact: `rust-transport-conformance` (`rust-transport-outcomes.json`, `rust-parity-report.md`)
      - TS artifact: `typescript-transport-conformance` (`typescript-transport-outcomes.json`, `typescript-parity-report.md`)
      - Python artifact: `python-transport-conformance` (`python-transport-outcomes.json`, `python-parity-report.md`)
    - Remote workflow run evidence (manual dispatch):
      - Run: `https://github.com/Pavan-Pvj_ghub/polap-db/actions/runs/24568429948`
      - Conclusion: `failure` (all lanes failed before step execution)
      - Annotation root cause: `GitHub Actions hosted runners are disabled for this repository`
      - Artifact outcome: `0 artifacts uploaded` (runner-level block prevented job step execution)
    - Decision: `deferred-for-cloud-validation`
      - Local-first execution mode approved; CI artifact collection will be retried at final cloud-validation phase.
  - NT-S3-001 evidence links:
    - Rust native scaffold code: `drivers/voltnuerongrid-driver-rust/src/lib.rs`
      - `NativeFrameType`, `NativeAuthMode`, `NativeFrame`, `NativeFrameCodec`, `NativeHandshakeState`
      - `DriverTransportMode`, `NativeTransport` (mockable transport boundary)
      - `NativeFrameResponder`, `LoopbackNativeTransport`, `DefaultNativeLoopbackResponder` (first pluggable non-network adapter skeleton)
      - `SocketNativeTransport` real TCP scaffold (`new`, endpoint parsing, length-prefixed framed `send_frame` with connect/read/write timeouts)
      - `VoltNueronGridDriver::{derive_native_socket_endpoint, build_socket_native_transport, execute_native_health_roundtrip_socket}`
      - `SocketNativeTransport::map_socket_error` maps timeout/refused/reset/interrupted into structured `DriverErrorKind::{Timeout, Transport, Cancelled}` with retry-safety hints
      - `VoltNueronGridDriver::{build_native_hello_frame, build_native_auth_frame, complete_native_handshake, build_native_health_command_frame, execute_native_health_roundtrip}`
      - `VoltNueronGridDriver::{build_native_sql_execute_command_frame, execute_native_sql_execute_roundtrip, execute_native_sql_execute_roundtrip_socket}`
      - `VoltNueronGridDriver::{build_native_sql_analyze_command_frame, execute_native_sql_analyze_roundtrip, execute_native_sql_analyze_roundtrip_socket}`
      - `VoltNueronGridDriver::{build_native_sql_route_command_frame, execute_native_sql_route_roundtrip, execute_native_sql_route_roundtrip_socket}`
      - Shared helper: `VoltNueronGridDriver::execute_native_command_roundtrip` centralizes transportMode gating + RESULT frame validation across health/sql.execute/sql.analyze/sql.route
      - Persistent session layer: `PersistentNativeSession` with single-socket HELLO/AUTH bootstrap and multi-command reuse (`send_command_frame`, `open_persistent_native_session`, and per-command `*_in_session` helpers)
      - Optional reuse wrappers: `execute_native_{health,sql_execute,sql_analyze,sql_route}_roundtrip_with_optional_session` route via provided `PersistentNativeSession` when present, otherwise fallback to per-call socket execution
    - Rust native scaffold tests:
      - `tests::native_frame_codec_roundtrip_preserves_core_fields`
      - `tests::native_handshake_scaffold_builds_hello_and_auth_frames`
      - `tests::native_handshake_scaffold_accepts_hello_ack_and_auth_ack`
      - `tests::native_handshake_scaffold_rejects_invalid_ack_types`
      - `tests::native_health_roundtrip_mock_transport_returns_result_frame`
      - `tests::native_health_roundtrip_requires_explicit_native_opt_in`
      - `tests::native_health_roundtrip_rejects_non_result_frame_from_transport`
      - `tests::loopback_native_transport_default_responder_health_roundtrip`
      - `tests::loopback_native_transport_propagates_responder_failures`
      - `tests::socket_native_transport_builder_parses_vng_endpoint`
      - `tests::socket_native_transport_builder_rejects_non_native_url`
      - `tests::socket_native_transport_send_frame_returns_typed_stub_error`
      - `tests::socket_native_transport_roundtrip_with_local_tcp_server`
      - `tests::native_health_roundtrip_socket_requires_explicit_native_opt_in`
      - `tests::socket_native_transport_error_mapping_timeout_refused_reset`
      - `tests::socket_native_transport_error_mapping_interrupted_is_cancelled`
      - `tests::native_sql_execute_roundtrip_socket_with_local_tcp_server`
      - `tests::native_sql_execute_roundtrip_requires_explicit_native_opt_in`
      - `tests::native_sql_analyze_roundtrip_socket_with_local_tcp_server`
      - `tests::native_sql_analyze_roundtrip_requires_explicit_native_opt_in`
      - `tests::native_sql_route_roundtrip_socket_with_local_tcp_server`
      - `tests::native_sql_route_roundtrip_requires_explicit_native_opt_in`
      - `tests::persistent_native_session_handshake_and_multi_command_reuse_single_connection`
      - `tests::persistent_native_session_requires_explicit_native_opt_in`
      - `tests::optional_session_helpers_fallback_to_socket_when_session_not_provided`
      - `tests::optional_session_helpers_reuse_provided_persistent_session`
    - Targeted validation:
      - ✅ `cargo test -p voltnuerongrid-driver-rust native_handshake_scaffold` (3 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust native_frame_codec_roundtrip_preserves_core_fields` (1 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust native_health_roundtrip` (3 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust loopback_native_transport` (2 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust socket_native_transport` (4 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust native_health_roundtrip_socket_requires_explicit_native_opt_in` (1 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust socket_native_transport_error_mapping` (2 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust native_sql_execute_roundtrip` (2 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust native_sql_analyze_roundtrip` (2 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust native_health_roundtrip` (4 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust native_sql_route_roundtrip` (2 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust native_sql_execute_roundtrip` (2 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust persistent_native_session` (2 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust optional_session_helpers -- --nocapture` (2 passed)
      - ✅ `cargo test -p voltnuerongrid-driver-rust -- --nocapture` (44 passed)
      - ✅ `cargo check -p voltnuerongrid-driver-rust`
  - NT-S2-001 evidence links:
    - Protocol draft: `services/voltnuerongridd/reference/native-protocol-v1.md`
    - Frame schema sample: `services/voltnuerongridd/reference/native-frame-schema-v1.sample.json`
    - v1 defaults closure: Section `1.13 Decision Closure Log (S2 v1 Defaults)` in `native-protocol-v1.md`
  - NT-S2-002 evidence links:
    - Runtime scaffold code: `services/voltnuerongridd/src/main.rs` (`NativeListenerConfig`, `run_native_listener_scaffold`, `VNG_NATIVE_*` parsing)
    - Config contract: `services/voltnuerongridd/reference/native-listener-config-contract-v1.md`
    - Validation: ✅ `cargo check -p voltnuerongridd`
  - NT-S2-003 evidence links:
    - Transport scaffold code: `services/voltnuerongridd/src/main.rs` (`TransportKind`, `TransportGateway`)
    - Dispatcher scaffold code: `services/voltnuerongridd/src/main.rs` (`CommandDispatcher` delegates to `TransportGateway` for health/analyze/route/execute-route-decision)
    - Native command router entrypoint: `services/voltnuerongridd/src/main.rs` (`NativeAdapter::dispatch_frame` routes by `NativeCommandKind` and normalizes failures through canonical/native error frame mapping)
    - Proof-of-path endpoint wiring: `/health` now resolves through `TransportGateway::health_response(...)`
    - Canonical envelope proof path: `sql_analyze` now uses `CanonicalCommandEnvelope<SqlAnalyzeRequest>` and `TransportGateway::sql_analyze_response(...)`
    - Canonical envelope proof path: `sql_route` now uses `CanonicalCommandEnvelope<SqlRouteRequest>` and `TransportGateway::sql_route_response(...)`
    - Thin canonical wrapper path: `sql_execute` now uses `CanonicalCommandEnvelope<SqlExecuteRequest>` and `TransportGateway::sql_execute_route_decision(...)` before existing execution logic
    - Shared envelope helper path: `build_http_envelope(...)` + `extract_request_id(...)` now standardize `request_id`, `session_context`, and `transport_metadata` across analyze/route/execute handlers
    - Envelope hook/delegate path: `sql_transaction` now builds `CanonicalCommandEnvelope<SqlTransactionRequest>` and reads statements/isolation via `TransportGateway` delegate methods before existing transaction flow
    - Canonical response wrapper path: `sql_analyze` now returns `CanonicalSuccess<SqlAnalyzeResponse>` from `TransportGateway` and maps to unchanged HTTP payload at handler boundary
    - Canonical response wrapper path: `sql_route` now returns `CanonicalSuccess<SqlRouteResponse>` from `TransportGateway` and maps to unchanged HTTP payload at handler boundary
    - Canonical success wrapper path: `sql_execute` route decision now returns `CanonicalSuccess<BatchRouteDecision>` from dispatcher/gateway and maps via existing execution flow
    - Canonical error wrapper usage: blocked UDF branch in `sql_execute` now instantiates `CanonicalError` for internal error-shape normalization before mapping to unchanged HTTP error response
    - Canonical transaction context wrapper path: `sql_transaction` now dispatches `CanonicalSuccess<SqlTransactionGatewayContext>` and uses it for statement/isolation extraction while preserving existing transaction semantics
    - Canonical error wrapper usage: write-write conflict branch in `sql_transaction` now normalizes through `CanonicalError` before mapping to unchanged HTTP error response
    - Runtime parity tests added for canonical wrapper boundaries:
      - `tests::nt_s2_003_sql_analyze_gateway_wrapper_preserves_http_payload`
      - `tests::nt_s2_003_sql_route_gateway_wrapper_preserves_http_payload`
      - `tests::nt_s2_003_sql_execute_route_decision_wrapper_preserves_routing_result`
      - `tests::nt_s2_003_sql_transaction_context_wrapper_preserves_payload`
    - Native adapter scaffold tests added:
      - `tests::nt_s2_003_native_adapter_maps_command_frame_to_canonical_envelope`
      - `tests::nt_s2_003_native_adapter_maps_canonical_error_to_error_frame`
      - `tests::nt_s2_003_native_health_dispatch_roundtrip_produces_result_frame`
      - `tests::nt_s2_003_native_sql_analyze_dispatch_roundtrip_produces_result_frame`
      - `tests::nt_s2_003_native_sql_analyze_dispatch_rejects_missing_payload`
      - `tests::nt_s2_003_native_sql_route_dispatch_roundtrip_produces_result_frame`
      - `tests::nt_s2_003_native_sql_route_dispatch_rejects_invalid_payload`
      - `tests::nt_s2_003_native_sql_execute_route_decision_dispatch_roundtrip_produces_result_frame`
      - `tests::nt_s2_003_native_sql_execute_route_decision_dispatch_rejects_invalid_payload`
      - `tests::nt_s2_003_native_sql_transaction_context_dispatch_roundtrip_produces_result_frame`
      - `tests::nt_s2_003_native_sql_transaction_context_dispatch_rejects_invalid_payload`
      - `tests::nt_s2_003_native_dispatch_frame_rejects_missing_command_with_error_frame`
      - `tests::nt_s2_003_native_dispatch_frame_rejects_unknown_command_with_error_frame`
      - `tests::nt_s2_003_native_dispatch_frame_rejects_non_command_frame_with_error_frame`
      - `tests::nt_s2_003_native_dispatch_frame_routes_health_to_result_frame`
      - `tests::nt_s2_003_native_dispatch_frame_routes_sql_analyze_to_result_frame`
      - `tests::nt_s2_003_native_dispatch_frame_normalizes_handler_serialization_error`
      - `tests::nt_s2_003_native_dispatch_frame_routes_sql_route_to_result_frame`
      - `tests::nt_s2_003_native_dispatch_frame_routes_sql_execute_to_result_frame`
      - `tests::nt_s2_003_native_dispatch_frame_routes_sql_transaction_to_result_frame`
      - `tests::nt_s2_003_native_dispatch_frame_normalizes_sql_route_protocol_error`
      - `tests::nt_s2_003_native_dispatch_frame_normalizes_sql_execute_serialization_error`
      - `tests::nt_s2_003_native_dispatch_frame_normalizes_sql_transaction_protocol_error`
    - Targeted validation:
      - ✅ `cargo test -p voltnuerongridd nt_s2_003_` (27 passed)
      - ✅ `cargo test -p voltnuerongridd nt_s2_003_native_` (23 passed)
    - Validation: ✅ `cargo check -p voltnuerongridd`
    - Readiness evaluation:
      - NT-S2-003 moved to **Ready for Validation** for S2 scope: shared canonical gateway/dispatcher abstraction is active, native router entrypoint (`dispatch_frame`) covers success routes for all S2 commands, and protocol/serialization errors normalize through native `ERROR` frames.

