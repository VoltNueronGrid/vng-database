param(
  [string]$OutputPath = "tests/kpi/results/gates/release-r3-autonomous-readiness.json"
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

$ws8ClosurePath = Resolve-ArtifactPath -Candidates @(
  "tests/kpi/results/ws8/ci-ws8-closure-gate-summary.json",
  "tests/kpi/results/ws8/ws8-closure-gate-summary.json"
)
if (-not $ws8ClosurePath) {
  & "tests/kpi/scripts/run-ws8-closure-gate.ps1" -OutputPath "tests/kpi/results/ws8/ws8-closure-gate-summary.json"
  $ws8ClosurePath = "tests/kpi/results/ws8/ws8-closure-gate-summary.json"
}

$ws7ClosurePath = Resolve-ArtifactPath -Candidates @(
  "tests/kpi/results/ws7/ci-ws7-closure-gate-summary.json",
  "tests/kpi/results/ws7/ws7-closure-gate-summary.json"
)
if (-not $ws7ClosurePath) {
  & "tests/kpi/scripts/run-ws7-closure-gate.ps1" -OutputPath "tests/kpi/results/ws7/ws7-closure-gate-summary.json"
  $ws7ClosurePath = "tests/kpi/results/ws7/ws7-closure-gate-summary.json"
}

$ws8Closure = Get-Content -Raw -Path $ws8ClosurePath | ConvertFrom-Json
$ws8Release = Get-Content -Raw -Path "tests/kpi/results/gates/ws8-release-readiness.json" | ConvertFrom-Json
$ws7Closure = Get-Content -Raw -Path $ws7ClosurePath | ConvertFrom-Json
$ws8RuntimeIncluded = $false
$ws8RuntimeStatus = "not_included"
if ($null -ne $ws8Release.highlights) {
  if ($null -ne $ws8Release.highlights.ws8_runtime_pack_included) {
    $ws8RuntimeIncluded = [bool]$ws8Release.highlights.ws8_runtime_pack_included
  }
  if ($null -ne $ws8Release.highlights.ws8_runtime_pack_status) {
    $ws8RuntimeStatus = [string]$ws8Release.highlights.ws8_runtime_pack_status
  }
}
$ws8Exit = if ([string]$ws8Closure.status -eq "passed" -or [string]$ws8Closure.validation_posture -eq "ready_for_validation") { 0 } else { 1 }
$ws7Exit = if ([string]$ws7Closure.status -eq "passed" -or [string]$ws7Closure.validation_posture -eq "ready_for_validation") { 0 } else { 1 }
$status = if ($ws8Exit -eq 0 -and $ws7Exit -eq 0 -and $ws8Closure.validation_posture -eq "ready_for_validation" -and $ws7Closure.validation_posture -eq "ready_for_validation" -and $ws8Release.release_readiness -eq "ready_for_validation") { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate = "release-r3-autonomous-readiness"
  status = $status
  release = "R3"
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = [ordered]@{
    ws8_closure = [ordered]@{
      exit_code = $ws8Exit
      validation_posture = [string]$ws8Closure.validation_posture
      source = $ws8ClosurePath
    }
    ws8_release_summary = [ordered]@{
      release_readiness = [string]$ws8Release.release_readiness
      source = "tests/kpi/results/gates/ws8-release-readiness.json"
      ws8_runtime_pack_included = $ws8RuntimeIncluded
      ws8_runtime_pack_status = $ws8RuntimeStatus
    }
    ws7_closure = [ordered]@{
      exit_code = $ws7Exit
      validation_posture = [string]$ws7Closure.validation_posture
      source = $ws7ClosurePath
    }
  }
  highlights = [ordered]@{
    ws8_runtime_pack_included = $ws8RuntimeIncluded
    ws8_runtime_pack_status = $ws8RuntimeStatus
    ws8_runtime_pack_artifact = if ($ws8RuntimeIncluded) { "tests/kpi/results/ws8/tenant-autonomous-runtime-smoke.json" } else { "not_included" }
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "R3 autonomous readiness gate: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
