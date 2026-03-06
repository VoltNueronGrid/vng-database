param(
  [string]$SummaryPath = "tests/kpi/results/ws6/ws6-gate-summary.json",
  [string]$ChaosMatrixPath = "tests/kpi/results/ws6/ws6-chaos-fault-matrix.json",
  [string]$TrendPath = "tests/kpi/results/ws6/ws6-gate-trend-comparison.json",
  [string]$BadgePath = "tests/kpi/results/ws6/ws6-failover-stability-badge.json",
  [string]$OutputPath = "tests/kpi/results/gates/ws6-release-readiness.json"
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

foreach ($path in @($SummaryPath, $ChaosMatrixPath, $TrendPath, $BadgePath)) {
  if (!(Test-Path -Path $path)) {
    throw "Required WS6 artifact missing at $path"
  }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$chaos = Get-Content -Raw -Path $ChaosMatrixPath | ConvertFrom-Json
$trend = Get-Content -Raw -Path $TrendPath | ConvertFrom-Json
$badge = Get-Content -Raw -Path $BadgePath | ConvertFrom-Json

$checks = [ordered]@{
  ws6_gate_passed = ([string]$summary.status -eq "passed")
  chaos_fault_matrix_passed = ([string]$chaos.status -eq "passed")
  trend_not_regressed = ([string]$trend.trend_state -ne "regressed")
  stability_badge_passed = ([string]$badge.status -eq "passed")
}

$status = if (($checks.Values | Where-Object { $_ -eq $false }).Count -eq 0) { "passed" } else { "failed" }
$releaseReadiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }

$artifact = [ordered]@{
  gate = "ws6-release-failover-readiness"
  status = $status
  release_readiness = $releaseReadiness
  release_targets = @("R2")
  scope = @("WS6", "REQ-17")
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  sources = [ordered]@{
    summary = $SummaryPath
    chaos_fault_matrix = $ChaosMatrixPath
    trend_comparison = $TrendPath
    stability_badge = $BadgePath
  }
  checks = $checks
  highlights = [ordered]@{
    pack_count = @($summary.packs).Count
    chaos_fault_modes = [int]$chaos.total_fault_modes
    chaos_passed_modes = [int]$chaos.passed_modes
    trend_state = [string]$trend.trend_state
    badge_message = [string]$badge.message
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS6 release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
