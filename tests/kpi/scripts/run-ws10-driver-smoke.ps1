param(
  [string]$OutputPath = "tests/kpi/results/ws10/driver-smoke.json"
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
$command = "cargo test -p voltnuerongrid-driver-rust validates_driver_contract; config contract parse checks (json/yaml/properties)"
$outputLines = @()
$exitCode = 1
$contractChecks = [ordered]@{
  json = $false
  yaml = $false
  properties = $false
}

try {
  $outputLines = & cargo test -p voltnuerongrid-driver-rust validates_driver_contract 2>&1
  $testExit = $LASTEXITCODE

  $driverJsonRaw = Get-Content -Raw -Path "reference/config-contracts/ws14/driver-routing-config.json"
  $driverYamlRaw = Get-Content -Raw -Path "reference/config-contracts/ws14/driver-routing-config.yaml"
  $driverPropertiesRaw = Get-Content -Raw -Path "reference/config-contracts/ws14/driver-routing-config.properties"

  $contractChecks.json = (
    $driverJsonRaw -match '"baseUrl"\s*:\s*' -and
    $driverJsonRaw -match '"maxConnections"\s*:\s*'
  )
  $contractChecks.yaml = (
    $driverYamlRaw -match '(?m)^\s*baseUrl\s*:\s*' -and
    $driverYamlRaw -match '(?m)^\s*maxConnections\s*:\s*'
  )
  $contractChecks.properties = (
    $driverPropertiesRaw -match '(?m)^\s*driver\.baseUrl\s*=' -and
    $driverPropertiesRaw -match '(?m)^\s*driver\.pool\.maxConnections\s*='
  )

  $configExit = if ($contractChecks.json -and $contractChecks.yaml -and $contractChecks.properties) { 0 } else { 1 }
  $exitCode = if ($testExit -eq 0 -and $configExit -eq 0) { 0 } else { 1 }
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$status = if ($exitCode -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws10-driver-baseline"
  status = $status
  command = $command
  config_contract_checks = $contractChecks
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS10 driver smoke failed."
  exit 1
}

Write-Host "WS10 driver smoke passed. Artifact: $OutputPath"
