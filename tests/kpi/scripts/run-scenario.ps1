param(
  [Parameter(Mandatory = $true)][string]$ScenarioPath,
  [Parameter(Mandatory = $true)][string]$BaseUrl,
  [Parameter(Mandatory = $true)][string]$SqlUrl,
  [Parameter(Mandatory = $true)][string]$OutputDir,
  [string]$TargetsPath = "",
  [ValidateSet("none", "bearer", "apiKey")][string]$AuthMode = "none",
  [string]$AuthToken = "",
  [string]$ApiKeyHeaderName = "X-API-Key",
  [int]$RequestTimeoutSec = 10,
  [hashtable]$ExtraHeaders = @{},
  [string]$ProfileName = "default"
)

$ErrorActionPreference = "Stop"

if (!(Test-Path $OutputDir)) {
  New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
}

if ([string]::IsNullOrWhiteSpace($TargetsPath)) {
  $TargetsPath = Join-Path (Join-Path (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)) "config") "targets.yaml"
}

$scenarioName = [System.IO.Path]::GetFileNameWithoutExtension($ScenarioPath)
$resultPath = Join-Path $OutputDir "$scenarioName-result.json"

function Get-YamlValue {
  param(
    [string[]]$Lines,
    [string]$Section,
    [string]$SubSection,
    [string]$Key,
    [string]$DefaultValue
  )

  $inSection = $false
  $inSubSection = [string]::IsNullOrWhiteSpace($SubSection)
  foreach ($line in $Lines) {
    if (!$inSection) {
      if ($line -match "^\s*${Section}:\s*$") {
        $inSection = $true
      }
      continue
    }

    if ($line -match "^[A-Za-z0-9_]+:\s*$") {
      break
    }

    if (-not [string]::IsNullOrWhiteSpace($SubSection)) {
      if (-not $inSubSection -and $line -match "^\s{2}${SubSection}:\s*$") {
        $inSubSection = $true
        continue
      }
      if ($inSubSection -and $line -match "^\s{2}[A-Za-z0-9_]+:\s*$" -and $line -notmatch "^\s{2}${SubSection}:\s*$") {
        break
      }
      if (-not $inSubSection) {
        continue
      }
      if ($line -match "^\s{4}${Key}:\s*(.+)\s*$") {
        return $Matches[1].Trim()
      }
    }
    else {
      if ($line -match "^\s{2}${Key}:\s*(.+)\s*$") {
        return $Matches[1].Trim()
      }
    }
  }
  return $DefaultValue
}

$targetLines = Get-Content -Path $TargetsPath
$targetsConfig = @{
  kpis = @{
    oltp_latency = @{
      p95_ms = [double](Get-YamlValue -Lines $targetLines -Section "kpis" -SubSection "oltp_latency" -Key "p95_ms" -DefaultValue "20")
      p99_ms = [double](Get-YamlValue -Lines $targetLines -Section "kpis" -SubSection "oltp_latency" -Key "p99_ms" -DefaultValue "60")
    }
    olap_latency = @{
      p95_ms = [double](Get-YamlValue -Lines $targetLines -Section "kpis" -SubSection "olap_latency" -Key "p95_ms" -DefaultValue "800")
      p99_ms = [double](Get-YamlValue -Lines $targetLines -Section "kpis" -SubSection "olap_latency" -Key "p99_ms" -DefaultValue "1500")
    }
    htap_mixed_throughput = @{
      read_qps_min = [double](Get-YamlValue -Lines $targetLines -Section "kpis" -SubSection "htap_mixed_throughput" -Key "read_qps_min" -DefaultValue "25000")
      write_tps_min = [double](Get-YamlValue -Lines $targetLines -Section "kpis" -SubSection "htap_mixed_throughput" -Key "write_tps_min" -DefaultValue "10000")
    }
    failover = @{
      rto_sec_max = [double](Get-YamlValue -Lines $targetLines -Section "kpis" -SubSection "failover" -Key "rto_sec_max" -DefaultValue "30")
      rpo_committed_data_loss = [double](Get-YamlValue -Lines $targetLines -Section "kpis" -SubSection "failover" -Key "rpo_committed_data_loss" -DefaultValue "0")
    }
  }
  run_defaults = @{
    duration_seconds = [int](Get-YamlValue -Lines $targetLines -Section "run_defaults" -SubSection "" -Key "duration_seconds" -DefaultValue "300")
  }
}

function Get-RequestHeaders {
  param(
    [string]$Mode,
    [string]$Token,
    [string]$ApiHeader,
    [hashtable]$Extras
  )

  $headers = @{}
  if ($Extras) {
    foreach ($k in $Extras.Keys) {
      $headers[$k] = [string]$Extras[$k]
    }
  }

  if ($Mode -eq "bearer" -and ![string]::IsNullOrWhiteSpace($Token)) {
    $headers["Authorization"] = "Bearer $Token"
  }
  elseif ($Mode -eq "apiKey" -and ![string]::IsNullOrWhiteSpace($Token)) {
    $headers[$ApiHeader] = $Token
  }
  return $headers
}

function Get-PercentileValue {
  param(
    [double[]]$Values,
    [double]$Percentile
  )

  if ($Values.Count -eq 0) { return 0.0 }
  $sorted = $Values | Sort-Object
  $index = [Math]::Ceiling($Percentile * $sorted.Count) - 1
  if ($index -lt 0) { $index = 0 }
  if ($index -ge $sorted.Count) { $index = $sorted.Count - 1 }
  return [double]$sorted[$index]
}

function Invoke-TimedRequest {
  param(
    [Parameter(Mandatory = $true)][ValidateSet("GET", "POST")][string]$Method,
    [Parameter(Mandatory = $true)][string]$Uri,
    [object]$Body = $null,
    [hashtable]$Headers = @{},
    [int]$TimeoutSec = 10
  )

  $started = Get-Date
  if ($Method -eq "GET") {
    $response = Invoke-RestMethod -Method Get -Uri $Uri -Headers $Headers -TimeoutSec $TimeoutSec
  }
  else {
    $payload = if ($Body) { $Body | ConvertTo-Json -Depth 10 } else { "{}" }
    $response = Invoke-RestMethod -Method Post -Uri $Uri -Headers $Headers -Body $payload -ContentType "application/json" -TimeoutSec $TimeoutSec
  }
  $elapsed = (Get-Date) - $started
  return @{
    response = $response
    elapsed_ms = [Math]::Round($elapsed.TotalMilliseconds, 3)
  }
}

$headers = Get-RequestHeaders -Mode $AuthMode -Token $AuthToken -ApiHeader $ApiKeyHeaderName -Extras $ExtraHeaders
$runDefaults = $targetsConfig.run_defaults
$healthProbe = Invoke-TimedRequest -Method GET -Uri "$BaseUrl/health" -Headers $headers -TimeoutSec $RequestTimeoutSec
$status = "passed"
$metrics = @{}
$notes = @()
$thresholds = @{}

switch ($scenarioName) {
  "oltp-latency" {
    $oltpThresholds = $targetsConfig.kpis.oltp_latency
    $sampleCount = 30
    if ($runDefaults.duration_seconds) {
      $sampleCount = [Math]::Min([int][Math]::Max(10, [int]($runDefaults.duration_seconds / 10)), 120)
    }
    $latencies = @()
    for ($i = 0; $i -lt $sampleCount; $i++) {
      $run = Invoke-TimedRequest -Method POST -Uri "$SqlUrl/api/v1/sql/transaction" -Body @{
        statements = @(
          "BEGIN",
          "INSERT INTO kpi_probe(id, v) VALUES ($i, 'ok')",
          "COMMIT"
        )
      } -Headers $headers -TimeoutSec $RequestTimeoutSec
      $latencies += [double]$run.elapsed_ms
    }
    $metrics = @{
      sample_count = $latencies.Count
      p95_latency_ms = (Get-PercentileValue -Values $latencies -Percentile 0.95)
      p99_latency_ms = (Get-PercentileValue -Values $latencies -Percentile 0.99)
      threshold_p95_ms = [double]$oltpThresholds.p95_ms
      threshold_p99_ms = [double]$oltpThresholds.p99_ms
    }
    $thresholds = $oltpThresholds
    if ($metrics.p95_latency_ms -gt $metrics.threshold_p95_ms -or $metrics.p99_latency_ms -gt $metrics.threshold_p99_ms) {
      $status = "failed"
    }
  }
  "olap-latency" {
    $olapThresholds = $targetsConfig.kpis.olap_latency
    $sampleCount = 20
    if ($runDefaults.duration_seconds) {
      $sampleCount = [Math]::Min([int][Math]::Max(10, [int]($runDefaults.duration_seconds / 15)), 100)
    }
    $latencies = @()
    for ($i = 0; $i -lt $sampleCount; $i++) {
      $run = Invoke-TimedRequest -Method POST -Uri "$BaseUrl/api/v1/olap/query" -Body @{
        query = "SELECT SUM(v) FROM kpi_probe WHERE ts > now() - interval '1 hour'"
        max_rows = 1000
      } -Headers $headers -TimeoutSec $RequestTimeoutSec
      $latencies += [double]$run.elapsed_ms
    }
    $metrics = @{
      sample_count = $latencies.Count
      p95_latency_ms = (Get-PercentileValue -Values $latencies -Percentile 0.95)
      p99_latency_ms = (Get-PercentileValue -Values $latencies -Percentile 0.99)
      threshold_p95_ms = [double]$olapThresholds.p95_ms
      threshold_p99_ms = [double]$olapThresholds.p99_ms
    }
    $thresholds = $olapThresholds
    if ($metrics.p95_latency_ms -gt $metrics.threshold_p95_ms -or $metrics.p99_latency_ms -gt $metrics.threshold_p99_ms) {
      $status = "failed"
    }
  }
  "htap-mixed-throughput" {
    $htapThresholds = $targetsConfig.kpis.htap_mixed_throughput
    $durationSeconds = if ($runDefaults.duration_seconds) { [int][Math]::Min([int]$runDefaults.duration_seconds, 20) } else { 10 }
    $deadline = (Get-Date).AddSeconds($durationSeconds)
    $readOps = 0
    $writeOps = 0
    while ((Get-Date) -lt $deadline) {
      [void](Invoke-TimedRequest -Method POST -Uri "$SqlUrl/api/v1/sql/transaction" -Body @{
        statements = @("BEGIN", "UPDATE kpi_probe SET v = 'mix' WHERE id = 1", "COMMIT")
      } -Headers $headers -TimeoutSec $RequestTimeoutSec)
      [void](Invoke-TimedRequest -Method POST -Uri "$BaseUrl/api/v1/olap/query" -Body @{
        query = "SELECT COUNT(*) FROM kpi_probe"
        max_rows = 10
      } -Headers $headers -TimeoutSec $RequestTimeoutSec)
      $writeOps += 1
      $readOps += 1
    }
    $readQps = [Math]::Round(($readOps / [double]$durationSeconds), 3)
    $writeTps = [Math]::Round(($writeOps / [double]$durationSeconds), 3)
    $metrics = @{
      duration_seconds = $durationSeconds
      read_operations = $readOps
      write_operations = $writeOps
      read_qps = $readQps
      write_tps = $writeTps
      threshold_read_qps_min = [double]$htapThresholds.read_qps_min
      threshold_write_tps_min = [double]$htapThresholds.write_tps_min
    }
    $thresholds = $htapThresholds
    if ($readQps -lt $metrics.threshold_read_qps_min -or $writeTps -lt $metrics.threshold_write_tps_min) {
      $status = "failed"
    }
  }
  "failover-rto-rpo" {
    $failoverThresholds = $targetsConfig.kpis.failover
    $failover = Invoke-TimedRequest -Method GET -Uri "$BaseUrl/api/v1/failover/status" -Headers $headers -TimeoutSec $RequestTimeoutSec
    $reportedRto = [double]$failover.response.rto_seconds_target
    $reportedRpo = [double]$failover.response.rpo_data_loss_rows_target
    $metrics = @{
      reported_rto_seconds = $reportedRto
      reported_rpo_rows = $reportedRpo
      threshold_rto_seconds = [double]$failoverThresholds.rto_sec_max
      threshold_rpo_rows = [double]$failoverThresholds.rpo_committed_data_loss
    }
    $thresholds = $failoverThresholds
    if ($reportedRto -gt $metrics.threshold_rto_seconds -or $reportedRpo -gt $metrics.threshold_rpo_rows) {
      $status = "failed"
    }
  }
  default {
    $status = "failed"
    $notes += "Unknown scenario name '$scenarioName'."
  }
}

$result = @{
  profile = $ProfileName
  scenario = $scenarioName
  base_url = $BaseUrl
  sql_url = $SqlUrl
  status = $status
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  auth = @{
    mode = $AuthMode
    has_token = (-not [string]::IsNullOrWhiteSpace($AuthToken))
    api_key_header = $ApiKeyHeaderName
    extra_header_count = $headers.Keys.Count
  }
  run_config = @{
    timeout_seconds = $RequestTimeoutSec
    targets_path = $TargetsPath
    scenario_id = $scenarioName
    scenario_type = $scenarioName
  }
  health = @{
    status = $healthProbe.response.status
    elapsed_ms = $healthProbe.elapsed_ms
  }
  thresholds = $thresholds
  metrics = $metrics
  notes = $notes
}

$result | ConvertTo-Json -Depth 8 | Out-File -FilePath $resultPath -Encoding utf8
Write-Host "Generated KPI result: $resultPath ($status)"
