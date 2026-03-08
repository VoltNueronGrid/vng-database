param(
  [string]$SummaryPath = "tests/kpi/results/ws8/ws8-gate-summary.json",
  [string]$AutonomyMatrixPath = "tests/kpi/results/ws8/ws8-autonomy-matrix.json",
  [string]$TrendPath = "tests/kpi/results/ws8/ws8-gate-trend-comparison.json",
  [string]$BadgePath = "tests/kpi/results/ws8/ws8-autonomy-stability-badge.json",
  [string]$OutputPath = "tests/kpi/results/gates/ws8-release-readiness.json"
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
foreach ($path in @($SummaryPath, $AutonomyMatrixPath, $TrendPath, $BadgePath)) {
  if (!(Test-Path -Path $path)) { throw "Required WS8 artifact missing at $path" }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$matrix = Get-Content -Raw -Path $AutonomyMatrixPath | ConvertFrom-Json
$trend = Get-Content -Raw -Path $TrendPath | ConvertFrom-Json
$badge = Get-Content -Raw -Path $BadgePath | ConvertFrom-Json

$checks = [ordered]@{
  ws8_gate_passed = ([string]$summary.status -eq "passed")
  ws8_autonomy_matrix_passed = ([string]$matrix.status -eq "passed")
  ws8_trend_not_regressed = ([string]$trend.trend_state -ne "regressed")
  ws8_badge_passed = ([string]$badge.status -eq "passed")
}
$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate = "ws8-release-autonomy-readiness"
  status = $status
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  release_targets = @("R3")
  scope = @("WS8", "REQ-29")
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  sources = [ordered]@{
    summary = $SummaryPath
    autonomy_matrix = $AutonomyMatrixPath
    trend_comparison = $TrendPath
    stability_badge = $BadgePath
  }
  checks = $checks
  highlights = [ordered]@{
    pack_count = @($summary.packs).Count
    autonomy_controls = [int]$matrix.total_controls
    ws8_runtime_pack_included = [bool]$matrix.runtime_pack_included
    ws8_runtime_pack_status = if ([bool]$matrix.runtime_pack_included) { [string](($matrix.matrix | Where-Object { $_.evidence_pack -eq "ws8-tenant-autonomous-runtime" } | Select-Object -First 1).status) } else { "not_included" }
    trend_state = [string]$trend.trend_state
    badge_message = [string]$badge.message
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS8 release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
