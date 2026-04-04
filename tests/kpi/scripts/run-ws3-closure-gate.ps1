param(
  [string]$OutputPath = "tests/kpi/results/ws3/ws3-closure-gate-summary.json"
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

$requiredScripts = @(
  "tests/kpi/scripts/run-ws3-gate.ps1",
  "tests/kpi/scripts/run-ws3-query-routing-smoke.ps1",
  "tests/kpi/scripts/run-ws3-htap-target-contract-smoke.ps1",
  "tests/kpi/scripts/run-ws3-performance-score.ps1",
  "tests/kpi/scripts/run-ws3-gate-trend-compare.ps1",
  "tests/kpi/scripts/run-ws3-performance-stability-badge.ps1",
  "tests/kpi/scripts/run-ws3-release-summary.ps1"
)

Write-Host "[CLOSURE GATE] Pre-flight check: required WS3 scripts"
foreach ($script in $requiredScripts) {
  if (-not (Test-Path -Path $script)) {
    Write-Error "Required script missing: $script"
    exit 1
  }
}
Write-Host "[CLOSURE GATE] Pre-flight passed"

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
    [string[]]$ArgumentList = @(),
    [string]$WorkingDirectory
  )

  $shell = if (Get-Command pwsh -ErrorAction SilentlyContinue) {
    (Get-Command pwsh).Source
  } else {
    "powershell.exe"
  }

  $process = Start-Process -FilePath $shell `
    -ArgumentList (@("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $ScriptPath) + $ArgumentList) `
    -WorkingDirectory $WorkingDirectory `
    -NoNewWindow `
    -PassThru `
    -Wait
  return $process.ExitCode
}

$start = Get-Date
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path
$ws3SummaryPath = "tests/kpi/results/ws3/ws3-gate-summary.json"
$ws3ReleasePath = "tests/kpi/results/gates/ws3-release-readiness.json"
$ws3TrendPath = "tests/kpi/results/ws3/ws3-gate-trend-comparison.json"
$ws3BadgePath = "tests/kpi/results/ws3/ws3-performance-stability-badge.json"

$runs = @()
$status = "passed"

try {
  $exitCode = Invoke-PowerShellScript -WorkingDirectory $repoRoot -ScriptPath "tests/kpi/scripts/run-ws3-gate.ps1" -ArgumentList @(
    "-OutputPath", $ws3SummaryPath,
    "-ReleaseSummaryOutputPath", $ws3ReleasePath
  )
  $artifactStatus = Get-ArtifactStatus -ArtifactPath $ws3SummaryPath
  if ($artifactStatus -eq "passed") {
    $runs += [ordered]@{ pack = "ws3-gate"; status = "passed"; detail = "ok"; artifact = $ws3SummaryPath }
  } else {
    $status = "failed"
    $detail = if ($exitCode -ne 0) { "exit_code=$exitCode;$artifactStatus" } else { $artifactStatus }
    $runs += [ordered]@{ pack = "ws3-gate"; status = "failed"; detail = $detail; artifact = $ws3SummaryPath }
  }
} catch {
  $status = "failed"
  $runs += [ordered]@{ pack = "ws3-gate"; status = "failed"; detail = $_.Exception.Message; artifact = $ws3SummaryPath }
}

$checks = [ordered]@{
  ws3_gate_passed = $false
  ws3_release_summary_passed = $false
  ws3_all_packs_passed = $false
  ws3_trend_stable_or_improved = $false
  ws3_stability_badge_green = $false
}

# Validate ws3-gate-summary.json
$gateStatus = Get-ArtifactStatus -ArtifactPath $ws3SummaryPath
if ($gateStatus -eq "passed") {
  try {
    $gateJson = Get-Content -Raw -Path $ws3SummaryPath | ConvertFrom-Json
    $checks.ws3_gate_passed = $true
    $checks.ws3_all_packs_passed = ($gateJson.status -eq "passed")
  } catch {
    $checks.ws3_gate_passed = $false
  }
}

# Validate ws3-release-readiness.json
$releaseStatus = Get-ArtifactStatus -ArtifactPath $ws3ReleasePath
if ($releaseStatus -eq "passed") {
  try {
    $releaseJson = Get-Content -Raw -Path $ws3ReleasePath | ConvertFrom-Json
    $checks.ws3_release_summary_passed = ($releaseJson.status -eq "passed")
  } catch {
    $checks.ws3_release_summary_passed = $false
  }
}

# Validate ws3-gate-trend-comparison.json
try {
  if (Test-Path -Path $ws3TrendPath) {
    $trendJson = Get-Content -Raw -Path $ws3TrendPath | ConvertFrom-Json
    $checks.ws3_trend_stable_or_improved = (@("stable", "improved", "baseline_established") -contains [string]$trendJson.trend_state)
  }
} catch {
  $checks.ws3_trend_stable_or_improved = $false
}

# Validate ws3-performance-stability-badge.json
try {
  if (Test-Path -Path $ws3BadgePath) {
    $badgeJson = Get-Content -Raw -Path $ws3BadgePath | ConvertFrom-Json
    $checks.ws3_stability_badge_green = ([string]$badgeJson.color -eq "green")
  }
} catch {
  $checks.ws3_stability_badge_green = $false
}

# Determine final status
$passedChecks = ($checks.Values | Where-Object { $_ -eq $true }).Count
$totalChecks = $checks.Count

if ($passedChecks -eq $totalChecks) {
  $status = "passed"
  $validationPosture = "ready_for_validation"
} else {
  $status = "failed"
  $validationPosture = "validation_blocked"
}

$elapsed = (Get-Date) - $start

# Create output JSON
$output = [ordered]@{
  id = "ws3-closure-gate"
  timestamp = (Get-Date -Format "yyyy-MM-ddTHH:mm:ssZ")
  status = $status
  validation_posture = $validationPosture
  checks = $checks
  total_checks = $totalChecks
  passed_checks = $passedChecks
  failed_checks = $totalChecks - $passedChecks
  runs = $runs
  elapsed_ms = [int]$elapsed.TotalMilliseconds
  scope = "HTAP query execution and routing (Epic 3, REQ-31)"
  release_target = "R3"
}

$output | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "[CLOSURE GATE] WS3 HTAP Query Execution and Routing"
Write-Host "Status: $status ($passedChecks / $totalChecks checks passed)"
Write-Host "Validation Posture: $validationPosture"
Write-Host "Artifact: $OutputPath"

exit $(if ($status -eq "passed") { 0 } else { 1 })
