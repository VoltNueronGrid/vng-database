param(
  [string]$SummaryPath = "tests/kpi/results/ws6/ws6-gate-summary.json",
  [string]$TrendPath = "tests/kpi/results/ws6/ws6-gate-trend-comparison.json",
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-failover-stability-badge.json"
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

if (!(Test-Path -Path $SummaryPath)) {
  throw "WS6 summary not found at $SummaryPath"
}
if (!(Test-Path -Path $TrendPath)) {
  throw "WS6 trend comparison not found at $TrendPath"
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$trend = Get-Content -Raw -Path $TrendPath | ConvertFrom-Json

$totalPacks = @($summary.packs).Count
$passedPacks = @($summary.packs | Where-Object { $_.status -eq "passed" }).Count
$gateStatus = [string]$summary.status
$trendState = [string]$trend.trend_state

$color = "yellow"
if ($gateStatus -eq "passed" -and ($trendState -eq "stable" -or $trendState -eq "improved")) {
  $color = "green"
} elseif ($gateStatus -eq "failed" -or $trendState -eq "regressed") {
  $color = "red"
}

$message = "$passedPacks/$totalPacks $trendState"

$badge = [ordered]@{
  label = "ws6-failover-stability"
  message = $message
  color = $color
  status = if ($color -eq "red") { "failed" } else { "passed" }
  source_summary = $SummaryPath
  source_trend = $TrendPath
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
}

$badge | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "WS6 failover stability badge artifact: $OutputPath ($message)"
if ($badge.status -ne "passed") { exit 1 }
