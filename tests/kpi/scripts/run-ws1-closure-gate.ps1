param(
  [string]$OutputPath = "tests/kpi/results/ws1/ws1-closure-gate-summary.json"
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

$start = Get-Date
$ws1SummaryPath = "tests/kpi/results/ws1/ws1-gate-summary.json"
$ws1ReleasePath = "tests/kpi/results/gates/ws1-release-readiness.json"
$ws1CoveragePath = "tests/kpi/results/ws1/ws1-udf-coverage-matrix.json"
$ws1TrendPath = "tests/kpi/results/ws1/ws1-gate-trend-comparison.json"
$ws1BadgePath = "tests/kpi/results/ws1/ws1-udf-stability-badge.json"
$ws1RuntimeSmokePath = "tests/kpi/results/ws1/sql-execute-udf-smoke.json"

$runs = @()
$status = "passed"

try {
  $global:LASTEXITCODE = 0
  & "tests/kpi/scripts/run-ws1-gate.ps1" -OutputPath $ws1SummaryPath -ReleaseSummaryOutputPath $ws1ReleasePath 2>&1 | Out-Null
  if (-not $?) {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws1-gate"; status = "failed"; detail = "script_invocation_failed"; artifact = $ws1SummaryPath }
  } elseif ($global:LASTEXITCODE -ne 0) {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws1-gate"; status = "failed"; detail = "exit_code=$global:LASTEXITCODE"; artifact = $ws1SummaryPath }
  } else {
    $runs += [ordered]@{ pack = "ws1-gate"; status = "passed"; detail = "ok"; artifact = $ws1SummaryPath }
  }
} catch {
  $status = "failed"
  $runs += [ordered]@{ pack = "ws1-gate"; status = "failed"; detail = $_.Exception.Message; artifact = $ws1SummaryPath }
}

$checks = [ordered]@{
  ws1_gate_passed = $false
  ws1_release_summary_passed = $false
  ws1_coverage_controls_all_passed = $false
  ws1_trend_allowed = $false
  ws1_stability_badge_green = $false
  ws1_runtime_udf_smoke_passed = $false
}

if ($status -eq "passed") {
  foreach ($path in @($ws1SummaryPath, $ws1ReleasePath, $ws1CoveragePath, $ws1TrendPath, $ws1BadgePath, $ws1RuntimeSmokePath)) {
    if (!(Test-Path -Path $path)) {
      $status = "failed"
      $runs += [ordered]@{ pack = "ws1-artifact-presence"; status = "failed"; detail = "missing:$path"; artifact = $path }
    }
  }
}

if ($status -eq "passed") {
  $summary = Get-Content -Raw -Path $ws1SummaryPath | ConvertFrom-Json
  $release = Get-Content -Raw -Path $ws1ReleasePath | ConvertFrom-Json
  $coverage = Get-Content -Raw -Path $ws1CoveragePath | ConvertFrom-Json
  $trend = Get-Content -Raw -Path $ws1TrendPath | ConvertFrom-Json
  $badge = Get-Content -Raw -Path $ws1BadgePath | ConvertFrom-Json
  $runtime = Get-Content -Raw -Path $ws1RuntimeSmokePath | ConvertFrom-Json

  $checks.ws1_gate_passed = ([string]$summary.status -eq "passed")
  $checks.ws1_release_summary_passed = ([string]$release.status -eq "passed")
  $checks.ws1_coverage_controls_all_passed = ([int]$coverage.failed_controls -eq 0)
  $checks.ws1_trend_allowed = (@("stable", "improved", "baseline_established") -contains [string]$trend.trend_state)
  $checks.ws1_stability_badge_green = ([string]$badge.color -eq "green")
  $checks.ws1_runtime_udf_smoke_passed = ([string]$runtime.status -eq "passed")

  if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) { $status = "failed" }
}

$finished = Get-Date
$summaryOut = [ordered]@{
  gate = "ws1-closure-gate"
  status = $status
  validation_posture = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  artifacts = [ordered]@{
    ws1_gate = $ws1SummaryPath
    ws1_release = $ws1ReleasePath
    ws1_coverage = $ws1CoveragePath
    ws1_trend = $ws1TrendPath
    ws1_badge = $ws1BadgePath
    ws1_runtime_smoke = $ws1RuntimeSmokePath
  }
  checks = $checks
  runs = $runs
}

$summaryOut | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS1 closure gate summary: $OutputPath ($status)"
if ($status -eq "passed") {
  $outDir = Split-Path -Parent $OutputPath
  $ciMirror = Join-Path $outDir "ci-ws1-closure-gate-summary.json"
  if ($ciMirror -ne $OutputPath) {
    Copy-Item -LiteralPath $OutputPath -Destination $ciMirror -Force
    Write-Host "CI mirror: $ciMirror"
  }
}
if ($status -ne "passed") { exit 1 }
