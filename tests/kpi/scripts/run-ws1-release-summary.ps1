param(
  [string]$SummaryPath = "tests/kpi/results/ws1/ws1-gate-summary.json",
  [string]$CoverageMatrixPath = "tests/kpi/results/ws1/ws1-udf-coverage-matrix.json",
  [string]$TrendPath = "tests/kpi/results/ws1/ws1-gate-trend-comparison.json",
  [string]$BadgePath = "tests/kpi/results/ws1/ws1-udf-stability-badge.json",
  [string]$OutputPath = "tests/kpi/results/gates/ws1-release-readiness.json"
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
foreach ($path in @($SummaryPath, $CoverageMatrixPath, $TrendPath, $BadgePath)) {
  if (!(Test-Path -Path $path)) { throw "Required WS1 artifact missing at $path" }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$matrix = Get-Content -Raw -Path $CoverageMatrixPath | ConvertFrom-Json
$trend = Get-Content -Raw -Path $TrendPath | ConvertFrom-Json
$badge = Get-Content -Raw -Path $BadgePath | ConvertFrom-Json

$checks = [ordered]@{
  ws1_gate_passed = ([string]$summary.status -eq "passed")
  ws1_udf_coverage_passed = ([string]$matrix.status -eq "passed")
  ws1_trend_not_regressed = ([string]$trend.trend_state -ne "regressed")
  ws1_badge_passed = ([string]$badge.status -eq "passed")
}
$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate = "ws1-release-udf-readiness"
  status = $status
  release_readiness = if ($status -eq "passed") { "in_progress_with_evidence" } else { "blocked" }
  release_targets = @("R1", "R3")
  scope = @("WS1", "REQ-03")
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  sources = [ordered]@{
    summary = $SummaryPath
    coverage_matrix = $CoverageMatrixPath
    trend = $TrendPath
    badge = $BadgePath
  }
  checks = $checks
  highlights = [ordered]@{
    pack_count = @($summary.packs).Count
    coverage_controls = [int]$matrix.total_controls
    trend_state = [string]$trend.trend_state
    badge_message = [string]$badge.message
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS1 release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
