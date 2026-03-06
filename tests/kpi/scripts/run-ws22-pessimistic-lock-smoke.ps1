param(
  [string]$OutputPath = "tests/kpi/results/ws22/ws22-pessimistic-lock-smoke.json"
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
$command = "cargo test -p voltnuerongridd ws22_; runtime contract checks for pessimistic lock APIs"
$outputLines = @()
$exitCode = 1
$contractChecks = [ordered]@{
  acquire_route_present = $false
  release_route_present = $false
  acquire_logic_present = $false
  release_logic_present = $false
}

try {
  $outputLines = & cargo test -p voltnuerongridd ws22_ -- --nocapture 2>&1
  $testExit = $LASTEXITCODE

  $runtimeRaw = Get-Content -Raw -Path "services/voltnuerongridd/src/main.rs"
  $contractChecks.acquire_route_present = ($runtimeRaw -match '/api/v1/sql/locks/pessimistic/acquire')
  $contractChecks.release_route_present = ($runtimeRaw -match '/api/v1/sql/locks/pessimistic/release')
  $contractChecks.acquire_logic_present = ($runtimeRaw -match 'fn acquire_pessimistic_lock\(')
  $contractChecks.release_logic_present = ($runtimeRaw -match 'fn release_pessimistic_lock\(')

  $contractExit = if (
    $contractChecks.acquire_route_present -and
    $contractChecks.release_route_present -and
    $contractChecks.acquire_logic_present -and
    $contractChecks.release_logic_present
  ) { 0 } else { 1 }
  $exitCode = if ($testExit -eq 0 -and $contractExit -eq 0) { 0 } else { 1 }
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$status = if ($exitCode -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws22-pessimistic-lock-baseline"
  status = $status
  command = $command
  contract_checks = $contractChecks
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS22 pessimistic lock smoke failed."
  exit 1
}

Write-Host "WS22 pessimistic lock smoke passed. Artifact: $OutputPath"
