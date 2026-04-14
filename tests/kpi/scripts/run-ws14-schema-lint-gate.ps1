param(
  [string]$OutputPath = "tests/kpi/results/ws14/schema-lint-gate.json"
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

function Resolve-FirstExistingPath {
  param([string[]]$Candidates)
  $valid = @($Candidates | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
  $found = $valid | Where-Object { Test-Path $_ } | Select-Object -First 1
  if ([string]::IsNullOrWhiteSpace($found)) {
    return $valid[0]
  }
  return $found
}

Ensure-OutputDir -PathValue $OutputPath
$start = Get-Date
$checks = @()

$files = @{
  driverYaml = Resolve-FirstExistingPath -Candidates @(
    "reference/config-contracts/ws14/driver-routing-config.yaml",
    "services/voltnuerongridd/reference/config-contracts/ws14/driver-routing-config.yaml"
  )
  driverJson = Resolve-FirstExistingPath -Candidates @(
    "reference/config-contracts/ws14/driver-routing-config.json",
    "services/voltnuerongridd/reference/config-contracts/ws14/driver-routing-config.json"
  )
  driverProps = Resolve-FirstExistingPath -Candidates @(
    "reference/config-contracts/ws14/driver-routing-config.properties",
    "services/voltnuerongridd/reference/config-contracts/ws14/driver-routing-config.properties"
  )
  securityYaml = Resolve-FirstExistingPath -Candidates @(
    "reference/config-contracts/ws14/security-control-config.yaml",
    "services/voltnuerongridd/reference/config-contracts/ws14/security-control-config.yaml"
  )
  securityJson = Resolve-FirstExistingPath -Candidates @(
    "reference/config-contracts/ws14/security-control-config.json",
    "services/voltnuerongridd/reference/config-contracts/ws14/security-control-config.json"
  )
  securityProps = Resolve-FirstExistingPath -Candidates @(
    "reference/config-contracts/ws14/security-control-config.properties",
    "services/voltnuerongridd/reference/config-contracts/ws14/security-control-config.properties"
  )
}

foreach ($k in $files.Keys) {
  Add-Check -Name ("exists_" + $k) -Ok (Test-Path $files[$k]) -Detail $files[$k]
}

if (Test-Path $files.driverJson) {
  try {
    $j = Get-Content -Raw -Path $files.driverJson | ConvertFrom-Json
    Add-Check -Name "driver_json_schema" -Ok ($null -ne $j.driver.pool.maxConnections) -Detail "driver.pool.maxConnections exists"
  } catch {
    Add-Check -Name "driver_json_schema" -Ok $false -Detail $_.Exception.Message
  }
}

if (Test-Path $files.securityJson) {
  try {
    $j = Get-Content -Raw -Path $files.securityJson | ConvertFrom-Json
    Add-Check -Name "security_json_schema" -Ok (
      $null -ne $j.security.allowedOperatorRoles -and
      $null -ne $j.security.encryptionAtRestRequired -and
      $null -ne $j.security.kmsKeyRefEnv
    ) -Detail "security allowedOperatorRoles + encryptionAtRestRequired + kmsKeyRefEnv exist"
  } catch {
    Add-Check -Name "security_json_schema" -Ok $false -Detail $_.Exception.Message
  }
}

if (Test-Path $files.driverYaml) {
  $y = Get-Content -Raw -Path $files.driverYaml
  Add-Check -Name "driver_yaml_has_driver_root" -Ok ($y -match "(?m)^driver:") -Detail "driver root"
  Add-Check -Name "driver_yaml_has_pool_root" -Ok ($y -match "(?m)^\s{2}pool:") -Detail "pool root"
  Add-Check -Name "driver_yaml_has_timeout" -Ok ($y -match "requestTimeoutMs:\s*\d+") -Detail "requestTimeoutMs numeric"
}

if (Test-Path $files.securityYaml) {
  $y = Get-Content -Raw -Path $files.securityYaml
  Add-Check -Name "security_yaml_has_security_root" -Ok ($y -match "(?m)^security:") -Detail "security root"
  Add-Check -Name "security_yaml_has_roles" -Ok ($y -match "(?m)^\s{2}allowedOperatorRoles:") -Detail "allowedOperatorRoles root"
  Add-Check -Name "security_yaml_has_encryption_required" -Ok ($y -match "(?m)^\s{2}encryptionAtRestRequired:\s*(true|false)\s*$") -Detail "encryptionAtRestRequired boolean"
  Add-Check -Name "security_yaml_has_kms_ref" -Ok ($y -match "(?m)^\s{2}kmsKeyRefEnv:\s*.+$") -Detail "kmsKeyRefEnv present"
  Add-Check -Name "security_yaml_has_token_ttl" -Ok ($y -match "tokenTtlSeconds:\s*\d+") -Detail "tokenTtlSeconds numeric"
}

if (Test-Path $files.driverProps) {
  $p = Get-Content -Path $files.driverProps
  $validLines = @($p | Where-Object { $_ -match "^\s*[a-zA-Z0-9_.-]+\s*=\s*.+$" })
  Add-Check -Name "driver_properties_kv_format" -Ok ($validLines.Count -eq $p.Count) -Detail "all lines are key=value"
}

if (Test-Path $files.securityProps) {
  $p = Get-Content -Path $files.securityProps
  $validLines = @($p | Where-Object { $_ -match "^\s*[a-zA-Z0-9_.-]+\s*=\s*.+$" })
  Add-Check -Name "security_properties_kv_format" -Ok ($validLines.Count -eq $p.Count) -Detail "all lines are key=value"
}

$status = if (($checks | Where-Object { -not $_.ok }).Count -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws14-schema-lint-gate"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS14 schema lint gate failed."
  exit 1
}

Write-Host "WS14 schema lint gate passed. Artifact: $OutputPath"
