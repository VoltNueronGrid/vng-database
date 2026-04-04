---
description: "Use when writing or editing PowerShell gate scripts, smoke scripts, release summaries, CI gate steps, or any tests/kpi/scripts file. Covers packs pattern, artifact schema, status derivation, and Windows compatibility."
applyTo: "tests/kpi/scripts/**"
---
# VoltNueronGrid DB — Gate Script Conventions

## Script Types

| Type | Naming Pattern | Artifact Path |
|------|---------------|---------------|
| Smoke pack | `run-{wsN}-{feature}-smoke.ps1` | `tests/kpi/results/{wsN}/{feature}-smoke.json` |
| Gate orchestrator | `run-{wsN}-gate.ps1` | `tests/kpi/results/{wsN}/{wsN}-gate-summary.json` |
| Closure gate | `run-{wsN}-closure-gate.ps1` | `tests/kpi/results/{wsN}/{wsN}-closure-gate-summary.json` |
| Release summary | `run-{wsN}-release-summary.ps1` | `tests/kpi/results/gates/{wsN}-release-readiness.json` |
| CI variant | `ci-` prefix on artifact name | `tests/kpi/results/gates/ci-{name}.json` |

## Gate Orchestrator Pattern (`$packs` array)

```powershell
$packs = @()

# Run a smoke pack
$result = & "$PSScriptRoot/run-wsN-feature-smoke.ps1" -BaseUrl $BaseUrl -OutputPath $SomePath
$packs += @{ pack = "wsN-feature"; status = if ($LASTEXITCODE -eq 0) { "passed" } else { "failed" }; detail = "..." }

# Derive overall gate status from pack statuses — do NOT use $LASTEXITCODE from nested calls
$overallStatus = if ($packs | Where-Object { $_.status -eq "failed" }) { "failed" } else { "passed" }

$summary = @{
    gate             = "wsN"
    status           = $overallStatus
    started_at_utc   = $startTime
    finished_at_utc  = (Get-Date -Format "o")
    duration_ms      = [int]((Get-Date) - $startTime).TotalMilliseconds
    packs            = $packs
}
$summary | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath -Encoding UTF8
```

**Critical**: Derive `status` from the `$packs` array result values — **not** `$LASTEXITCODE` from the child PowerShell call, which can inherit stale values.

## Smoke Artifact Schema

```json
{
  "smoke": "wsN-feature-name",
  "status": "passed | failed",
  "checks_passed": 7,
  "checks_total": 8,
  "generated_at_utc": "2026-...",
  "checks": [ { "name": "...", "passed": true, "detail": "..." } ]
}
```

## Release Readiness Artifact Schema

```json
{
  "gate": "wsN-release-readiness",
  "status": "passed | failed",
  "release_readiness": "ready_for_validation | blocked",
  "release_targets": ["R1"],
  "generated_at_utc": "2026-...",
  "sources": { "summary": "tests/kpi/results/wsN/wsN-gate-summary.json" },
  "checks": { "wsN_gate_passed": true },
  "highlights": { }
}
```

## Windows Compatibility Notes

- For JSON HTTP from smoke scripts, dot-source `tests/kpi/scripts/kpi-http-helpers.ps1` (`Invoke-HttpJson`). Simple probes may use `Invoke-RestMethod` / `Invoke-WebRequest` — **never** read error bodies via `GetResponseStream()` on PS7 `Invoke-WebRequest` failures, and avoid raw `HttpClient` in scripts
- When a closure gate uses `Start-Process` to run another gate script, set `-WorkingDirectory` to the **repository root** so `cargo` and relative artifact paths resolve; prefer `pwsh` when available
- On success, `run-ws1-closure-gate.ps1`, `run-ws22-closure-gate.ps1`, and `run-release-r1-sql-udf-gate.ps1` **mirror** the canonical `ci-*` filenames next to the default artifact when `-OutputPath` is not already the CI path (matches `.github/workflows/ci.yml` uploads). The same mirror pattern applies to **R2** (`run-release-r2-failover-gate.ps1`, `run-release-ops-resilience-gate.ps1`) and **R3** (`run-release-r3-*.ps1` autonomous/plugin/agent-authoring/udf-runtime gates); see the “Release gate CI mirrors (R2 / R3)” section below.
- For background server launch use `Start-Process` with `-PassThru`, not detached `cmd.exe /c ...`
- On Windows, `$LASTEXITCODE` from `Invoke-Expression` or nested `pwsh -File` can be stale — always capture output text and parse JSON artifact status
- Use `-Encoding UTF8` when writing JSON artifacts

## Live Server Dependency

Scripts that call `http://127.0.0.1:8080/...` require `voltnuerongridd` to be running:
- In CI: wrap in a bash step that starts the server, waits for `/health`, then calls the script, and always kills the server in a `trap cleanup EXIT`
- Locally: run `cargo run -p voltnuerongridd` in a separate terminal first

## WS1A legacy numeric evaluation smoke (no HTTP)

- **Script:** `tests/kpi/scripts/run-ws1a-legacy-numeric-eval-smoke.ps1`
- **Default artifact:** `tests/kpi/results/ws1a/ws1a-legacy-numeric-eval-smoke.json`
- **Behavior:** Asserts `eval_legacy_numeric_aggregation` exists in `crates/voltnuerongrid-sql/src/legacy_aggregations.rs`, then runs `cargo test -p voltnuerongrid-sql legacy_aggregations::numeric_tests`
- **Gate wiring:** Invoked as a pack from `tests/kpi/scripts/run-ws1a-gate.ps1` (alongside parity/gap/UDF bridge packs). CI runs the full WS1A gate with `-OutputPath "./tests/kpi/results/ws1a/ci-ws1a-gate-summary.json"`; pack artifacts still land under `tests/kpi/results/ws1a/` unless a script overrides `-OutputPath`
- **Parameters:** `-RepoRoot` (default `"."`) — must be the repository root when not running from CI

## Parquet / Excel ingest HTTP contract (base64 body)

These routes use the same **ingest runtime RBAC** as CSV/JSON (`ingest.connectors` resource, `require_ingest_runtime_privilege`, tenant or operator headers per matrix).

| Method | Path | JSON body fields | Notes |
|--------|------|------------------|--------|
| `POST` | `/api/v1/ingest/parquet` | `connector_id` (string), `parquet_data_base64` (string) | Payload must be **standard base64** (RFC 4648) of a Parquet file. Decode failures → `400` with `reason: invalid_base64_payload`. Parse failures → `400` with `reason: parquet_parse_failed`. |
| `POST` | `/api/v1/ingest/excel` | `connector_id` (string), `xlsx_data_base64` (string) | **`.xlsx` only** (first worksheet). Decode failures → `400` `invalid_base64_payload`. Parse failures → `400` `excel_parse_failed`. |

- **Status counts:** `GET /api/v1/ingest/status` includes `parquet_connectors`, `excel_connectors`, and `total_records_loaded` sums CSV + JSON + Parquet + Excel for the caller’s scope.
- **Smokes:** Parser/contract checks (no live HTTP) are in `tests/kpi/scripts/run-ws4-ingest-parser-smoke.ps1` (includes Parquet/Excel module/route/status-field checks). Live HTTP coverage for binary formats can be added later with small fixture base64 or file upload.

## Release gate CI mirrors (R2 / R3)

On **passed** runs, these scripts copy the written summary to the `ci-*` filename next to it when `-OutputPath` is not already the CI path (same pattern as R1 / WS1 closure):

- `run-release-r2-failover-gate.ps1` → `ci-release-r2-failover-readiness.json`
- `run-release-ops-resilience-gate.ps1` → `ci-release-ops-resilience-readiness.json`
- `run-release-r3-autonomous-gate.ps1`, `run-release-r3-plugin-gate.ps1`, `run-release-r3-agent-authoring-gate.ps1`, `run-release-r3-udf-runtime-gate.ps1` → matching `ci-release-r3-*.json`
