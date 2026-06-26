# Implementation Plan: Studio Database Lifecycle Fix

**Branch**: `main` | **Date**: 2026-06-26 | **Spec**: [spec.md](spec.md)

## Summary

Add a `Pending` connection state to Studio's connection state machine. Before transitioning to `Active`, check database existence via `GET /api/v1/admin/databases`. Show a create-or-select modal when the database does not exist. Also document native protocol scope boundary as an ADR.

## Technical Context

**Language/Version**: TypeScript / React (Studio UI)

**Primary Dependencies**: Studio connection context, `GET /api/v1/admin/databases`, `POST /api/v1/admin/databases`

**Storage**: No storage changes — server-side database catalog is already correct

**Testing**: Studio lifecycle smoke gate (server-side API testing via PowerShell)

**Target Platform**: Browser (HTTP-only; native TCP is driver-only)

## Constitution Check

- **Durable HTAP correctness**: PASS — workspace scope prevents cross-database data access
- **Security/RBAC/tenant isolation**: PASS — unauthorized connection returns 401/403, no phantom workspace
- **Performance evidence**: N/A — UI state machine transition
- **Modular Rust/reuse**: N/A — UI change; server APIs already exist
- **Native interface parity**: PASS — scope boundary documented; Studio is HTTP-only
- **Autonomous/plugin governance**: N/A
- **Evidence and tracker truth**: Studio lifecycle smoke gate

## Project Structure

```text
ui/voltnuerongrid-studio/src/
├── contexts/connection.ts    # Add Pending state
├── components/ConnectionForm.tsx   # Add database existence check
└── components/CreateDatabaseModal.tsx  # New modal component

tests/kpi/scripts/
└── run-studio-connection-lifecycle-smoke.ps1  # New gate

docs/adr/
└── adr-001-native-protocol-studio-scope.md   # Architecture decision record
```

## Implementation Phases

### Phase 1: Server-Side Smoke Gate (validates existing server behavior)

Create `run-studio-connection-lifecycle-smoke.ps1` that:
1. Calls `GET /api/v1/admin/databases` — verifies endpoint works
2. Calls `POST /api/v1/admin/databases` — creates a test database
3. Calls `GET /api/v1/admin/databases` again — verifies the new database appears
4. Calls the SQL endpoint scoped to the database — verifies isolation

### Phase 2: Native Protocol ADR (C3)

Write `docs/adr/adr-001-native-protocol-studio-scope.md` stating:
- Native TCP protocol is not accessible from browser fetch contexts
- Studio uses HTTP exclusively for all server communication
- Native protocol is for language drivers only (Rust, Python, TypeScript drivers)
- Update physical view gap to "closed"

### Phase 3: Studio UI Changes

Requires UI development environment. Tracked separately.

## Definition of Done

- [x] Server-side APIs for database lifecycle exist (`GET`/`POST /api/v1/admin/databases`)
- [ ] Studio lifecycle smoke gate (`run-studio-connection-lifecycle-smoke.ps1`) passes
- [ ] Native protocol ADR written (`docs/adr/adr-001-native-protocol-studio-scope.md`)
- [ ] Physical view gap for native protocol Studio marked closed
- [ ] Studio UI connection state machine updated (tracked separately — requires UI dev env)
