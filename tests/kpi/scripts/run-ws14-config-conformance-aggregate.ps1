param(
  [string]$OutputPath = "tests/kpi/results/ws14/config-conformance-aggregate.json"
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

function Parse-Properties {
  param([string]$Path)
  $result = @{}
  foreach ($line in Get-Content -Path $Path) {
    $trimmed = $line.Trim()
    if ([string]::IsNullOrWhiteSpace($trimmed)) { continue }
    if ($trimmed.StartsWith("#")) { continue }
    $parts = $trimmed -split "=", 2
    if ($parts.Length -eq 2) {
      $result[$parts[0].Trim()] = $parts[1].Trim()
    }
  }
  return $result
}

Ensure-OutputDir -PathValue $OutputPath
$start = Get-Date
$checks = @()

$driverYamlPath = "reference/config-contracts/ws14/driver-routing-config.yaml"
$driverJsonPath = "reference/config-contracts/ws14/driver-routing-config.json"
$driverPropsPath = "reference/config-contracts/ws14/driver-routing-config.properties"
$securityYamlPath = "reference/config-contracts/ws14/security-control-config.yaml"
$securityJsonPath = "reference/config-contracts/ws14/security-control-config.json"
$securityPropsPath = "reference/config-contracts/ws14/security-control-config.properties"

$driverJson = Get-Content -Raw -Path $driverJsonPath | ConvertFrom-Json
$securityJson = Get-Content -Raw -Path $securityJsonPath | ConvertFrom-Json
$driverProps = Parse-Properties -Path $driverPropsPath
$securityProps = Parse-Properties -Path $securityPropsPath
$driverYaml = Get-Content -Raw -Path $driverYamlPath
$securityYaml = Get-Content -Raw -Path $securityYamlPath

# Driver conformance checks across JSON / YAML / properties
Add-Check -Name "driver_baseurl_json_vs_props" -Ok ($driverJson.driver.baseUrl -eq $driverProps["driver.baseUrl"]) -Detail "baseUrl parity"
Add-Check -Name "driver_baseurl_json_vs_yaml" -Ok ($driverYaml -match ("baseUrl:\s*`"" + [regex]::Escape([string]$driverJson.driver.baseUrl) + "`"")) -Detail "baseUrl present in YAML"
Add-Check -Name "driver_pool_max_json_vs_props" -Ok ([string]$driverJson.driver.pool.maxConnections -eq $driverProps["driver.pool.maxConnections"]) -Detail "pool.maxConnections parity"
Add-Check -Name "driver_pool_min_json_vs_props" -Ok ([string]$driverJson.driver.pool.minConnections -eq $driverProps["driver.pool.minConnections"]) -Detail "pool.minConnections parity"
Add-Check -Name "driver_timeout_json_vs_props" -Ok ([string]$driverJson.driver.requestTimeoutMs -eq $driverProps["driver.requestTimeoutMs"]) -Detail "requestTimeoutMs parity"

# Security conformance checks across JSON / YAML / properties
Add-Check -Name "security_admin_env_json_vs_props" -Ok ($securityJson.security.adminApiKeyEnv -eq $securityProps["security.adminApiKeyEnv"]) -Detail "adminApiKeyEnv parity"
Add-Check -Name "security_admin_env_json_vs_yaml" -Ok ($securityYaml -match ("adminApiKeyEnv:\s*`"" + [regex]::Escape([string]$securityJson.security.adminApiKeyEnv) + "`"")) -Detail "adminApiKeyEnv present in YAML"
Add-Check -Name "security_tls_json_vs_props" -Ok (([string]$securityJson.security.tlsRequired).ToLower() -eq ([string]$securityProps["security.tlsRequired"]).ToLower()) -Detail "tlsRequired parity"
Add-Check -Name "security_mtls_json_vs_props" -Ok (([string]$securityJson.security.mtlsRequired).ToLower() -eq ([string]$securityProps["security.mtlsRequired"]).ToLower()) -Detail "mtlsRequired parity"
Add-Check -Name "security_encryption_json_vs_props" -Ok (([string]$securityJson.security.encryptionAtRestRequired).ToLower() -eq ([string]$securityProps["security.encryptionAtRestRequired"]).ToLower()) -Detail "encryptionAtRestRequired parity"
Add-Check -Name "security_kms_json_vs_props" -Ok ([string]$securityJson.security.kmsKeyRefEnv -eq [string]$securityProps["security.kmsKeyRefEnv"]) -Detail "kmsKeyRefEnv parity"
Add-Check -Name "security_encryption_json_vs_yaml" -Ok ($securityYaml -match ("encryptionAtRestRequired:\s*" + ([string]$securityJson.security.encryptionAtRestRequired).ToLower())) -Detail "encryptionAtRestRequired present in YAML"
Add-Check -Name "security_kms_json_vs_yaml" -Ok ($securityYaml -match ("kmsKeyRefEnv:\s*`"" + [regex]::Escape([string]$securityJson.security.kmsKeyRefEnv) + "`"")) -Detail "kmsKeyRefEnv present in YAML"
Add-Check -Name "security_ttl_json_vs_props" -Ok ([string]$securityJson.security.tokenTtlSeconds -eq $securityProps["security.tokenTtlSeconds"]) -Detail "tokenTtlSeconds parity"

$rolesFromJson = @($securityJson.security.allowedOperatorRoles)
$rolesFromProps = @(($securityProps["security.allowedOperatorRoles"] -split ",") | ForEach-Object { $_.Trim() })
$rolesParity = ($rolesFromJson.Count -eq $rolesFromProps.Count) -and (($rolesFromJson | Sort-Object) -join "," -eq ($rolesFromProps | Sort-Object) -join ",")
Add-Check -Name "security_roles_json_vs_props" -Ok $rolesParity -Detail "allowedOperatorRoles parity"

$passedCount = @($checks | Where-Object { $_.ok }).Count
$totalCount = $checks.Count
$conformancePercent = if ($totalCount -eq 0) { 0 } else { [math]::Round(($passedCount * 100.0) / $totalCount, 2) }
$status = if ($passedCount -eq $totalCount) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws14-config-conformance-aggregate"
  status = $status
  conformance_percent = $conformancePercent
  passed_checks = $passedCount
  total_checks = $totalCount
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS14 config conformance aggregate failed."
  exit 1
}

Write-Host "WS14 config conformance aggregate passed. Artifact: $OutputPath"
