param(
  [string]$CurrentSummaryPath = "tests/kpi/results/ws7/ws7-gate-summary.json",
  [string]$PriorSummaryPath = "tests/kpi/results/ws7/ws7-gate-summary.previous.json",
  [string]$OutputPath = "tests/kpi/results/ws7/ws7-gate-trend-comparison.json"
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
if (!(Test-Path -Path $CurrentSummaryPath)) {
  throw "Current WS7 summary not found at $CurrentSummaryPath"
}

$current = Get-Content -Raw -Path $CurrentSummaryPath | ConvertFrom-Json

if (!(Test-Path -Path $PriorSummaryPath)) {
  $baseline = [ordered]@{
    report = "ws7-gate-trend-comparison"
    status = "passed"
    trend_state = "baseline_established"
    generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
    current_summary = $CurrentSummaryPath
    prior_summary = $PriorSummaryPath
    detail = "No prior summary artifact found; baseline established from current run."
  }
  $baseline | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
  Write-Host "WS7 trend comparison artifact: $OutputPath (baseline_established)"
  exit 0
}

$prior = Get-Content -Raw -Path $PriorSummaryPath | ConvertFrom-Json
$currentPacks = @{}
$priorPacks = @{}
foreach ($p in $current.packs) { $currentPacks[[string]$p.pack] = [string]$p.status }
foreach ($p in $prior.packs) { $priorPacks[[string]$p.pack] = [string]$p.status }

$allPackNames = @($currentPacks.Keys + $priorPacks.Keys | Sort-Object -Unique)
$changes = @()
$regressions = @()
$improvements = @()

foreach ($name in $allPackNames) {
  $before = if ($priorPacks.ContainsKey($name)) { $priorPacks[$name] } else { "missing" }
  $after = if ($currentPacks.ContainsKey($name)) { $currentPacks[$name] } else { "missing" }
  if ($before -ne $after) { $changes += [ordered]@{ pack = $name; before = $before; after = $after } }
  if ($before -eq "passed" -and $after -eq "failed") { $regressions += $name }
  elseif (($before -eq "failed" -or $before -eq "missing") -and $after -eq "passed") { $improvements += $name }
}

$durationDelta = [int]$current.duration_ms - [int]$prior.duration_ms
$trendState = "stable"
if ($regressions.Count -gt 0 -or ([string]$prior.status -eq "passed" -and [string]$current.status -eq "failed")) {
  $trendState = "regressed"
} elseif ($improvements.Count -gt 0 -or ([string]$prior.status -eq "failed" -and [string]$current.status -eq "passed")) {
  $trendState = "improved"
}

$status = if ($trendState -eq "regressed") { "failed" } else { "passed" }

$artifact = [ordered]@{
  report = "ws7-gate-trend-comparison"
  status = $status
  trend_state = $trendState
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  current_summary = $CurrentSummaryPath
  prior_summary = $PriorSummaryPath
  gate_status = [ordered]@{ before = [string]$prior.status; after = [string]$current.status }
  duration_ms = [ordered]@{ before = [int]$prior.duration_ms; after = [int]$current.duration_ms; delta = $durationDelta }
  pack_changes = $changes
  regressions = $regressions
  improvements = $improvements
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS7 trend comparison artifact: $OutputPath ($trendState)"
if ($status -ne "passed") { exit 1 }
