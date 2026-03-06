param(
  [string]$SummaryPath = "tests/kpi/results/ws3/ws3-gate-summary.json",
  [string]$ScorePath = "tests/kpi/results/ws3/ws3-performance-score.json",
  [string]$TrendPath = "tests/kpi/results/ws3/ws3-gate-trend-comparison.json",
  [string]$BadgePath = "tests/kpi/results/ws3/ws3-performance-stability-badge.json",
  [string]$OutputPath = "tests/kpi/results/gates/ws3-release-readiness.json"
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
foreach ($path in @($SummaryPath, $ScorePath, $TrendPath, $BadgePath)) {
  if (!(Test-Path -Path $path)) { throw "Required WS3 artifact missing at $path" }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$score = Get-Content -Raw -Path $ScorePath | ConvertFrom-Json
$trend = Get-Content -Raw -Path $TrendPath | ConvertFrom-Json
$badge = Get-Content -Raw -Path $BadgePath | ConvertFrom-Json

$checks = [ordered]@{
  ws3_gate_passed = ([string]$summary.status -eq "passed")
  ws3_score_passed = ([string]$score.status -eq "passed")
  ws3_trend_not_regressed = ([string]$trend.trend_state -ne "regressed")
  ws3_badge_passed = ([string]$badge.status -eq "passed")
}
$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate = "ws3-release-performance-readiness"
  status = $status
  release_readiness = if ($status -eq "passed") { "in_progress_with_evidence" } else { "blocked" }
  release_targets = @("R3", "R4")
  scope = @("WS3", "REQ-31")
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  sources = [ordered]@{
    summary = $SummaryPath
    score = $ScorePath
    trend = $TrendPath
    badge = $BadgePath
  }
  checks = $checks
  highlights = [ordered]@{
    pack_count = @($summary.packs).Count
    score = [int]$score.score
    trend_state = [string]$trend.trend_state
    badge_message = [string]$badge.message
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS3 release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
