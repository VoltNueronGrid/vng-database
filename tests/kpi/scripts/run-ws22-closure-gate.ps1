param(
  [string]$OutputPath = "tests/kpi/results/ws22/ws22-closure-gate-summary.json"
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

$start = Get-Date
$ws22SummaryPath = "tests/kpi/results/ws22/ws22-gate-summary.json"
$ws22SmokePath = "tests/kpi/results/ws22/ws22-pessimistic-lock-smoke.json"

$runs = @()
$status = "passed"

try {
  $global:LASTEXITCODE = 0
  & "tests/kpi/scripts/run-ws22-gate.ps1" -OutputPath $ws22SummaryPath 2>&1 | Out-Null
  if (-not $?) {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws22-gate"; status = "failed"; detail = "script_invocation_failed"; artifact = $ws22SummaryPath }
  } elseif ($global:LASTEXITCODE -ne 0) {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws22-gate"; status = "failed"; detail = "exit_code=$global:LASTEXITCODE"; artifact = $ws22SummaryPath }
  } else {
    $runs += [ordered]@{ pack = "ws22-gate"; status = "passed"; detail = "ok"; artifact = $ws22SummaryPath }
  }
} catch {
  $status = "failed"
  $runs += [ordered]@{ pack = "ws22-gate"; status = "failed"; detail = $_.Exception.Message; artifact = $ws22SummaryPath }
}

$checks = [ordered]@{
  ws22_gate_passed = $false
  ws22_smoke_passed = $false
  ws22_contract_checks_all_passed = $false
}

if ($status -eq "passed") {
  foreach ($path in @($ws22SummaryPath, $ws22SmokePath)) {
    if (!(Test-Path -Path $path)) {
      $status = "failed"
      $runs += [ordered]@{ pack = "ws22-artifact-presence"; status = "failed"; detail = "missing:$path"; artifact = $path }
    }
  }
}

if ($status -eq "passed") {
  $summary = Get-Content -Raw -Path $ws22SummaryPath | ConvertFrom-Json
  $smoke = Get-Content -Raw -Path $ws22SmokePath | ConvertFrom-Json

  $checks.ws22_gate_passed = ([string]$summary.status -eq "passed")
  $checks.ws22_smoke_passed = ([string]$smoke.status -eq "passed")
  $checks.ws22_contract_checks_all_passed = (
    $smoke.contract_checks.acquire_route_present -eq $true -and
    $smoke.contract_checks.release_route_present -eq $true -and
    $smoke.contract_checks.acquire_logic_present -eq $true -and
    $smoke.contract_checks.release_logic_present -eq $true
  )

  if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) { $status = "failed" }
}

$finished = Get-Date
$summaryOut = [ordered]@{
  gate = "ws22-closure-gate"
  status = $status
  validation_posture = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  artifacts = [ordered]@{
    ws22_gate = $ws22SummaryPath
    ws22_smoke = $ws22SmokePath
  }
  checks = $checks
  runs = $runs
}

$summaryOut | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "WS22 closure gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
