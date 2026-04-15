# VoltNueronGrid IDE Extensions (WS9A Scaffold)

This folder contains a baseline contract scaffold for IDE integrations:

- Visual Studio
- Cursor
- Antigravity
- Windsor
- JetBrains
- Eclipse

## Phase 1 implementation (VSCode/Cursor)

Initial extension development starts in `vscode-cursor/` with:

- Connection wizard for Admin, Operator, and Tenant modes
- Secure secret storage through VS Code SecretStorage
- Connectivity checks for:
  - `GET /health`
  - `POST /api/v1/sql/execute`
  - `GET /api/v1/ingest/schema/registry`

This implements the first slice of IDE-002 from the sub-task roadmap.

## Phase 2 adapter scaffolds

`phase2/` now includes implementation scaffolds for:

- AntiGravity
- Windsor
- Eclipse
- Jetbrains

Each scaffold includes an adapter plan and connection samples for local, docker, cloud, and custom runtime targets.

## Contract model

- Shared API contract: `contracts/common-api-contract.json`
- Per-IDE adapter manifests:
  - `contracts/visual-studio.manifest.json`
  - `contracts/cursor.manifest.json`
  - `contracts/antigravity.manifest.json`
  - `contracts/jetbrains.manifest.json`
  - `contracts/eclipse.manifest.json`
