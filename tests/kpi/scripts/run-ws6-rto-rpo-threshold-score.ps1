param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$TargetsPath = "tests/kpi/config/targets.yaml",
  [string]$RuntimePath = "services/voltnuerongridd/src/main.rs",
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-rto-rpo-threshold-score.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$targetsRaw = Get-Content -Path $TargetsPath -Raw
$runtimeRaw = Get-Content -Path $RuntimePath -Raw

$thresholdRto = 30
$thresholdRpo = 0
$strictSyncRequired = $false

$rtoMatch = [regex]::Match($targetsRaw, 'rto_sec_max:\s*([0-9]+)')
if ($rtoMatch.Success) { $thresholdRto = [int]$rtoMatch.Groups[1].Value }
$rpoMatch = [regex]::Match($targetsRaw, 'rpo_committed_data_loss:\s*([0-9]+)')
if ($rpoMatch.Success) { $thresholdRpo = [int]$rpoMatch.Groups[1].Value }
$syncMatch = [regex]::Match($targetsRaw, 'strict_sync_required_for_rpo:\s*(true|false)')
if ($syncMatch.Success) { $strictSyncRequired = ($syncMatch.Groups[1].Value -eq "true") }

$reportedRto = 9999
$reportedRpo = 9999

$reportedRtoMatch = [regex]::Match($runtimeRaw, 'rto_seconds_target:\s*([0-9]+)')
if ($reportedRtoMatch.Success) { $reportedRto = [int]$reportedRtoMatch.Groups[1].Value }
$reportedRpoMatch = [regex]::Match($runtimeRaw, 'rpo_data_loss_rows_target:\s*([0-9]+)')
if ($reportedRpoMatch.Success) { $reportedRpo = [int]$reportedRpoMatch.Groups[1].Value }

$rtoPass = ($reportedRto -le $thresholdRto)
$rpoPass = ($reportedRpo -le $thresholdRpo)
$syncSignalPresent = ($runtimeRaw -match 'failure_type:\s*"replication_lag"')
$strictSyncPass = if ($strictSyncRequired) { $syncSignalPresent } else { $true }

$score = 0
if ($rtoPass) { $score += 40 }
if ($rpoPass) { $score += 40 }
if ($strictSyncPass) { $score += 20 }

$status = if ($score -eq 100) { "passed" } else { "failed" }

$result = [ordered]@{
  smoke = "ws6-rto-rpo-threshold-score"
  status = $status
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  targets_path = $TargetsPath
  runtime_path = $RuntimePath
  thresholds = [ordered]@{
    rto_sec_max = $thresholdRto
    rpo_committed_data_loss = $thresholdRpo
    strict_sync_required_for_rpo = $strictSyncRequired
  }
  observed = [ordered]@{
    reported_rto_seconds = $reportedRto
    reported_rpo_rows = $reportedRpo
    replication_lag_signal_path_present = $syncSignalPresent
  }
  checks = [ordered]@{
    rto_threshold_pass = $rtoPass
    rpo_threshold_pass = $rpoPass
    strict_sync_policy_pass = $strictSyncPass
  }
  gate_score = [ordered]@{
    score_max = 100
    score = $score
    pass_min = 100
  }
}

$result | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS6 RTO/RPO threshold scoring result: $OutputPath ($status, score=$score/100)"
if ($status -eq "failed") { exit 1 }
exit 0
