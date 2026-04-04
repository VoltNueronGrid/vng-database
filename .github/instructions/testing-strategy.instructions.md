---
description: "Use when writing tests, debugging failing tests, adding gate scripts, or determining which tests to run for a given change. Covers cargo test patterns, gate script structure, live-server requirements, and naming."
---
# VoltNueronGrid DB — Testing Strategy

## Test Layers

| Layer | Command | When Required |
|-------|---------|---------------|
| Crate unit tests | `cargo test -p voltnuerongrid-{crate}` | Always — no server needed |
| Service integration tests | `cargo test -p voltnuerongridd {prefix}_` | Always — no server needed* |
| Live HTTP smoke tests (PowerShell) | `pwsh ./tests/kpi/scripts/run-{ws}-*.ps1 -BaseUrl http://127.0.0.1:8080` | Server must be running |
| Gate orchestrators | `pwsh ./tests/kpi/scripts/run-{ws}-gate.ps1` | Server needed for runtime packs |

\* WS3 tests (`ws3_`) require a live server — they will timeout without one.

## Starting the Server for Live Tests

```bash
cargo run -p voltnuerongridd
# Listens on http://127.0.0.1:8080
# Wait for /health to return 200 before running smoke scripts
```

## Test Naming Conventions

- `ws{N}_{feature}` — workstream integration tests
- `h{NN}_{feature}` — hardening tests
- `operator_auth_{scenario}` — RBAC operator tests
- `sql_runtime_{scenario}` — SQL endpoint runtime tests
- `ingest_{scenario}` — ingestion pipeline tests
- `store_{scenario}` — storage/index/constraint tests

## Gate Script Conventions

- Scripts: `tests/kpi/scripts/run-{wsN}-{name}-smoke.ps1` or `run-{wsN}-gate.ps1`
- Artifacts: `tests/kpi/results/{wsN}/{artifact-name}.json`
- Release summaries: `tests/kpi/results/gates/`
- **Never** infer gate success from `$LASTEXITCODE` in nested PowerShell calls — read the emitted artifact JSON `status` field
- Gate scripts use a `$packs` array; each pack emits `{status: "passed"|"failed", ...}`

## AppState in Tests

- Always use `state_with_key()` — never construct `AppState` directly
- When adding new `AppState` fields, update `state_with_key()` first, then write tests

## Known Failing Paths (2026-04-04)

- **WS1 live runtime smokes**: PowerShell HTTP invocation fails (`GetResponseStream` unavailable). Use `Invoke-RestMethod` or `Invoke-WebRequest` instead of creating `HttpClient` directly.
- **WS3 query routing tests**: timeout — server must be running AND HTAP routing (point→OLTP, aggregate→OLAP, mixed→hybrid) must be implemented.

## Adding a New Test

1. For a new endpoint: add a `ws{N}_{feature}` test in `services/voltnuerongridd/src/main.rs` under `#[cfg(test)]`
2. For a new gate artifact: create `tests/kpi/scripts/run-{ws}-{name}-smoke.ps1` emitting JSON
3. Wire the new smoke into `run-{ws}-gate.ps1` via the `$packs` array
4. Wire the gate script into `.github/workflows/ci.yml`
