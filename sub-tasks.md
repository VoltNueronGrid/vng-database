# Sub-Tasks Plan — Completion Roadmap

**Project:** VoltNueronGrid DB (`polap-db`)  
**Last updated:** 2026-04-16  
**Program sign-off:** Approved by requester (governance intent), with technical gates still source-of-truth for release JSON states.

---

## IDE Extension Refactoring: Full Database Client (2026-04-15)

**Objective:** Transform VSCode extension from simple wizard to professional database client with connection management, database explorer, SQL editor, query execution, and advanced features.

**Current State:** v0.3.0 with modular architecture, explorer lazy loading, schema cache configurability, full connection management editor (including SSL and advanced options), status bar quick switch, richer SQL completion/hover/signature/snippet support, complete query execution/results/history workflows, and Phase 9 optimization slice (query-result cache + SQL table-reference cache reuse + startup diagnostics deferral)  
**Target State:** v0.3.0+ with full database client UI/UX  
**Estimated Effort:** 21-34 days (10 phases, 30+ tasks)

### Phase 1: Architecture & Core Infrastructure [EST: 2-3 days]

| ID | Task | Status | Owner | Target |
|---|---|---|---|---|
| IDE-1.1 | Create modular extension directory structure (providers, views, services, commands, ui, models) | ✅ Complete | Dev | 2026-04-15 |
| IDE-1.2 | Define shared TypeScript data models (Connection, Schema, Table, Column, QueryResult) | ✅ Complete | Dev | 2026-04-15 |
| IDE-1.3 | Build ConnectionManager service (add/delete/switch, SecretStorage, persistence) | ✅ Complete | Dev | 2026-04-15 |

### Phase 2: Database Explorer & Tree Views [EST: 2-3 days]

| ID | Task | Status | Owner | Target |
|---|---|---|---|---|
| IDE-2.1 | Create TreeDataProvider for database explorer (schemas, tables, columns with lazy load) | ✅ Complete | Dev | 2026-04-19 |
| IDE-2.2 | Implement context menu actions (Copy Name, Show DDL, SQL Template, Dump, Drop, Edit, etc.) | ✅ Complete | Dev | 2026-04-15 |

### Phase 3: Connection Management UI [EST: 3-4 days]

| ID | Task | Status | Owner | Target |
|---|---|---|---|---|
| IDE-3.1 | Build connection config webview (React/HTML form with validation, SSL tab, advanced options) | ✅ Complete | Dev | 2026-04-22 |
| IDE-3.2 | Create connection list panel with add/edit/delete/test/switch actions | ✅ Complete | Dev | 2026-04-22 |
| IDE-3.3 | Add status bar connection indicator with quick switcher | ✅ Complete | Dev | 2026-04-23 |

### Phase 4: SQL Editor Integration [EST: 3-4 days]

| ID | Task | Status | Owner | Target |
|---|---|---|---|---|
| IDE-4.1 | Detect .sql files and add execute toolbar with keyboard shortcuts (Ctrl+Enter) | ✅ Complete | Dev | 2026-04-15 |
| IDE-4.2 | Implement SQL autocomplete provider (tables, columns, keywords, functions) | ✅ Complete | Dev | 2026-04-25 |
| IDE-4.3 | Add SQL syntax highlighting and diagnostics (invalid table/column detection) | ✅ Complete | Dev | 2026-04-26 |

### Phase 5: Query Execution & Results [EST: 2-3 days]

| ID | Task | Status | Owner | Target |
|---|---|---|---|---|
| IDE-5.1 | Build QueryExecutionService (parse, execute, stream, cancel, timeout handling) | ✅ Complete | Dev | 2026-04-27 |
| IDE-5.2 | Create results display webview (paginated table, sort, filter, export CSV/JSON) | ✅ Complete | Dev | 2026-04-28 |
| IDE-5.3 | Implement query history sidebar (recent queries, re-execute, search, persistence) | ✅ Complete | Dev | 2026-04-29 |

### Phase 6: Advanced Features [EST: 3-4 days]

Execution order: **Active now (current implementation phase)**.

| ID | Task | Status | Owner | Target |
|---|---|---|---|---|
| IDE-6.1 | Build inline table editor (edit cells, add/delete rows, save to database) | ✅ Complete | Dev | 2026-05-01 |
| IDE-6.2 | Create schema management UI (create/alter table wizard, DDL preview) | ✅ Complete | Dev | 2026-05-02 |
| IDE-6.3 | Implement comprehensive settings panel (editor, results, connection, keybindings) | ✅ Complete | Dev | 2026-05-02 |
| IDE-6.4 | Define and register keyboard shortcuts (Ctrl+Enter, Ctrl+Shift+F, Ctrl+Alt+C, etc.) | ✅ Complete | Dev | 2026-05-03 |

**Phase 6 exit criteria (switch gate to Phase 7)**
- All Phase 6 tasks (IDE-6.1 to IDE-6.4) are marked complete with no open critical defects.
- Core workflow validation passes for inline editing, schema management, settings, and registered shortcuts.

Progress note (2026-04-16): **PHASE 6 COMPLETE** — IDE-6.1 includes stronger type-aware validation with cell-level error surfacing plus partial-save recovery metadata/export; IDE-6.2 has create/alter table wizards that generate DDL previews and optional execution; IDE-6.3 has a comprehensive settings panel (editor, SQL, results, connection) with persistent configuration; IDE-6.4 has all table-editor and schema-wizard keyboard shortcuts wired. All Phase 6 tasks complete with 11/11 unit tests passing and clean build. **Phase 6 exit criteria met — ready for Phase 7: UI Polish & Accessibility.**

### Phase 7: UI Polish & Accessibility [EST: 2-3 days]

Execution order: **Next (starts immediately after Phase 6 exit criteria are met)**.

| ID | Task | Status | Owner | Target |
|---|---|---|---|---|
| IDE-7.1 | Create professional icon set (database, schema, table, execute, etc.) with light/dark theme support | ✅ Complete | Design/Dev | 2026-05-04 |
| IDE-7.2 | Add accessibility features (ARIA labels, keyboard navigation, screen reader support, color contrast) | ✅ Complete | Dev/QA | 2026-05-05 |
| IDE-7.3 | Implement status messages and notifications (connecting, query running, errors, success) | ✅ Complete | Dev | 2026-05-05 |

### Phase 8: Testing & Documentation [EST: 2-3 days]

| ID | Task | Status | Owner | Target |
|---|---|---|---|---|
| IDE-8.1 | Write unit tests for models, services, providers (target 80%+ coverage) | ✅ Complete | QA | 2026-05-07 |
| IDE-8.2 | Write integration tests for core workflows (connection → query → results, autocomplete, tree) | ✅ Complete | QA | 2026-05-08 |
| IDE-8.3 | Write documentation (README, FEATURE_GUIDE, ARCHITECTURE, troubleshooting) | ✅ Complete | Dev/Doc | 2026-05-08 |

Progress note (2026-04-16): Phase 8 is complete. Extracted SQL autocomplete/search intelligence into a pure module and added workflow-style tests for completion context, alias/table resolution, signature context, and diagnostic suggestion parsing. Expanded query workflow tests for connection -> query -> results-state publication -> query-history behavior, and fixed execution/history ID collision defects found by those tests. Raised low-coverage modules by adding targeted tests for `Schema` and `TableEditorSql` branches. Explicit coverage run via `node --test --experimental-test-coverage out/test/**/*.test.js` reports 95.29% line coverage overall, with `Schema` at 100.00%, `TableEditorSql` at 94.69%, and `SqlIntelligence` at 90.66%.

### Phase 9: Performance & Optimization [EST: 1-2 days]

| ID | Task | Status | Owner | Target |
|---|---|---|---|---|
| IDE-9.1 | Profile UI performance and fix bottlenecks (tree render, autocomplete, results table) | ✅ Complete | Dev/Perf | 2026-05-09 |
| IDE-9.2 | Implement caching strategy (schema cache, query result cache, connection pool reuse) | ✅ Complete | Dev | 2026-05-09 |
| IDE-9.3 | Optimize bundle size (code splitting, lazy loading, measure startup time < 1s) | ✅ Complete | Dev | 2026-05-10 |

Progress note (2026-04-16): Phase 9 optimization slice implemented and validated. Added query-result caching with TTL/max-entry controls in `QueryExecutionService`, reused SQL table-reference shaping via schema-timestamp-aware cache, and removed eager startup diagnostics scan in favor of active-document updates. Validation evidence: `npm run build` passing, `npm test` passing (42/42), coverage run reporting 93.98% line coverage, and startup benchmark (`npm run benchmark:startup`) reporting cold-load metrics over 10 runs: min 91.977 ms, avg 110.916 ms, p95 146.446 ms, max 146.446 ms.

### Phase 10: Release & Publishing [EST: 1 day]

| ID | Task | Status | Owner | Target |
|---|---|---|---|---|
| IDE-10.1 | Bump version to 0.3.0, write CHANGELOG, tag release | 🟨 In Progress | Dev/Release | 2026-05-11 |
| IDE-10.2 | Package VSIX, test on clean install, publish to VS Code Marketplace | 🟨 In Progress | Release | 2026-05-11 |

Progress note (2026-04-16): Release prep advanced to version `0.3.0` with changelog added and VSIX artifact generated: `ui/ide-extensions/vscode-cursor/voltnuerongrid-vscode-cursor-0.3.0.vsix`. Clean-install validation is now passing in an isolated Cursor profile (`--user-data-dir` and `--extensions-dir`): install command reported `Extension 'voltnuerongrid-vscode-cursor-0.3.0.vsix' was successfully installed.` and list command reported `voltnuerongrid.voltnuerongrid-vscode-cursor@0.3.0`. Remaining release operations: git tag creation on release commit and Marketplace/private-feed publish with credentials.

---

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
| IDE extension Phase 9/10 validation | ✅ Partial complete | `npm run build` pass, `npm test` pass (42/42), coverage 93.98% line, VSIX `0.3.0` packaged |

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

Phase 1 priority target: VSCode + Cursor. Additional IDE extensions are scheduled as a later-phase expansion.

| ID | Task | Status | Dependencies |
|---|---|---|---|
| IDE-001 | Choose first IDE target (Phase 1: VSCode/Cursor) | ✅ | prioritization |
| IDE-002 | Implement connection wizard (URL, admin key, tenant/user headers) | ✅ Done | IDE-001 |
| IDE-003 | Add query runner + schema introspection + diagnostics | ✅ Done | IDE-002 |
| IDE-004 | Add auth-aware feature gating and permission errors | ✅ Done | IDE-003 |
| IDE-005 | Package extension + publish private feed + smoke tests | 🟨 In progress | IDE-004 |
| IDE-006 | Add additional IDE extensions (Phase 2): AntiGravity, Windsor, Eclipse, Jetbrains | 🟨 In progress | IDE-005 |

IDE-005 progress note (2026-04-15): VSCode/Cursor extension has local smoke script and packaging config in place; remaining step is private-feed publish with environment credentials.
IDE-005 env blocker note (2026-04-15): local package attempt is blocked by npm authentication (401); publish remains pending until registry credentials are refreshed.
IDE-005 closeout evidence (2026-04-15): VSIX artifact produced at `ui/ide-extensions/vscode-cursor/voltnuerongrid-vscode-cursor-0.1.0.vsix`; private-feed publish remains blocked pending feed endpoint details, PAT source, and Azure DevOps CLI extension readiness.
IDE-006 progress note (2026-04-15): Added Windsor contract and Phase 2 adapter scaffolds for AntiGravity, Windsor, Eclipse, and Jetbrains with implementation plans and connection samples.

### 4.2.1 IDE UX parity correction plan (requested)

Reported mismatch: expected rich "new/edit connection" form + database list tree behavior, but current extension shows action-only commands and runtime-target picker instead of connection-centric UX.

| ID | Task | Status | Owner | Target | Dependencies | Acceptance criteria |
|---|---|---|---|---|---|---|
| IDE-UX-001 | Rework **Create New Connection** flow to open a dedicated connection editor webview/panel (not command-only/runtime-target dialog) | ✅ Complete | Dev | 2026-04-18 | IDE-3.1/IDE-3.2 | Clicking "Create New Connection" opens form with host, port, username, password, database, SSL, advanced options; supports Save and Connect |
| IDE-UX-002 | Ensure **Edit Connection** opens the same rich editor with values prefilled from selected saved connection | ✅ Complete | Dev | 2026-04-18 | IDE-UX-001 | Editing any connection shows full pre-populated form and persists updates |
| IDE-UX-003 | Replace/augment left sidebar root with **Connections -> Databases** tree for active connection | ✅ Complete | Dev | 2026-04-19 | IDE-2.1/IDE-3.2 | Sidebar lists saved connections and, when connected, shows expandable databases/schemas/tables/columns |
| IDE-UX-004 | Add per-connection actions in tree/context menu: Connect, Disconnect, Edit, Delete, Refresh | ✅ Complete | Dev | 2026-04-19 | IDE-UX-003 | User can disconnect directly from sidebar and tree updates immediately |
| IDE-UX-005 | Implement empty-state view in sidebar: "No connections available. Please create a new one." with CTA button | ✅ Complete | Dev/UX | 2026-04-20 | IDE-UX-001 | When no saved connections exist, message + action are visible and keyboard accessible |
| IDE-UX-006 | Wire empty-state CTA to open connection editor in split/adjacent panel | ✅ Complete | Dev | 2026-04-20 | IDE-UX-005 | Clicking CTA opens the create connection page beside explorer and allows immediate data entry |
| IDE-UX-007 | Add state transitions and telemetry-safe notifications for connect/disconnect/create/edit failures | ✅ Complete | Dev | 2026-04-21 | IDE-UX-001..006 | Clear non-blocking messages on success/failure; no secrets in logs |
| IDE-UX-008 | Add integration tests for: empty state -> create -> connect -> expand db tree -> disconnect | ✅ Complete | QA/Dev | 2026-04-22 | IDE-UX-001..007 | Test suite validates end-to-end UX parity and prevents regression |

Implementation note: this correction block is prioritized ahead of multi-IDE adapters to avoid propagating incorrect UX patterns to Phase 2 extension targets.

**Daily execution order (stand-up checklist)**

- [x] **Day 1 (2026-04-18):** Complete IDE-UX-001 and start IDE-UX-002; demo create/edit form parity in-panel.
- [x] **Day 2 (2026-04-19):** Complete IDE-UX-003 and IDE-UX-004; verify left tree expand/collapse and disconnect action.
- [x] **Day 3 (2026-04-20):** Complete IDE-UX-005 and IDE-UX-006; validate empty-state copy and CTA open-in-adjacent-panel behavior.
- [x] **Day 4 (2026-04-21):** Complete IDE-UX-007; confirm user-facing notifications and secret-safe logging on failures.
- [x] **Day 5 (2026-04-22):** Complete IDE-UX-008 and run end-to-end regression pass for empty state -> create -> connect -> explore -> disconnect.
- [ ] **Daily closeout:** Update task statuses in this table and record blockers/owner handoffs before end-of-day.

**Definition of Done for this week**

- UX parity is achieved for create/edit connection, and no runtime-target-only flow blocks connection entry.
- Sidebar supports connection lifecycle (connect/disconnect) plus expandable database exploration from active connections.
- Integration coverage passes for empty-state -> create -> connect -> expand tree -> disconnect with no critical regressions.

IDE-UX progress note (2026-04-16): Connection workflows now include telemetry-safe failure notifications and secret-redacted error messaging for create/edit/connect/disconnect/test flows. Added integration coverage for empty -> create -> connect -> expand -> disconnect via the connection explorer flow helpers. Phase 7 polish is complete with themed explorer icons, ARIA/live-region support across key webviews, and polished connection lifecycle notifications.

**Next execution kickoff (immediate)**

- Start with **Phase 8: Testing & Documentation** to deepen provider/service coverage, add broader workflow integration tests, and refresh extension documentation.
- Treat **Phase 9**, **Phase 10**, and the productization tracks below as backlog items that remain intentionally open beyond this session.

### 4.3 MCP track — production-ready server capability

| ID | Task | Status | Dependencies | Evidence |
|---|---|---|---|---|
| MCP-001 | Define MCP scope (query, schema, health, benchmark, admin actions) | ✅ Complete | API security policy | [crates/voltnuerongrid-mcp/README.md](crates/voltnuerongrid-mcp/README.md#tools) |
| MCP-002 | Build MCP server process with auth + scoped operations | ✅ Complete | MCP-001 | [src/lib.rs](crates/voltnuerongrid-mcp/src/lib.rs#L131-L190) + auth module |
| MCP-003 | Add tool schemas/resources and safety guardrails | ✅ Complete | MCP-002 | [src/tools.rs](crates/voltnuerongrid-mcp/src/tools.rs) + [src/guardrails.rs](crates/voltnuerongrid-mcp/src/guardrails.rs) |
| MCP-004 | Integration test with Cursor/client and permission boundary checks | ✅ Complete | MCP-003 | [tests/integration_tests.rs](crates/voltnuerongrid-mcp/tests/integration_tests.rs) - 12 tests pass |
| MCP-005 | Operationalize (packaging, docs, observability) | ✅ Complete | MCP-004 | [README.md](crates/voltnuerongrid-mcp/README.md) + [OPERATIONS.md](crates/voltnuerongrid-mcp/OPERATIONS.md) |

### MCP Implementation Summary

**Completed Deliverables:**
- ✅ New crate: `crates/voltnuerongrid-mcp/` with 5 modules (auth, tools, guardrails, integration, lib)
- ✅ 28 unit tests (auth, guardrails, tools, integration) - all passing
- ✅ 12 integration tests covering permission boundaries and error scenarios - all passing
- ✅ 4 tools implemented: query, schema, health, benchmark with full documentation
- ✅ Multi-level auth (Admin → Operator → Tenant) with proper error codes (401/403)
- ✅ Safety guardrails: DDL prevention, size limits, timeout controls, SQL injection detection
- ✅ Comprehensive docs: README.md (5KB), OPERATIONS.md (4KB)
- ✅ Zero warnings on compilation (strict mode)

**Test Coverage:**
- Authentication & authorization: 10 tests
- Query validation & guardrails: 8 tests  
- Permission boundaries: 6 tests
- Tool execution & integration: 8 tests
- Error handling: 4 tests
- Total passing: **40 tests** (28 unit + 12 integration)

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

