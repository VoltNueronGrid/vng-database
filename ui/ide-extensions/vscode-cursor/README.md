# VoltNueronGrid VSCode/Cursor Extension (Phase 1)

This extension starts IDE-002 from the roadmap.

## Implemented in this slice

- Connection wizard for three modes:
  - Admin
  - Operator
  - Tenant
- Runtime targets:
  - Local
  - Docker
  - Cloud
  - Custom
- Secure storage for sensitive values using VS Code SecretStorage
- One-click test command against runtime endpoints:
  - `GET /health`
  - `POST /api/v1/sql/execute`
  - `GET /api/v1/ingest/schema/registry`
- Query tools:
  - Run query (`vng.runQuery`)
  - Analyze query (`vng.analyzeQuery`)
  - Show schema registry (`vng.showSchemaRegistry`)

## Commands

- `VoltNueronGrid: Connection Wizard`
- `VoltNueronGrid: Test Connection`

## Build

```bash
npm install
npm run build
```

## Local smoke test

```powershell
pwsh ./smoke-test.ps1 -BaseUrl "http://127.0.0.1:8080" -AdminKey "secret"
```

## Notes

- This is a starter implementation for VSCode/Cursor only.
- Additional IDE extensions (AntiGravity, Windsor, Eclipse, Jetbrains) remain Phase 2.
- Private feed publishing is still pending and remains part of IDE-005.
