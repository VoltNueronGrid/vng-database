param(
  [string]$OutputPath = "tests/kpi/results/ws7/ws7-closure-gate-summary.json"
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
$ws7SummaryPath = "tests/kpi/results/ws7/ws7-gate-summary.json"
$ws7ReleasePath = "tests/kpi/results/gates/ws7-release-readiness.json"
$ws7MatrixPath = "tests/kpi/results/ws7/ws7-compliance-matrix.json"
$ws7TrendPath = "tests/kpi/results/ws7/ws7-gate-trend-comparison.json"
$ws7BadgePath = "tests/kpi/results/ws7/ws7-plugin-stability-badge.json"

$runs = @()
$status = "passed"

try {
  $global:LASTEXITCODE = 0
  & "tests/kpi/scripts/run-ws7-gate.ps1" -OutputPath $ws7SummaryPath -ReleaseSummaryOutputPath $ws7ReleasePath 2>&1 | Out-Null
  if (-not $?) {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws7-gate"; status = "failed"; detail = "script_invocation_failed"; artifact = $ws7SummaryPath }
  } elseif ($global:LASTEXITCODE -ne 0) {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws7-gate"; status = "failed"; detail = "exit_code=$global:LASTEXITCODE"; artifact = $ws7SummaryPath }
  } else {
    $runs += [ordered]@{ pack = "ws7-gate"; status = "passed"; detail = "ok"; artifact = $ws7SummaryPath }
  }
} catch {
  $status = "failed"
  $runs += [ordered]@{ pack = "ws7-gate"; status = "failed"; detail = $_.Exception.Message; artifact = $ws7SummaryPath }
}

$checks = [ordered]@{
  ws7_gate_passed = $false
  ws7_release_summary_passed = $false
  ws7_all_packs_passed = $false
  ws7_compliance_controls_all_passed = $false
  ws7_trend_allowed = $false
  ws7_stability_badge_green = $false
}

if ($status -eq "passed") {
  foreach ($path in @($ws7SummaryPath, $ws7ReleasePath, $ws7MatrixPath, $ws7TrendPath, $ws7BadgePath)) {
    if (!(Test-Path -Path $path)) {
      $status = "failed"
      $runs += [ordered]@{ pack = "ws7-artifact-presence"; status = "failed"; detail = "missing:$path"; artifact = $path }
    }
  }
}

if ($status -eq "passed") {
  $summary = Get-Content -Raw -Path $ws7SummaryPath | ConvertFrom-Json
  $release = Get-Content -Raw -Path $ws7ReleasePath | ConvertFrom-Json
  $matrix = Get-Content -Raw -Path $ws7MatrixPath | ConvertFrom-Json
  $trend = Get-Content -Raw -Path $ws7TrendPath | ConvertFrom-Json
  $badge = Get-Content -Raw -Path $ws7BadgePath | ConvertFrom-Json

  $checks.ws7_gate_passed = ([string]$summary.status -eq "passed")
  $checks.ws7_release_summary_passed = ([string]$release.status -eq "passed")
  $checks.ws7_all_packs_passed = ((@($summary.packs | Where-Object { $_.status -ne "passed" }).Count) -eq 0)
  $checks.ws7_compliance_controls_all_passed = ([int]$matrix.failed_controls -eq 0)
  $checks.ws7_trend_allowed = (@("stable", "improved", "baseline_established") -contains [string]$trend.trend_state)
  $checks.ws7_stability_badge_green = ([string]$badge.color -eq "green")

  if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) { $status = "failed" }
}

$finished = Get-Date
$summaryOut = [ordered]@{
  gate = "ws7-closure-gate"
  status = $status
  validation_posture = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  artifacts = [ordered]@{
    ws7_gate = $ws7SummaryPath
    ws7_release = $ws7ReleasePath
    ws7_compliance_matrix = $ws7MatrixPath
    ws7_trend = $ws7TrendPath
    ws7_badge = $ws7BadgePath
  }
  checks = $checks
  runs = $runs
}

$summaryOut | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS7 closure gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
