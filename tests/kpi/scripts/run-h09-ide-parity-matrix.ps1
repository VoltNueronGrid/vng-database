param(
  [string]$OutputPath = "tests/kpi/results/h09/h09-ide-parity-matrix.json",
  [string]$MatrixPath = "reference/h09-cross-ide-parity-test-matrix.md"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Add-Check {
  param([string]$Name, [bool]$Ok, [string]$Detail)
  $script:checks += [ordered]@{
    check = $Name
    ok = $Ok
    detail = $Detail
  }
}

Ensure-OutputDir -PathValue $OutputPath

$start = Get-Date
$checks = @()

$ws9aContractArtifact = "tests/kpi/results/ws9a/ide-contract-smoke.json"
$ws9aGateArtifact = "tests/kpi/results/ws9a/ws9a-gate-summary.json"

Add-Check -Name "h09_matrix_doc_exists" -Ok (Test-Path $MatrixPath) -Detail $MatrixPath
Add-Check -Name "ws9a_contract_artifact_exists" -Ok (Test-Path $ws9aContractArtifact) -Detail $ws9aContractArtifact
Add-Check -Name "ws9a_gate_artifact_exists" -Ok (Test-Path $ws9aGateArtifact) -Detail $ws9aGateArtifact

$ws9aContractPassed = $false
if (Test-Path $ws9aContractArtifact) {
  try {
    $contractJson = Get-Content -Raw -Path $ws9aContractArtifact | ConvertFrom-Json
    $ws9aContractPassed = ([string]$contractJson.status -eq "passed")
  } catch {
    $ws9aContractPassed = $false
  }
}
Add-Check -Name "ws9a_contract_status_passed" -Ok $ws9aContractPassed -Detail "status=passed"

$ws9aGatePassed = $false
if (Test-Path $ws9aGateArtifact) {
  try {
    $gateJson = Get-Content -Raw -Path $ws9aGateArtifact | ConvertFrom-Json
    $ws9aGatePassed = ([string]$gateJson.status -eq "passed")
  } catch {
    $ws9aGatePassed = $false
  }
}
Add-Check -Name "ws9a_gate_status_passed" -Ok $ws9aGatePassed -Detail "status=passed"

$status = if (($checks | Where-Object { -not $_.ok }).Count -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "h09-ide-parity-matrix"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  checks = $checks
  matrix_path = $MatrixPath
  baseline_artifacts = @($ws9aContractArtifact, $ws9aGateArtifact)
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "H-09 IDE parity matrix smoke failed."
  exit 1
}

Write-Host "H-09 IDE parity matrix smoke passed. Artifact: $OutputPath"
