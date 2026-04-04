# VoltNueronGrid DB — Copilot Workspace Instructions

## Project

Rust workspace for a distributed HTAP database engine with SQL, ingest, storage, auth, failover, AI-autonomous, and plugin subsystems. Primary codebase: `crates/voltnuerongrid-*`, service binary: `services/voltnuerongridd/src/main.rs`, driver: `drivers/voltnuerongrid-driver-rust/`.

## Build & Test Commands

```bash
cargo check -p voltnuerongridd                          # fast compile check
cargo build                                             # full build
cargo test -p voltnuerongrid-sql                        # SQL crate unit tests
cargo test -p voltnuerongrid-store                      # store crate (28+ tests)
cargo test -p voltnuerongrid-ingest                     # ingest crate (17+ tests)
cargo test -p voltnuerongridd                           # all service tests (45+)
cargo test -p voltnuerongridd ws3_ -- --test-threads=1 # WS3 integration (needs live server)
```

Gate scripts in `tests/kpi/scripts/` run with PowerShell (`pwsh`). For live smoke packs:
1. Start server: `cargo run -p voltnuerongridd` (listens on `http://127.0.0.1:8080`)
2. Run script: `pwsh ./tests/kpi/scripts/run-ws5-gate.ps1 -BaseUrl http://127.0.0.1:8080`

## Crate & Naming Conventions

- Crate folders: `voltnuerongrid-{name}` (hyphens). Import: `voltnuerongrid_{name}` (underscores).
- New crates must export traits explicitly; service imports with `use voltnuerongrid_{name}::TraitName`.
- Three crates are intentional stubs (`voltnuerongrid-core`, `voltnuerongrid-failover`, `voltnuerongrid-meta`); all logic lives in `main.rs`. Do not add implementation to these unless beginning a deliberate extraction.

## RBAC / Auth Patterns — NEVER BYPASS

All protected endpoints must enforce in this order:
1. **Admin check**: `VNG_ADMIN_API_KEY` env var + `x-vng-admin-key` header
2. **Operator identity**: `x-vng-operator-id` header + registered role binding (for operator-scoped paths)
3. **Tenant scoping**: `x-vng-tenant-id` + `x-vng-user-id` headers (for tenant-facing paths)

Mixed operator-or-tenant surfaces check `x-vng-admin-key` OR `x-vng-operator-id` first; fall through to tenant headers. Never return data across tenant boundaries. Always return `401` for missing credentials, `403` for insufficient privilege.

## Test Naming Conventions

- `ws{N}_{feature}` — workstream integration tests (e.g. `ws3_query_routing`, `ws22_lock_acquire`)
- `h{NN}_{feature}` — hardening tests (e.g. `h07_sql_data_plane_pool_acquire_release_on_sql_handlers`)
- `operator_auth_*` — operator RBAC tests
- `sql_runtime_*` — SQL endpoint runtime tests
- `ingest_*` — ingestion tests

`AppState` test helpers: always use `state_with_key()` from the tests module. When adding new fields to `AppState`, update `state_with_key()` before writing new tests.

## Gate Script Conventions

- Location: `tests/kpi/scripts/run-{wsN}-{name}-smoke.ps1` (smoke) / `run-{wsN}-gate.ps1` (gate)
- Artifacts: `tests/kpi/results/{wsN}/{artifact-name}.json`
- Gate scripts use `$packs` array pattern. Derive gate success from emitted JSON `status` fields — do **not** rely on `$LASTEXITCODE` from nested PowerShell, it can stay stale.
- Release summaries go in `tests/kpi/results/gates/`.

## Security Rules

- Never log or return raw API keys or secrets.
- Encryption-at-rest and TLS config enforced via security contract in WS5; adding new endpoints means adding them to the contract.
- KMS key references: never hardcode; resolve via `VNG_KMS_*` env vars.
- Plugin manifests must be signed; never load unsigned plugin manifests.

## Key Blockers (as of 2026-04-04)

- **WS3 gate BLOCKED**: query routing tests timeout; HTAP routing (point→OLTP, agg→OLAP, mixed→hybrid) not complete; performance score 40/100.
- **WS1 live smoke failing**: PowerShell HTTP pack errors (`GetResponseStream`); fix requires `Invoke-WebRequest`/`Invoke-RestMethod` instead.
- **REQ-07, 08, 10, 19, 21, 27**: not started (benchmarks, cloud SaaS, scale, concurrency, cache).

## Status Tracker

- `status_tracker.md` — main tracker (REQ-01..REQ-31, sections 2–9)
- `status-tracker-sprintwise-v1.md` — sprint breakdown (Sprint 0-11)
- Gate Reality Check: section 5.22 of `status_tracker.md`
