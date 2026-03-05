param(
  [string]$OutputPath = "tests/kpi/results/ws5/operator-auth-smoke.json"
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
$command = "cargo test -p voltnuerongridd operator_auth; cargo test -p voltnuerongrid-auth validates_security_config; cargo test -p voltnuerongrid-auth ws5_; security contract checks (json/yaml/properties)"
$outputLines = @()
$exitCode = 1
$securityContractChecks = [ordered]@{
  json_tls = $false
  json_encryption = $false
  json_kms = $false
  yaml_tls = $false
  yaml_encryption = $false
  yaml_kms = $false
  properties_tls = $false
  properties_encryption = $false
  properties_kms = $false
}

try {
  $first = & cargo test -p voltnuerongridd operator_auth 2>&1
  $firstExit = $LASTEXITCODE
  $second = & cargo test -p voltnuerongrid-auth validates_security_config 2>&1
  $secondExit = $LASTEXITCODE
  $third = & cargo test -p voltnuerongrid-auth ws5_ 2>&1
  $thirdExit = $LASTEXITCODE
  $outputLines = @($first + $second + $third)

  $jsonRaw = Get-Content -Raw -Path "reference/config-contracts/ws14/security-control-config.json"
  $yamlRaw = Get-Content -Raw -Path "reference/config-contracts/ws14/security-control-config.yaml"
  $propsRaw = Get-Content -Raw -Path "reference/config-contracts/ws14/security-control-config.properties"

  $securityContractChecks.json_tls = ($jsonRaw -match '"tlsRequired"\s*:\s*true')
  $securityContractChecks.json_encryption = ($jsonRaw -match '"encryptionAtRestRequired"\s*:\s*true')
  $securityContractChecks.json_kms = ($jsonRaw -match '"kmsKeyRefEnv"\s*:\s*"[^"]+"')
  $securityContractChecks.yaml_tls = ($yamlRaw -match '(?m)^\s*tlsRequired\s*:\s*true\s*$')
  $securityContractChecks.yaml_encryption = ($yamlRaw -match '(?m)^\s*encryptionAtRestRequired\s*:\s*true\s*$')
  $securityContractChecks.yaml_kms = ($yamlRaw -match '(?m)^\s*kmsKeyRefEnv\s*:\s*".+"\s*$')
  $securityContractChecks.properties_tls = ($propsRaw -match '(?m)^\s*security\.tlsRequired\s*=\s*true\s*$')
  $securityContractChecks.properties_encryption = ($propsRaw -match '(?m)^\s*security\.encryptionAtRestRequired\s*=\s*true\s*$')
  $securityContractChecks.properties_kms = ($propsRaw -match '(?m)^\s*security\.kmsKeyRefEnv\s*=\s*.+\s*$')

  $contractExit = if (
    $securityContractChecks.json_tls -and
    $securityContractChecks.json_encryption -and
    $securityContractChecks.json_kms -and
    $securityContractChecks.yaml_tls -and
    $securityContractChecks.yaml_encryption -and
    $securityContractChecks.yaml_kms -and
    $securityContractChecks.properties_tls -and
    $securityContractChecks.properties_encryption -and
    $securityContractChecks.properties_kms
  ) { 0 } else { 1 }

  $exitCode = if ($firstExit -eq 0 -and $secondExit -eq 0 -and $thirdExit -eq 0 -and $contractExit -eq 0) { 0 } else { 1 }
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$status = if ($exitCode -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws5-operator-auth-baseline"
  status = $status
  command = $command
  security_contract_checks = $securityContractChecks
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS5 operator-auth smoke failed."
  exit 1
}

Write-Host "WS5 operator-auth smoke passed. Artifact: $OutputPath"
