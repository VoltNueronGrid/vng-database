param(
  [string]$OutputPath = "tests/kpi/results/gates/release-r3-udf-runtime-readiness.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Resolve-ArtifactPath {
  param([string[]]$Candidates)

  foreach ($candidate in $Candidates) {
    if (Test-Path -Path $candidate) {
      return $candidate
    }
  }
  return $null
}
Ensure-OutputDir -PathValue $OutputPath

${ws1ClosurePath} = Resolve-ArtifactPath -Candidates @(
  "tests/kpi/results/ws1/ci-ws1-closure-gate-summary.json",
  "tests/kpi/results/ws1/ws1-closure-gate-summary.json"
)
if (-not $ws1ClosurePath) {
  & "tests/kpi/scripts/run-ws1-closure-gate.ps1" -OutputPath "tests/kpi/results/ws1/ws1-closure-gate-summary.json"
  $ws1ClosurePath = "tests/kpi/results/ws1/ws1-closure-gate-summary.json"
}

& "tests/kpi/scripts/run-release-r3-autonomous-gate.ps1" -OutputPath "tests/kpi/results/gates/release-r3-autonomous-readiness.json"
$r3AutoExit = if ($LASTEXITCODE -eq 0) { 0 } else { 1 }

$ws1ReleasePath = Resolve-ArtifactPath -Candidates @(
  "tests/kpi/results/gates/ci-ws1-release-readiness.json",
  "tests/kpi/results/gates/ws1-release-readiness.json"
)
if (-not $ws1ReleasePath) {
  throw "WS1 release summary not found."
}

$ws1GateSummaryPath = Resolve-ArtifactPath -Candidates @(
  "tests/kpi/results/ws1/ci-ws1-gate-summary.json",
  "tests/kpi/results/ws1/ws1-gate-summary.json"
)
if (-not $ws1GateSummaryPath) {
  throw "WS1 gate summary not found."
}

$ws1Closure = Get-Content -Raw -Path $ws1ClosurePath | ConvertFrom-Json
$ws1Release = Get-Content -Raw -Path $ws1ReleasePath | ConvertFrom-Json
$ws1GateSummary = Get-Content -Raw -Path $ws1GateSummaryPath | ConvertFrom-Json
$r3Auto = Get-Content -Raw -Path "tests/kpi/results/gates/release-r3-autonomous-readiness.json" | ConvertFrom-Json
$r3AutoExit = if ([string]$r3Auto.status -eq "passed") { 0 } else { 1 }
$ws1ValidationPosture = [string]$ws1Closure.validation_posture
$ws1ValidationSource = $ws1ClosurePath
$ws1Exit = if ([string]$ws1Closure.status -eq "passed" -or [string]$ws1Closure.validation_posture -eq "ready_for_validation") { 0 } else { 1 }
if ($ws1Exit -ne 0 -and [string]$ws1GateSummary.status -eq "passed" -and [string]$ws1Release.status -eq "passed") {
  $ws1Exit = 0
  $ws1ValidationPosture = "ready_for_validation"
  $ws1ValidationSource = $ws1GateSummaryPath
}
$ws8RuntimeIncluded = $false
$ws8RuntimeStatus = "not_included"
if ($null -ne $r3Auto.highlights) {
  if ($null -ne $r3Auto.highlights.ws8_runtime_pack_included) {
    $ws8RuntimeIncluded = [bool]$r3Auto.highlights.ws8_runtime_pack_included
  }
  if ($null -ne $r3Auto.highlights.ws8_runtime_pack_status) {
    $ws8RuntimeStatus = [string]$r3Auto.highlights.ws8_runtime_pack_status
  }
}

$status = if (
  $ws1Exit -eq 0 -and
  $r3AutoExit -eq 0 -and
  $ws1ValidationPosture -eq "ready_for_validation" -and
  $ws1Release.status -eq "passed" -and
  $r3Auto.status -eq "passed"
) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate = "release-r3-udf-runtime-readiness"
  status = $status
  release = "R3"
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = [ordered]@{
    ws1_closure = [ordered]@{
      exit_code = $ws1Exit
      validation_posture = $ws1ValidationPosture
      source = $ws1ValidationSource
    }
    ws1_release_summary = [ordered]@{
      status = [string]$ws1Release.status
      source = $ws1ReleasePath
    }
    r3_autonomous_gate = [ordered]@{
      exit_code = $r3AutoExit
      status = [string]$r3Auto.status
      source = "tests/kpi/results/gates/release-r3-autonomous-readiness.json"
      ws8_runtime_pack_included = $ws8RuntimeIncluded
      ws8_runtime_pack_status = $ws8RuntimeStatus
    }
  }
  highlights = [ordered]@{
    ws8_runtime_pack_included = $ws8RuntimeIncluded
    ws8_runtime_pack_status = $ws8RuntimeStatus
    ws8_runtime_pack_artifact = if ($ws8RuntimeIncluded) { "tests/kpi/results/ws8/tenant-autonomous-runtime-smoke.json" } else { "not_included" }
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "R3 UDF runtime readiness gate: $OutputPath ($status)"
if ($status -eq "passed") {
  $outDir = Split-Path -Parent $OutputPath
  $ciMirror = Join-Path $outDir "ci-release-r3-udf-runtime-readiness.json"
  if ($ciMirror -ne $OutputPath) {
    Copy-Item -LiteralPath $OutputPath -Destination $ciMirror -Force
    Write-Host "CI mirror: $ciMirror"
  }
}
if ($status -ne "passed") { exit 1 }
