param(
  [string]$OutputPath = "tests/kpi/results/ws8/ws8-gate-summary.json",
  [string]$ReleaseSummaryOutputPath = "tests/kpi/results/gates/ws8-release-readiness.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

Ensure-OutputDir -PathValue $OutputPath
Ensure-OutputDir -PathValue $ReleaseSummaryOutputPath

$summaryPath = "tests/kpi/results/ws8/ws8-gate-summary.json"
$summaryParent = Split-Path -Parent $summaryPath
if (!(Test-Path -Path $summaryParent)) { New-Item -Path $summaryParent -ItemType Directory -Force | Out-Null }
$previousSummaryPath = "tests/kpi/results/ws8/ws8-gate-summary.previous.json"
if (Test-Path -Path $summaryPath) { Copy-Item -Path $summaryPath -Destination $previousSummaryPath -Force }

$packs = @(
  [ordered]@{
    Name = "ws8-control-plane"
    Script = "tests/kpi/scripts/run-ws8-control-plane-smoke.ps1"
    Runner = {
      & "tests/kpi/scripts/run-ws8-control-plane-smoke.ps1" -OutputPath "tests/kpi/results/ws8/control-plane-smoke.json"
    }
  },
  [ordered]@{
    Name = "ws8-guardrail-policy"
    Script = "tests/kpi/scripts/run-ws8-guardrail-policy-smoke.ps1"
    Runner = {
      & "tests/kpi/scripts/run-ws8-guardrail-policy-smoke.ps1" -OutputPath "tests/kpi/results/ws8/ws8-guardrail-policy-smoke.json"
    }
  },
  [ordered]@{
    Name = "ws8-mode-governance"
    Script = "tests/kpi/scripts/run-ws8-mode-governance-smoke.ps1"
    Runner = {
      & "tests/kpi/scripts/run-ws8-mode-governance-smoke.ps1" -OutputPath "tests/kpi/results/ws8/ws8-mode-governance-smoke.json"
    }
  },
  [ordered]@{
    Name = "ws8a-audit-trail"
    Script = "tests/kpi/scripts/run-ws8a-audit-smoke.ps1"
    Runner = {
      & "tests/kpi/scripts/run-ws8a-audit-smoke.ps1" -OutputPath "tests/kpi/results/ws8a/audit-trail-smoke.json"
    }
  },
  [ordered]@{
    Name = "ws8a-audit-companion"
    Script = "tests/kpi/scripts/run-ws8a-audit-companion-smoke.ps1"
    Runner = {
      & "tests/kpi/scripts/run-ws8a-audit-companion-smoke.ps1" `
        -OutputPath "tests/kpi/results/ws8a/audit-companion-smoke.json" `
        -ReportPath "tests/kpi/results/ws8a/audit-companion-report.json"
    }
  }
)

$start = Get-Date
$results = @()
foreach ($pack in $packs) {
  $packStart = Get-Date
  & $pack.Runner
  $packExit = $LASTEXITCODE
  $packEnd = Get-Date
  $results += [ordered]@{
    pack = $pack.Name
    script = $pack.Script
    status = if ($packExit -eq 0) { "passed" } else { "failed" }
    started_at_utc = $packStart.ToUniversalTime().ToString("o")
    finished_at_utc = $packEnd.ToUniversalTime().ToString("o")
    duration_ms = [int](($packEnd - $packStart).TotalMilliseconds)
  }
}

$overall = if ((@($results | Where-Object { $_.status -ne "passed" }).Count) -eq 0) { "passed" } else { "failed" }
$end = Get-Date
$summary = [ordered]@{
  gate = "ws8-autonomous-control-plane"
  status = $overall
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $end.ToUniversalTime().ToString("o")
  duration_ms = [int](($end - $start).TotalMilliseconds)
  packs = $results
}

$summary | ConvertTo-Json -Depth 10 | Set-Content -Path $summaryPath
$summary | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
if ($OutputPath -ne $summaryPath) {
  Copy-Item -Path $summaryPath -Destination $OutputPath -Force
}

& "tests/kpi/scripts/run-ws8-autonomy-matrix-export.ps1" `
  -SummaryPath $summaryPath `
  -OutputPath "tests/kpi/results/ws8/ws8-autonomy-matrix.json"
& "tests/kpi/scripts/run-ws8-gate-trend-compare.ps1" `
  -CurrentSummaryPath $summaryPath `
  -PriorSummaryPath $previousSummaryPath `
  -OutputPath "tests/kpi/results/ws8/ws8-gate-trend-comparison.json"
& "tests/kpi/scripts/run-ws8-autonomy-stability-badge.ps1" `
  -SummaryPath $summaryPath `
  -TrendPath "tests/kpi/results/ws8/ws8-gate-trend-comparison.json" `
  -OutputPath "tests/kpi/results/ws8/ws8-autonomy-stability-badge.json"
& "tests/kpi/scripts/run-ws8-release-summary.ps1" `
  -SummaryPath $summaryPath `
  -AutonomyMatrixPath "tests/kpi/results/ws8/ws8-autonomy-matrix.json" `
  -TrendPath "tests/kpi/results/ws8/ws8-gate-trend-comparison.json" `
  -BadgePath "tests/kpi/results/ws8/ws8-autonomy-stability-badge.json" `
  -OutputPath $ReleaseSummaryOutputPath

Write-Host "WS8 gate summary: $OutputPath ($overall)"
if ($overall -ne "passed") { exit 1 }
