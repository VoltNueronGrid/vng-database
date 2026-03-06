param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$RuntimePath = "services/voltnuerongridd/src/main.rs",
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-replication-lag-scenarios-smoke.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$runtimeFile = Join-Path $RepoRoot $RuntimePath
$runtimeRaw = Get-Content -Path $runtimeFile -Raw

$global:LASTEXITCODE = 0
$failureSignalOutput = & cargo test -p voltnuerongridd ws12_failure_signal_ -- --nocapture 2>&1
$failureSignalExit = $LASTEXITCODE

$global:LASTEXITCODE = 0
$reconcileOutput = & cargo test -p voltnuerongridd ws12_reconcile_marks_critical_resolved -- --nocapture 2>&1
$reconcileExit = $LASTEXITCODE

$checks = [ordered]@{
  failure_signal_route_declared = ($runtimeRaw -match '\.route\("/api/v1/sre/failure/signal",\s*post\(sre_failure_signal\)\)')
  failure_reconcile_route_declared = ($runtimeRaw -match '\.route\("/api/v1/sre/failure/reconcile",\s*post\(sre_failure_reconcile\)\)')
  replication_lag_failure_type_present = ($runtimeRaw -match 'failure_type:\s*"replication_lag"')
  failure_signal_tests_passed = ($failureSignalExit -eq 0)
  reconcile_tests_passed = ($reconcileExit -eq 0)
}

$scenarios = @(
  [ordered]@{ scenario = "replication_lag_critical"; expected_action = "queue_auto_remediation" },
  [ordered]@{ scenario = "critical_signal_reconcile"; expected_action = "mark_resolved_and_remove_from_queue" }
)

$status = "passed"
if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) {
  $status = "failed"
}

$result = [ordered]@{
  smoke = "ws6-replication-lag-failure-scenarios"
  status = $status
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  runtime_path = $RuntimePath
  commands = @(
    "cargo test -p voltnuerongridd ws12_failure_signal_ -- --nocapture",
    "cargo test -p voltnuerongridd ws12_reconcile_marks_critical_resolved -- --nocapture"
  )
  scenarios = $scenarios
  checks = $checks
  output_excerpt = ((@($failureSignalOutput) + @($reconcileOutput) | Select-Object -First 20) -join "`n")
}

$result | ConvertTo-Json -Depth 12 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS6 replication-lag scenarios smoke result: $OutputPath ($status)"
if ($status -eq "failed") { exit 1 }
exit 0
