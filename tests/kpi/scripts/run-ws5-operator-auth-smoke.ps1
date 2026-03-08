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

function Invoke-CargoTestCapture {
  param([string[]]$Arguments)

  $tempFile = [System.IO.Path]::GetTempFileName()
  try {
    $commandText = "cargo " + (($Arguments | ForEach-Object {
      if ($_ -match "\s") { '"' + $_ + '"' } else { $_ }
    }) -join " ")
    $process = Start-Process -FilePath "cmd.exe" -ArgumentList "/c", "$commandText > `"$tempFile`" 2>&1" -Wait -PassThru -NoNewWindow
    $text = if (Test-Path -Path $tempFile) { Get-Content -Path $tempFile -Raw } else { "" }
    $ok = ($text -match "test result: ok\." -and $text -notmatch "test result: FAILED" -and $text -notmatch "(?m)^error:")
    return [pscustomobject]@{
      Ok = $ok
      Text = $text
      ExitCode = $process.ExitCode
    }
  } finally {
    if (Test-Path -Path $tempFile) {
      Remove-Item -Path $tempFile -Force -ErrorAction SilentlyContinue
    }
  }
}

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
  $first = Invoke-CargoTestCapture -Arguments @("test", "-p", "voltnuerongridd", "operator_auth")
  $second = Invoke-CargoTestCapture -Arguments @("test", "-p", "voltnuerongrid-auth", "validates_security_config")
  $third = Invoke-CargoTestCapture -Arguments @("test", "-p", "voltnuerongrid-auth", "ws5_")
  $outputLines = @($first.Text + $second.Text + $third.Text)

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

  $exitCode = if ($first.Ok -and $second.Ok -and $third.Ok -and $contractExit -eq 0) { 0 } else { 1 }
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
