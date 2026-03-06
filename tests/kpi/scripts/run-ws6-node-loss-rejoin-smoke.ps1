param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$RuntimePath = "services/voltnuerongridd/src/main.rs",
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-node-loss-rejoin-smoke.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$runtimeRaw = Get-Content -Path $RuntimePath -Raw

$steps = @()
$overallPassed = $true

function Run-Step {
  param(
    [string]$Name,
    [string]$CommandText,
    [scriptblock]$Runner
  )
  $started = Get-Date
  $global:LASTEXITCODE = 0
  $output = & $Runner 2>&1
  $exitCode = $LASTEXITCODE
  $finished = Get-Date
  $passed = ($? -and $exitCode -eq 0)
  if (-not $passed) { $script:overallPassed = $false }
  $script:steps += [ordered]@{
    step = $Name
    command = $CommandText
    status = if ($passed) { "passed" } else { "failed" }
    duration_ms = [int](($finished - $started).TotalMilliseconds)
    output_excerpt = (($output | Select-Object -First 8) -join "`n")
  }
}

Run-Step -Name "node_loss_signal_critical" `
  -CommandText "cargo test -p voltnuerongridd ws12_failure_signal_queues_auto_remediation -- --nocapture" `
  -Runner { cargo test -p voltnuerongridd ws12_failure_signal_queues_auto_remediation -- --nocapture }

Run-Step -Name "failover_drill_execution" `
  -CommandText "cargo test -p voltnuerongridd ws12_dr_hook_executes_failover_when_not_dry_run -- --nocapture" `
  -Runner { cargo test -p voltnuerongridd ws12_dr_hook_executes_failover_when_not_dry_run -- --nocapture }

Run-Step -Name "node_rejoin_reconcile" `
  -CommandText "cargo test -p voltnuerongridd ws12_reconcile_marks_critical_resolved -- --nocapture" `
  -Runner { cargo test -p voltnuerongridd ws12_reconcile_marks_critical_resolved -- --nocapture }

$contractChecks = [ordered]@{
  failover_status_route = ($runtimeRaw -match '\.route\("/api/v1/failover/status",\s*get\(failover_status\)\)')
  failover_simulate_route = ($runtimeRaw -match '\.route\("/api/v1/failover/simulate",\s*post\(failover_simulate\)\)')
  failure_signal_route = ($runtimeRaw -match '\.route\("/api/v1/sre/failure/signal",\s*post\(sre_failure_signal\)\)')
  failure_reconcile_route = ($runtimeRaw -match '\.route\("/api/v1/sre/failure/reconcile",\s*post\(sre_failure_reconcile\)\)')
}

if (($contractChecks.Values | Where-Object { $_ -eq $false }).Count -gt 0) {
  $overallPassed = $false
}

$status = if ($overallPassed) { "passed" } else { "failed" }
$result = [ordered]@{
  smoke = "ws6-node-loss-rejoin-sequence"
  status = $status
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  runtime_path = $RuntimePath
  sequence = @("node_loss_signal_critical", "failover_drill_execution", "node_rejoin_reconcile")
  contract_checks = $contractChecks
  steps = $steps
}

$result | ConvertTo-Json -Depth 12 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS6 node-loss/rejoin smoke result: $OutputPath ($status)"
if ($status -eq "failed") { exit 1 }
exit 0
