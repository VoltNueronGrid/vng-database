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
$command = "cargo test -p voltnuerongrid-driver-rust; driver request/header and config contract checks"
$outputLines = @()
$exitCode = 1
$contractChecks = [ordered]@{
  json = $false
  yaml = $false
  properties = $false
}

try {
  $testRun = Invoke-CargoTestCapture -Arguments @("test", "-p", "voltnuerongrid-driver-rust")
  $outputLines = @($testRun.Text)

  $driverJsonRaw = Get-Content -Raw -Path "reference/config-contracts/ws14/driver-routing-config.json"
  $driverYamlRaw = Get-Content -Raw -Path "reference/config-contracts/ws14/driver-routing-config.yaml"
  $driverPropertiesRaw = Get-Content -Raw -Path "reference/config-contracts/ws14/driver-routing-config.properties"

  $contractChecks.json = (
    $driverJsonRaw -match '"driver"\s*:\s*\{' -and
    $driverJsonRaw -match '"baseUrl"\s*:\s*"http://127\.0\.0\.1:8080"' -and
    $driverJsonRaw -match '"tenantHeaderName"\s*:\s*"x-vng-tenant-id"' -and
    $driverJsonRaw -match '"userHeaderName"\s*:\s*"x-vng-user-id"' -and
    $driverJsonRaw -match '"minConnections"\s*:\s*2' -and
    $driverJsonRaw -match '"maxConnections"\s*:\s*16'
  )
  $contractChecks.yaml = (
    $driverYamlRaw -match '(?m)^driver:\s*$' -and
    $driverYamlRaw -match '(?m)^\s+baseUrl\s*:\s*"?http://127\.0\.0\.1:8080"?' -and
    $driverYamlRaw -match '(?m)^\s+tenantHeaderName\s*:\s*"?x-vng-tenant-id"?' -and
    $driverYamlRaw -match '(?m)^\s+userHeaderName\s*:\s*"?x-vng-user-id"?' -and
    $driverYamlRaw -match '(?m)^\s+pool:\s*$' -and
    $driverYamlRaw -match '(?m)^\s+\s+minConnections\s*:\s*2\s*$' -and
    $driverYamlRaw -match '(?m)^\s+\s+maxConnections\s*:\s*16\s*$'
  )
  $contractChecks.properties = (
    $driverPropertiesRaw -match '(?m)^\s*driver\.baseUrl\s*=' -and
    $driverPropertiesRaw -match '(?m)^\s*driver\.pool\.maxConnections\s*=' -and
    $driverPropertiesRaw -match '(?m)^\s*driver\.tenantHeaderName\s*=\s*x-vng-tenant-id' -and
    $driverPropertiesRaw -match '(?m)^\s*driver\.userHeaderName\s*=\s*x-vng-user-id'
  )

  $configExit = if ($contractChecks.json -and $contractChecks.yaml -and $contractChecks.properties) { 0 } else { 1 }
  $exitCode = if ($testRun.Ok -and $configExit -eq 0) { 0 } else { 1 }
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
