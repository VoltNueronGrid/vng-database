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

- Use `Invoke-RestMethod` or `Invoke-WebRequest` for HTTP calls — **not** `[System.Net.Http.HttpClient]` directly (missing `GetResponseStream` method)
- For background server launch use `Start-Process` with `-PassThru`, not detached `cmd.exe /c ...`
- On Windows, `$LASTEXITCODE` from `Invoke-Expression` or nested `pwsh -File` can be stale — always capture output text and parse JSON artifact status
- Use `-Encoding UTF8` when writing JSON artifacts

## Live Server Dependency

Scripts that call `http://127.0.0.1:8080/...` require `voltnuerongridd` to be running:
- In CI: wrap in a bash step that starts the server, waits for `/health`, then calls the script, and always kills the server in a `trap cleanup EXIT`
- Locally: run `cargo run -p voltnuerongridd` in a separate terminal first
