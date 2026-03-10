param(
  [string]$OutputPath = "tests/kpi/results/h05/h05-kms-region-failover-runtime-smoke.json",
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$AdminApiKey = "h05-runtime-key",
  [int]$Port = 18251,
  [string]$PrimaryKmsKeyRef = "",
  [string]$SecondaryKmsKeyRef = "",
  [string]$TertiaryKmsKeyRef = ""
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Invoke-CargoCapture {
  param([scriptblock]$Command)

  $previousPreference = $ErrorActionPreference
  try {
    $ErrorActionPreference = "Continue"
    $global:LASTEXITCODE = 0
    $output = & $Command 2>&1
    $exitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previousPreference
  }

  return [pscustomobject]@{
    Output = @($output)
    ExitCode = $exitCode
  }
}

function Invoke-HttpJson {
  param(
    [string]$Method,
    [string]$Uri,
    [hashtable]$Headers,
    [object]$Body = $null
  )

  $params = @{
    Method = $Method
    Uri = $Uri
    TimeoutSec = 15
    UseBasicParsing = $true
  }
  if ($Headers) { $params.Headers = $Headers }
  if ($null -ne $Body) {
    $params.Body = ($Body | ConvertTo-Json -Depth 8)
    $params.ContentType = "application/json"
  }

  try {
    $response = Invoke-WebRequest @params
    $json = if ($response.Content) { $response.Content | ConvertFrom-Json } else { $null }
    return [pscustomobject]@{ StatusCode = [int]$response.StatusCode; Json = $json; Content = $response.Content }
  } catch {
    $statusCode = 0
    $content = ""
    if ($_.Exception.Response) {
      $statusCode = [int]$_.Exception.Response.StatusCode.value__
      $reader = New-Object System.IO.StreamReader($_.Exception.Response.GetResponseStream())
      $content = $reader.ReadToEnd()
      $reader.Close()
    }
    $json = if ($content) { try { $content | ConvertFrom-Json } catch { $null } } else { $null }
    return [pscustomobject]@{ StatusCode = $statusCode; Json = $json; Content = $content }
  }
}

function Start-ServiceProcess {
  param(
    [string]$WorkingDirectory,
    [int]$BindPort,
    [string]$AdminKey,
    [string]$LogPath,
    [string]$PrimaryKeyRef,
    [string]$SecondaryKeyRef,
    [string]$TertiaryKeyRef
  )

  $command = 'set VNG_NODE_ID=node-h05 && set VNG_CLUSTER_MODE=single && set VNG_HTTP_BIND=127.0.0.1:' + $BindPort + ' && set VNG_ADMIN_API_KEY=' + $AdminKey + ' && set VNG_KMS_KEY_URI=' + $PrimaryKeyRef + ' && set VNG_KMS_KEY_URI_REGION_B=' + $SecondaryKeyRef + ' && set VNG_KMS_KEY_URI_REGION_C=' + $TertiaryKeyRef + ' && set VNG_KMS_FAILOVER_KEY_REF_ENVS=VNG_KMS_KEY_URI_REGION_B,VNG_KMS_KEY_URI_REGION_C && target\debug\voltnuerongridd.exe > "' + $LogPath + '" 2>&1'
  Start-Process -FilePath "cmd.exe" -ArgumentList "/c", $command -WorkingDirectory $WorkingDirectory -PassThru -WindowStyle Hidden
}

function Get-ProviderDrillMode {
  param([string[]]$KeyRefs)

  foreach ($keyRef in $KeyRefs) {
    if ([string]::IsNullOrWhiteSpace($keyRef)) {
      continue
    }
    if ($keyRef.StartsWith("arn:aws:kms:") -or $keyRef.StartsWith("aws-kms://") -or $keyRef.StartsWith("azure-kms://") -or $keyRef.Contains(".vault.azure.net/keys/") -or $keyRef.StartsWith("gcp-kms://") -or $keyRef.StartsWith("projects/")) {
      return "provider_cli"
    }
  }
  return "generic_runtime"
}

function Stop-ProcessTree {
  param([System.Diagnostics.Process]$RootProcess)

  if ($null -eq $RootProcess) {
    return
  }

  try {
    $children = Get-CimInstance Win32_Process -Filter "ParentProcessId=$($RootProcess.Id)"
    foreach ($child in @($children)) {
      Stop-Process -Id $child.ProcessId -Force -ErrorAction SilentlyContinue
    }
  } catch {
  }

  try {
    if (-not $RootProcess.HasExited) {
      Stop-Process -Id $RootProcess.Id -Force -ErrorAction SilentlyContinue
    }
  } catch {
  }
}

function Wait-ForHealth {
  param([string]$BaseUrl)

  $deadline = (Get-Date).AddSeconds(90)
  do {
    Start-Sleep -Milliseconds 750
    $response = Invoke-HttpJson -Method Get -Uri "$BaseUrl/health" -Headers @{}
    if ($response.StatusCode -eq 200 -and $response.Json.status -eq "ok") {
      return $response.Json
    }
  } while ((Get-Date) -lt $deadline)

  throw "Timed out waiting for service health at $BaseUrl"
}

Ensure-OutputDir -PathValue $OutputPath
$logsDir = Join-Path (Split-Path -Parent $OutputPath) "runtime-logs"
Ensure-OutputDir -PathValue (Join-Path $logsDir "placeholder.log")

$baseUrl = "http://127.0.0.1:$Port"
$logPath = Join-Path $logsDir "h05-service-runtime.log"
$primaryKeyRef = if (![string]::IsNullOrWhiteSpace($PrimaryKmsKeyRef)) { $PrimaryKmsKeyRef } elseif (![string]::IsNullOrWhiteSpace($env:VNG_KMS_KEY_URI)) { $env:VNG_KMS_KEY_URI } else { "kms://region-a/key-primary" }
$secondaryKeyRef = if (![string]::IsNullOrWhiteSpace($SecondaryKmsKeyRef)) { $SecondaryKmsKeyRef } elseif (![string]::IsNullOrWhiteSpace($env:VNG_KMS_KEY_URI_REGION_B)) { $env:VNG_KMS_KEY_URI_REGION_B } else { "kms://region-b/key-secondary" }
$tertiaryKeyRef = if (![string]::IsNullOrWhiteSpace($TertiaryKmsKeyRef)) { $TertiaryKmsKeyRef } elseif (![string]::IsNullOrWhiteSpace($env:VNG_KMS_KEY_URI_REGION_C)) { $env:VNG_KMS_KEY_URI_REGION_C } else { "kms://region-c/key-tertiary" }
$providerDrillMode = Get-ProviderDrillMode -KeyRefs @($primaryKeyRef, $secondaryKeyRef, $tertiaryKeyRef)
$process = $null
$status = "passed"
$checks = [ordered]@{}
$observations = [ordered]@{}

try {
  $build = Invoke-CargoCapture -Command { cargo build -p voltnuerongridd }
  if ($build.ExitCode -ne 0) {
    throw ((@($build.Output) | Select-Object -First 20) -join "`n")
  }

  $process = Start-ServiceProcess -WorkingDirectory $RepoRoot -BindPort $Port -AdminKey $AdminApiKey -LogPath $logPath -PrimaryKeyRef $primaryKeyRef -SecondaryKeyRef $secondaryKeyRef -TertiaryKeyRef $tertiaryKeyRef
  $health = Wait-ForHealth -BaseUrl $baseUrl

  $headers = @{
    "x-vng-admin-key" = $AdminApiKey
    "x-vng-operator-id" = "security-bot"
  }

  $primary = Invoke-HttpJson -Method Get -Uri "$baseUrl/api/v1/security/kms/status" -Headers $headers
  $failover = Invoke-HttpJson -Method Post -Uri "$baseUrl/api/v1/security/kms/outage/simulate" -Headers $headers -Body @{
    unavailable_envs = @("VNG_KMS_KEY_URI")
    note = "primary_region_outage"
  }
  $allOut = Invoke-HttpJson -Method Post -Uri "$baseUrl/api/v1/security/kms/outage/simulate" -Headers $headers -Body @{
    unavailable_envs = @("VNG_KMS_KEY_URI", "VNG_KMS_KEY_URI_REGION_B", "VNG_KMS_KEY_URI_REGION_C")
    note = "all_regions_outage"
  }
  $reconcile = Invoke-HttpJson -Method Post -Uri "$baseUrl/api/v1/security/kms/outage/reconcile" -Headers $headers -Body @{
    note = "restore_all_regions"
  }

  $checks.service_health_ok = ($health.status -eq "ok")
  $checks.primary_status_uses_primary_region = ($primary.StatusCode -eq 200 -and $primary.Json.status -eq "ok" -and [string]$primary.Json.selected_env -eq "VNG_KMS_KEY_URI" -and -not [bool]$primary.Json.failover_used)
  $checks.primary_outage_fails_over_to_secondary = ($failover.StatusCode -eq 200 -and [string]$failover.Json.resolution_state -eq "failover_active" -and [string]$failover.Json.selected_env -eq "VNG_KMS_KEY_URI_REGION_B" -and [bool]$failover.Json.failover_used)
  $checks.all_regions_outage_reports_unresolved = ($allOut.StatusCode -eq 200 -and [string]$allOut.Json.resolution_state -eq "unresolved" -and $null -eq $allOut.Json.selected_env)
  $checks.reconcile_restores_primary_region = ($reconcile.StatusCode -eq 200 -and [string]$reconcile.Json.selected_env -eq "VNG_KMS_KEY_URI" -and -not [bool]$reconcile.Json.failover_used)

  $observations.health = $health
  $observations.provider_drill_mode = $providerDrillMode
  $observations.configured_key_refs = @($primaryKeyRef, $secondaryKeyRef, $tertiaryKeyRef)
  $observations.primary = $primary.Json
  $observations.failover = $failover.Json
  $observations.all_regions_out = $allOut.Json
  $observations.reconcile = $reconcile.Json

  if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -gt 0) {
    $status = "failed"
  }
} catch {
  $status = "failed"
  $observations.error = $_.Exception.Message
} finally {
  Stop-ProcessTree -RootProcess $process
}

$artifact = [ordered]@{
  smoke = "h05-kms-region-failover-runtime"
  status = $status
  hardening_item = "H-05"
  base_url = $baseUrl
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  observations = $observations
}

$artifact | ConvertTo-Json -Depth 12 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "H05 KMS runtime failover smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }