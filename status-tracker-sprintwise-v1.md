# VoltNueronGrid DB — Sprint-Wise Status Tracker v1

**Source of truth:**
- `reference/voltnuerongrid-db-design.md`
- `reference/voltnuerongrid-ws.md`
- Reference style: `../maas/maas-v2/final-design/STATUS_TRACKER.md`

**Purpose:** Sprint-by-sprint execution view — tracks all requirements, epics, hardening items, prerequisites, releases, and governance closures.

**Last updated:** 2026-04-14 (session 20)

---

## Status Legend

| Status | Meaning |
|---|---|
| ✅ Done | Merged + validated + evidence attached |
| 🟡 Ready for Validation | Implemented, pending verification |
| 🔵 In Progress | Active implementation |
| ⬜ Not Started | Not yet started |
| 🔴 Blocked | Waiting dependency/decision |

---

## Sprint Overview

| Sprint | Focus | Time Frame | Overall Status |
|---|---|---|---|
| Sprint 0 | Foundation & Prerequisites | Completed | ✅ Done (PR-007 88%) |
| Sprint 1 | Core Engine Bootstrap (WS0, WS1, WS2) | In Flight | 🔵 In Progress |
| Sprint 2 | SQL Parity + Row Store + HTAP Query (WS1A, WS2A, WS3) | In Flight | 🔵 In Progress |
| Sprint 3 | Ingestion + Pessimistic Locking (WS4, WS22) | In Flight | 🔵 In Progress |
| Sprint 4 | Streaming + Security (WS4A, WS5) | In Flight | 🟡 Mixed (WS5 Ready for Validation) |
| Sprint 5 | Distributed HA/FT + Release R1 Gate (WS6) | In Flight | 🟡 Ready for Validation |
| Sprint 6 | Plugin + AI + Audit (WS7, WS8, WS8A) | In Flight | 🟡 Ready for Validation |
| Sprint 7 | UX/DX + Drivers + i18n (WS9, WS9A, WS10, WS11) | In Flight | 🟡 Ready for Validation |
| Sprint 8 | Reliability + Ops + Config (WS12, WS13, WS14) + Release R2 Gate | In Flight | 🟡 Ready for Validation |
| Sprint 9 | Competitive + P0 Hardening (WS15, H-01..H-04) | In Flight | 🔵 Mixed |
| Sprint 10 | P1 Hardening (H-05..H-08) + Release R3 Gate | Not Started | ⬜ Not Started |
| Sprint 11 | P2 Hardening + Ecosystem Polish (H-09, H-10) + Release R4 Gate | Not Started | ⬜ Not Started |

---

## Requirement ↔ Sprint Mapping

| Req ID | Requirement Area | Primary Sprint(s) | Primary Epic(s) | Status |
|---|---|---|---|---|
| REQ-01 | ANSI SQL + AI chat/extract/ingest/export | Sprint 1, Sprint 6 | Epic 1, Epic 8 | 🔵 In Progress |
| REQ-02 | DB/table/view/materialized view/function lifecycle | Sprint 1 | Epic 1 | 🔵 In Progress |
| REQ-03 | Rust/JS/Python function support | Sprint 1 | Epic 1 | 🟡 Ready for Validation |
| REQ-04 | HA/FT/elasticity/i18n/UTF-8 | Sprint 5, Sprint 7 | Epic 6, Epic 11, Epic 12 | 🔵 In Progress |
| REQ-05 | Separate compute and data files | Sprint 1, Sprint 5 | Epic 2, Epic 6 | 🔵 In Progress |
| REQ-06 | CSV/Parquet/JSON/Excel + enterprise source ingest | Sprint 3, Sprint 6 | Epic 4, Epic 4A, Epic 7 | 🔵 In Progress |
| REQ-07 | Multithreaded high-speed import | Sprint 3 | Epic 4 | ⬜ Not Started |
| REQ-08 | Local + cloud SaaS operation | Sprint 8 | Epic 13 | ⬜ Not Started |
| REQ-09 | Extensible plugin ecosystem | Sprint 6 | Epic 7 | 🟡 Ready for Validation |
| REQ-10 | Trillion-row scale + high-speed retrieval | Sprint 2, Sprint 5 | Epic 2, Epic 3, Epic 6 | ⬜ Not Started |
| REQ-11 | Indexes + constraints | Sprint 1 | Epic 2, Epic 15 | 🔵 In Progress |
| REQ-12 | Seeded functions + plan-plat parity | Sprint 1, Sprint 2 | Epic 1, Epic 1A | 🔵 In Progress |
| REQ-13 | Multi-user roles and privileges | Sprint 4 | Epic 5 | 🔵 In Progress |
| REQ-14 | UI + engine separation | Sprint 7 | Epic 9 | 🔵 In Progress |
| REQ-15 | Driver support (multi-language) | Sprint 7 | Epic 10 | 🔵 In Progress |
| REQ-16 | SSL + encryption/decryption | Sprint 4 | Epic 5 | 🔵 In Progress |
| REQ-17 | Distributed failover + zero data loss | Sprint 5, Sprint 8 | Epic 6, Epic 12 | 🟡 Ready for Validation |
| REQ-18 | Stream in/out + events for debug/audit | Sprint 4 | Epic 4A, Epic 8A | 🔵 In Progress |
| REQ-19 | Blazing ingest/update/read at scale | Sprint 2, Sprint 3, Sprint 5 | Epic 3, Epic 4, Epic 6 | ⬜ Not Started |
| REQ-20 | Azure/AWS/GCP/OCI + Docker + Kubernetes | Sprint 8 | Epic 13 | 🔵 In Progress |
| REQ-21 | Any-number-user concurrency | Sprint 2, Sprint 5, Sprint 7 | Epic 3, Epic 10, Epic 12 | ⬜ Not Started |
| REQ-22 | Pessimistic locking | Sprint 3 | Epic 1, Epic 3 | ✅ Done |
| REQ-23 | ACID transactions | Sprint 1, Sprint 2 | Epic 1, Epic 2, Epic 3 | 🔵 In Progress |
| REQ-24 | Config via properties/YAML/JSON | Sprint 8 | Epic 14 | 🔵 In Progress |
| REQ-25 | Native connection + pooling | Sprint 7, Sprint 8 | Epic 10, Epic 14 | 🔵 In Progress |
| REQ-26 | Plugin model for streaming sources/sinks | Sprint 4, Sprint 6 | Epic 4A, Epic 7 | 🔵 In Progress |
| REQ-27 | Native cache engine (Redis-like compat) | Sprint 2, Sprint 8 | Epic 3, Epic 14 | ⬜ Not Started |
| REQ-28 | IDE extensions (VS/Cursor/Antigravity/JetBrains/Eclipse) | Sprint 7 | Epic 9A | 🔵 In Progress |
| REQ-29 | Fully autonomous operations | Sprint 6, Sprint 8 | Epic 8, Epic 14 | 🟡 Ready for Validation |
| REQ-30 | AI agent authoring for objects/plugins | Sprint 6 | Epic 8, Epic 7 | 🟡 Ready for Validation |
| REQ-31 | HTAP (OLTP + OLAP) extreme performance | Sprint 2 | Epic 2, Epic 3 | 🔵 In Progress |

---

## Sprint 0 — Foundation & Prerequisites

**Goal:** Lock naming, scaffolding, scope, CI, and KPI harness.
**Status:** ✅ Done (PR-007 at 88%)

### Prerequisite Gate

| ID | Prerequisite | Owner | Status | Completion | Notes |
|---|---|---|---|---|---|
| PR-001 | Lock naming/folder consistency (`reference/voltnuerongrid-db-design.md`, `reference/voltnuerongrid-ws.md`) | Architecture Board | ✅ Done | 100% | Completed in docs |
| PR-002 | Create deployment scaffolds (`deploy/local/single-node.yml`, `deploy/local/multi-node.yml`, `deploy/helm/voltnuerongrid`) | Platform/SRE | ✅ Done | 100% | Compose + Helm scaffolds created, including starter local config files for single/multi node profiles |
| PR-003 | Freeze R1 scope (HTAP baseline, SQL core, ingest core, RBAC baseline, basic drivers) | Program Governance | ✅ Done | 100% | Approved by stakeholder; baseline scope locked |
| PR-004 | Acceptance harness skeleton aligned to KPI table | QA/Performance | ✅ Done | 100% | KPI harness scaffold created under `tests/kpi` with scenarios, targets, and runner entry points |
| PR-005 | Repo skeleton for modules/crates from architecture | Platform Engineering | ✅ Done | 100% | Rust workspace and core module skeletons created (`crates/`, `services/`, `drivers/`, `tools/`, UI placeholder) |
| PR-006 | Define immediate start order and ownership assignment | Program Governance | ✅ Done | 100% | Owner assignment matrix and execution order published in tracker sections |
| PR-007 | Validate single-node and multi-node local/cloud smoke pathways | Platform/SRE + QA | 🔵 In Progress | 88% | Phase 1+2 complete; phase 3 now supports deferred execution (`-AllowMissingEnv`) with readiness tracking; env-driven real-cloud profiles and gate report tooling in place pending endpoint/auth handoff |

### Sprint 0 Deliverables
- [x] Naming and folder conventions locked
- [x] Deployment scaffolds (Docker Compose + Helm)
- [x] R1 scope freeze approved
- [x] KPI acceptance harness skeleton
- [x] Repo workspace skeleton (`crates/`, `services/`, `drivers/`, `tools/`, UI)
- [x] Owner assignment matrix published
- [ ] Cloud smoke pathway validation (pending endpoint/token handoff)

---

## Sprint 1 — Core Engine Bootstrap

**Goal:** Foundation CI/governance, SQL parser/analyzer/DDL-DML/function registry, durability/storage baseline.
**Dependencies:** Sprint 0 (PR-001..PR-006 complete)
**Status:** 🔵 In Progress

### Workstreams

| WS ID | Epic | Scope Summary | Owner | Status | Dependencies | Validation Evidence |
|---|---|---|---|---|---|---|
| WS0 | Epic 0 | Workspace/CI/governance foundation | Platform + Program Governance | 🔵 In Progress | PR-003 (CI now runs runtime check + SQL tests + gate scripts + SQL analyze runtime smoke) | CI pipeline green with gate scripts |
| WS1 | Epic 1 | SQL parser/analyzer/DDL-DML/function registry | SQL Engine Team | 🔵 In Progress | WS0 | Runtime integration underway; `/api/v1/sql/analyze`, `/api/v1/sql/route`, `/api/v1/sql/execute`, and `/api/v1/sql/transaction` now enforce tenant-scoped user RBAC via `x-vng-tenant-id` + `x-vng-user-id` while preserving operator/admin access; `/api/v1/sql/execute` includes UDF runtime scaffold with explicit function catalog contract, per-language guard policies, and statement-level execution-plan routing evidence for Rust/JS/Python; gate orchestrator `run-ws1-gate.ps1` -> `tests/kpi/results/ws1/ws1-gate-summary.json`; UDF contract pack -> `tests/kpi/results/ws1/ws1-udf-contract-smoke.json`; runtime analyze/UDF smokes -> `tests/kpi/results/20260305-ws1/sql-analyze-smoke.json`, `tests/kpi/results/ws1/sql-execute-udf-smoke.json`; focused tenant SQL route/transaction tests in `voltnuerongridd`; workflow wiring in `.github/workflows/ci.yml` |
| WS2 | Epic 2 | Durability/storage/index/constraints | Storage Team | 🔵 In Progress | WS0 | Durability bootstrap + checkpoint/restart + disk-backed WAL adapter + WAL recovery wiring merged; store index/constraint runtime handlers now enforce operator auth + resource-scoped RBAC, validated by `tests/kpi/results/ws2/ws2-index-constraint-smoke.json`; gate orchestrator `run-ws2-gate.ps1` -> `tests/kpi/results/ws2/ws2-gate-summary.json`; workflow wiring in `.github/workflows/ci.yml` |

### Requirements Covered
- REQ-01 (ANSI SQL + AI extract) — SQL analyzer baseline in `crates/voltnuerongrid-sql` + runtime analyze/execute smokes plus tenant SQL route/transaction tests validating tenant-scoped user RBAC on SQL endpoints
- REQ-02 (DB/table/view lifecycle) — Statement classifier includes create/alter/drop/view/function lifecycle categories
- REQ-03 (Rust/JS/Python function support) — 🟡 Ready for Validation: WS1 runtime UDF scaffold with function-catalog contract, per-language guard-policy contract, and statement-level execution-plan routing evidence; closure/release linkage includes WS1 closure gate + R1 SQL/UDF gate + R3 UDF runtime gate
- REQ-05 (Separate compute/data files) — WS2 durability contract scaffold + validation smoke
- REQ-12 (Seeded functions + plan-plat parity) — P0/P1/P2 parity gap report with P2 stub closures
- REQ-23 (ACID transactions) — Transaction endpoint classifies and validates statements before commit path

### Sprint 1 Deliverables
- [ ] WS0: CI pipeline fully green with all gate scripts wired
- [ ] WS1: SQL parser + analyzer + DDL-DML statement classifier complete
- [x] WS1: `/api/v1/sql/analyze` endpoint online
- [x] WS1: `/api/v1/sql/execute` UDF runtime scaffold with function catalog
- [x] WS2: Durability bootstrap + checkpoint/restart + WAL adapter merged
- [ ] WS2: Index and constraint engine baseline

### Gate Evidence — WS1 UDF Runtime (REQ-03)

| Gate | Scope | Status Source | CI Summary Artifact | CI Badge Artifact |
|---|---|---|---|---|
| WS1 UDF Runtime Gate | Epic 1 + REQ-03 (polyglot UDF execution + function catalog contract + per-language guard policies + execution-plan routing evidence) | `tests/kpi/results/gates/ws1-release-readiness.json` | `tests/kpi/results/gates/ci-ws1-release-readiness.json` | `tests/kpi/results/gates/ci-ws1-udf-stability-badge.json` |

### Gate Evidence — WS1 Closure and R1/R3 Linkage

| Gate | Scope | Status Source | CI Summary Artifact |
|---|---|---|---|
| WS1 Closure Gate | WS1 UDF runtime validation posture check (`REQ-03`) | `tests/kpi/results/ws1/ws1-closure-gate-summary.json` | `tests/kpi/results/ws1/ci-ws1-closure-gate-summary.json` |
| Release R1 SQL/UDF Gate | R1 release-readiness linkage (`WS1` + `REQ-03` + R1 prerequisite checklist) | `tests/kpi/results/gates/release-r1-sql-udf-readiness.json` | `tests/kpi/results/gates/ci-release-r1-sql-udf-readiness.json` |
| Release R3 UDF Runtime Gate | R3 release-readiness linkage (`WS1` + `REQ-03` + autonomous R3 baseline) | `tests/kpi/results/gates/release-r3-udf-runtime-readiness.json` | `tests/kpi/results/gates/ci-release-r3-udf-runtime-readiness.json` |

---

## Sprint 2 — SQL Parity + Row Store + HTAP Query

**Goal:** Legacy aggregation parity, transactional row store with HTAP sync origin, HTAP query execution and routing.
**Dependencies:** Sprint 1 (WS1, WS2 foundations)
**Status:** 🔵 In Progress

### Workstreams

| WS ID | Epic | Scope Summary | Owner | Status | Dependencies | Validation Evidence |
|---|---|---|---|---|---|---|
| WS1A | Epic 1A | Legacy aggregation parity (P0/P1/P2) | Compute + Migration Team | 🔵 In Progress | WS1 | Bucketed manifests + P2 stub implementations + gap report outputs in place; gate orchestrator `run-ws1a-gate.ps1` -> `tests/kpi/results/ws1a/ws1a-gate-summary.json`; UDF bridge pack `run-ws1a-udf-contract-bridge-smoke.ps1` -> `tests/kpi/results/ws1a/ws1a-udf-contract-bridge-smoke.json`; workflow wiring in `.github/workflows/ci.yml` |
| WS2A | Epic 2 (E2.1a) | Transactional row store and HTAP sync origin | Storage Team | 🔵 In Progress | WS2 | Row-sync origin scaffold + smoke evidence captured; gate orchestrator `run-ws2a-gate.ps1` -> `tests/kpi/results/ws2a/ws2a-gate-summary.json`; workflow wiring in `.github/workflows/ci.yml` |
| WS3 | Epic 3 | HTAP query execution and routing | Query/Runtime Team | 🔵 In Progress | WS2 | Route-decision scaffold + runtime SQL dispatch endpoint `/api/v1/sql/execute` + gate orchestrator `run-ws3-gate.ps1` -> `tests/kpi/results/ws3/ws3-gate-summary.json`; performance target-contract/score/trend/badge/release artifacts; workflow wiring in `.github/workflows/ci.yml` |

### Requirements Covered
- REQ-12 (Seeded functions + parity) — P0/P1/P2 parity gap report with P2 stub closures (`tests/kpi/results/parity/legacy-aggregation-gap-report.json`)
- REQ-23 (ACID transactions) — Row store transactional path
- REQ-31 (HTAP extreme performance) — WS3 performance hardening: HTAP target-contract smoke, weighted performance scoring, trend comparator, stability badge, and WS3 release summary

### Sprint 2 Deliverables
- [ ] WS1A: Complete P0/P1 parity closures beyond P2 stubs
- [x] WS1A: Bucketed manifests + P2 stub implementations in place
- [x] WS2A: Row-sync origin scaffold + smoke evidence captured
- [x] WS3: Route-decision scaffold + runtime SQL dispatch endpoint online
- [ ] WS3: Full HTAP routing policy enforcement

### Gate Evidence — WS3 Performance (REQ-31)

| Gate | Scope | Status Source | CI Summary Artifact | CI Badge Artifact |
|---|---|---|---|---|
| WS3 HTAP Performance Gate | Epic 3 + REQ-31 (HTAP throughput target-contract parity + weighted performance score + trend stability) | `tests/kpi/results/gates/ws3-release-readiness.json` | `tests/kpi/results/gates/ci-ws3-release-readiness.json` | `tests/kpi/results/gates/ci-ws3-performance-stability-badge.json` |

---

## Sprint 3 — Ingestion + Pessimistic Locking

**Goal:** High-speed ingestion pipeline, pessimistic locking scaffold.
**Dependencies:** Sprint 1 (WS2 storage foundation)
**Status:** 🔵 In Progress

### Workstreams

| WS ID | Epic | Scope Summary | Owner | Status | Dependencies | Validation Evidence |
|---|---|---|---|---|---|---|
| WS4 | Epic 4 | High-speed ingestion pipeline | Ingestion Team | 🔵 In Progress | WS2 | Ingestion connector/registry scaffold + runtime ingest handlers now enforce mixed operator-or-tenant RBAC with tenant-scoped connector visibility, validated by `tests/kpi/results/ws4/ws4-ingest-parser-smoke.json`; gate orchestrator `run-ws4-gate.ps1` -> `tests/kpi/results/ws4/ws4-gate-summary.json`; workflow wiring in `.github/workflows/ci.yml` |
| WS22 | (Epic 1 sub) | Pessimistic locking baseline | SQL Engine Team | 🔵 In Progress | WS1 | Runtime pessimistic-lock scaffold endpoints (`/api/v1/sql/locks/pessimistic/acquire`, `/api/v1/sql/locks/pessimistic/release`) with conflict/ownership enforcement + lock contention metrics endpoint (`/api/v1/sql/locks/pessimistic/metrics`) exposing deadlock-detection vs cap-hit-timeout counts + contention ratio for trend artifacts + WS22 smoke/gate posture evidence (`tests/kpi/results/ws22/ws22-pessimistic-lock-smoke.json`, `tests/kpi/results/ws22/ws22-lock-contention-metrics-smoke.json`, `tests/kpi/results/ws22/ws22-gate-summary.json`) and unit evidence (`cargo test -p voltnuerongridd ws22_`) |

### Requirements Covered
- REQ-06 (CSV/Parquet/JSON/Excel + enterprise ingest) — WS4 ingest scaffold + CSV/JSON parser connectors with runtime endpoints now protected by mixed operator-or-tenant RBAC and backed by updated smoke evidence
- REQ-07 (Multithreaded high-speed import) — ⬜ Not Started: Ingest throughput benchmark pending
- REQ-19 (Blazing ingest/update/read at scale) — ⬜ Not Started: KPI benchmark gates pending
- REQ-22 (Pessimistic locking) — WS22 runtime scaffold with conflict/ownership enforcement + contention metrics endpoint for trend analysis

### Sprint 3 Deliverables
- [x] WS4: Ingestion connector/registry scaffold created
- [ ] WS4: Multi-format ingest (CSV/Parquet/JSON/Excel) runtime implementation
- [x] WS4: Runtime ingest endpoints enforce operator auth + resource-scoped RBAC
- [ ] WS4: Multithreaded import benchmark (REQ-07)
- [x] WS22: Pessimistic lock acquire/release endpoints online
- [x] WS22: Conflict/ownership enforcement unit tests passing
- [x] WS22: Lock contention metrics endpoint (`/api/v1/sql/locks/pessimistic/metrics`) with deadlock/cap-hit/timeout/grant/conflict/release counters + contention ratio
- [x] WS22: Contention metrics unit test + smoke script

### Gate Evidence — WS22 Pessimistic Locking (REQ-22)

| Gate | Scope | Status Source | CI Summary Artifact |
|---|---|---|---|
| WS22 Pessimistic Locking Gate | Epic 1 + REQ-22 (pessimistic lock acquire/release contracts + conflict/ownership + contention metrics posture) | `tests/kpi/results/ws22/ws22-gate-summary.json` | `tests/kpi/results/ws22/ci-ws22-gate-summary.json` |
| WS22 Lock Contention Metrics | Epic 1 + REQ-22 (deadlock-detection vs cap-hit-timeout vs wait-timeout vs grant vs conflict vs release counts + contention ratio) | `tests/kpi/results/ws22/ws22-lock-contention-metrics-smoke.json` | (included in ws22-gate-summary) |

### Gate Evidence — WS2 Index + Constraint Engine (REQ-11)

| Gate | Scope | Status Source |
|---|---|---|
| WS2 Index/Constraint Smoke | Epic 2 + REQ-11 (B-tree index engine + constraint validator with PK/Unique/NotNull + runtime endpoints protected by operator auth + RBAC) | `tests/kpi/results/ws2/ws2-index-constraint-smoke.json` |
| WS2 Gate Summary | Epic 2 (store durability + WAL + checkpoint + index/constraint) | `tests/kpi/results/ws2/ws2-gate-summary.json` |

### Gate Evidence — WS4 Ingest Parsers (REQ-06)

| Gate | Scope | Status Source |
|---|---|---|
| WS4 Ingest Parser Smoke | Epic 4 + REQ-06 (CSV + JSON/NDJSON connectors + runtime ingest endpoints protected by mixed operator-or-tenant RBAC) | `tests/kpi/results/ws4/ws4-ingest-parser-smoke.json` |
| WS4 Gate Summary | Epic 4 (ingest plugin scaffold + CSV/JSON parsers) | `tests/kpi/results/ws4/ws4-gate-summary.json` |

---

## Sprint 4 — Streaming + Security

**Goal:** Streaming in/out + event streams, auth/RBAC/TLS/TDE/KMS.
**Dependencies:** Sprint 3 (WS4 ingest baseline), Sprint 1 (WS0 for security)
**Status:** 🔵 In Progress / 🟡 WS5 Ready for Validation

### Workstreams

| WS ID | Epic | Scope Summary | Owner | Status | Dependencies | Validation Evidence |
|---|---|---|---|---|---|---|
| WS4A | Epic 4A | Streaming in/out + event streams | Ingestion + Eventing Team | 🔵 In Progress | WS4 | Source/sink interfaces + replayable envelope/event-log + replay-cursor durability bridge scaffold + gate orchestrator `run-ws4a-gate.ps1` -> `tests/kpi/results/ws4a/ws4a-gate-summary.json`; workflow wiring in `.github/workflows/ci.yml` |
| WS5 | Epic 5 | Auth, RBAC, TLS/TDE/KMS | Security Team | 🟡 Ready for Validation | WS0 | Operator admin-key auth gate scaffolded for autonomous control endpoints, then extended into registered operator identity + resource-scoped RBAC privilege matrix enforcement for failover/SRE/audit/autonomous plus mixed operator-or-tenant ingest handlers and tenant-scoped SQL runtime access + TLS/mTLS/encryption-at-rest/KMS security contract checks across JSON/YAML/properties + WS5 smoke harness + gate orchestrator `run-ws5-gate.ps1` -> `tests/kpi/results/ws5/ws5-gate-summary.json`; release-facing CI gate summary + badge `tests/kpi/results/gates/ci-ws5-gate-summary.json`, `tests/kpi/results/gates/ci-ws5-gate-badge.json`; combined DX/API cluster gate -> `tests/kpi/results/gates/release-dx-api-readiness.json`; workflow wiring in `.github/workflows/ci.yml` |

### Requirements Covered
- REQ-13 (Multi-user roles and privileges) — 🔵 In Progress: Shared RBAC privilege matrix + resource-scoped operator and tenant-user grants enforced in runtime across control-plane, storage, mixed ingest, and tenant-scoped SQL surfaces; broader user/tenant hierarchy still pending
- REQ-16 (SSL + encryption/decryption) — Security contract enforces TLS/mTLS + encryption-at-rest + KMS constraints
- REQ-18 (Stream in/out + events for debug/audit) — WS4A streaming + replay cursor scaffolds with gate summary
- REQ-26 (Plugin model for streaming sources/sinks) — WS4A + WS7 linkage in progress

### Sprint 4 Deliverables
- [x] WS4A: Source/sink interfaces + replayable envelope/event-log scaffold
- [x] WS4A: Replay-cursor durability bridge scaffold
- [ ] WS4A: Full streaming runtime with production replay semantics
- [x] WS5: TLS/mTLS + encryption-at-rest/KMS security contract checks
- [x] WS5: Operator admin-key auth gate for autonomous endpoints
- [x] WS5: Operator-scoped RBAC privilege matrix baseline (REQ-13)
- [ ] WS5: Full RBAC role matrix validation (REQ-13)

### Gate Evidence — WS5 Security

| Gate | Scope | Status Source | CI Summary Artifact | CI Badge Artifact |
|---|---|---|---|---|
| WS5 Security Gate | Epic 5 (Auth/RBAC/TLS/TDE/KMS) | `tests/kpi/results/ws5/ws5-gate-summary.json` | `tests/kpi/results/gates/ci-ws5-gate-summary.json` | `tests/kpi/results/gates/ci-ws5-gate-badge.json` |

---

## Sprint 5 — Distributed HA/FT + Release R1 Gate

**Goal:** Distributed HA/FT/autoscaling/anti-SPOF, close R1 release gate.
**Dependencies:** Sprint 1 (WS2), Sprint 2 (WS3)
**Status:** 🟡 WS6 Ready for Validation / R1 In Progress

### Workstreams

| WS ID | Epic | Scope Summary | Owner | Status | Dependencies | Validation Evidence |
|---|---|---|---|---|---|---|
| WS6 | Epic 6 | Distributed HA/FT/autoscaling/anti-SPOF | Distributed Systems Team | 🟡 Ready for Validation | WS2, WS3 | Failover leader-state scaffold + authenticated failover simulation now emits runtime handoff-report evidence (replay batch size, applied count, gap detection) from explicit multi-node replication transport events consumed by failover/DR/SRE runtime paths instead of seeded scaffold data + deep hardening packs (multi-node handoff matrix, replication-lag failure/reconcile, RTO/RPO threshold score, chaos node-loss/rejoin, flap-resistance, reconcile latency envelopes); post-gate exports for chaos fault-injection matrix, gate trend comparator, failover stability badge, release summary; closure gate `run-ws6-closure-gate.ps1` -> `tests/kpi/results/ws6/ws6-closure-gate-summary.json`; R2 release gate `run-release-r2-failover-gate.ps1` -> `tests/kpi/results/gates/release-r2-failover-readiness.json`; workflow wiring in `.github/workflows/ci.yml` |

### Requirements Covered
- REQ-04 (HA/FT/elasticity) — WS6 failover + anti-SPOF + chaos hardening packs
- REQ-05 (Separate compute/data) — Distributed storage separation in WS6 cluster topology
- REQ-17 (Distributed failover + zero data loss) — 🟡 Ready for Validation: WS6 closure + R2 failover release gate evidence + runtime handoff-report failover contract smoke now backed by explicit multi-node replication transport events
- REQ-10 (Trillion-row scale retrieval) — ⬜ Not Started: Scale benchmark report pending
- REQ-19 (Blazing performance at scale) — ⬜ Not Started: KPI benchmark gates pending
- REQ-21 (Any-number-user concurrency) — ⬜ Not Started: Concurrency stress tests pending

### Sprint 5 Deliverables
- [x] WS6: Failover leader-state scaffold + authenticated simulation endpoint
- [x] WS6: Multi-node handoff matrix + replication-lag scenarios
- [x] WS6: RTO/RPO threshold scoring
- [x] WS6: Chaos node-loss/rejoin + flap-resistance + reconcile latency
- [x] WS6: Closure gate + R2 failover release gate evidence
- [ ] R1 Release Gate: Single-node HTAP baseline + SQL/ingest/RBAC/basic drivers fully validated

### R1 Release Gate

| Release | Scope Snapshot | Status | Gate Criteria |
|---|---|---|---|
| R1 | Single-node HTAP baseline + SQL/ingest/RBAC/basic drivers | 🔵 In Progress | PR-002..PR-005 complete + KPI smoke baseline (`tests/kpi/results/gates/r1-gate-check.json`) + WS1 UDF closure posture (`tests/kpi/results/ws1/ws1-closure-gate-summary.json`) + release R1 SQL/UDF gate (`tests/kpi/results/gates/release-r1-sql-udf-readiness.json`) |

### Gate Evidence — WS6 Release + Closure

| Gate | Scope | Status Source | CI Summary Artifact | CI Badge Artifact |
|---|---|---|---|---|
| WS6 Failover Resilience Gate | Epic 6 + REQ-17 (Distributed HA/FT, failover, zero data loss) | `tests/kpi/results/gates/ws6-release-readiness.json` | `tests/kpi/results/gates/ci-ws6-release-readiness.json` | `tests/kpi/results/gates/ci-ws6-failover-stability-badge.json` |

| Gate | Scope | Status Source | CI Summary Artifact |
|---|---|---|---|
| WS6 Closure Gate | WS6 validation posture check | `tests/kpi/results/ws6/ws6-closure-gate-summary.json` | `tests/kpi/results/ws6/ci-ws6-closure-gate-summary.json` |
| Release R2 Failover Gate | R2 failover release-readiness linkage (`WS6` + Ops/Resilience cluster) | `tests/kpi/results/gates/release-r2-failover-readiness.json` | `tests/kpi/results/gates/ci-release-r2-failover-readiness.json` |

---

## Sprint 6 — Plugin + AI + Audit

**Goal:** Plugin framework + connector pack, AI-native control plane, audit engine + AI agent authoring.
**Dependencies:** Sprint 1 (WS1), Sprint 4 (WS4A, WS5), Sprint 5 (WS6)
**Status:** 🟡 All Ready for Validation

### Workstreams

| WS ID | Epic | Scope Summary | Owner | Status | Dependencies | Validation Evidence |
|---|---|---|---|---|---|---|
| WS7 | Epic 7 | Plugin framework + connector plugin pack | Extensibility Team | 🟡 Ready for Validation | WS1, WS4A | Signed manifest schema + checksum + keyring trust/revocation policy hooks + WS7 extended gate with compliance matrix/trend/badge/release summary; closure gate -> `tests/kpi/results/ws7/ws7-closure-gate-summary.json`; R3 linkage gate -> `tests/kpi/results/gates/release-r3-plugin-readiness.json`; workflow wiring in `.github/workflows/ci.yml` |
| WS8 | Epic 8 | AI-native + autonomous control plane | AI Platform Team | 🟡 Ready for Validation | WS1, WS6 | Typed autonomous action execution records + guardrail decision trace IDs + mode-governance/blast-radius policy-deny evidence; post-gate autonomy matrix/trend/badge/release summary; closure gate -> `tests/kpi/results/ws8/ws8-closure-gate-summary.json`; R3 linkage gate -> `tests/kpi/results/gates/release-r3-autonomous-readiness.json`; workflow wiring in `.github/workflows/ci.yml` |
| WS8A | Epic 8A | Data audit engine + companion | Audit/Compliance Team | 🟡 Ready for Validation | WS4A, WS5 | Audit event contract + append-only sink + runtime emission + companion query/export filters + AI agent authoring/object-plugin workflow evidence; WS8A gate -> `tests/kpi/results/ws8a/ws8a-gate-summary.json`; closure gate -> `tests/kpi/results/ws8a/ws8a-closure-gate-summary.json`; R3 linkage gate -> `tests/kpi/results/gates/release-r3-agent-authoring-readiness.json`; workflow wiring in `.github/workflows/ci.yml` |

### Requirements Covered
- REQ-01 (ANSI SQL + AI chat/extract) — WS8 AI-native control plane
- REQ-09 (Extensible plugin ecosystem) — 🟡 Ready for Validation: WS7 closure hardening with plugin boundary/integrity/policy gates
- REQ-26 (Plugin model for streaming sources/sinks) — WS7 plugin registration boundary + signed manifest policy/revocation checks
- REQ-29 (Fully autonomous operations) — 🟡 Ready for Validation: WS8 autonomous control-plane, guardrail policy, audit linkage, autonomy matrix, stability badge
- REQ-30 (AI agent authoring) — 🟡 Ready for Validation: WS8A agent authoring workflow smoke (object + plugin controls), matrix/trend/badge artifacts

### Sprint 6 Deliverables
- [x] WS7: Signed manifest schema + checksum + keyring trust/revocation hooks
- [x] WS7: Compliance matrix + trend comparator + stability badge
- [x] WS7: Closure gate + R3 plugin release gate evidence
- [x] WS8: Typed autonomous action execution records + guardrail trace IDs
- [x] WS8: Mode-governance/blast-radius policy evidence
- [x] WS8: Closure gate + R3 autonomous release gate evidence
- [x] WS8A: Audit event contract + append-only sink + companion query/export
- [x] WS8A: Agent authoring workflow smoke (object + plugin controls)
- [x] WS8A: Closure gate + R3 agent authoring release gate evidence
- [ ] Final validation sign-off for WS7, WS8, WS8A

### Gate Evidence — WS7 Plugin

| Gate | Scope | Status Source | CI Summary Artifact | CI Badge Artifact |
|---|---|---|---|---|
| WS7 Plugin Resilience Gate | Epic 7 + REQ-09 (Plugin registration boundary, signed manifest policy, revocation controls) | `tests/kpi/results/gates/ws7-release-readiness.json` | `tests/kpi/results/gates/ci-ws7-release-readiness.json` | `tests/kpi/results/gates/ci-ws7-plugin-stability-badge.json` |

| Gate | Scope | Status Source | CI Summary Artifact |
|---|---|---|---|
| WS7 Closure Gate | WS7 validation posture check | `tests/kpi/results/ws7/ws7-closure-gate-summary.json` | `tests/kpi/results/ws7/ci-ws7-closure-gate-summary.json` |
| Release R3 Plugin Gate | R3 plugin release-readiness linkage (`WS7` + `WS9A`) | `tests/kpi/results/gates/release-r3-plugin-readiness.json` | `tests/kpi/results/gates/ci-release-r3-plugin-readiness.json` |

### Gate Evidence — WS8 Autonomous

| Gate | Scope | Status Source | CI Summary Artifact | CI Badge Artifact |
|---|---|---|---|---|
| WS8 Autonomous Control-Plane Gate | Epic 8 + REQ-29 (autonomous action governance, guardrail policy, emergency-stop controls, audit linkage) | `tests/kpi/results/gates/ws8-release-readiness.json` | `tests/kpi/results/gates/ci-ws8-release-readiness.json` | `tests/kpi/results/gates/ci-ws8-autonomy-stability-badge.json` |

| Gate | Scope | Status Source | CI Summary Artifact |
|---|---|---|---|
| WS8 Closure Gate | WS8 validation posture check | `tests/kpi/results/ws8/ws8-closure-gate-summary.json` | `tests/kpi/results/ws8/ci-ws8-closure-gate-summary.json` |
| Release R3 Autonomous Gate | R3 autonomous release-readiness linkage (`WS8` + `WS7`) | `tests/kpi/results/gates/release-r3-autonomous-readiness.json` | `tests/kpi/results/gates/ci-release-r3-autonomous-readiness.json` |

### Gate Evidence — WS8A Agent Authoring

| Gate | Scope | Status Source | CI Summary Artifact | CI Badge Artifact |
|---|---|---|---|---|
| WS8A Agent Authoring Gate | Epic 8A + REQ-30 (audit companion flows + AI agent object/plugin authoring workflow guardrails) | `tests/kpi/results/gates/ws8a-release-readiness.json` | `tests/kpi/results/gates/ci-ws8a-release-readiness.json` | `tests/kpi/results/gates/ci-ws8a-agent-stability-badge.json` |

| Gate | Scope | Status Source | CI Summary Artifact |
|---|---|---|---|
| WS8A Closure Gate | WS8A validation posture check | `tests/kpi/results/ws8a/ws8a-closure-gate-summary.json` | `tests/kpi/results/ws8a/ci-ws8a-closure-gate-summary.json` |
| Release R3 Agent Authoring Gate | R3 agent-authoring release-readiness linkage (`WS8A` + `WS8` + `WS7`) | `tests/kpi/results/gates/release-r3-agent-authoring-readiness.json` | `tests/kpi/results/gates/ci-release-r3-agent-authoring-readiness.json` |

---

## Sprint 7 — UX/DX + Drivers + i18n

**Goal:** Studio UI, IDE extensions, drivers + pooling, internationalization.
**Dependencies:** Sprint 1 (WS1), Sprint 5 (WS6), Sprint 7 (WS10 feeds WS9A)
**Status:** 🟡 All Ready for Validation

### Workstreams

| WS ID | Epic | Scope Summary | Owner | Status | Dependencies | Validation Evidence |
|---|---|---|---|---|---|---|
| WS9 | Epic 9 | Studio UI | UX Team | 🟡 Ready for Validation | WS1, WS3 | Studio API client contracts + endpoint/header/type checks + contract script execution via WS9 smoke harness + gate orchestrator `run-ws9-gate.ps1` -> `tests/kpi/results/ws9/ws9-gate-summary.json`; combined DX/API cluster gate -> `tests/kpi/results/gates/release-dx-api-readiness.json`; workflow wiring in `.github/workflows/ci.yml` |
| WS9A | Epic 9A | IDE extension suite | DX Team | 🟡 Ready for Validation | WS1, WS10 | Shared IDE API contract + VS/Cursor/Antigravity/JetBrains/Eclipse adapter manifests + WS9A smoke harness + gate orchestrator `run-ws9a-gate.ps1` -> `tests/kpi/results/ws9a/ws9a-gate-summary.json`; combined DX/API cluster gate -> `tests/kpi/results/gates/release-dx-api-readiness.json`; workflow wiring in `.github/workflows/ci.yml` |
| WS10 | Epic 10 | Drivers + pooling + gateway/session routing | Integrations Team | 🟡 Ready for Validation | WS1, WS6 | Rust driver request builder now emits session/tenant/user/admin/operator headers and exposes SQL analyze/route/execute/transaction request builders + JSON/properties/YAML `DriverRoutingConfigContract` parsing/validation + WS10 smoke harness + gate orchestrator `run-ws10-gate.ps1` -> `tests/kpi/results/ws10/ws10-gate-summary.json`; combined DX/API cluster gate -> `tests/kpi/results/gates/release-dx-api-readiness.json`; workflow wiring in `.github/workflows/ci.yml` |
| WS11 | Epic 11 | Internationalization and UTF-8 | Platform + UX Team | 🟡 Ready for Validation | WS1 | Locale parsing + i18n catalog messages + runtime `/api/v1/i18n/messages` + locale fallback policy tests in SQL/runtime + WS11 smoke harness + gate orchestrator `run-ws11-gate.ps1` -> `tests/kpi/results/ws11/ws11-gate-summary.json`; workflow wiring in `.github/workflows/ci.yml` |

### Requirements Covered
- REQ-14 (UI + engine separation) — Studio API contract checks validate endpoint/header/type coverage
- REQ-15 (Driver support multi-language) — Rust driver baseline + tenant/user session header emission + JSON/YAML/properties routing contract parse coverage
- REQ-25 (Native connection + pooling) — Driver routing contract enforces pool min/max + timeout constraints
- REQ-28 (IDE extensions) — Shared IDE contract + provider manifests for VS/Cursor/Antigravity/JetBrains/Eclipse

### Sprint 7 Deliverables
- [x] WS9: Studio API client contracts + contract script execution
- [x] WS9A: Shared IDE API contract + all adapter manifests
- [x] WS10: Rust driver request builder + routing config contract across formats
- [x] WS11: Locale parsing + i18n catalog + runtime `/api/v1/i18n/messages`
- [x] Combined DX/API cluster gate evidence published
- [ ] Final validation sign-off for WS9, WS9A, WS10, WS11

---

## Sprint 8 — Reliability + Ops + Config + Release R2 Gate

**Goal:** Reliability/SRE/DR automation, multi-cloud deployment profiles, config contracts + tuning playbooks. Close R2 release gate.
**Dependencies:** Sprint 5 (WS6), Sprint 4 (WS5), Sprint 7 (WS10)
**Status:** 🟡 All Ready for Validation / R2 In Progress

### Workstreams

| WS ID | Epic | Scope Summary | Owner | Status | Dependencies | Validation Evidence |
|---|---|---|---|---|---|---|
| WS12 | Epic 12 | Reliability/SRE/DR automation | SRE Team | 🟡 Ready for Validation | WS6 | Runtime SRE hardening contracts: `/api/v1/sre/reliability/status`, `/api/v1/sre/rate-limit/check`, `/api/v1/sre/failure-budget/alerts`, `/api/v1/sre/dr/hooks/{policy,retry-plan,schedule,trigger,status}`, `/api/v1/sre/failure/{signal,reconcile}`, `/api/v1/sre/gate/{evaluate,export}`; includes file-backed DR policy/runtime persistence, scheduler queue scaffold, critical-signal reconciliation, gate-fail artifact exporter, multi-node failure signal ingestion; combined Ops/Resilience cluster gate -> `tests/kpi/results/gates/release-ops-resilience-readiness.json` |
| WS13 | Epic 13 | Multi-cloud deployment profiles | Platform/SRE | 🟡 Ready for Validation | WS0, WS12 | Deploy cloud profile contracts + provider runtime overrides `single-node`/`multi-node` + provider Helm values + provider runbook env matrices for AWS/Azure/GCP; WS13 smoke harnesses + CI gate orchestrator `run-ws13-gate.ps1` -> `tests/kpi/results/ws13/ws13-gate-summary.json`; combined Ops/Resilience cluster gate -> `tests/kpi/results/gates/release-ops-resilience-readiness.json`; workflow wiring in `.github/workflows/ci.yml` |
| WS14 | Epic 14 | Config contracts + tuning playbooks | Platform + SRE + Security | 🟡 Ready for Validation | WS5, WS10 | Driver/security config schemas YAML/JSON/properties + validation helpers + WS14 smoke harness + schema lint gate `run-ws14-schema-lint-gate.ps1` + config conformance aggregator + gate orchestrator `run-ws14-gate.ps1` -> `tests/kpi/results/ws14/ws14-gate-summary.json`; combined Ops/Resilience cluster gate -> `tests/kpi/results/gates/release-ops-resilience-readiness.json`; workflow wiring in `.github/workflows/ci.yml` |

### Requirements Covered
- REQ-04 (HA/FT/elasticity) — WS12 reliability contracts + DR hooks
- REQ-08 (Local + cloud SaaS operation) — ⬜ Not Started: Local/cloud deployment smoke tests pending
- REQ-17 (Distributed failover + zero data loss) — WS12 SRE DR automation contracts
- REQ-20 (Azure/AWS/GCP/OCI + Docker + K8s) — WS13 multi-cloud profile gates
- REQ-24 (Config via properties/YAML/JSON) — WS14 schema/conformance gates
- REQ-25 (Native connection + pooling) — WS14 driver/security config integration
- REQ-29 (Fully autonomous operations) — WS14 config-driven autonomous tuning

### Sprint 8 Deliverables
- [x] WS12: Full SRE endpoint suite (reliability, rate-limit, failure-budget, DR hooks, failure signal/reconcile, gate evaluate/export)
- [x] WS12: File-backed DR policy + scheduler queue scaffold
- [x] WS13: AWS/Azure/GCP cloud profile contracts + Helm values
- [x] WS13: Provider runbook env matrices
- [x] WS14: Config schema lint gate + conformance aggregator
- [x] Combined Ops/Resilience cluster gate evidence published
- [ ] R2 Release Gate: Distributed HTAP + HA + connectors + anti-SPOF fully validated

### R2 Release Gate

| Release | Scope Snapshot | Status | Gate Criteria |
|---|---|---|---|
| R2 | Distributed HTAP baseline + HA + connectors + anti-SPOF High closure | 🔵 In Progress | High SPOF closure + failover/RPO evidence + Ops/Resilience cluster readiness summary (`tests/kpi/results/gates/release-ops-resilience-readiness.json`) + WS6 release readiness summary (`tests/kpi/results/gates/ws6-release-readiness.json`) + release R2 failover gate (`tests/kpi/results/gates/release-r2-failover-readiness.json`) |

---

## Sprint 9 — Competitive + P0 Hardening

**Goal:** Competitive feature adoption track, P0 architecture hardening items (H-01..H-04).
**Dependencies:** Sprint 2 (WS3), Sprint 5 (WS6), Sprint 4 (WS5)
**Status:** 🔵 Mixed (WS15 Ready for Validation, H-01/H-02 In Progress, H-03/H-04 early)

### Workstreams

| WS ID | Epic | Scope Summary | Owner | Status | Dependencies | Validation Evidence |
|---|---|---|---|---|---|---|
| WS15 | Epic 15 | Competitive feature adoption track | Architecture + Query Team | 🟡 Ready for Validation | WS3 | Competitive adoption matrix contract scaffold `reference/competitive/ws15-feature-adoption-matrix.json` + scored implementation backlog `reference/competitive/ws15-implementation-backlog.json`; WS15 smoke harnesses + gate orchestrator `run-ws15-gate.ps1` -> `tests/kpi/results/ws15/ws15-gate-summary.json`; workflow wiring in `.github/workflows/ci.yml` |

### Architecture Hardening Backlog — P0 (Release Target: R2)

| ID | Hardening Item | Owner | Priority | Status | Completion | This Week Completed | Blocked By | Next Evidence Milestone |
|---|---|---|---|---|---|---|---|---|
| H-01 | Autonomous action blast-radius controls | AI Platform + Security | P0 | 🔵 In Progress | 96% | Added operator auth gate (`VNG_ADMIN_API_KEY` + `x-vng-admin-key`) for autonomous control endpoints, then layered registered operator identity + role binding enforcement via `x-vng-operator-id` plus a shared resource-scoped RBAC privilege matrix now enforced across control-plane, storage, mixed ingest, tenant-scoped SQL runtime handlers, and tenant-aware driver headers; retained versioned/checksummed DR policy-state persistence envelope with corruption fallback to `.bak` and legacy snapshot compatibility tests (`ws12_`) | Full RBAC integration pending | Extend operator-scoped RBAC into broader user/tenant privilege hierarchies |
| H-02 | HTAP sync correctness under failures | Storage + Distributed Systems | P0 | 🔵 In Progress | 95% | Added restart/replay integrity tests + matrix harness artifact `tests/kpi/results/h02/h02-restart-replay-matrix.json`; matrix now includes persisted WAL recovery signal plus multi-node replay/failover handoff matrix artifact `tests/kpi/results/h02/h02-multi-node-handoff-matrix.json`, and WS6 runtime now consumes explicit multi-node replication transport events for handoff replay | Full distributed transport runtime and leader-election integration not yet implemented | Replace in-memory transport with real network transport while preserving replay contract |
| H-03 | Control-plane resilience hardening | Distributed Systems | P0 | 🔵 In Progress | 15% | Control-plane clustering requirement and SPOF closure criteria documented | Cluster runtime implementation pending | Control-plane chaos test plan v1 |
| H-04 | Event durability hardening (outbox/replay) | Distributed Systems + SRE | P0 | 🔵 In Progress | 20% | Outbox and replay durability controls defined in architecture | Event bus/outbox services pending | Exactly-once replay test harness draft |

### Sprint 9 Deliverables
- [x] WS15: Competitive adoption matrix + scored implementation backlog
- [x] WS15: Gate orchestrator + CI wiring
- [x] H-01: Operator auth gate + runtime tests + WS5 smoke harness (65%)
- [x] H-02: Restart/replay integrity tests + WAL recovery signal (65%)
- [x] H-01: Policy persistence hardening (versioned/checksummed envelope + backup fallback + compatibility tests)
- [x] H-01: Resource-scoped operator RBAC grants
- [x] H-01: Role-based operator identity beyond shared admin key
- [ ] H-01: Full RBAC integration
- [x] H-02: Multi-node transport replay + failover handoff matrix
- [x] H-02: WS6 live transport-fed handoff runtime wiring
- [x] H-02: Explicit multi-node replication transport abstraction
- [ ] H-03: Control-plane chaos test plan v1 (15%)
- [ ] H-04: Exactly-once replay test harness draft (20%)

---

## Sprint 10 — P1 Hardening + Release R3 Gate

**Goal:** P1 architecture hardening items (H-05..H-08), close R3 release gate.
**Dependencies:** Sprint 9 (P0 hardening), Sprint 6 (WS7, WS8, WS8A), Sprint 7 (WS9A, WS10)
**Status:** ⬜ Not Started

### Architecture Hardening Backlog — P1 (Release Target: R3)

| ID | Hardening Item | Owner | Priority | Status | Completion | Blocked By | Next Evidence Milestone |
|---|---|---|---|---|---|---|---|
| H-05 | KMS multi-region failover hardening | Security | P1 | ⬜ Not Started | 0% | KMS integration code pending | KMS outage simulation checklist |
| H-06 | Distributed cache hardening | Query + SRE | P1 | ⬜ Not Started | 0% | Cache engine baseline not implemented | Cache resilience benchmark plan |
| H-07 | Driver/pooling storm hardening | Integrations | P1 | ⬜ Not Started | 0% | Driver implementations pending | Driver failover load test design |
| H-08 | Autonomous plugin supply-chain hardening | Security + AI Platform | P1 | ⬜ Not Started | 0% | Plugin builder pipeline pending | Supply-chain validation policy draft |

### Requirements Covered (Not Started — pending in this sprint)
- REQ-27 (Native cache engine, Redis-like compat) — ⬜ Not Started: Cache failover/invalidation tests

### Sprint 10 Deliverables
- [ ] H-05: KMS outage simulation + multi-region fallback
- [ ] H-06: Distributed cache resilience benchmark
- [ ] H-07: Driver/pooling failover load tests
- [ ] H-08: Plugin supply-chain signature/provenance evidence
- [ ] R3 Release Gate: Plugin GA + AI autonomous baseline + audit + IDE suite fully validated

### R3 Release Gate

| Release | Scope Snapshot | Status | Gate Criteria |
|---|---|---|---|
| R3 | Plugin GA + AI autonomous baseline + audit + IDE suite | 🔵 In Progress | Autonomous governance + audit evidence + plugin cert + Ops/Resilience cluster readiness summary (`tests/kpi/results/gates/release-ops-resilience-readiness.json`) + WS3 performance evidence (`tests/kpi/results/gates/ws3-release-readiness.json`) + WS7 release summary (`tests/kpi/results/gates/ws7-release-readiness.json`) + WS8 release summary (`tests/kpi/results/gates/ws8-release-readiness.json`) + WS8A release summary (`tests/kpi/results/gates/ws8a-release-readiness.json`) + release R3 plugin gate (`tests/kpi/results/gates/release-r3-plugin-readiness.json`) + release R3 autonomous gate (`tests/kpi/results/gates/release-r3-autonomous-readiness.json`) + release R3 agent authoring gate (`tests/kpi/results/gates/release-r3-agent-authoring-readiness.json`) + release R3 UDF runtime gate (`tests/kpi/results/gates/release-r3-udf-runtime-readiness.json`) |

---

## Sprint 11 — P2 Hardening + Ecosystem Polish + Release R4 Gate

**Goal:** P2 architecture hardening, SaaS maturity, ecosystem/multi-cloud hardening, close R4 release gate.
**Dependencies:** Sprint 10 (P1 hardening complete), all prior sprints
**Status:** ⬜ Not Started

### Architecture Hardening Backlog — P2 (Release Target: R4)

| ID | Hardening Item | Owner | Priority | Status | Completion | Blocked By | Next Evidence Milestone |
|---|---|---|---|---|---|---|---|
| H-09 | IDE extension parity/safety hardening | DX Team | P2 | ⬜ Not Started | 0% | SDK + IDE adapters pending | Cross-IDE parity test matrix draft |
| H-10 | Long-run maintainability hardening | Chief Architect + Release Eng | P2 | 🔵 In Progress | 10% | Governance process artifacts pending | ARB cadence + deprecation policy draft |

### Requirements Covered (Not Started — pending in this sprint)
- REQ-08 (Local + cloud SaaS operation) — ⬜ Not Started: Full local/cloud SaaS smoke tests
- REQ-10 (Trillion-row scale retrieval) — ⬜ Not Started: Scale benchmark report
- REQ-11 (Indexes + constraints) — 🔵 In Progress: B-tree index engine (IndexManager, BTreeIndex with create/drop/lookup/range_scan + unique enforcement) and constraint validator (ConstraintManager with PK/Unique/NotNull) in voltnuerongrid-store crate + runtime endpoints + smoke/gate evidence
- REQ-21 (Any-number-user concurrency) — ⬜ Not Started: Concurrency stress tests

### Sprint 11 Deliverables
- [ ] H-09: Cross-IDE parity + permission tests
- [ ] H-10: ARB sign-off + deprecation registry
- [ ] R4 Release Gate: SaaS maturity + medium SPOF closure + ecosystem/multi-cloud hardening

### R4 Release Gate

| Release | Scope Snapshot | Status | Gate Criteria |
|---|---|---|---|
| R4 | SaaS maturity + medium SPOF closure + ecosystem/multi-cloud hardening | ⬜ Not Started | RTO/RPO game-day success + global ops sign-off |

---

## Full Release Tracker

| Release | Scope Snapshot | Sprint Target | Status | Gate Criteria |
|---|---|---|---|---|
| R1 | Single-node HTAP baseline + SQL/ingest/RBAC/basic drivers | Sprint 5 | 🔵 In Progress | PR-002..PR-005 complete + KPI smoke baseline + WS1 UDF closure + R1 SQL/UDF gate |
| R2 | Distributed HTAP baseline + HA + connectors + anti-SPOF High closure | Sprint 8 | 🔵 In Progress | High SPOF closure + failover/RPO evidence + Ops/Resilience cluster readiness + WS6/R2 failover gates |
| R3 | Plugin GA + AI autonomous baseline + audit + IDE suite | Sprint 10 | 🔵 In Progress | Autonomous governance + audit evidence + plugin cert + all R3 sub-gates |
| R4 | SaaS maturity + medium SPOF closure + ecosystem/multi-cloud hardening | Sprint 11 | ⬜ Not Started | RTO/RPO game-day success + global ops sign-off |

---

## Owner Assignment Matrix

| Scope | DRI Team | Supporting Teams | Sprint | Current State |
|---|---|---|---|---|
| PR-007 closeout and KPI gate | Platform/SRE + QA | Runtime Team, Security | Sprint 0 | 🔵 In Progress |
| WS0 governance and CI | Platform + Program Governance | SRE | Sprint 1 | 🔵 In Progress |
| WS1 SQL core | SQL Engine Team | Query/Runtime Team | Sprint 1 | 🔵 In Progress |
| WS2/WS2A storage + HTAP row path | Storage Team | Distributed Systems Team | Sprint 1–2 | 🔵 In Progress |
| WS1A legacy aggregation parity | Compute + Migration Team | SQL Engine Team | Sprint 2 | 🔵 In Progress |
| WS3 query routing and execution | Query/Runtime Team | Storage Team | Sprint 2 | 🔵 In Progress |
| WS4/WS4A ingest + streaming/eventing | Ingestion Team | Eventing Team | Sprint 3–4 | 🔵 In Progress |
| WS22 pessimistic locking | SQL Engine Team | Query/Runtime Team | Sprint 3 | 🔵 In Progress |
| WS5 security and crypto | Security Team | Platform Team | Sprint 4 | 🟡 Ready for Validation |
| WS6 distributed HA/FT | Distributed Systems Team | SRE Team | Sprint 5 | 🟡 Ready for Validation |
| WS7 plugin framework + connector pack | Extensibility Team | Ingestion Team, Security Team | Sprint 6 | 🟡 Ready for Validation |
| WS8 autonomous control plane | AI Platform Team | Security Team, Runtime Team | Sprint 6 | 🟡 Ready for Validation |
| WS8A audit + AI agent authoring companion | Audit/Compliance Team | AI Platform Team, Extensibility Team, Runtime Team | Sprint 6 | 🟡 Ready for Validation |
| WS9 Studio UI API contract | UX Team | Runtime Team, Platform Team | Sprint 7 | 🟡 Ready for Validation |
| WS9A IDE extension contract | DX Team | Integrations Team, UX Team | Sprint 7 | 🟡 Ready for Validation |
| WS10 driver and pooling contract | Integrations Team | Platform Team, Security Team | Sprint 7 | 🟡 Ready for Validation |
| WS11 internationalization and UTF-8 | Platform + UX Team | Runtime Team | Sprint 7 | 🟡 Ready for Validation |
| Release DX/API contract cluster gate (WS5/WS9/WS9A/WS10) | Platform + Program Governance | Security, UX, DX, Integrations | Sprint 7 | 🔵 In Progress |
| WS12 reliability and DR automation | SRE Team | Distributed Systems Team | Sprint 8 | 🟡 Ready for Validation |
| WS13 multi-cloud deployment profiles | Platform/SRE | SRE Team, Security Team | Sprint 8 | 🟡 Ready for Validation |
| WS14 config contracts + tuning playbooks | Platform + SRE + Security | Integrations Team, Security Team | Sprint 8 | 🟡 Ready for Validation |
| Release Ops/Resilience cluster gate (WS12/WS13/WS14) | Platform + SRE | Distributed Systems Team, Security Team | Sprint 8 | 🔵 In Progress |
| WS15 competitive feature adoption track | Architecture + Query Team | AI Platform Team, Integrations Team | Sprint 9 | 🟡 Ready for Validation |
| H-01..H-04 P0 hardening | Various (see hardening backlog) | Cross-functional | Sprint 9 | 🔵 In Progress |
| H-05..H-08 P1 hardening | Various (see hardening backlog) | Cross-functional | Sprint 10 | ⬜ Not Started |
| H-09..H-10 P2 hardening | Various (see hardening backlog) | Cross-functional | Sprint 11 | ⬜ Not Started / 🔵 In Progress (H-10 at 10%) |

---

## PMO Action Queue (Week 2 Readiness)

- Finalize owner assignment for PR-007 and first implementation workstreams.
- R1 scope freeze approved; release gate checklist automation published (`tests/kpi/scripts/check-r1-gate.ps1`).
- Start scaffold implementation branch for workspace + deploy manifests.
- Populate real AWS/Azure/GCP endpoint + token environment variables and execute PR-007 true remote smoke packs to close final gate.
- Hardening review template for H-01..H-04 published at `reference/hardening-review-h01-h04-template.md`; schedule and assign attendees.

---

## Weekly Update Template (Copy/Paste)

```text
[Hardening/Workstream Update]
Week Ending: YYYY-MM-DD
Item ID: H-0X or WSX
Item Name:
Owner:
Priority:
Release Target:
Sprint: X

Status: <not_started|in_progress|blocked|at_risk|ready_for_validation|done>
Completion: <0-100%>
Risk Trend: <improving|stable|worsening>

This Week Completed:
- 
- 

Evidence Produced:
- 
- 

Blocked By:
- 

Decisions Needed:
- 

Next Evidence Milestone:
- Date:
- Expected Artifact:

Release Gate Impact: <none|medium|high>
```

---

## Session 73 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 246→249, exec 120→122, service 521→525 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_nulls_ordering: bool` field + detection | `voltnuerongrid-sql` | Detects `NULLS FIRST/LAST` in ORDER BY clauses (`S3-WS1-49`) | 3 (`nulls_ordering_tests` module) |
| `NullsOrdering { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.10 cost overhead | 2 |
| `GET /api/v1/store/wal/record/total` | `voltnuerongridd` | Return total WAL record count (operator-auth) | 2 |
| `GET /api/v1/store/rows/key/duplicates/count` | `voltnuerongridd` | Return duplicate-key count across row snapshot (operator-auth) | 2 |

---

## Session 74 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 249→252, exec 122→124, service 525→529 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_order_by_collation: bool` field + detection | `voltnuerongrid-sql` | Detects `ORDER BY ... COLLATE ...` usage (`S3-WS1-50`) | 3 (`order_by_collation_tests` module) |
| `CollationOrdering { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.10 cost overhead | 2 |
| `GET /api/v1/store/wal/value/duplicates/count` | `voltnuerongridd` | Return duplicate-value count across WAL entries (operator-auth) | 2 |
| `GET /api/v1/store/rows/value/duplicates/count` | `voltnuerongridd` | Return duplicate-value count across row snapshot (operator-auth) | 2 |

---

## Session 75 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 252→255, exec 124→126, service 529→533 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_order_by_positional: bool` field + detection | `voltnuerongrid-sql` | Detects positional ORDER BY usage like `ORDER BY 1` (`S3-WS1-51`) | 3 (`order_by_positional_tests` module) |
| `PositionalOrdering { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.08 cost overhead | 2 |
| `GET /api/v1/store/wal/value/distinct/count` | `voltnuerongridd` | Return distinct-value count across WAL entries (operator-auth) | 2 |
| `GET /api/v1/store/rows/value/distinct/count` | `voltnuerongridd` | Return distinct-value count across row snapshot (operator-auth) | 2 |

---

## Session 76 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 255→258, exec 126→128, service 533→537 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_order_by_expression: bool` field + detection | `voltnuerongrid-sql` | Detects computed ORDER BY expressions (`S3-WS1-52`) | 3 (`order_by_expression_tests` module) |
| `ExpressionOrdering { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.12 cost overhead | 2 |
| `GET /api/v1/store/wal/value/unique/count` | `voltnuerongridd` | Return unique-value count across WAL entries (operator-auth) | 2 |
| `GET /api/v1/store/rows/value/unique/count` | `voltnuerongridd` | Return unique-value count across row snapshot (operator-auth) | 2 |

---

## Session 77 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 258→261, exec 128→130, service 537→541 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_order_by_function_expression: bool` field + detection | `voltnuerongrid-sql` | Detects function-based ORDER BY expressions (`S3-WS1-53`) | 3 (`order_by_function_expression_tests` module) |
| `FunctionOrdering { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.18 cost overhead | 2 |
| `GET /api/v1/store/wal/value/trimmed/count` | `voltnuerongridd` | Count WAL values requiring trim (operator-auth) | 2 |
| `GET /api/v1/store/rows/value/trimmed/count` | `voltnuerongridd` | Count row values requiring trim (operator-auth) | 2 |

---

## Session 79 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 264→267, exec 132→134, service 545→549 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_order_by_desc_direction: bool` field + detection | `voltnuerongrid-sql` | Detects ORDER BY DESC direction (`S3-WS1-55`) | 3 (`order_by_desc_direction_tests` module) |
| `DirectionOrdering { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.05 cost overhead | 2 |
| `GET /api/v1/store/wal/order_by/desc_direction/count` | `voltnuerongridd` | Count DESC direction usage in WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/order_by/desc_direction/count` | `voltnuerongridd` | Count DESC direction usage in rows (operator-auth) | 2 |

---

## Session 80 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 267→270, exec 134→136, service 549→553 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_order_by_random: bool` field + detection | `voltnuerongrid-sql` | Detects ORDER BY random-function usage (`S3-WS1-56`) | 3 (`order_by_random_tests` module) |
| `RandomOrdering { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.20 cost overhead | 2 |
| `GET /api/v1/store/wal/order_by/random/count` | `voltnuerongridd` | Count random ORDER BY usage in WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/order_by/random/count` | `voltnuerongridd` | Count random ORDER BY usage in rows (operator-auth) | 2 |

---

## Session 81 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 270→273, exec 136→138, service 553→557 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_order_by_random_seeded: bool` field + detection | `voltnuerongrid-sql` | Detects seeded ORDER BY random-function usage (`S3-WS1-57`) | 3 (`order_by_random_seeded_tests` module) |
| `SeededRandomOrdering { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.22 cost overhead | 2 |
| `GET /api/v1/store/wal/order_by/random_seeded/count` | `voltnuerongridd` | Count seeded random ORDER BY usage in WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/order_by/random_seeded/count` | `voltnuerongridd` | Count seeded random ORDER BY usage in rows (operator-auth) | 2 |

---

## Session 82 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 273→276, exec 138→140, service 557→561 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_order_by_asc_direction: bool` field + detection | `voltnuerongrid-sql` | Detects explicit ORDER BY ASC direction (`S3-WS1-58`) | 3 (`order_by_asc_direction_tests` module) |
| `AscDirectionOrdering { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.03 cost overhead | 2 |
| `GET /api/v1/store/wal/order_by/asc_direction/count` | `voltnuerongridd` | Count ASC direction usage in WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/order_by/asc_direction/count` | `voltnuerongridd` | Count ASC direction usage in rows (operator-auth) | 2 |

---

## Session 83 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 276→279, exec 140→142, service 561→565 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_order_by_rand_alias: bool` field + detection | `voltnuerongrid-sql` | Detects explicit ORDER BY `RAND()` alias usage (`S3-WS1-59`) | 3 (`order_by_rand_alias_tests` module) |
| `RandAliasOrdering { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.19 cost overhead | 2 |
| `GET /api/v1/store/wal/order_by/rand_alias/count` | `voltnuerongridd` | Count `RAND()` alias ORDER BY usage in WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/order_by/rand_alias/count` | `voltnuerongridd` | Count `RAND()` alias ORDER BY usage in rows (operator-auth) | 2 |

---

## Session 84 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 279→282, exec 142→144, service 565→569 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_order_by_multi_column: bool` field + detection | `voltnuerongrid-sql` | Detects ORDER BY multi-key usage (`S3-WS1-60`) | 3 (`order_by_multi_column_tests` module) |
| `MultiColumnOrdering { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.02 cost overhead | 2 |
| `GET /api/v1/store/wal/order_by/multi_column/count` | `voltnuerongridd` | Count multi-column ORDER BY usage in WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/order_by/multi_column/count` | `voltnuerongridd` | Count multi-column ORDER BY usage in rows (operator-auth) | 2 |

---

## Session 85 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 282→285, exec 144→146, service 569→573 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_limit_offset_pagination: bool` field + detection | `voltnuerongrid-sql` | Detects combined LIMIT+OFFSET pagination (`S3-WS1-61`) | 3 (`limit_offset_pagination_tests` module) |
| `LimitOffsetPagination { input }` plan node | `voltnuerongrid-exec` | OLTP node; +0.03 cost overhead | 2 |
| `GET /api/v1/store/wal/pagination/limit_offset/count` | `voltnuerongridd` | Count LIMIT+OFFSET pagination usage in WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/pagination/limit_offset/count` | `voltnuerongridd` | Count LIMIT+OFFSET pagination usage in rows (operator-auth) | 2 |

---

## Session 86 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 285→288, exec 146→148, service 573→577 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_offset_only_pagination: bool` field + detection | `voltnuerongrid-sql` | Detects OFFSET-only pagination usage (`S3-WS1-62`) | 3 (`offset_only_pagination_tests` module) |
| `OffsetOnlyPagination { input }` plan node | `voltnuerongrid-exec` | OLTP node; +0.04 cost overhead | 2 |
| `GET /api/v1/store/wal/pagination/offset_only/count` | `voltnuerongridd` | Count OFFSET-only pagination usage in WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/pagination/offset_only/count` | `voltnuerongridd` | Count OFFSET-only pagination usage in rows (operator-auth) | 2 |

---

## Session 87 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 288→291, exec 148→150, service 577→581 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_having_without_group_by: bool` field + detection | `voltnuerongrid-sql` | Detects HAVING without GROUP BY usage (`S3-WS1-63`) | 3 (`having_without_group_by_tests` module) |
| `HavingWithoutGroupBy { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.06 cost overhead | 2 |
| `GET /api/v1/store/wal/having_without_group_by/count` | `voltnuerongridd` | Count HAVING-without-GROUP-BY usage in WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/having_without_group_by/count` | `voltnuerongridd` | Count HAVING-without-GROUP-BY usage in rows (operator-auth) | 2 |

---

## Session 88 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 291→294, exec 150→152, service 581→585 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_having_with_group_by: bool` field + detection | `voltnuerongrid-sql` | Detects HAVING with GROUP BY usage (`S3-WS1-64`) | 3 (`having_with_group_by_tests` module) |
| `HavingWithGroupBy { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.08 cost overhead | 2 |
| `GET /api/v1/store/wal/having_with_group_by/count` | `voltnuerongridd` | Count HAVING-with-GROUP-BY usage in WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/having_with_group_by/count` | `voltnuerongridd` | Count HAVING-with-GROUP-BY usage in rows (operator-auth) | 2 |

---

## Session 89 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 294→297, exec 152→154, service 585→589 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_group_by_rollup: bool` field + detection | `voltnuerongrid-sql` | Detects GROUP BY ROLLUP(...) usage (`S3-WS1-65`) | 3 (`group_by_rollup_tests` module) |
| `GroupByRollup { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.12 cost overhead | 2 |
| `GET /api/v1/store/wal/group_by/rollup/count` | `voltnuerongridd` | Count GROUP-BY-ROLLUP usage in WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/group_by/rollup/count` | `voltnuerongridd` | Count GROUP-BY-ROLLUP usage in rows (operator-auth) | 2 |

---

## Session 90 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 297→300, exec 154→156, service 589→593 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_group_by_cube: bool` field + detection | `voltnuerongrid-sql` | Detects GROUP BY CUBE(...) usage (`S3-WS1-66`) | 3 (`group_by_cube_tests` module) |
| `GroupByCube { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.15 cost overhead | 2 |
| `GET /api/v1/store/wal/group_by/cube/count` | `voltnuerongridd` | Count GROUP-BY-CUBE usage in WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/group_by/cube/count` | `voltnuerongridd` | Count GROUP-BY-CUBE usage in rows (operator-auth) | 2 |

---

## Session 78 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 261→264, exec 130→132, service 541→545 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_order_by_case_expression: bool` field + detection | `voltnuerongrid-sql` | Detects ORDER BY CASE expressions (`S3-WS1-54`) | 3 (`order_by_case_expression_tests` module) |
| `CaseOrdering { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.14 cost overhead | 2 |
| `GET /api/v1/store/wal/value/case_variant/count` | `voltnuerongridd` | Count case-variant WAL values (operator-auth) | 2 |
| `GET /api/v1/store/rows/value/case_variant/count` | `voltnuerongridd` | Count case-variant row values (operator-auth) | 2 |

---

## Session 72 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 243→246, exec 118→120, service 517→521 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_window_order: bool` field + detection | `voltnuerongrid-sql` | Detects window `ORDER BY` clauses without `PARTITION BY` (`S3-WS1-48`) | 3 (`window_order_tests` module) |
| `WindowOrder { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.20 cost overhead | 2 |
| `GET /api/v1/store/wal/non_deleted/newest` | `voltnuerongridd` | Return newest non-deleted WAL sequence (operator-auth) | 2 |
| `GET /api/v1/store/rows/value/blank/count` | `voltnuerongridd` | Count blank values across row snapshot (operator-auth) | 2 |

---

## Session 71 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 240→243, exec 116→118, service 513→517 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_window_partition: bool` field + detection | `voltnuerongrid-sql` | Detects window `PARTITION BY` clauses (`S3-WS1-47`) | 3 (`window_partition_tests` module) |
| `WindowPartition { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.25 cost overhead | 2 |
| `GET /api/v1/store/wal/non_deleted/oldest` | `voltnuerongridd` | Return oldest non-deleted WAL sequence (operator-auth) | 2 |
| `GET /api/v1/store/rows/key/non_blank/count` | `voltnuerongridd` | Count non-blank keys across row snapshot (operator-auth) | 2 |

---

## Session 70 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 237→240, exec 114→116, service 509→513 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_named_window: bool` field + detection | `voltnuerongrid-sql` | Detects named window clause `WINDOW ... AS (...)` (`S3-WS1-46`) | 3 (`named_window_tests` module) |
| `NamedWindow { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.30 cost overhead | 2 |
| `GET /api/v1/store/wal/non_deleted/latest` | `voltnuerongridd` | Return latest non-deleted WAL sequence (operator-auth) | 2 |
| `GET /api/v1/store/rows/value/non_blank/count` | `voltnuerongridd` | Count non-blank values across row snapshot (operator-auth) | 2 |

---

## Session 69 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 234→237, exec 112→114, service 505→509 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_window_frame: bool` field + detection | `voltnuerongrid-sql` | Detects explicit window frame clauses (`ROWS/RANGE ...`) (`S3-WS1-45`) | 3 (`window_frame_tests` module) |
| `WindowFrame { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.55 cost overhead | 2 |
| `GET /api/v1/store/wal/non_deleted/count` | `voltnuerongridd` | Count non-deleted WAL records (operator-auth) | 2 |
| `GET /api/v1/store/rows/key/non_empty/count` | `voltnuerongridd` | Count non-empty keys across row snapshot (operator-auth) | 2 |

---

## Session 68 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 231→234, exec 110→112, service 501→505 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_filter: bool` field + detection | `voltnuerongrid-sql` | Detects aggregate `FILTER (WHERE ...)` clause (`S3-WS1-44`) | 3 (`filter_agg_tests` module) |
| `AggregateFilter { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.60 cost overhead | 2 |
| `GET /api/v1/store/wal/non_deleted/span` | `voltnuerongridd` | Oldest/newest non-deleted WAL sequence and span (operator-auth) | 2 |
| `GET /api/v1/store/rows/value/non_empty/count` | `voltnuerongridd` | Count non-empty values across row snapshot (operator-auth) | 2 |

---

## Session 67 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 228→231, exec 108→110, service 497→501 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_not_exists: bool` field + detection | `voltnuerongrid-sql` | Detects `NOT EXISTS (...)` subquery predicate (`S3-WS1-43`) | 3 (`not_exists_tests` module) |
| `NotExists { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.8x row reduction, +2.00 cost overhead | 2 |
| `GET /api/v1/store/wal/mutation/count/non_deleted` | `voltnuerongridd` | Count non-deleted WAL mutations (operator-auth) | 2 |
| `GET /api/v1/store/rows/value/empty/count` | `voltnuerongridd` | Count empty string values across row snapshot (operator-auth) | 2 |

---

## Session 66 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 225→228, exec 106→108, service 493→497 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_recursive_cte: bool` field + detection | `voltnuerongrid-sql` | Detects `WITH RECURSIVE ... AS (...)` CTE clause (`S3-WS1-42`) | 3 (`recursive_cte_tests` module) |
| `RecursiveCte { input }` plan node | `voltnuerongrid-exec` | Hybrid node; +3.00 cost overhead | 2 |
| `GET /api/v1/store/wal/mutation/span` | `voltnuerongridd` | Oldest/newest non-delete WAL sequence and mutation span (operator-auth) | 2 |
| `GET /api/v1/store/rows/value/non_null/count` | `voltnuerongridd` | Count non-null values across row snapshot (operator-auth) | 2 |

---

## Session 65 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 222→225, exec 104→106, service 489→493 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_with_cte: bool` field + detection | `voltnuerongrid-sql` | Detects `WITH ... AS (...)` CTE clause (`S3-WS1-41`) | 3 (`with_cte_tests` module) |
| `WithCte { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.15 cost overhead | 2 |
| `GET /api/v1/store/wal/record/deleted` | `voltnuerongridd` | Count deleted/tombstone WAL records (operator-auth) | 2 |
| `GET /api/v1/store/rows/key/max` | `voltnuerongridd` | Maximum key in row snapshot (operator-auth) | 2 |

---

## Session 64 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 219→222, exec 102→104, service 485→489 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_qualify: bool` field + detection | `voltnuerongrid-sql` | Detects `QUALIFY` clause (`S3-WS1-40`) | 3 (`qualify_tests` module) |
| `Qualify { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.6x row reduction, +0.20 cost | 2 |
| `GET /api/v1/store/wal/record/mutations` | `voltnuerongridd` | WAL mutation record count (operator-auth) | 2 |
| `GET /api/v1/store/rows/field/cardinality` | `voltnuerongridd` | Distinct field-key cardinality across row snapshot (operator-auth) | 2 |

---

## Session 63 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 216→219, exec 100→102, service 481→485 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_intersect: bool` field + detection | `voltnuerongrid-sql` | Detects `INTERSECT` set operation (`S3-WS1-39`) | 3 (`intersect_tests` module) |
| `Intersect { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.7x row reduction, +0.50 cost | 2 |
| `GET /api/v1/store/wal/record/active` | `voltnuerongridd` | Count non-delete WAL records (operator-auth) | 2 |
| `GET /api/v1/store/rows/key/min` | `voltnuerongridd` | Minimum key in row snapshot (operator-auth) | 2 |

---

## Session 62 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 213→216, exec 98→100, service 477→481 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_except: bool` field + detection | `voltnuerongrid-sql` | Detects `EXCEPT` set operation (`S3-WS1-38`) | 3 (`except_tests` module) |
| `Except { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.8x row reduction, +0.45 cost | 2 |
| `GET /api/v1/store/wal/seq/span` | `voltnuerongridd` | Oldest/newest WAL sequence and span (operator-auth) | 2 |
| `GET /api/v1/store/rows/key/empty/count` | `voltnuerongridd` | Count empty-string keys in row snapshot (operator-auth) | 2 |

---

## Session 61 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 210→213, exec 96→98, service 473→477 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_using_join: bool` field + detection | `voltnuerongrid-sql` | Detects `JOIN ... USING (...)` clause (`S3-WS1-37`) | 3 (`using_join_tests` module) |
| `UsingJoin { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.25 cost overhead | 2 |
| `GET /api/v1/store/wal/entry/oldest` | `voltnuerongridd` | Oldest WAL entry sequence + has_entry (operator-auth) | 2 |
| `GET /api/v1/store/rows/field/types` | `voltnuerongridd` | Total field count + unique type count estimate (operator-auth) | 2 |

---

## Session 60 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 207→210, exec 94→96, service 469→473 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_natural_join: bool` field + detection | `voltnuerongrid-sql` | Detects `NATURAL JOIN` clause (`S3-WS1-36`) | 3 (`natural_join_tests` module) |
| `NaturalJoin { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.35 cost overhead | 2 |
| `GET /api/v1/store/wal/validate` | `voltnuerongridd` | Validate WAL sequence monotonicity + record count (operator-auth) | 2 |
| `GET /api/v1/store/rows/checksum` | `voltnuerongridd` | Deterministic checksum over sorted row keys + row count (operator-auth) | 2 |

---

## Session 59 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline → New:** sql 204→207, exec 92→94, service 465→469 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_grouping_sets: bool` field + detection | `voltnuerongrid-sql` | Detects `GROUPING SETS` in grouped SELECT (`S3-WS1-35`) | 3 (`grouping_sets_tests` module) |
| `GroupingSets { input }` plan node | `voltnuerongrid-exec` | OLAP node; 1.5x row amplification, +0.70 cost | 2 |
| `GET /api/v1/store/wal/delete/count` | `voltnuerongridd` | Count WAL tombstone delete records (operator-auth) | 2 |
| `GET /api/v1/store/rows/key/median` | `voltnuerongridd` | Median key from sorted MVCC snapshot keys (operator-auth) | 2 |

---

## Session 58 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 201→204, exec 90→92, service 461→465 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_full_text_search: bool` field + detection | `voltnuerongrid-sql` | Detects `MATCH(` / `@@` full-text patterns (`S3-WS1-34`) | 3 (`full_text_search_tests` module) |
| `FullTextSearch { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.3× row selectivity, +0.60 cost | 2 |
| `GET /api/v1/store/wal/size/bytes` | `voltnuerongridd` | WAL size estimate in bytes (operator-auth) | 2 |
| `GET /api/v1/store/rows/distinct/count` | `voltnuerongridd` | Distinct row count from MVCC store (operator-auth) | 2 |

---

## Session 57 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 198→201, exec 88→90, service 457→461 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_cross_join: bool` field + detection | `voltnuerongrid-sql` | Detects `CROSS JOIN` keyword (`S3-WS1-33`) | 3 (`cross_join_tests` module) |
| `CrossJoin { input }` plan node | `voltnuerongrid-exec` | OLAP node; row estimate squared, +0.30 cost | 2 |
| `GET /api/v1/store/wal/entry/count` | `voltnuerongridd` | Total WAL entry count (operator-auth) | 2 |
| `GET /api/v1/store/rows/version/latest` | `voltnuerongridd` | Latest WAL sequence as row version (operator-auth) | 2 |

---

## Session 56 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 195→198, exec 86→88, service 453→457 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_values: bool` field + detection | `voltnuerongrid-sql` | Detects `VALUES (` inline subquery (`S3-WS1-32`) | 3 (`values_tests` module) |
| `Values { input }` plan node | `voltnuerongrid-exec` | OLTP node; pass-through rows, +0.02 cost | 2 |
| `GET /api/v1/store/wal/max/seq` | `voltnuerongridd` | Maximum WAL sequence number (operator-auth) | 2 |
| `GET /api/v1/store/rows/snapshot/size` | `voltnuerongridd` | Total row count as snapshot size (operator-auth) | 2 |

---

## Session 55 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 192→195, exec 84→86, service 449→453 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_fetch: bool` field + detection | `voltnuerongrid-sql` | Detects `FETCH NEXT`/`FETCH FIRST` keywords (`S3-WS1-31`) | 3 (`fetch_tests` module) |
| `Fetch { input }` plan node | `voltnuerongrid-exec` | OLTP node; pass-through rows, +0.05 cost | 2 |
| `GET /api/v1/store/wal/min/seq` | `voltnuerongridd` | Minimum WAL sequence number (operator-auth) | 2 |
| `GET /api/v1/store/rows/count/all` | `voltnuerongridd` | Total row count across all MVCC rows (operator-auth) | 2 |

---

## Session 54 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 189→192, exec 82→84, service 445→449 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_pivot: bool` field + detection | `voltnuerongrid-sql` | Detects `PIVOT`/`UNPIVOT` keyword (`S3-WS1-30`) | 3 (`pivot_tests` module) |
| `Pivot { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.9× row selectivity, +0.8 cost | 2 |
| `GET /api/v1/store/rows/key/shortest` | `voltnuerongridd` | Shortest key in MVCC snapshot (operator-auth) | 2 |
| S54 `wal_age` tests | `voltnuerongridd` | Re-validate existing WAL age endpoint (S22) | 2 |

---

## Session 53 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 186→189, exec 80→82, service 441→445 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_lateral: bool` field + detection | `voltnuerongrid-sql` | Detects `LATERAL` keyword in query (`S3-WS1-29`) | 3 (`lateral_tests` module) |
| `Lateral { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.7× row selectivity, +0.7 cost | 2 |
| `GET /api/v1/store/wal/write/count` | `voltnuerongridd` | Non-delete WAL record count (operator-auth) | 2 |
| `GET /api/v1/store/rows/key/longest` | `voltnuerongridd` | Longest key in MVCC snapshot (operator-auth) | 2 |

---

## Session 52 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline to New:** sql 183>186, exec 78>80, service 437>441 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|-|
| `has_window_agg: bool` field + detection | `voltnuerongrid-sql` | Detects `COUNT()/SUM()/AVG()/ROW_NUMBER OVER` (S3-WS1-28) | 3 (`window_agg_tests` module) |
| `WindowAgg { input }` plan node | `voltnuerongrid-exec` | OLAP node; `has_aggregation()=true`, +1.5 cost | 2 |
| `GET /api/v1/store/rows/field/count` | `voltnuerongridd` | Total field count + row count across all MVCC rows (operator-auth) | 2 |
| `GET /api/v1/store/wal/entry/latest` | `voltnuerongridd` | Latest WAL entry sequence + has_entry flag (operator-auth) | 2 |

---

## Session 51 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline to New:** sql 180>183, exec 76>78, service 433>437 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|-|
| `has_json_op: bool` field + detection | `voltnuerongrid-sql` | Detects `->`, `JSON_EXTRACT`, `JSON_VALUE` (S3-WS1-27) | 3 (`json_op_tests` module) |
| `JsonOp { input }` plan node | `voltnuerongrid-exec` | OLAP node; +0.4 cost | 2 |
| `GET /api/v1/store/rows/payload/size` | `voltnuerongridd` | Total field count + row count across all MVCC rows (operator-auth) | 2 |
| `GET /api/v1/store/wal/flush/count` | `voltnuerongridd` | Total WAL write count (operator-auth) | 2 |

---

## Session 50 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline to New:** sql 177>180, exec 74>76, service 429>433 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|-|
| `has_regexp: bool` field + detection | `voltnuerongrid-sql` | Detects `REGEXP`, `RLIKE`, `SIMILAR TO` (S3-WS1-26) | 3 (`regexp_tests` module) |
| `Regexp { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.7x row selectivity, +0.5 cost | 2 |
| `GET /api/v1/store/rows/count/range` | `voltnuerongridd` | Row count with optional `?prefix=` key filter (operator-auth) | 2 |
| `GET /api/v1/store/wal/checkpoint/age` | `voltnuerongridd` | Checkpoint count + oldest/newest WAL sequence (operator-auth) | 2 |

---

## Session 49 Implementation Log

**Date:** 2026-04-08 (Sprint 9 continuation)
**Test Baseline to New:** sql 174>177, exec 72>74, service 425>429 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|-|
| `has_is_null: bool` field + detection | `voltnuerongrid-sql` | Detects `IS NULL` / `IS NOT NULL` (S3-WS1-25) | 3 (`is_null_tests` module) |
| `IsNull { input }` plan node | `voltnuerongrid-exec` | OLTP node; pass-through rows, +0.1 cost | 2 |
| `GET /api/v1/store/rows/value/search` | `voltnuerongridd` | Returns keys of rows whose payload contains `?value=` substring (operator-auth) | 2 |
| `GET /api/v1/store/wal/record/count` | `voltnuerongridd` | Total WAL record count (operator-auth) | 2 |

---

## Session 48 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline to New:** sql 171>174, exec 70>72, service 421>425 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|-|
| `has_in_subquery: bool` field + detection | `voltnuerongrid-sql` | Detects `IN (SELECT` / `IN(SELECT` (S3-WS1-24); updated 2 existing planner tests to scalar subquery | 3 (`in_subquery_tests` module) |
| `InSubquery { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.6x row selectivity, +0.8 cost | 2 |
| `GET /api/v1/store/rows/count/distinct` | `voltnuerongridd` | Distinct value count across all MVCC rows (operator-auth) | 2 |
| `GET /api/v1/store/rows/key/exists` | `voltnuerongridd` | Key existence check (`?key=` param) (operator-auth) | 2 |

---

## Session 47 Fix Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Fix:** Added missing INTERVAL detection block in ast.rs (S3-WS1-23). Fixed 2 failing SQL tests (169>171) and 2 failing exec tests (68>70).

---

## Session 46 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 165→168, exec 66→68, service 413→417 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|-|
| `has_trim: bool` field + detection | `voltnuerongrid-sql` | Detects `TRIM(`, `LTRIM(`, `RTRIM(` via `up_trim` buffer (`S3-WS1-22`) | 3 (`trim_tests` module) |
| `Trim { input }` plan node | `voltnuerongrid-exec` | OLTP node; pass-through rows, +0.05 cost | 2 |
| `GET /api/v1/store/wal/age` | `voltnuerongridd` | oldest_sequence, newest_sequence, sequence_span from live WAL (operator-auth) | 2 |
| `GET /api/v1/store/rows/first/key` | `voltnuerongridd` | First alphabetically-sorted key in MVCC row store (operator-auth) | 2 |

---

## Session 45 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 162→165, exec 64→66, service 409→413 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|-|
| `has_not_in: bool` field + detection | `voltnuerongrid-sql` | Detects `NOT IN (` / `NOT IN(` (`S3-WS1-21`); refined `has_not` exclusion | 3 (`not_in_tests` module) |
| `NotIn { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.7× row selectivity, +0.4 cost | 2 |
| `GET /api/v1/store/wal/unique/keys` | `voltnuerongridd` | Unique WAL key count (operator-auth) | 2 |
| `GET /api/v1/store/rows/xid/history` | `voltnuerongridd` | current_xid + next_xid + total_transactions (operator-auth) | 2 |

---

## Session 44 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 159→162, exec 62→64, service 405→409 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|-|
| `has_any_all: bool` field + detection | `voltnuerongrid-sql` | Detects `ANY(`, `ALL(` quantifiers (`S3-WS1-20`) | 3 (`any_all_tests` module) |
| `AnyAll { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.8× row selectivity, +0.6 cost | 2 |
| `GET /api/v1/store/wal/delta` | `voltnuerongridd` | WAL insert/delete delta counts (operator-auth) | 2 |
| `GET /api/v1/store/rows/tombstone/count` | `voltnuerongridd` | Tombstone (`__deleted__`) record count via WAL (operator-auth) | 2 |

---

## Session 43 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 156→159, exec 60→62, service 401→405 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_exists: bool` field + detection | `voltnuerongrid-sql` | Detects `EXISTS(` / `EXISTS (` subquery predicate (`S3-WS1-19`) | 3 (`exists_tests` module) |
| `Exists { input }` plan node | `voltnuerongrid-exec` | OLAP node; 0.5× row reduction, +1.2 cost | 2 |
| `GET /api/v1/store/wal/checkpoint/latest` | `voltnuerongridd` | Latest WAL checkpoint id + record count (operator-auth) | 2 |
| `GET /api/v1/store/rows/scan/visible` | `voltnuerongridd` | Scan visible rows at current MVCC snapshot, optional `limit` (operator-auth) | 2 |

---

## Session 42 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 153→156, exec 58→60, service 397→401 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_math_fn: bool` field + detection | `voltnuerongrid-sql` | Detects `ABS(`, `ROUND(`, `CEIL(`, `FLOOR(` in query (`S3-WS1-18`) | 3 (`math_fn_tests` module) |
| `MathFn { input }` plan node | `voltnuerongrid-exec` | OLTP pass-through node; +0.09 cost | 2 |
| `GET /api/v1/store/wal/by-key` | `voltnuerongridd` | WAL records filtered by `key_prefix` (operator-auth) | 2 |
| `GET /api/v1/store/rows/keys/count` | `voltnuerongridd` | Count of distinct row keys in MVCC store (operator-auth) | 2 |

---

## Session 41 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 150→153, exec 56→58, service 393→397 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_concat: bool` field + detection | `voltnuerongrid-sql` | Detects `CONCAT(` and `||` pipe operator in query (`S3-WS1-17`) | 3 (`concat_tests` module) |
| `Concat { input }` plan node | `voltnuerongrid-exec` | OLTP pass-through node; +0.08 cost | 2 |
| `GET /api/v1/store/wal/latest` | `voltnuerongridd` | Last WAL record or `has_record=false` when empty (operator-auth) | 2 |
| `GET /api/v1/store/rows/total` | `voltnuerongridd` | Total row count across all MVCC versions incl. tombstones (operator-auth) | 2 |

---

## Session 40 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 147→150, exec 54→56, service 389→393 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_date_fn: bool` field + detection | `voltnuerongrid-sql` | Detects `NOW(`, `DATE_TRUNC(`, `EXTRACT(` in query (`S3-WS1-16`) | 3 (`date_fn_tests` module) |
| `DateFn { input }` plan node | `voltnuerongrid-exec` | OLTP pass-through node; +0.12 cost | 2 |
| `GET /api/v1/store/wal/size` | `voltnuerongridd` | WAL record count + estimated byte size (operator-auth) | 2 |
| `GET /api/v1/store/rows/visible` | `voltnuerongridd` | Visible row count at current MVCC snapshot xid (operator-auth) | 2 |

---

## Session 39 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 144→147, exec 52→54, service 385→389 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_string_fn: bool` field + detection | `voltnuerongrid-sql` | Detects `LENGTH(`, `UPPER(`, `LOWER(`, `SUBSTR(` in query (`S3-WS1-15`) | 3 (`string_fn_tests` module) |
| `StringFn { input }` plan node | `voltnuerongrid-exec` | OLTP pass-through node; +0.1 cost | 2 |
| `GET /api/v1/store/wal/range` | `voltnuerongridd` | WAL records within `[from_seq, to_seq]` range (operator-auth) | 2 |
| `GET /api/v1/store/rows/xid` | `voltnuerongridd` | Returns `current_xid` and `next_xid` from MVCC row store (operator-auth) | 2 |

---

## Session 38 Implementation Log

**Date:** 2026-04-07 (Sprint 9 continuation)
**Test Baseline → New:** sql 141→144, exec 50→52, service 381→385 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_nullif: bool` field + detection | `voltnuerongrid-sql` | Detects `NULLIF(` in query (`S3-WS1-14`) | 3 (`nullif_tests` module) |
| `Nullif { input }` plan node | `voltnuerongrid-exec` | OLTP pass-through node; +0.15 cost | 2 |
| `GET /api/v1/store/wal/head` | `voltnuerongridd` | First N WAL records with configurable limit (operator-auth) | 2 |
| `GET /api/v1/store/rows/modified` | `voltnuerongridd` | Row keys modified after `since_xid` via MVCC `was_modified_after` (operator-auth) | 2 |

---

## Session 37 Implementation Log

**Date:** 2026-04-06 (Sprint 9 continuation)
**Test Baseline → New:** sql 138→141, exec 48→50, service 377→381 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_cast: bool` field + detection | `voltnuerongrid-sql` | Detects `CAST(` and `::` type-cast operator (`S3-WS1-13`) | 3 (`cast_tests` module) |
| `Cast { input }` plan node | `voltnuerongrid-exec` | OLTP pass-through node; +0.2 cost | 2 |
| `GET /api/v1/ingest/schema/fields` | `voltnuerongridd` | Inferred field/type list for connector schema (operator-auth) | 2 |
| `GET /api/v1/store/wal/seq` | `voltnuerongridd` | WAL latest_sequence, wal_len, checkpoint_count (operator-auth) | 2 |

---

## Session 36 Implementation Log

**Date:** 2026-04-06 (Sprint 9 continuation)
**Test Baseline → New:** sql 135→138, exec 46→48, service 373→377 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_coalesce: bool` field + detection | `voltnuerongrid-sql` | Detects `COALESCE(` in query (`S3-WS1-12`) | 3 (`coalesce_tests` module) |
| `Coalesce { input }` plan node | `voltnuerongrid-exec` | OLTP-routed pass-through node; +0.3 cost | 2 |
| `GET /api/v1/connectors/health` | `voltnuerongridd` | Health status for all registered connectors (operator-auth) | 2 |
| `GET /api/v1/store/rows/page/stats` | `voltnuerongridd` | Page-level MVCC row store stats: page_count, total/visible rows (operator-auth) | 2 |

---

## Session 35 Implementation Log

**Date:** 2026-04-06 (Sprint 9 continuation)
**Test Baseline → New:** sql 132→135, exec 44→46, service 369→373 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_case: bool` field + detection | `voltnuerongrid-sql` | Detects `CASE WHEN` expressions anywhere in query (`S3-WS1-11`) | 3 (`case_tests` module) |
| `Case { input }` plan node | `voltnuerongrid-exec` | OLAP-routed CASE WHEN node; 0.9× row estimate, +1.5 cost | 2 |
| `GET /api/v1/store/rows/version` | `voltnuerongridd` | Returns MVCC row store current_xid, page_count, total_rows (operator-auth) | 2 |
| `GET /api/v1/store/htap/stats` | `voltnuerongridd` | Returns OLAP store table_count and total_entries (operator-auth) | 2 |

---

## Session 34 Implementation Log

**Date:** 2025 (Sprint 9 continuation)
**Test Baseline → New:** sql 129→132, exec 42→44, service 365→369 (+9 total)

| Item | Crate | Change | Tests Added |
|---|---|---|---|
| `has_not: bool` field + detection | `voltnuerongrid-sql` | Detects `NOT IN`, `NOT LIKE`, `NOT BETWEEN` in WHERE clause (`S3-WS1-10`) | 3 (`not_tests` module) |
| `Not { input }` plan node | `voltnuerongrid-exec` | OLTP-routed NOT predicate node in `LogicalPlan`; 0.85× row estimate, +0.6 cost | 2 |
| `GET /api/v1/store/rows/keys` | `voltnuerongridd` | Returns row store primary keys with optional prefix filter (operator-auth) | 2 |
| `POST /api/v1/store/wal/truncate` | `voltnuerongridd` | Truncates WAL up to sequence via forced checkpoint (operator-auth) | 2 |

---

## Definition of Done (Tracker)

A tracker row moves to **Done** only when:
- Implementation is merged and CI green.
- Required tests/benchmarks for that row pass.
- Evidence artifacts are attached.
- Dependencies in prerequisite gate are satisfied.
- Risk register impact is updated.
