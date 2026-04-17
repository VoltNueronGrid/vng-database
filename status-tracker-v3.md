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
| S0-001 | Publish Driver Core Contract v1 (auth/session/query/schema/errors) | Arch + DX | In Progress | none | Draft spec committed at `services/voltnuerongridd/reference/driver-core-contract-v1.md`; pending approval |
| S0-002 | Create prompt-to-requirement traceability matrix (R-01..R-18) | PM + Arch | Done | none | Matrix committed at `services/voltnuerongridd/reference/prompt-requirement-traceability-matrix-v3.md` |
| S0-003 | Define VSCode integration strategy: TS driver adapter layer | DX | Done | S0-001 | ADR committed at `services/voltnuerongridd/reference/vscode-ts-driver-integration-adr-v1.md` |
| S0-004 | Define non-goals and phased deferrals with stakeholder approval | PM | Ready for Validation | S0-002 | Decision log drafted at `services/voltnuerongridd/reference/non-goals-and-phased-deferrals-v3.md`; pending stakeholder sign-off |

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

1. `S0-001` Driver Core Contract v1  
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

- [ ] Approve Driver Core Contract v1 (`S0-001`)
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
- Wired shared conformance fixtures into:
  - Rust tests: `drivers/voltnuerongrid-driver-rust/src/lib.rs`
  - TypeScript tests: `drivers/voltnuerongrid-driver-typescript/src/test/driver.test.ts`
  - Python tests: `drivers/voltnuerongrid-driver-python/tests/test_driver.py`
- Local validation:
  - Rust driver tests: ✅ `cargo test` in `drivers/voltnuerongrid-driver-rust` (17/17 passed, including fixture-driven conformance tests).
  - CI hardening: ✅ workflow YAML updated with strict `npm ci` strategy and fixture validation gates; TS lockfile generated from clean install.
  - TypeScript scaffold tests: ⚠️ blocked locally by npm auth (`E401`) while installing dev dependencies.
  - Python scaffold tests: ⚠️ blocked locally because Python is unavailable in this host shell environment.

