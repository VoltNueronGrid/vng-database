param(
  [string]$SummaryPath = "tests/kpi/results/ws22/ws22-gate-summary.json",
  [string]$SmokePath = "tests/kpi/results/ws22/ws22-pessimistic-lock-smoke.json",
  [string]$TrendPath = "tests/kpi/results/ws22/ws22-gate-trend-comparison.json",
  [string]$BadgePath = "tests/kpi/results/ws22/ws22-pessimistic-lock-stability-badge.json",
  [string]$OutputPath = "tests/kpi/results/gates/ws22-release-readiness.json"
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
foreach ($path in @($SummaryPath, $SmokePath, $TrendPath, $BadgePath)) {
  if (!(Test-Path -Path $path)) { throw "Required WS22 artifact missing at $path" }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$smoke = Get-Content -Raw -Path $SmokePath | ConvertFrom-Json
$trend = Get-Content -Raw -Path $TrendPath | ConvertFrom-Json
$badge = Get-Content -Raw -Path $BadgePath | ConvertFrom-Json

$checks = [ordered]@{
  ws22_gate_passed = ([string]$summary.status -eq "passed")
  ws22_smoke_passed = ([string]$smoke.status -eq "passed")
  ws22_trend_not_regressed = ([string]$trend.trend_state -ne "regressed")
  ws22_badge_passed = ([string]$badge.status -eq "passed")
}
$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate = "ws22-release-lock-readiness"
  status = $status
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  release_targets = @("R1")
  scope = @("WS22", "REQ-22")
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  sources = [ordered]@{
    summary = $SummaryPath
    smoke = $SmokePath
    trend = $TrendPath
    badge = $BadgePath
  }
  checks = $checks
  highlights = [ordered]@{
    pack_count = @($summary.packs).Count
    trend_state = [string]$trend.trend_state
    badge_message = [string]$badge.message
    contract_checks = $smoke.contract_checks
    ws22_lock_contention_metrics = $summary.ws22_lock_contention_metrics
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS22 release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
