param(
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-closure-gate-summary.json"
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

function Get-ArtifactStatus {
  param([string]$ArtifactPath)

  if (!(Test-Path -Path $ArtifactPath)) {
    return "missing_artifact"
  }

  try {
    $json = Get-Content -Raw -Path $ArtifactPath | ConvertFrom-Json
    if ($null -ne $json.status) {
      return [string]$json.status
    }
    return "present"
  } catch {
    return "invalid_artifact"
  }
}

function Invoke-PowerShellScript {
  param(
    [string]$ScriptPath,
    [string[]]$ArgumentList = @()
  )

  $process = Start-Process -FilePath "powershell.exe" `
    -ArgumentList (@("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $ScriptPath) + $ArgumentList) `
    -NoNewWindow `
    -PassThru `
    -Wait
  return $process.ExitCode
}

$start = Get-Date
$ws6SummaryPath = "tests/kpi/results/ws6/ws6-gate-summary.json"
$ws6ReleasePath = "tests/kpi/results/gates/ws6-release-readiness.json"
$ws6ChaosPath = "tests/kpi/results/ws6/ws6-chaos-fault-matrix.json"
$ws6TrendPath = "tests/kpi/results/ws6/ws6-gate-trend-comparison.json"
$ws6BadgePath = "tests/kpi/results/ws6/ws6-failover-stability-badge.json"

$runs = @()
$status = "passed"

try {
  $exitCode = Invoke-PowerShellScript -ScriptPath "tests/kpi/scripts/run-ws6-gate.ps1" -ArgumentList @(
    "-OutputPath", $ws6SummaryPath,
    "-ReleaseSummaryOutputPath", $ws6ReleasePath
  )
  $artifactStatus = Get-ArtifactStatus -ArtifactPath $ws6SummaryPath
  if ($artifactStatus -eq "passed") {
    $runs += [ordered]@{ pack = "ws6-gate"; status = "passed"; detail = "ok"; artifact = $ws6SummaryPath }
  } else {
    $status = "failed"
    $detail = if ($exitCode -ne 0) { "exit_code=$exitCode;$artifactStatus" } else { $artifactStatus }
    $runs += [ordered]@{ pack = "ws6-gate"; status = "failed"; detail = $detail; artifact = $ws6SummaryPath }
  }
} catch {
  $status = "failed"
  $runs += [ordered]@{ pack = "ws6-gate"; status = "failed"; detail = $_.Exception.Message; artifact = $ws6SummaryPath }
}

$checks = [ordered]@{
  ws6_gate_passed = $false
  ws6_release_summary_passed = $false
  ws6_all_packs_passed = $false
  ws6_chaos_fault_modes_all_passed = $false
  ws6_trend_allowed = $false
  ws6_stability_badge_green = $false
}

if ($status -eq "passed") {
  foreach ($path in @($ws6SummaryPath, $ws6ReleasePath, $ws6ChaosPath, $ws6TrendPath, $ws6BadgePath)) {
    if (!(Test-Path -Path $path)) {
      $status = "failed"
      $runs += [ordered]@{ pack = "ws6-artifact-presence"; status = "failed"; detail = "missing:$path"; artifact = $path }
    }
  }
}

if ($status -eq "passed") {
  $summary = Get-Content -Raw -Path $ws6SummaryPath | ConvertFrom-Json
  $release = Get-Content -Raw -Path $ws6ReleasePath | ConvertFrom-Json
  $chaos = Get-Content -Raw -Path $ws6ChaosPath | ConvertFrom-Json
  $trend = Get-Content -Raw -Path $ws6TrendPath | ConvertFrom-Json
  $badge = Get-Content -Raw -Path $ws6BadgePath | ConvertFrom-Json

  $checks.ws6_gate_passed = ([string]$summary.status -eq "passed")
  $checks.ws6_release_summary_passed = ([string]$release.status -eq "passed")
  $checks.ws6_all_packs_passed = ((@($summary.packs | Where-Object { $_.status -ne "passed" }).Count) -eq 0)
  $checks.ws6_chaos_fault_modes_all_passed = ([int]$chaos.failed_modes -eq 0)
  $checks.ws6_trend_allowed = (@("stable", "improved", "baseline_established") -contains [string]$trend.trend_state)
  $checks.ws6_stability_badge_green = ([string]$badge.color -eq "green")

  if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) {
    $status = "failed"
  }
}

$finished = Get-Date
$summaryOut = [ordered]@{
  gate = "ws6-closure-gate"
  status = $status
  validation_posture = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  artifacts = [ordered]@{
    ws6_gate = $ws6SummaryPath
    ws6_release = $ws6ReleasePath
    ws6_chaos_matrix = $ws6ChaosPath
    ws6_trend = $ws6TrendPath
    ws6_badge = $ws6BadgePath
  }
  checks = $checks
  runs = $runs
}

$summaryOut | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS6 closure gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
