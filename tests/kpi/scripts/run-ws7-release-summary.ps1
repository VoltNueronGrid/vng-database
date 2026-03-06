param(
  [string]$SummaryPath = "tests/kpi/results/ws7/ws7-gate-summary.json",
  [string]$ComplianceMatrixPath = "tests/kpi/results/ws7/ws7-compliance-matrix.json",
  [string]$TrendPath = "tests/kpi/results/ws7/ws7-gate-trend-comparison.json",
  [string]$BadgePath = "tests/kpi/results/ws7/ws7-plugin-stability-badge.json",
  [string]$OutputPath = "tests/kpi/results/gates/ws7-release-readiness.json"
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
foreach ($path in @($SummaryPath, $ComplianceMatrixPath, $TrendPath, $BadgePath)) {
  if (!(Test-Path -Path $path)) { throw "Required WS7 artifact missing at $path" }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$matrix = Get-Content -Raw -Path $ComplianceMatrixPath | ConvertFrom-Json
$trend = Get-Content -Raw -Path $TrendPath | ConvertFrom-Json
$badge = Get-Content -Raw -Path $BadgePath | ConvertFrom-Json

$checks = [ordered]@{
  ws7_gate_passed = ([string]$summary.status -eq "passed")
  ws7_compliance_matrix_passed = ([string]$matrix.status -eq "passed")
  ws7_trend_not_regressed = ([string]$trend.trend_state -ne "regressed")
  ws7_badge_passed = ([string]$badge.status -eq "passed")
}

$status = if (($checks.Values | Where-Object { $_ -eq $false }).Count -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  gate = "ws7-release-plugin-readiness"
  status = $status
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  release_targets = @("R3")
  scope = @("WS7", "REQ-09", "REQ-26")
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  sources = [ordered]@{
    summary = $SummaryPath
    compliance_matrix = $ComplianceMatrixPath
    trend_comparison = $TrendPath
    stability_badge = $BadgePath
  }
  checks = $checks
  highlights = [ordered]@{
    pack_count = @($summary.packs).Count
    compliance_controls = [int]$matrix.total_controls
    trend_state = [string]$trend.trend_state
    badge_message = [string]$badge.message
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS7 release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
