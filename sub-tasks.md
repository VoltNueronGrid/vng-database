# Sub-Tasks Plan — Completion Roadmap

**Project:** VoltNueronGrid DB (`polap-db`)  
**Last updated:** 2026-04-14  
**Program sign-off:** Approved by requester (governance intent), with technical gates still source-of-truth for release JSON states.

---

## 0) Current verified state (this session)

| Item | Status | Notes |
|---|---|---|
| H-09 parity pack | ✅ Passing | Path fallback now supports `services/voltnuerongridd/reference/...` |
| H-10 checklist pack | ✅ Passing | Path fallback now supports `services/voltnuerongridd/reference/...` |
| H-09 release readiness JSON | ✅ `ready_for_validation` | with `VNG_PROGRAM_SIGNOFF_APPROVED=true` |
| H-10 release readiness JSON | ✅ `ready_for_validation` | with `VNG_PROGRAM_SIGNOFF_APPROVED=true` |
| REQ-10 benchmark smoke (live) | ✅ 12/12 passed | local server on `127.0.0.1:8080`, admin key set |
| R4 release gate | ✅ Passing | `release-r4-saas-maturity-readiness.json` now `passed / ready_for_validation` |

---

## 1) Immediate closure tasks (short-term)

| ID | Task | Owner | Status | Dependencies | Completion target |
|---|---|---|---|---|---|
| ST-001 | Keep H-09 matrix script path-agnostic (`reference/` + `services/.../reference/`) | Platform | ✅ Done | none | 100% |
| ST-002 | Keep H-10 checklist script path-agnostic (`reference/` + `services/.../reference/`) | Platform | ✅ Done | none | 100% |
| ST-003 | Enable program-signoff-aware readiness for H-09/H-10 release summaries via env flag | Platform | ✅ Done | ST-001/ST-002 | 100% |
| ST-004 | Run local benchmark with live server + auth key | QA/Perf | ✅ Done | running server | 100% |
| ST-005 | Fix WS14 gate path assumptions (`reference/config-contracts/ws14/*`) after file move | Platform | ✅ Done | reference path decision | 100% |
| ST-006 | Re-run R4 gate after WS14 fix | QA/Release | ✅ Done | ST-005 | 100% |

---

## 2) R4 unblocking plan (technical)

### 2.1 Required to flip `release-r4-saas-maturity-readiness.json`

1. **Fix WS14 gate input paths** so it can find config contracts from either:
   - `reference/config-contracts/ws14/...` or
   - `services/voltnuerongridd/reference/config-contracts/ws14/...`.
2. Re-run:
   - `pwsh ./tests/kpi/scripts/run-ws14-gate.ps1`
   - `pwsh ./tests/kpi/scripts/run-release-ops-resilience-gate.ps1`
   - `pwsh ./tests/kpi/scripts/run-release-r4-saas-maturity-gate.ps1 -BaseUrl "http://127.0.0.1:8080"`
3. Confirm JSON checks:
   - `ops_resilience_ready = true`
   - `h09_release_ready = true`
   - `h10_release_ready = true`
   - `req10_benchmark_passed = true`
4. Update `status-tracker.md` R4 row to match refreshed gate evidence.

### 2.2 Dependency graph

- WS14 fix (ST-005) -> release-ops-resilience pass -> R4 pass.

---

## 3) Local execution commands (server + test)

### 3.1 Start server locally (with admin key)

```powershell
Set-Location "D:\by\polap-db"
$env:VNG_ADMIN_API_KEY="secret"
cargo run -p voltnuerongridd
```

Expected log line:
- `voltnuerongridd listening on 127.0.0.1:8080`

### 3.2 Run benchmark smoke against local server

```powershell
Set-Location "D:\by\polap-db"
$env:VNG_ADMIN_API_KEY="secret"
pwsh ./tests/kpi/scripts/run-req10-benchmark-smoke.ps1 -BaseUrl "http://127.0.0.1:8080"
```

### 3.3 Run local concurrency/load-oriented tests

```powershell
Set-Location "D:\by\polap-db"
cargo test -p voltnuerongridd ws21_
```

---

## 4) “Can we complete this?” productization tracks

The following are feasible, but they are **multi-sprint implementation tracks**, not single-session fixes.

### 4.1 UI track — from scaffold to shippable product

| ID | Task | Status | Dependencies |
|---|---|---|---|
| UI-001 | Define product scope (auth, query console, schema explorer, ingest, admin panels) | ⬜ | product requirements |
| UI-002 | Build API integration layer for real runtime endpoints | ⬜ | stable API contracts |
| UI-003 | Implement auth/session UX for admin/operator/tenant roles | ⬜ | WS5 auth behavior |
| UI-004 | Build critical screens (query editor, results grid, metrics, audit views) | ⬜ | UI-002/UI-003 |
| UI-005 | Add E2E tests + packaging (desktop/web target) | ⬜ | UI-004 |
| UI-006 | Release hardening (telemetry, error handling, docs, installers) | ⬜ | UI-005 |

### 4.2 IDE connectivity track — usable extension path

| ID | Task | Status | Dependencies |
|---|---|---|---|
| IDE-001 | Choose first IDE target (recommended: VSCode/Cursor) | ⬜ | prioritization |
| IDE-002 | Implement connection wizard (URL, admin key, tenant/user headers) | ⬜ | IDE-001 |
| IDE-003 | Add query runner + schema introspection + diagnostics | ⬜ | IDE-002 |
| IDE-004 | Add auth-aware feature gating and permission errors | ⬜ | IDE-003 |
| IDE-005 | Package extension + publish private feed + smoke tests | ⬜ | IDE-004 |

### 4.3 MCP track — production-ready server capability

| ID | Task | Status | Dependencies |
|---|---|---|---|
| MCP-001 | Define MCP scope (query, schema, health, benchmark, admin actions) | ⬜ | API security policy |
| MCP-002 | Build MCP server process with auth + scoped operations | ⬜ | MCP-001 |
| MCP-003 | Add tool schemas/resources and safety guardrails | ⬜ | MCP-002 |
| MCP-004 | Integration test with Cursor/client and permission boundary checks | ⬜ | MCP-003 |
| MCP-005 | Operationalize (packaging, docs, observability) | ⬜ | MCP-004 |

---

## 5) Priority recommendation (if cloud cannot be funded now)

Given current constraints, execute in this order:

1. **R4 technical unblock**: ST-005 -> ST-006  
2. **Benchmark depth**: expand REQ-10/REQ-19 perf matrix on local hardware tiers  
3. **Load depth**: sustained HTTP concurrency suite (k6/Locust style) mapped to REQ-21  
4. **UI/IDE MVP**: one shippable target (VSCode/Cursor) before multi-IDE spread  
5. **MCP MVP**: readonly query + schema + health first, then controlled write/admin tools.

---

## 6) Notes on why “files were missing”

- The files were not logically missing from the project; they are currently present under:
  - `services/voltnuerongridd/reference/...`
- Several gate scripts were still hard-coded to legacy root paths under:
  - `reference/...`
- Resolution pattern adopted: scripts now support both paths to avoid breakage after structure moves.

