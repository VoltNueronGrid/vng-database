---
name: workstream-scaffold
description: "Scaffold a new VoltNueronGrid workstream. Use when: adding a new WS, creating a new smoke script, creating a new gate orchestrator, creating a new release readiness script, wiring a new gate into CI."
argument-hint: "WS number and name, e.g. 'WS16 caching layer'"
---
# Workstream Scaffold Skill

Scaffold all required artifacts for a new VoltNueronGrid workstream following project conventions.

## When to Use
- Creating a new WS (workstream) with gate script coverage
- Adding a new smoke pack to an existing WS
- Wiring a new gate into `.github/workflows/ci.yml`

## Required Artifacts per Workstream

For a new `WS{N}` workstream named `{feature}`:

| File | Purpose |
|------|---------|
| `tests/kpi/scripts/run-ws{N}-{feature}-smoke.ps1` | Smoke pack |
| `tests/kpi/scripts/run-ws{N}-gate.ps1` | Gate orchestrator |
| `tests/kpi/scripts/run-ws{N}-release-summary.ps1` | Release readiness |
| `tests/kpi/results/ws{N}/` | Output directory (add a `.gitkeep`) |

## Procedure

### Step 1 — Create smoke script
File: `tests/kpi/scripts/run-ws{N}-{feature}-smoke.ps1`

```powershell
param(
    [string]$BaseUrl = "http://127.0.0.1:8080",
    [string]$OutputPath = "tests/kpi/results/ws{N}/ws{N}-{feature}-smoke.json"
)
$checks = @()
# Add checks here — each emits { name, passed, detail }
$passed = ($checks | Where-Object { -not $_.passed }).Count -eq 0
$result = @{
    smoke           = "ws{N}-{feature}"
    status          = if ($passed) { "passed" } else { "failed" }
    checks_passed   = ($checks | Where-Object { $_.passed }).Count
    checks_total    = $checks.Count
    generated_at_utc = (Get-Date -Format "o")
    checks          = $checks
}
$result | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath -Encoding UTF8
if (-not $passed) { exit 1 }
```

### Step 2 — Create gate orchestrator
File: `tests/kpi/scripts/run-ws{N}-gate.ps1`

```powershell
param(
    [string]$BaseUrl = "http://127.0.0.1:8080",
    [string]$OutputPath = "tests/kpi/results/ws{N}/ws{N}-gate-summary.json"
)
$startTime = Get-Date
$packs = @()
# Pack 1: unit tests
$output = & cargo test -p voltnuerongridd ws{N}_ 2>&1
$packs += @{ pack="ws{N}-unit-tests"; status=if($LASTEXITCODE -eq 0){"passed"}else{"failed"}; detail=$output[-1] }
# Pack 2: smoke script
& "$PSScriptRoot/run-ws{N}-{feature}-smoke.ps1" -BaseUrl $BaseUrl -OutputPath "tests/kpi/results/ws{N}/ws{N}-{feature}-smoke.json"
$smokeResult = Get-Content "tests/kpi/results/ws{N}/ws{N}-{feature}-smoke.json" | ConvertFrom-Json
$packs += @{ pack="ws{N}-{feature}-smoke"; status=$smokeResult.status; artifact="tests/kpi/results/ws{N}/ws{N}-{feature}-smoke.json" }
# Derive overall status from pack statuses
$overallStatus = if ($packs | Where-Object { $_.status -eq "failed" }) { "failed" } else { "passed" }
$summary = @{
    gate="ws{N}"; status=$overallStatus
    started_at_utc=$startTime.ToString("o"); finished_at_utc=(Get-Date -Format "o")
    duration_ms=[int]((Get-Date)-$startTime).TotalMilliseconds; packs=$packs
}
$summary | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath -Encoding UTF8
if ($overallStatus -eq "failed") { exit 1 }
```

### Step 3 — Wire into CI
Add a step to `.github/workflows/ci.yml`:

```yaml
- name: Run WS{N} {feature} gate
  shell: pwsh
  run: ./tests/kpi/scripts/run-ws{N}-gate.ps1 -OutputPath "./tests/kpi/results/ws{N}/ci-ws{N}-gate-summary.json"
```

### Step 4 — Add tracker row
Add a row to `status_tracker.md` section 4:

```
| WS{N} | Epic {M} | {feature description} | {Owner Team} | In Progress | WS{prev} |
```

### Step 5 — Create results directory
```powershell
New-Item -ItemType Directory -Force "tests/kpi/results/ws{N}"
New-Item -ItemType File -Force "tests/kpi/results/ws{N}/.gitkeep"
```
