param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-multi-node-cluster-chaos-smoke.json"
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
    return [pscustomobject]@{ Ok = $ok; Text = $text; ExitCode = $process.ExitCode }
  } finally {
    if (Test-Path -Path $tempFile) { Remove-Item -Path $tempFile -Force -ErrorAction SilentlyContinue }
  }
}

$cases = @(
  [ordered]@{
    step = "runtime_targeted_handoffs_across_rotations"
    crate = "voltnuerongridd"
    arguments = @("test", "-p", "voltnuerongridd", "h03_multi_node_cluster_runtime_chaos_replays_targeted_handoffs_across_rotations", "--", "--nocapture")
    command = "cargo test -p voltnuerongridd h03_multi_node_cluster_runtime_chaos_replays_targeted_handoffs_across_rotations -- --nocapture"
  },
  [ordered]@{
    step = "targeted_transport_exports_only_addressed_events"
    crate = "voltnuerongrid-store"
    arguments = @("test", "-p", "voltnuerongrid-store", "multi_node_replication_transport_exports_only_targeted_events", "--", "--nocapture")
    command = "cargo test -p voltnuerongrid-store multi_node_replication_transport_exports_only_targeted_events -- --nocapture"
  },
  [ordered]@{
    step = "targeted_transport_respects_last_applied_sequence"
    crate = "voltnuerongrid-store"
    arguments = @("test", "-p", "voltnuerongrid-store", "multi_node_replication_transport_respects_last_applied_sequence", "--", "--nocapture")
    command = "cargo test -p voltnuerongrid-store multi_node_replication_transport_respects_last_applied_sequence -- --nocapture"
  },
  [ordered]@{
    step = "multi_node_handoff_replays_only_unapplied_mutations"
    crate = "voltnuerongrid-store"
    arguments = @("test", "-p", "voltnuerongrid-store", "multi_node_failover_handoff_replays_only_unapplied_mutations", "--", "--nocapture")
    command = "cargo test -p voltnuerongrid-store multi_node_failover_handoff_replays_only_unapplied_mutations -- --nocapture"
  }
)

$steps = @()
$overallPassed = $true
foreach ($case in $cases) {
  $started = Get-Date
  $run = Invoke-CargoTestCapture -Arguments $case.arguments
  $finished = Get-Date
  $passed = ($run.Ok -and $run.ExitCode -eq 0)
  if (-not $passed) { $overallPassed = $false }
  $steps += [ordered]@{
    step = $case.step
    crate = $case.crate
    command = $case.command
    status = if ($passed) { "passed" } else { "failed" }
    duration_ms = [int](($finished - $started).TotalMilliseconds)
    output_excerpt = (($run.Text -split "`r?`n" | Select-Object -First 8) -join "`n")
  }
}

$artifact = [ordered]@{
  smoke = "ws6-multi-node-cluster-runtime-chaos"
  status = if ($overallPassed) { "passed" } else { "failed" }
  hardening_item = "H-03"
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  certification_scope = @(
    "multi_node_targeted_transport_filtering",
    "multi_node_replay_respects_last_applied_sequence",
    "multi_node_handoff_replays_only_unapplied_mutations",
    "cluster_runtime_targeted_handoff_churn"
  )
  steps = $steps
}

$artifact | ConvertTo-Json -Depth 12 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS6 multi-node cluster chaos smoke result: $OutputPath ($($artifact.status))"
if ($artifact.status -eq "failed") { exit 1 }
exit 0