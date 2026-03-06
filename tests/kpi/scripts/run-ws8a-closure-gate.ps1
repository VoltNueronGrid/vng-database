param(
  [string]$OutputPath = "tests/kpi/results/ws8a/ws8a-closure-gate-summary.json"
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
$ws8aSummaryPath = "tests/kpi/results/ws8a/ws8a-gate-summary.json"
$ws8aReleasePath = "tests/kpi/results/gates/ws8a-release-readiness.json"
$ws8aMatrixPath = "tests/kpi/results/ws8a/ws8a-agent-authoring-matrix.json"
$ws8aTrendPath = "tests/kpi/results/ws8a/ws8a-gate-trend-comparison.json"
$ws8aBadgePath = "tests/kpi/results/ws8a/ws8a-agent-stability-badge.json"

$runs = @()
$status = "passed"

try {
  $global:LASTEXITCODE = 0
  & "tests/kpi/scripts/run-ws8a-gate.ps1" -OutputPath $ws8aSummaryPath -ReleaseSummaryOutputPath $ws8aReleasePath 2>&1 | Out-Null
  if (-not $?) {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws8a-gate"; status = "failed"; detail = "script_invocation_failed"; artifact = $ws8aSummaryPath }
  } elseif ($global:LASTEXITCODE -ne 0) {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws8a-gate"; status = "failed"; detail = "exit_code=$global:LASTEXITCODE"; artifact = $ws8aSummaryPath }
  } else {
    $runs += [ordered]@{ pack = "ws8a-gate"; status = "passed"; detail = "ok"; artifact = $ws8aSummaryPath }
  }
} catch {
  $status = "failed"
  $runs += [ordered]@{ pack = "ws8a-gate"; status = "failed"; detail = $_.Exception.Message; artifact = $ws8aSummaryPath }
}

$checks = [ordered]@{
  ws8a_gate_passed = $false
  ws8a_release_summary_passed = $false
  ws8a_all_packs_passed = $false
  ws8a_controls_all_passed = $false
  ws8a_trend_allowed = $false
  ws8a_stability_badge_green = $false
}

if ($status -eq "passed") {
  foreach ($path in @($ws8aSummaryPath, $ws8aReleasePath, $ws8aMatrixPath, $ws8aTrendPath, $ws8aBadgePath)) {
    if (!(Test-Path -Path $path)) {
      $status = "failed"
      $runs += [ordered]@{ pack = "ws8a-artifact-presence"; status = "failed"; detail = "missing:$path"; artifact = $path }
    }
  }
}

if ($status -eq "passed") {
  $summary = Get-Content -Raw -Path $ws8aSummaryPath | ConvertFrom-Json
  $release = Get-Content -Raw -Path $ws8aReleasePath | ConvertFrom-Json
  $matrix = Get-Content -Raw -Path $ws8aMatrixPath | ConvertFrom-Json
  $trend = Get-Content -Raw -Path $ws8aTrendPath | ConvertFrom-Json
  $badge = Get-Content -Raw -Path $ws8aBadgePath | ConvertFrom-Json

  $checks.ws8a_gate_passed = ([string]$summary.status -eq "passed")
  $checks.ws8a_release_summary_passed = ([string]$release.status -eq "passed")
  $checks.ws8a_all_packs_passed = ((@($summary.packs | Where-Object { $_.status -ne "passed" }).Count) -eq 0)
  $checks.ws8a_controls_all_passed = ([int]$matrix.failed_controls -eq 0)
  $checks.ws8a_trend_allowed = (@("stable", "improved", "baseline_established") -contains [string]$trend.trend_state)
  $checks.ws8a_stability_badge_green = ([string]$badge.color -eq "green")

  if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) { $status = "failed" }
}

$finished = Get-Date
$summaryOut = [ordered]@{
  gate = "ws8a-closure-gate"
  status = $status
  validation_posture = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  artifacts = [ordered]@{
    ws8a_gate = $ws8aSummaryPath
    ws8a_release = $ws8aReleasePath
    ws8a_matrix = $ws8aMatrixPath
    ws8a_trend = $ws8aTrendPath
    ws8a_badge = $ws8aBadgePath
  }
  checks = $checks
  runs = $runs
}

$summaryOut | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS8A closure gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
