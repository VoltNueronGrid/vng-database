param(
  [string]$OutputPath = "tests/kpi/results/h04/h04-service-integrated-outbox-runtime.json",
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$AdminApiKey = "h04-runtime-key",
  [int]$Port = 18241
)

$ErrorActionPreference = "Stop"

$kpiScriptsRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $kpiScriptsRoot "kpi-http-helpers.ps1")
$PSDefaultParameterValues['Invoke-HttpJson:TimeoutSec'] = 15

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

function Start-ServiceProcess {
  param(
    [string]$WorkingDirectory,
    [int]$BindPort,
    [string]$AdminKey,
    [string]$LogPath,
    [string]$OutboxWalPath,
    [string]$CursorWalPath
  )

  $command = 'set VNG_NODE_ID=node-h04 && set VNG_CLUSTER_MODE=single && set VNG_HTTP_BIND=127.0.0.1:' + $BindPort + ' && set VNG_ADMIN_API_KEY=' + $AdminKey + ' && set VNG_INGEST_OUTBOX_BROKER_MODE=file_wal && set VNG_INGEST_OUTBOX_WAL_PATH=' + $OutboxWalPath + ' && set VNG_INGEST_OUTBOX_CURSOR_WAL_PATH=' + $CursorWalPath + ' && target\debug\voltnuerongridd.exe > "' + $LogPath + '" 2>&1'
  Start-Process -FilePath "cmd.exe" -ArgumentList "/c", $command -WorkingDirectory $WorkingDirectory -PassThru -WindowStyle Hidden
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
$stateDir = Join-Path (Split-Path -Parent $OutputPath) "runtime-state"
Ensure-OutputDir -PathValue (Join-Path $stateDir "placeholder.wal")

$baseUrl = "http://127.0.0.1:$Port"
$logPath = Join-Path $logsDir "h04-service-runtime.log"
$runSuffix = "{0}-{1}" -f $PID, ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
$outboxWalPath = Join-Path $stateDir ("h04-ingest-outbox-" + $runSuffix + ".wal")
$cursorWalPath = Join-Path $stateDir ("h04-ingest-cursors-" + $runSuffix + ".wal")
$process = $null
$status = "passed"
$checks = [ordered]@{}
$observations = [ordered]@{}

try {
  $build = Invoke-CargoCapture -Command { cargo build -p voltnuerongridd }
  if ($build.ExitCode -ne 0) {
    throw ((@($build.Output) | Select-Object -First 20) -join "`n")
  }

  $process = Start-ServiceProcess -WorkingDirectory $RepoRoot -BindPort $Port -AdminKey $AdminApiKey -LogPath $logPath -OutboxWalPath $outboxWalPath -CursorWalPath $cursorWalPath
  $health = Wait-ForHealth -BaseUrl $baseUrl

  $tenantHeaders = @{
    "x-vng-tenant-id" = "acme"
    "x-vng-user-id" = "analyst-acme"
  }

  $ingest = Invoke-HttpJson -Method Post -Uri "$baseUrl/api/v1/ingest/csv" -Headers $tenantHeaders -Body @{
    connector_id = "orders"
    csv_data = "id,value`n1,a`n2,b`n"
  }
  $outboxStatus = Invoke-HttpJson -Method Get -Uri "$baseUrl/api/v1/ingest/outbox/status" -Headers $tenantHeaders
  $firstReplay = Invoke-HttpJson -Method Post -Uri "$baseUrl/api/v1/ingest/outbox/replay" -Headers $tenantHeaders -Body @{
    connector_id = "orders"
    consumer_id = "projection-a"
    max_items = 10
    acknowledge = $true
  }
  $secondReplay = Invoke-HttpJson -Method Post -Uri "$baseUrl/api/v1/ingest/outbox/replay" -Headers $tenantHeaders -Body @{
    connector_id = "orders"
    consumer_id = "projection-a"
    max_items = 10
    acknowledge = $true
  }
  $independentReplay = Invoke-HttpJson -Method Post -Uri "$baseUrl/api/v1/ingest/outbox/replay" -Headers $tenantHeaders -Body @{
    connector_id = "orders"
    consumer_id = "projection-b"
    max_items = 10
    acknowledge = $true
  }

  $checks.service_health_ok = ($health.status -eq "ok")
  $checks.ingest_request_succeeds = ($ingest.StatusCode -eq 200 -and $ingest.Json.status -eq "ok" -and [int]$ingest.Json.records_parsed -eq 2)
  $checks.outbox_status_reports_events = ($outboxStatus.StatusCode -eq 200 -and $outboxStatus.Json.status -eq "ok" -and [int]$outboxStatus.Json.total_events -eq 2 -and [int]$outboxStatus.Json.stream_count -eq 1)
  $checks.outbox_status_reports_broker_mode = ([string]$outboxStatus.Json.broker_mode -eq "file_wal")
  $checks.first_consumer_replay_delivers_events = ($firstReplay.StatusCode -eq 200 -and [int]$firstReplay.Json.delivered_count -eq 2 -and [string]$firstReplay.Json.delivery_state -eq "delivered_and_acked")
  $checks.first_consumer_second_replay_is_empty = ($secondReplay.StatusCode -eq 200 -and [int]$secondReplay.Json.delivered_count -eq 0 -and [string]$secondReplay.Json.delivery_state -eq "already_acknowledged")
  $checks.second_consumer_replays_independently = ($independentReplay.StatusCode -eq 200 -and [int]$independentReplay.Json.delivered_count -eq 2)

  $observations.health = $health
  $observations.runtime_state = [ordered]@{
    outbox_wal_path = $outboxWalPath
    cursor_wal_path = $cursorWalPath
  }
  $observations.ingest = $ingest.Json
  $observations.outbox_status = $outboxStatus.Json
  $observations.first_replay = $firstReplay.Json
  $observations.second_replay = $secondReplay.Json
  $observations.independent_replay = $independentReplay.Json

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
  smoke = "h04-service-integrated-outbox-runtime"
  status = $status
  hardening_item = "H-04"
  base_url = $baseUrl
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  observations = $observations
}

$artifact | ConvertTo-Json -Depth 12 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "H04 service-integrated outbox runtime smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }