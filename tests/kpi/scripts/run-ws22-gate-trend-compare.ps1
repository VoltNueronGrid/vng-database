param(
  [string]$CurrentSummaryPath = "tests/kpi/results/ws22/ws22-gate-summary.json",
  [string]$PriorSummaryPath = "tests/kpi/results/ws22/ws22-gate-summary.previous.json",
  [string]$OutputPath = "tests/kpi/results/ws22/ws22-gate-trend-comparison.json"
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
if (!(Test-Path -Path $CurrentSummaryPath)) { throw "Current WS22 summary not found at $CurrentSummaryPath" }
$current = Get-Content -Raw -Path $CurrentSummaryPath | ConvertFrom-Json

if (!(Test-Path -Path $PriorSummaryPath)) {
  $baseline = [ordered]@{
    report = "ws22-gate-trend-comparison"
    status = "passed"
    trend_state = "baseline_established"
    generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
    current_summary = $CurrentSummaryPath
    prior_summary = $PriorSummaryPath
  }
  $baseline | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
  Write-Host "WS22 trend comparison artifact: $OutputPath (baseline_established)"
  exit 0
}

$prior = Get-Content -Raw -Path $PriorSummaryPath | ConvertFrom-Json
$durationDelta = [int]$current.duration_ms - [int]$prior.duration_ms
$trendState = "stable"
if ([string]$prior.status -eq "failed" -and [string]$current.status -eq "passed") { $trendState = "improved" }
if ([string]$prior.status -eq "passed" -and [string]$current.status -eq "failed") { $trendState = "regressed" }
$status = if ($trendState -eq "regressed") { "failed" } else { "passed" }

$artifact = [ordered]@{
  report = "ws22-gate-trend-comparison"
  status = $status
  trend_state = $trendState
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  current_summary = $CurrentSummaryPath
  prior_summary = $PriorSummaryPath
  gate_status = [ordered]@{ before = [string]$prior.status; after = [string]$current.status }
  duration_ms = [ordered]@{ before = [int]$prior.duration_ms; after = [int]$current.duration_ms; delta = $durationDelta }
}

$priorLock = $prior.ws22_lock_contention_metrics
$currentLock = $current.ws22_lock_contention_metrics
if ($currentLock) {
  $curDl = [int64]($currentLock.deadlock_detections)
  $curCap = [int64]($currentLock.scan_cap_timeouts)
  if ($priorLock) {
    $priorDl = [int64]($priorLock.deadlock_detections)
    $priorCap = [int64]($priorLock.scan_cap_timeouts)
    $artifact.lock_contention_metrics_trend = [ordered]@{
      deadlock_detections = [ordered]@{ before = $priorDl; after = $curDl; delta = ($curDl - $priorDl) }
      scan_cap_timeouts = [ordered]@{ before = $priorCap; after = $curCap; delta = ($curCap - $priorCap) }
      interpretation = "Higher deltas may indicate increased lock contention or richer ws22 test coverage; compare with gate status."
    }
  } else {
    $artifact.lock_contention_metrics_trend = [ordered]@{
      trend_state = "baseline_established"
      deadlock_detections = [ordered]@{ before = $null; after = $curDl; delta = $null }
      scan_cap_timeouts = [ordered]@{ before = $null; after = $curCap; delta = $null }
      interpretation = "Prior summary had no ws22_lock_contention_metrics; next run will compute deltas."
    }
  }
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS22 trend comparison artifact: $OutputPath ($trendState)"
if ($status -ne "passed") { exit 1 }
