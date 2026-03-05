param(
  [string]$OutputPath = "tests/kpi/results/ws14/config-contract-smoke.json"
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

$driverJsonPath = "reference/config-contracts/ws14/driver-routing-config.json"
$securityJsonPath = "reference/config-contracts/ws14/security-control-config.json"

$start = Get-Date
$command = "cargo test -p voltnuerongrid-driver-rust validates_driver_contract; cargo test -p voltnuerongrid-auth validates_security_config; json schema parse checks"
$outputLines = @()
$exitCode = 1
$schemaOk = $false

try {
  $first = & cargo test -p voltnuerongrid-driver-rust validates_driver_contract 2>&1
  $firstExit = $LASTEXITCODE
  $second = & cargo test -p voltnuerongrid-auth validates_security_config 2>&1
  $secondExit = $LASTEXITCODE

  $driverSchema = Get-Content -Raw -Path $driverJsonPath | ConvertFrom-Json
  $securitySchema = Get-Content -Raw -Path $securityJsonPath | ConvertFrom-Json
  $schemaOk = (
    $null -ne $driverSchema.driver.baseUrl -and
    $null -ne $driverSchema.driver.pool.maxConnections -and
    $null -ne $securitySchema.security.adminApiKeyEnv -and
    $null -ne $securitySchema.security.allowedOperatorRoles
  )

  $outputLines = @($first + $second)
  $exitCode = if ($firstExit -eq 0 -and $secondExit -eq 0 -and $schemaOk) { 0 } else { 1 }
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$status = if ($exitCode -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws14-config-contract-baseline"
  status = $status
  command = $command
  schema_ok = $schemaOk
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS14 config contract smoke failed."
  exit 1
}

Write-Host "WS14 config contract smoke passed. Artifact: $OutputPath"
