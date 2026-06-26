# Feature Specification: Studio Database Lifecycle Fix

**Feature Branch**: `studio-lifecycle`

**Created**: 2026-06-26

**Status**: Not Started

**Priority**: P1 — user-facing correctness: connecting to a non-existent database must not silently open an empty workspace

**Input**: Fix the Studio connection state machine so that connecting to a non-existent database prompts the user to create it, and connecting to an existing database scopes the workspace to that database only.

---

## Background and Problem Statement

The current Studio connection flow has a lifecycle bug:

- When a user enters a database name that does not exist, Studio either silently opens an empty workspace or fails with a non-actionable error
- The architecture scenario view states: "No databases exist and user enters a database name → UI asks whether to create empty or sample/default database before opening workspace. No phantom valid connection and no implicit resources."
- Constitution Principle V: "UI and IDE workflows MUST use the same authorization and validation rules as runtime endpoints."
- Constitution Principle II: "Tenant-facing reads or writes MUST never cross database boundaries."

The fix involves adding a `Pending` state to the connection state machine, checking database existence via `GET /api/v1/admin/databases`, and showing a create-or-select modal when the database does not exist.

---

## User Scenarios & Testing

### User Story 1 — Non-existent database prompts creation (Priority: P1)

**Acceptance Scenarios**:
1. **Given** no databases exist, **When** user enters database name `mydb` and clicks Connect, **Then** a modal appears asking: "Database `mydb` not found. Create empty database / Create with sample data / Select different database."
2. **Given** the modal appears, **When** user selects "Create empty database", **Then** `POST /api/v1/admin/databases` is called and the workspace opens scoped to `mydb` with no tables.
3. **Given** the modal appears, **When** user clicks Cancel, **Then** the connection remains in `Disconnected` state — no phantom workspace is opened.

---

### User Story 2 — Existing database opens scoped workspace (Priority: P1)

**Acceptance Scenarios**:
1. **Given** database `mydb` exists with 3 tables, **When** user connects to `mydb`, **Then** workspace tree shows exactly those 3 tables under `mydb` and no resources from other databases.
2. **Given** user is not authorized for `mydb`, **When** connection is attempted, **Then** a 401/403 error is shown with an actionable message — no empty workspace is opened.

---

### User Story 3 — Scope boundary for native protocol (Priority: P2)

**Acceptance Scenarios**:
1. **Given** the architecture decision that native TCP protocol is driver-only (not browser-accessible), **When** Studio shows native protocol option, **Then** it shows a tooltip "Desktop-native only — not available in browser-based Studio."
2. **Given** the documented scope boundary, **When** architecture views are updated, **Then** the physical view gap for "Native protocol validation path in Studio" is marked closed.

---

## Technical Design

### Connection State Machine

**New states**: `Disconnected` → `Pending` → `Active` (or back to `Disconnected`)

**`Pending` state behavior:**
1. Call `GET /api/v1/admin/databases` to get the list of databases
2. If target database not in list → show create-or-select modal
3. If target database in list but auth fails → show 401/403 error
4. If target database in list and auth succeeds → transition to `Active` with `active_database = target`

### Server-Side Requirements

The server already has:
- `GET /api/v1/admin/databases` endpoint (returns database list)
- `POST /api/v1/admin/databases` endpoint (creates database)
- Authorization checks on all database endpoints

No server-side changes needed for US1/US2.

### Architecture Decision Record (C3 dependency)

Native protocol is browser-inaccessible (TCP socket cannot be reached from a browser fetch context). Studio uses HTTP exclusively. This is a scope boundary, not a bug. An ADR must be written and the physical view gap closed.

---

## File Impact

| File | Change |
|---|---|
| `ui/voltnuerongrid-studio/src/` | Connection state machine: add `Pending` state, database existence check, create modal |
| `tests/kpi/scripts/run-studio-connection-lifecycle-smoke.ps1` | New smoke gate (server-side only) |
| `tests/kpi/results/studio/` | New directory for gate artifacts |
| `.specify/memory/architecture-physical-view.md` | Mark native protocol scope boundary closed |

---

## Acceptance Gates

| Gate | Command | Must Pass |
|------|---------|-----------|
| Studio lifecycle smoke | `pwsh run-studio-connection-lifecycle-smoke.ps1` | status: passed |
| Native protocol ADR | ADR document exists | Documented scope boundary |

## Definition of Done

- [ ] Connection to non-existent DB shows create-or-select modal
- [ ] Connection to existing DB opens workspace scoped to that DB only
- [ ] Unauthorized connection shows 401/403 error with actionable message
- [ ] Studio lifecycle smoke gate passes
- [ ] Native protocol scope boundary documented (see C3)
- [ ] Tracker REQ-14 updated with gate evidence

## Related Items

- tasks-v4.md C2, C3, P8
- Architecture scenario view: `docs/architecture-scenario-view.md`
- Physical view gap: `.specify/memory/architecture-physical-view.md`
- Studio source: `ui/voltnuerongrid-studio/src/`
