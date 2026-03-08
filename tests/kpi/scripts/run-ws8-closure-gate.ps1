param(
  [string]$OutputPath = "tests/kpi/results/ws8/ws8-closure-gate-summary.json",
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [switch]$IncludeRuntimeSmokes
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

& "tests/kpi/scripts/run-ws8-gate.ps1" `
  -OutputPath "tests/kpi/results/ws8/ws8-gate-summary.json" `
  -ReleaseSummaryOutputPath "tests/kpi/results/gates/ws8-release-readiness.json" `
  -BaseUrl $BaseUrl `
  -IncludeRuntimeSmokes:$IncludeRuntimeSmokes
$gateSummaryPath = "tests/kpi/results/ws8/ws8-gate-summary.json"
$gateExit = 1
if (Test-Path -Path $gateSummaryPath) {
  try {
    $gateSummary = Get-Content -Raw -Path $gateSummaryPath | ConvertFrom-Json
    $gateExit = if ([string]$gateSummary.status -eq "passed") { 0 } else { 1 }
  } catch {
    $gateExit = if ($LASTEXITCODE -eq 0) { 0 } else { 1 }
  }
} elseif ($LASTEXITCODE -eq 0) {
  $gateExit = 0
}

$requiredArtifacts = @(
  "tests/kpi/results/ws8/ws8-gate-summary.json",
  "tests/kpi/results/ws8/ws8-autonomy-matrix.json",
  "tests/kpi/results/ws8/ws8-gate-trend-comparison.json",
  "tests/kpi/results/ws8/ws8-autonomy-stability-badge.json",
  "tests/kpi/results/gates/ws8-release-readiness.json"
)

if ($IncludeRuntimeSmokes) {
  $requiredArtifacts += "tests/kpi/results/ws8/tenant-autonomous-runtime-smoke.json"
}

$artifactChecks = @()
foreach ($artifactPath in $requiredArtifacts) {
  $exists = Test-Path -Path $artifactPath
  $statusValue = "missing"
  if ($exists) {
    $json = Get-Content -Raw -Path $artifactPath | ConvertFrom-Json
    if ($null -ne $json.status) {
      $statusValue = [string]$json.status
    } elseif ($null -ne $json.release_readiness) {
      $statusValue = [string]$json.release_readiness
    } else {
      $statusValue = "present"
    }
  }
  $artifactChecks += [ordered]@{
    artifact = $artifactPath
    exists = $exists
    status = $statusValue
  }
}

$hasMissing = (@($artifactChecks | Where-Object { $_.exists -ne $true }).Count) -gt 0
$hasFailed = (@($artifactChecks | Where-Object { $_.status -eq "failed" -or $_.status -eq "blocked" }).Count) -gt 0
$posture = if ($gateExit -eq 0 -and -not $hasMissing -and -not $hasFailed) { "ready_for_validation" } else { "blocked" }
$status = if ($posture -eq "ready_for_validation") { "passed" } else { "failed" }

$summary = [ordered]@{
  gate = "ws8-closure"
  status = $status
  validation_posture = $posture
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  gate_exit_code = $gateExit
  artifacts = $artifactChecks
}
$summary | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS8 closure gate summary: $OutputPath ($posture)"
if ($status -ne "passed") { exit 1 }
