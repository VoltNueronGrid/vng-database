# Tasks: Studio Database Lifecycle Fix

**Input**: `.specify/features/studio-lifecycle/plan.md`, `.specify/features/studio-lifecycle/spec.md`

## Phase 1: Server-Side Smoke Gate (C2)

- [ ] T001 Create `tests/kpi/scripts/run-studio-connection-lifecycle-smoke.ps1`
  - Pack 1: `GET /api/v1/admin/databases` with admin key — verify 200 OK, returns JSON array
  - Pack 2: `POST /api/v1/admin/databases` with name `studio_lifecycle_test_db` — verify 200/201 OK
  - Pack 3: `GET /api/v1/admin/databases` — verify `studio_lifecycle_test_db` in list
  - Pack 4: `POST /api/v1/sql/execute` with `USE studio_lifecycle_test_db; SELECT * FROM information_schema.tables` — verify response scoped to that DB
  - Pack 5: `DELETE /api/v1/admin/databases/studio_lifecycle_test_db` — cleanup
- [ ] T002 Create `tests/kpi/results/studio/` directory and gate artifact `studio-connection-lifecycle-smoke.json`
- [ ] T003 Update tracker REQ-14 (Studio database lifecycle) with gate evidence

---

## Phase 2: Native Protocol ADR (C3)

- [ ] T004 Create `docs/adr/adr-001-native-protocol-studio-scope.md` — Architecture Decision Record stating native TCP is driver-only
- [ ] T005 Update `.specify/memory/architecture-physical-view.md` — mark native protocol Studio scope gap as "closed" with ADR reference
- [ ] T006 Update `docs/architecture-summary-2026-06-23.md` — note native protocol scope boundary decision

---

## Phase 3: Studio UI (tracked separately — requires UI dev environment)

- [ ] T007 DEFERRED: Add `Pending` connection state to Studio connection context
- [ ] T008 DEFERRED: Add database existence check on connect — call `GET /api/v1/admin/databases`
- [ ] T009 DEFERRED: Add `CreateDatabaseModal` component shown when database not found
- [ ] T010 DEFERRED: Add error display for 401/403 unauthorized connection

---

## Phase 4: Documentation

- [ ] T011 Update `docs/tasks-v4.md` C2, C3 status to DONE
