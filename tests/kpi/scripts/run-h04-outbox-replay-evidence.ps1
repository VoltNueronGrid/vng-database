param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$StorePath = "crates/voltnuerongrid-store/src/lib.rs",
  [string]$SyncPath = "crates/voltnuerongrid-store/src/htap_sync.rs",
  [string]$WalAdapterPath = "crates/voltnuerongrid-store/src/wal_adapter.rs",
  [string]$OutputPath = "tests/kpi/results/h04/h04-outbox-replay-evidence.json"
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
    step = "wal_adapter_recovery_roundtrip"
    arguments = @("test", "-p", "voltnuerongrid-store", "recovers_state_from_wal_adapter_records", "--", "--nocapture")
    command = "cargo test -p voltnuerongrid-store recovers_state_from_wal_adapter_records -- --nocapture"
  },
  [ordered]@{
    step = "checkpoint_boundary_clears_pending_wal"
    arguments = @("test", "-p", "voltnuerongrid-store", "checkpoints_after_threshold", "--", "--nocapture")
    command = "cargo test -p voltnuerongrid-store checkpoints_after_threshold -- --nocapture"
  },
  [ordered]@{
    step = "replay_after_restore_preserves_integrity"
    arguments = @("test", "-p", "voltnuerongrid-store", "replay_after_restore_preserves_integrity_without_faults", "--", "--nocapture")
    command = "cargo test -p voltnuerongrid-store replay_after_restore_preserves_integrity_without_faults -- --nocapture"
  },
  [ordered]@{
    step = "replay_gap_detection_fault_injected"
    arguments = @("test", "-p", "voltnuerongrid-store", "replay_after_restore_detects_gap_when_fault_injected", "--", "--nocapture")
    command = "cargo test -p voltnuerongrid-store replay_after_restore_detects_gap_when_fault_injected -- --nocapture"
  },
  [ordered]@{
    step = "replay_only_unapplied_mutations"
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
    command = $case.command
    status = if ($passed) { "passed" } else { "failed" }
    duration_ms = [int](($finished - $started).TotalMilliseconds)
    output_excerpt = (($run.Text -split "`r?`n" | Select-Object -First 8) -join "`n")
  }
}

$storeRaw = Get-Content -Path $StorePath -Raw
$syncRaw = Get-Content -Path $SyncPath -Raw
$walAdapterRaw = Get-Content -Path $WalAdapterPath -Raw

$contractChecks = [ordered]@{
  wal_adapter_append_contract = ($walAdapterRaw -match 'fn append\(&self, record: &WalRecord\)')
  wal_adapter_recovery_contract = ($StoreRaw -match 'pub fn recover_from_adapter<A: WalAdapter>')
  checkpoint_clears_wal = ($StoreRaw -match 'self\.wal\.clear\(\);')
  sync_snapshot_restore_contract = ($syncRaw -match 'pub fn snapshot\(&self\) -> SyncOriginSnapshot' -and $syncRaw -match 'pub fn restore\(snapshot: SyncOriginSnapshot\) -> Self')
  replay_only_unapplied_contract = ($syncRaw -match 'pub fn build_failover_handoff_batch\(' -and $syncRaw -match 'origin\.export_since\(self\.last_applied_sequence, max_items\)')
}

if (($contractChecks.Values | Where-Object { $_ -eq $false }).Count -gt 0) {
  $overallPassed = $false
}

$artifact = [ordered]@{
  pack = "h04-outbox-replay-evidence"
  status = if ($overallPassed) { "passed" } else { "failed" }
  hardening_item = "H-04"
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  evidence_scope = @(
    "wal_recovery_roundtrip",
    "checkpoint_boundary_enforcement",
    "replay_after_restore_integrity",
    "replay_gap_detection",
    "replay_only_unapplied_semantics"
  )
  contract_checks = $contractChecks
  steps = $steps
}

$artifact | ConvertTo-Json -Depth 12 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "H04 outbox/replay evidence result: $OutputPath ($($artifact.status))"
if ($artifact.status -eq "failed") { exit 1 }
exit 0