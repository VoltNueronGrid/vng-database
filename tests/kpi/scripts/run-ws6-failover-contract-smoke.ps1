param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$RuntimePath = "services/voltnuerongridd/src/main.rs",
  [string]$OutputPath = "tests/kpi/results/ws6/failover-contract-smoke.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
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

$runtimeFile = Join-Path $RepoRoot $RuntimePath
$runtimeRaw = Get-Content -Path $runtimeFile -Raw

$checks = [ordered]@{
  failover_status_route = ($runtimeRaw -match '\.route\("/api/v1/failover/status",\s*get\(failover_status\)\)')
  failover_simulate_route = ($runtimeRaw -match '\.route\("/api/v1/failover/simulate",\s*post\(failover_simulate\)\)')
  failover_status_tracks_critical_signals = ($runtimeRaw -match 'unresolved_critical_count:\s*usize')
  failover_status_degrades_when_critical_signals_present = ($runtimeRaw -match 'status:\s*if unresolved_critical_count > 0')
  failover_rto_target_declared = ($runtimeRaw -match 'rto_seconds_target:\s*30')
  failover_rpo_target_declared = ($runtimeRaw -match 'rpo_data_loss_rows_target:\s*0')
  failover_requires_operator_auth = ($runtimeRaw -match 'require_operator_auth\(&headers,\s*&state\)\?;')
  failover_response_includes_handoff_report = ($runtimeRaw -match 'handoff_report:\s*FailoverHandoffReportResponse')
  failover_runtime_builds_handoff_report = ($runtimeRaw -match 'build_failover_handoff_report\(')
  failover_uses_replication_transport = ($runtimeRaw -match 'replication_transport')
}

$testResult = Invoke-CargoCapture -Command { cargo test -p voltnuerongridd failover_ -- --nocapture }
$testOutput = $testResult.Output
$testExit = $testResult.ExitCode
$checks.failover_rotation_tests_pass = ($testExit -eq 0)

$status = "passed"
if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) {
  $status = "failed"
}

$result = [ordered]@{
  smoke = "ws6-failover-contract"
  status = $status
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  runtime_path = $RuntimePath
  command = "cargo test -p voltnuerongridd failover_ -- --nocapture"
  checks = $checks
  output_excerpt = (($testOutput | Select-Object -First 20) -join "`n")
}

$result | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS6 failover contract smoke result: $OutputPath ($status)"
if ($status -eq "failed") { exit 1 }
exit 0
