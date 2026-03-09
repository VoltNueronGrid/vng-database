param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$RuntimePath = "services/voltnuerongridd/src/main.rs",
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-control-plane-chaos-smoke.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

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

$runtimeRaw = Get-Content -Path $RuntimePath -Raw
$steps = @()
$overallPassed = $true

function Add-StepResult {
  param(
    [string]$Name,
    [string[]]$Arguments,
    [string]$CommandText
  )

  $started = Get-Date
  $result = Invoke-CargoTestCapture -Arguments $Arguments
  $finished = Get-Date
  $passed = ($result.Ok -and $result.ExitCode -eq 0)
  if (-not $passed) {
    $script:overallPassed = $false
  }

  $script:steps += [ordered]@{
    step = $Name
    command = $CommandText
    status = if ($passed) { "passed" } else { "failed" }
    duration_ms = [int](($finished - $started).TotalMilliseconds)
    output_excerpt = (($result.Text -split "`r?`n" | Select-Object -First 8) -join "`n")
  }
}

Add-StepResult `
  -Name "failover_status_baseline_health" `
  -Arguments @("test", "-p", "voltnuerongridd", "failover_status_reports_healthy_without_critical_signals", "--", "--nocapture") `
  -CommandText "cargo test -p voltnuerongridd failover_status_reports_healthy_without_critical_signals -- --nocapture"

Add-StepResult `
  -Name "failover_status_degrades_on_critical_signal" `
  -Arguments @("test", "-p", "voltnuerongridd", "failover_status_reports_degraded_with_unresolved_critical_signal", "--", "--nocapture") `
  -CommandText "cargo test -p voltnuerongridd failover_status_reports_degraded_with_unresolved_critical_signal -- --nocapture"

Add-StepResult `
  -Name "control_plane_chaos_cycle_recovers_after_reconcile" `
  -Arguments @("test", "-p", "voltnuerongridd", "h03_control_plane_chaos_cycle_recovers_after_failover_and_reconcile", "--", "--nocapture") `
  -CommandText "cargo test -p voltnuerongridd h03_control_plane_chaos_cycle_recovers_after_failover_and_reconcile -- --nocapture"

Add-StepResult `
  -Name "critical_signal_auto_remediation_queue" `
  -Arguments @("test", "-p", "voltnuerongridd", "ws12_failure_signal_queues_auto_remediation", "--", "--nocapture") `
  -CommandText "cargo test -p voltnuerongridd ws12_failure_signal_queues_auto_remediation -- --nocapture"

$contractChecks = [ordered]@{
  failover_status_route = ($runtimeRaw -match '\.route\("/api/v1/failover/status",\s*get\(failover_status\)\)')
  failover_simulate_route = ($runtimeRaw -match '\.route\("/api/v1/failover/simulate",\s*post\(failover_simulate\)\)')
  failure_reconcile_route = ($runtimeRaw -match '\.route\("/api/v1/sre/failure/reconcile",\s*post\(sre_failure_reconcile\)\)')
  failover_status_tracks_critical_signals = ($runtimeRaw -match 'unresolved_critical_count:\s*usize')
  failover_status_degrades_when_critical_signals_present = ($runtimeRaw -match 'status:\s*if unresolved_critical_count > 0')
}

if (($contractChecks.Values | Where-Object { $_ -eq $false }).Count -gt 0) {
  $overallPassed = $false
}

$status = if ($overallPassed) { "passed" } else { "failed" }
$artifact = [ordered]@{
  smoke = "ws6-control-plane-chaos-certification"
  status = $status
  hardening_item = "H-03"
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  runtime_path = $RuntimePath
  certification_scope = @(
    "degraded_failover_status_under_unresolved_critical_signals",
    "leader_churn_and_handoff_recovery",
    "critical_signal_reconcile_recovery",
    "auto_remediation_queue_signal"
  )
  contract_checks = $contractChecks
  steps = $steps
}

$artifact | ConvertTo-Json -Depth 12 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS6 control-plane chaos smoke result: $OutputPath ($status)"
if ($status -eq "failed") { exit 1 }
exit 0