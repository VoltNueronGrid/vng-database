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
Ensure-OutputDir -PathValue $OutputPath

& "tests/kpi/scripts/run-ws1-closure-gate.ps1" -OutputPath "tests/kpi/results/ws1/ws1-closure-gate-summary.json"
$ws1Exit = $LASTEXITCODE
& "tests/kpi/scripts/run-release-r3-autonomous-gate.ps1" -OutputPath "tests/kpi/results/gates/release-r3-autonomous-readiness.json"
$r3AutoExit = $LASTEXITCODE

$ws1Closure = Get-Content -Raw -Path "tests/kpi/results/ws1/ws1-closure-gate-summary.json" | ConvertFrom-Json
$ws1Release = Get-Content -Raw -Path "tests/kpi/results/gates/ws1-release-readiness.json" | ConvertFrom-Json
$r3Auto = Get-Content -Raw -Path "tests/kpi/results/gates/release-r3-autonomous-readiness.json" | ConvertFrom-Json

$status = if (
  $ws1Exit -eq 0 -and
  $r3AutoExit -eq 0 -and
  $ws1Closure.validation_posture -eq "ready_for_validation" -and
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
      validation_posture = [string]$ws1Closure.validation_posture
      source = "tests/kpi/results/ws1/ws1-closure-gate-summary.json"
    }
    ws1_release_summary = [ordered]@{
      status = [string]$ws1Release.status
      source = "tests/kpi/results/gates/ws1-release-readiness.json"
    }
    r3_autonomous_gate = [ordered]@{
      exit_code = $r3AutoExit
      status = [string]$r3Auto.status
      source = "tests/kpi/results/gates/release-r3-autonomous-readiness.json"
    }
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "R3 UDF runtime readiness gate: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
