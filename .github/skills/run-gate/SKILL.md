---
name: run-gate
description: "Run and evaluate VoltNueronGrid gate scripts. Use when: running WS gate, running smoke tests, checking gate status, re-running a failed gate, checking release readiness, starting the server for live tests, evaluating test results."
argument-hint: "WS number or gate name, e.g. 'ws5' or 'ws3 with live server'"
---
# Run Gate Skill

Run any VoltNueronGrid workstream gate and evaluate its results.

## When to Use
- Running a specific WS gate script
- Interpreting gate artifact JSON status
- Deciding whether a gate is ready for validation
- Diagnosing a failed gate pack

## Procedure

### Step 1 — Determine gate requirements
- Does the gate need a live server? Check if it calls `$BaseUrl` or `/api/v1/...`
- If yes, start the server first (Step 2). Otherwise skip to Step 3.

### Step 2 — Start the server (if needed)
```bash
cargo run -p voltnuerongridd
# Wait for: curl http://127.0.0.1:8080/health returns 200
```

On Windows, launch via PowerShell to get a clean background process:
```powershell
$srv = Start-Process cargo -ArgumentList 'run','-p','voltnuerongridd' `
    -WorkingDirectory 'D:\by\polap-db' -PassThru
# Wait for /health
do { Start-Sleep 2 } until ((Invoke-RestMethod http://127.0.0.1:8080/health -Method Get 2>$null).status -eq 'ok')
```

### Step 3 — Run the gate
```powershell
pwsh ./tests/kpi/scripts/run-{wsN}-gate.ps1 `
    -BaseUrl "http://127.0.0.1:8080" `
    -OutputPath "./tests/kpi/results/{wsN}/{wsN}-gate-summary.json"
```

### Step 4 — Evaluate results
Read the gate artifact — do NOT rely on `$LASTEXITCODE`:
```powershell
$r = Get-Content ./tests/kpi/results/{wsN}/{wsN}-gate-summary.json | ConvertFrom-Json
$r.status          # "passed" or "failed"
$r.packs | Where-Object { $_.status -eq "failed" }  # failed packs
```

### Step 5 — Interpret failures
| Failed Pack | Likely Cause | Fix Path |
|-------------|-------------|----------|
| `ws3-query-routing` | HTAP routing not implemented / server not running | Implement point→OLTP, agg→OLAP routing + run with live server |
| `ws1-*-runtime-smoke` | PowerShell HTTP invocation error | Replace `HttpClient` with `Invoke-RestMethod` |
| `ws*-perf-score` | Score below threshold | See `ws3-performance-score.json` for control breakdown |
| Any gate pack | Server not running | Start `voltnuerongridd` first |

### Step 6 — Check release readiness
```powershell
$rr = Get-Content ./tests/kpi/results/gates/{wsN}-release-readiness.json | ConvertFrom-Json
$rr.release_readiness  # "ready_for_validation" or "blocked"
```

## Gate Script Locations
- Smoke scripts: `tests/kpi/scripts/run-{wsN}-{feature}-smoke.ps1`
- Gate orchestrators: `tests/kpi/scripts/run-{wsN}-gate.ps1`
- Artifacts: `tests/kpi/results/{wsN}/` and `tests/kpi/results/gates/`
