param(
  [string]$OutputPath = "tests/kpi/results/ws22/ws22-pessimistic-lock-smoke.json"
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
$command = "cargo test -p voltnuerongridd ws22_ -- --test-threads=1 --nocapture; runtime contract checks for pessimistic lock APIs"
$outputLines = @()
$exitCode = 1
$ws22LockContentionMetrics = $null
$contractChecks = [ordered]@{
  acquire_route_present = $false
  release_route_present = $false
  acquire_logic_present = $false
  release_logic_present = $false
  wait_timeout_request_field_present = $false
  wait_timeout_state_present = $false
  wait_timeout_reason_present = $false
  wait_timeout_unit_test_present = $false
  deadlock_state_present = $false
  deadlock_reason_present = $false
  deadlock_unit_test_present = $false
  deadlock_multi_hop_unit_test_present = $false
  deadlock_scan_cap_present = $false
  scan_cap_timeout_reason_present = $false
  scan_cap_unit_test_present = $false
  wait_edge_release_cleanup_unit_test_present = $false
  wait_edge_expiry_cleanup_unit_test_present = $false
  gate_metrics_emit_test_present = $false
  gate_metrics_json_line_present = $false
}

try {
  $outputLines = & cargo test -p voltnuerongridd ws22_ -- --test-threads=1 --nocapture 2>&1
  $testExit = $LASTEXITCODE

  $runtimeRaw = Get-Content -Raw -Path "services/voltnuerongridd/src/main.rs"
  $contractChecks.acquire_route_present = ($runtimeRaw -match '/api/v1/sql/locks/pessimistic/acquire')
  $contractChecks.release_route_present = ($runtimeRaw -match '/api/v1/sql/locks/pessimistic/release')
  $contractChecks.acquire_logic_present = ($runtimeRaw -match 'fn acquire_pessimistic_lock\(')
  $contractChecks.release_logic_present = ($runtimeRaw -match 'fn release_pessimistic_lock\(')
  $contractChecks.wait_timeout_request_field_present = ($runtimeRaw -match 'wait_timeout_ms')
  $contractChecks.wait_timeout_state_present = ($runtimeRaw -match 'lock_state:\s*"wait_timeout"')
  $contractChecks.wait_timeout_reason_present = ($runtimeRaw -match 'pessimistic_lock_wait_timeout')
  $contractChecks.wait_timeout_unit_test_present = (($outputLines -join "`n") -match 'ws22_pessimistic_lock_wait_timeout_returns_request_timeout')
  $contractChecks.deadlock_state_present = ($runtimeRaw -match 'lock_state:\s*"deadlock_risk"')
  $contractChecks.deadlock_reason_present = ($runtimeRaw -match 'pessimistic_lock_deadlock_risk')
  $contractChecks.deadlock_unit_test_present = (($outputLines -join "`n") -match 'ws22_pessimistic_lock_detects_deadlock_risk_cycle')
  $contractChecks.deadlock_multi_hop_unit_test_present = (($outputLines -join "`n") -match 'ws22_pessimistic_lock_detects_deadlock_risk_multi_hop_cycle')
  $contractChecks.deadlock_scan_cap_present = ($runtimeRaw -match 'DEADLOCK_SCAN_MAX_HOPS')
  $contractChecks.scan_cap_timeout_reason_present = ($runtimeRaw -match 'pessimistic_lock_wait_timeout_scan_cap_reached')
  $contractChecks.scan_cap_unit_test_present = (($outputLines -join "`n") -match 'ws22_pessimistic_lock_scan_cap_returns_timeout_diagnostic')
  $contractChecks.wait_edge_release_cleanup_unit_test_present = (($outputLines -join "`n") -match 'ws22_pessimistic_lock_release_cleans_wait_edges_for_resource')
  $contractChecks.wait_edge_expiry_cleanup_unit_test_present = (($outputLines -join "`n") -match 'ws22_pessimistic_lock_expiry_cleans_wait_edges_for_resource')
  $contractChecks.gate_metrics_emit_test_present = ($runtimeRaw -match 'fn zzz_ws22_gate_lock_contention_metrics_emit')
  $outText = $outputLines -join "`n"
  $contractChecks.gate_metrics_json_line_present = ($outText -match 'WS22_GATE_LOCK_METRICS_JSON:\{')
  $metricsMatch = [regex]::Match($outText, 'WS22_GATE_LOCK_METRICS_JSON:(\{[^\n]+\})')
  if ($metricsMatch.Success) {
    try {
      $ws22LockContentionMetrics = $metricsMatch.Groups[1].Value | ConvertFrom-Json
    } catch {
      $ws22LockContentionMetrics = $null
    }
  }

  $contractExit = if (
    $contractChecks.acquire_route_present -and
    $contractChecks.release_route_present -and
    $contractChecks.acquire_logic_present -and
    $contractChecks.release_logic_present -and
    $contractChecks.wait_timeout_request_field_present -and
    $contractChecks.wait_timeout_state_present -and
    $contractChecks.wait_timeout_reason_present -and
    $contractChecks.wait_timeout_unit_test_present -and
    $contractChecks.deadlock_state_present -and
    $contractChecks.deadlock_reason_present -and
    $contractChecks.deadlock_unit_test_present -and
    $contractChecks.deadlock_multi_hop_unit_test_present -and
    $contractChecks.deadlock_scan_cap_present -and
    $contractChecks.scan_cap_timeout_reason_present -and
    $contractChecks.scan_cap_unit_test_present -and
    $contractChecks.wait_edge_release_cleanup_unit_test_present -and
    $contractChecks.wait_edge_expiry_cleanup_unit_test_present -and
    $contractChecks.gate_metrics_emit_test_present -and
    $contractChecks.gate_metrics_json_line_present
  ) { 0 } else { 1 }
  $exitCode = if ($testExit -eq 0 -and $contractExit -eq 0) { 0 } else { 1 }
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$status = if ($exitCode -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws22-pessimistic-lock-baseline"
  status = $status
  command = $command
  contract_checks = $contractChecks
  ws22_lock_contention_metrics = if ($null -ne $ws22LockContentionMetrics) {
    [ordered]@{
      deadlock_detections = [int64]$ws22LockContentionMetrics.deadlock_detections
      scan_cap_timeouts = [int64]$ws22LockContentionMetrics.scan_cap_timeouts
      source = "zzz_ws22_gate_lock_contention_metrics_emit"
      note = "Cumulative counts from acquire_pessimistic_lock during ws22_ suite (--test-threads=1)."
    }
  } else {
    $null
  }
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS22 pessimistic lock smoke failed."
  exit 1
}

Write-Host "WS22 pessimistic lock smoke passed. Artifact: $OutputPath"
