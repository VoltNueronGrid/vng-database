param(
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-process-isolated-cluster-chaos-smoke.json",
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$AdminApiKey = "ws6-process-chaos-key"
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
    TimeoutSec = 10
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

function Start-NodeProcess {
  param(
    [string]$NodeId,
    [int]$Port,
    [string]$LogPath
  )

  $command = 'set VNG_NODE_ID=' + $NodeId + ' && set VNG_CLUSTER_MODE=multi && set VNG_HTTP_BIND=127.0.0.1:' + $Port + ' && set VNG_ADMIN_API_KEY=' + $AdminApiKey + ' && target\debug\voltnuerongridd.exe > "' + $LogPath + '" 2>&1'
  return Start-Process -FilePath "cmd.exe" -ArgumentList "/c", $command -WorkingDirectory $RepoRoot -PassThru -WindowStyle Hidden
}

function Stop-NodeProcessTree {
  param([System.Diagnostics.Process]$RootProcess)

  if ($null -eq $RootProcess) {
    return
  }

  try {
    $children = Get-CimInstance Win32_Process -Filter "ParentProcessId=$($RootProcess.Id)"
    foreach ($child in @($children)) {
      try {
        Stop-Process -Id $child.ProcessId -Force -ErrorAction SilentlyContinue
      } catch {
      }
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
  param(
    [string]$BaseUrl,
    [string]$ExpectedNodeId
  )

  $deadline = (Get-Date).AddSeconds(90)
  do {
    Start-Sleep -Milliseconds 750
    $response = Invoke-HttpJson -Method Get -Uri "$BaseUrl/health" -Headers @{}
    if ($response.StatusCode -eq 200 -and $response.Json.node_id -eq $ExpectedNodeId) {
      return $response.Json
    }
  } while ((Get-Date) -lt $deadline)

  throw "Timed out waiting for $ExpectedNodeId health at $BaseUrl"
}

Ensure-OutputDir -PathValue $OutputPath
$logsDir = Join-Path (Split-Path -Parent $OutputPath) "process-isolated-logs"
Ensure-OutputDir -PathValue (Join-Path $logsDir "placeholder.log")

$headers = @{
  "x-vng-admin-key" = $AdminApiKey
  "x-vng-operator-id" = "platform-admin"
}

$nodes = @(
  [ordered]@{ node_id = "node-1"; base_url = "http://127.0.0.1:18181"; port = 18181 },
  [ordered]@{ node_id = "node-2"; base_url = "http://127.0.0.1:18182"; port = 18182 },
  [ordered]@{ node_id = "node-3"; base_url = "http://127.0.0.1:18183"; port = 18183 }
)

$processes = @()
$status = "passed"
$checks = [ordered]@{}
$observations = [ordered]@{}

try {
  $buildResult = Invoke-CargoCapture -Command { cargo build -p voltnuerongridd }
  if ($buildResult.ExitCode -ne 0) {
    throw ((@($buildResult.Output) | Select-Object -First 20) -join "`n")
  }

  foreach ($node in $nodes) {
    $logPath = Join-Path $logsDir ($node.node_id + ".log")
    $processes += [ordered]@{
      node_id = $node.node_id
      base_url = $node.base_url
      log_path = $logPath
      process = Start-NodeProcess -NodeId $node.node_id -Port $node.port -LogPath $logPath
    }
  }

  $health = @{}
  foreach ($node in $nodes) {
    $health[$node.node_id] = Wait-ForHealth -BaseUrl $node.base_url -ExpectedNodeId $node.node_id
  }

  $node1Signal = Invoke-HttpJson -Method Post -Uri "$($nodes[0].base_url)/api/v1/sre/failure/signal" -Headers $headers -Body @{
    node_id = "node-2"
    transport = "raft"
    failure_type = "leader_heartbeat_timeout"
    severity = "critical"
    message = "process_isolated_node2_unreachable"
  }
  $node1Degraded = Invoke-HttpJson -Method Get -Uri "$($nodes[0].base_url)/api/v1/failover/status" -Headers @{}
  $node1Failover = Invoke-HttpJson -Method Post -Uri "$($nodes[0].base_url)/api/v1/failover/simulate" -Headers $headers -Body @{
    new_leader_node_id = "node-2"
    reason = "process_isolated_cluster_runtime_chaos"
    requested_by = "process-isolated-pack"
  }
  $node1Reconcile = Invoke-HttpJson -Method Post -Uri "$($nodes[0].base_url)/api/v1/sre/failure/reconcile" -Headers $headers -Body @{
    resolve_all_critical = $true
    note = "process_isolated_cluster_runtime_chaos_reconcile"
  }
  $node1Recovered = Invoke-HttpJson -Method Get -Uri "$($nodes[0].base_url)/api/v1/failover/status" -Headers @{}

  $node2Status = Invoke-HttpJson -Method Get -Uri "$($nodes[1].base_url)/api/v1/failover/status" -Headers @{}

  $node3Signal = Invoke-HttpJson -Method Post -Uri "$($nodes[2].base_url)/api/v1/sre/failure/signal" -Headers $headers -Body @{
    node_id = "node-1"
    transport = "raft"
    failure_type = "leader_heartbeat_timeout"
    severity = "critical"
    message = "process_isolated_node1_unreachable"
  }
  $node3Degraded = Invoke-HttpJson -Method Get -Uri "$($nodes[2].base_url)/api/v1/failover/status" -Headers @{}
  $node2AfterNode3Signal = Invoke-HttpJson -Method Get -Uri "$($nodes[1].base_url)/api/v1/failover/status" -Headers @{}
  $node3Reconcile = Invoke-HttpJson -Method Post -Uri "$($nodes[2].base_url)/api/v1/sre/failure/reconcile" -Headers $headers -Body @{
    resolve_all_critical = $true
    note = "process_isolated_cluster_runtime_chaos_reconcile_node3"
  }
  $node3Recovered = Invoke-HttpJson -Method Get -Uri "$($nodes[2].base_url)/api/v1/failover/status" -Headers @{}

  $checks.multi_process_nodes_healthy = (($health.Values | Where-Object { $_.status -ne "ok" }).Count -eq 0)
  $checks.node1_degrades_after_live_signal = ($node1Signal.StatusCode -eq 200 -and $node1Degraded.Json.status -eq "degraded" -and [int]$node1Degraded.Json.unresolved_critical_count -ge 1)
  $checks.node1_failover_executes_live_handoff = ($node1Failover.StatusCode -eq 200 -and $node1Failover.Json.status -eq "ok" -and [string]$node1Failover.Json.new_leader_node_id -eq "node-2" -and $null -ne $node1Failover.Json.handoff_report)
  $checks.node1_reconcile_recovers_health = ($node1Reconcile.StatusCode -eq 200 -and [int]$node1Reconcile.Json.unresolved_critical_count -eq 0 -and $node1Recovered.Json.status -eq "healthy" -and [string]$node1Recovered.Json.leader_node_id -eq "node-2")
  $checks.node2_remains_isolated_from_node1_faults = ($node2Status.StatusCode -eq 200 -and $node2Status.Json.status -eq "healthy" -and [int]$node2Status.Json.unresolved_critical_count -eq 0)
  $checks.node3_degrades_independently = ($node3Signal.StatusCode -eq 200 -and $node3Degraded.Json.status -eq "degraded" -and [int]$node3Degraded.Json.unresolved_critical_count -ge 1)
  $checks.node2_ignores_node3_local_faults = ($node2AfterNode3Signal.StatusCode -eq 200 -and $node2AfterNode3Signal.Json.status -eq "healthy" -and [int]$node2AfterNode3Signal.Json.unresolved_critical_count -eq 0)
  $checks.node3_reconcile_recovers_health = ($node3Reconcile.StatusCode -eq 200 -and [int]$node3Reconcile.Json.unresolved_critical_count -eq 0 -and $node3Recovered.Json.status -eq "healthy")

  $observations.node1 = [ordered]@{
    degraded_status = $node1Degraded.Json
    failover = $node1Failover.Json
    recovered_status = $node1Recovered.Json
  }
  $observations.node2 = [ordered]@{
    status_before_remote_fault = $node2Status.Json
    status_after_node3_fault = $node2AfterNode3Signal.Json
  }
  $observations.node3 = [ordered]@{
    degraded_status = $node3Degraded.Json
    recovered_status = $node3Recovered.Json
  }

  if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -gt 0) {
    $status = "failed"
  }
} catch {
  $status = "failed"
  $observations.error = $_.Exception.Message
} finally {
  foreach ($entry in $processes) {
    Stop-NodeProcessTree -RootProcess $entry.process
  }
}

$artifact = [ordered]@{
  smoke = "ws6-process-isolated-cluster-runtime-chaos"
  status = $status
  hardening_item = "H-03"
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  certification_scope = @(
    "multi_process_runtime_bootstrap",
    "live_failover_signal_degradation",
    "live_failover_and_reconcile_cycle",
    "process_isolated_control_plane_state"
  )
  nodes = $nodes
  checks = $checks
  observations = $observations
}

$artifact | ConvertTo-Json -Depth 12 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS6 process-isolated cluster chaos smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }