param(
  [string]$OutputPath = "tests/kpi/results/gates/release-r3-agent-authoring-readiness.json"
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

& "tests/kpi/scripts/run-ws8a-closure-gate.ps1" -OutputPath "tests/kpi/results/ws8a/ws8a-closure-gate-summary.json"
$ws8aExit = $LASTEXITCODE
& "tests/kpi/scripts/run-ws8-closure-gate.ps1" -OutputPath "tests/kpi/results/ws8/ws8-closure-gate-summary.json"
$ws8Exit = $LASTEXITCODE
& "tests/kpi/scripts/run-ws7-closure-gate.ps1" -OutputPath "tests/kpi/results/ws7/ws7-closure-gate-summary.json"
$ws7Exit = $LASTEXITCODE

$ws8aClosure = Get-Content -Raw -Path "tests/kpi/results/ws8a/ws8a-closure-gate-summary.json" | ConvertFrom-Json
$ws8aRelease = Get-Content -Raw -Path "tests/kpi/results/gates/ws8a-release-readiness.json" | ConvertFrom-Json
$ws8Closure = Get-Content -Raw -Path "tests/kpi/results/ws8/ws8-closure-gate-summary.json" | ConvertFrom-Json
$ws7Closure = Get-Content -Raw -Path "tests/kpi/results/ws7/ws7-closure-gate-summary.json" | ConvertFrom-Json

$status = if (
  $ws8aExit -eq 0 -and
  $ws8Exit -eq 0 -and
  $ws7Exit -eq 0 -and
  $ws8aClosure.validation_posture -eq "ready_for_validation" -and
  $ws8Closure.validation_posture -eq "ready_for_validation" -and
  $ws7Closure.validation_posture -eq "ready_for_validation" -and
  $ws8aRelease.release_readiness -eq "ready_for_validation"
) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate = "release-r3-agent-authoring-readiness"
  status = $status
  release = "R3"
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = [ordered]@{
    ws8a_closure = [ordered]@{
      exit_code = $ws8aExit
      validation_posture = [string]$ws8aClosure.validation_posture
      source = "tests/kpi/results/ws8a/ws8a-closure-gate-summary.json"
    }
    ws8a_release_summary = [ordered]@{
      release_readiness = [string]$ws8aRelease.release_readiness
      source = "tests/kpi/results/gates/ws8a-release-readiness.json"
    }
    ws8_closure = [ordered]@{
      exit_code = $ws8Exit
      validation_posture = [string]$ws8Closure.validation_posture
      source = "tests/kpi/results/ws8/ws8-closure-gate-summary.json"
    }
    ws7_closure = [ordered]@{
      exit_code = $ws7Exit
      validation_posture = [string]$ws7Closure.validation_posture
      source = "tests/kpi/results/ws7/ws7-closure-gate-summary.json"
    }
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "R3 agent authoring readiness gate: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
