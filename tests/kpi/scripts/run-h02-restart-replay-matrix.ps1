param(
  [string]$OutputPath = "tests/kpi/results/h02/h02-restart-replay-matrix.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Invoke-TestCase {
  param([string]$Filter)

  $output = @()
  $exitCode = 1
  try {
    $output = & cargo test -p voltnuerongrid-store $Filter 2>&1
    $exitCode = $LASTEXITCODE
  } catch {
    $output += $_.Exception.Message
    $exitCode = 1
  }

  [ordered]@{
    test_filter = $Filter
    status = if ($exitCode -eq 0) { "passed" } else { "failed" }
    exit_code = $exitCode
    output_excerpt = (($output | Select-Object -First 20) -join "`n")
  }
}

Ensure-OutputDir -PathValue $OutputPath

$start = Get-Date
$cases = @(
  "preserves_continuity_after_checkpoint_and_restore",
  "replay_after_restore_preserves_integrity_without_faults",
  "replay_after_restore_detects_gap_when_fault_injected",
  "recovers_state_from_wal_adapter_records",
  "detects_sequence_gap_after_fault_injection",
  "detects_duplicate_sequences_after_fault_injection",
  "detects_out_of_order_sequences_after_fault_injection"
)

$results = @()
foreach ($case in $cases) {
  $results += Invoke-TestCase -Filter $case
}

$hasFailures = $results | Where-Object { $_.status -ne "passed" }
$finished = Get-Date

$artifact = [ordered]@{
  harness = "h02-restart-replay-matrix"
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  status = if ($hasFailures.Count -eq 0) { "passed" } else { "failed" }
  matrix = $results
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($hasFailures.Count -gt 0) {
  Write-Error "H-02 restart/replay fault matrix failed."
  exit 1
}

Write-Host "H-02 restart/replay fault matrix passed. Artifact: $OutputPath"
