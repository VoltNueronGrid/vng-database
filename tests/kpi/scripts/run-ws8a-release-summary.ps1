param(
  [string]$SummaryPath = "tests/kpi/results/ws8a/ws8a-gate-summary.json",
  [string]$MatrixPath = "tests/kpi/results/ws8a/ws8a-agent-authoring-matrix.json",
  [string]$TrendPath = "tests/kpi/results/ws8a/ws8a-gate-trend-comparison.json",
  [string]$BadgePath = "tests/kpi/results/ws8a/ws8a-agent-stability-badge.json",
  [string]$OutputPath = "tests/kpi/results/gates/ws8a-release-readiness.json"
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
foreach ($path in @($SummaryPath, $MatrixPath, $TrendPath, $BadgePath)) {
  if (!(Test-Path -Path $path)) { throw "Required WS8A artifact missing at $path" }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$matrix = Get-Content -Raw -Path $MatrixPath | ConvertFrom-Json
$trend = Get-Content -Raw -Path $TrendPath | ConvertFrom-Json
$badge = Get-Content -Raw -Path $BadgePath | ConvertFrom-Json

$checks = [ordered]@{
  ws8a_gate_passed = ([string]$summary.status -eq "passed")
  ws8a_matrix_passed = ([string]$matrix.status -eq "passed")
  ws8a_trend_not_regressed = ([string]$trend.trend_state -ne "regressed")
  ws8a_badge_passed = ([string]$badge.status -eq "passed")
}
$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate = "ws8a-release-agent-authoring-readiness"
  status = $status
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  release_targets = @("R3")
  scope = @("WS8A", "REQ-30")
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  sources = [ordered]@{
    summary = $SummaryPath
    matrix = $MatrixPath
    trend = $TrendPath
    badge = $BadgePath
  }
  checks = $checks
  highlights = [ordered]@{
    pack_count = @($summary.packs).Count
    controls = [int]$matrix.total_controls
    trend_state = [string]$trend.trend_state
    badge_message = [string]$badge.message
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS8A release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
