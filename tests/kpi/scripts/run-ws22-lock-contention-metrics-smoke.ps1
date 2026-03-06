param(
  [string]$OutputPath = "tests/kpi/results/ws22/ws22-lock-contention-metrics-smoke.json"
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

$start = Get-Date
$command = "cargo test -p voltnuerongridd ws22_pessimistic_lock_contention_metrics; runtime contention-metrics contract checks"
$outputLines = @()
$exitCode = 1
$contractChecks = [ordered]@{
  metrics_route_present = $false
  metrics_handler_present = $false
  metrics_response_struct_present = $false
  contention_metrics_struct_present = $false
  deadlock_detections_counter_present = $false
  scan_cap_timeouts_counter_present = $false
  wait_timeouts_counter_present = $false
  lock_grants_counter_present = $false
  lock_conflicts_counter_present = $false
  lock_releases_counter_present = $false
  contention_ratio_field_present = $false
  contention_metrics_unit_test_present = $false
}

try {
  $outputLines = & cargo test -p voltnuerongridd ws22_pessimistic_lock_contention_metrics -- --nocapture 2>&1
  $testExit = $LASTEXITCODE

  $runtimeRaw = Get-Content -Raw -Path "services/voltnuerongridd/src/main.rs"
  $contractChecks.metrics_route_present = ($runtimeRaw -match '/api/v1/sql/locks/pessimistic/metrics')
  $contractChecks.metrics_handler_present = ($runtimeRaw -match 'fn sql_pessimistic_lock_metrics\(')
  $contractChecks.metrics_response_struct_present = ($runtimeRaw -match 'struct PessimisticLockContentionMetricsResponse')
  $contractChecks.contention_metrics_struct_present = ($runtimeRaw -match 'struct PessimisticLockContentionMetrics')
  $contractChecks.deadlock_detections_counter_present = ($runtimeRaw -match 'deadlock_detections:\s*Arc<AtomicU64>')
  $contractChecks.scan_cap_timeouts_counter_present = ($runtimeRaw -match 'scan_cap_timeouts:\s*Arc<AtomicU64>')
  $contractChecks.wait_timeouts_counter_present = ($runtimeRaw -match 'wait_timeouts:\s*Arc<AtomicU64>')
  $contractChecks.lock_grants_counter_present = ($runtimeRaw -match 'lock_grants:\s*Arc<AtomicU64>')
  $contractChecks.lock_conflicts_counter_present = ($runtimeRaw -match 'lock_conflicts:\s*Arc<AtomicU64>')
  $contractChecks.lock_releases_counter_present = ($runtimeRaw -match 'lock_releases:\s*Arc<AtomicU64>')
  $contractChecks.contention_ratio_field_present = ($runtimeRaw -match 'contention_ratio:\s*f64')
  $contractChecks.contention_metrics_unit_test_present = (($outputLines -join "`n") -match 'ws22_pessimistic_lock_contention_metrics_counts_outcomes')

  $contractExit = if (
    $contractChecks.metrics_route_present -and
    $contractChecks.metrics_handler_present -and
    $contractChecks.metrics_response_struct_present -and
    $contractChecks.contention_metrics_struct_present -and
    $contractChecks.deadlock_detections_counter_present -and
    $contractChecks.scan_cap_timeouts_counter_present -and
    $contractChecks.wait_timeouts_counter_present -and
    $contractChecks.lock_grants_counter_present -and
    $contractChecks.lock_conflicts_counter_present -and
    $contractChecks.lock_releases_counter_present -and
    $contractChecks.contention_ratio_field_present -and
    $contractChecks.contention_metrics_unit_test_present
  ) { 0 } else { 1 }
  $exitCode = if ($testExit -eq 0 -and $contractExit -eq 0) { 0 } else { 1 }
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$status = if ($exitCode -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws22-lock-contention-metrics"
  status = $status
  command = $command
  contract_checks = $contractChecks
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS22 lock contention metrics smoke failed."
  exit 1
}

Write-Host "WS22 lock contention metrics smoke passed. Artifact: $OutputPath"
