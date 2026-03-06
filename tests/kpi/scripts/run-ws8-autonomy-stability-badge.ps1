param(
  [string]$SummaryPath = "tests/kpi/results/ws8/ws8-gate-summary.json",
  [string]$TrendPath = "tests/kpi/results/ws8/ws8-gate-trend-comparison.json",
  [string]$OutputPath = "tests/kpi/results/ws8/ws8-autonomy-stability-badge.json"
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
if (!(Test-Path -Path $SummaryPath)) { throw "WS8 summary not found at $SummaryPath" }
if (!(Test-Path -Path $TrendPath)) { throw "WS8 trend comparison not found at $TrendPath" }

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$trend = Get-Content -Raw -Path $TrendPath | ConvertFrom-Json
$total = @($summary.packs).Count
$passed = @($summary.packs | Where-Object { $_.status -eq "passed" }).Count
$trendState = [string]$trend.trend_state
$color = if ([string]$summary.status -eq "passed" -and (@("stable","improved","baseline_established") -contains $trendState)) { "green" } elseif ($trendState -eq "regressed") { "red" } else { "yellow" }
$message = "$passed/$total $trendState"
$badge = [ordered]@{
  label = "ws8-autonomy-stability"
  message = $message
  color = $color
  status = if ($color -eq "red") { "failed" } else { "passed" }
  source_summary = $SummaryPath
  source_trend = $TrendPath
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
}
$badge | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "WS8 autonomy stability badge artifact: $OutputPath ($message)"
if ($badge.status -ne "passed") { exit 1 }
