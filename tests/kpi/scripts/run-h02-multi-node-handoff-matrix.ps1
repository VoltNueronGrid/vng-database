param(
  [string]$OutputPath = "tests/kpi/results/h02/h02-multi-node-handoff-matrix.json"
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
  "multi_node_replica_replay_applies_contiguous_transport_batch",
  "multi_node_failover_handoff_replays_only_unapplied_mutations",
  "multi_node_failover_handoff_reports_gap_when_transport_drops_sequence"
)

$results = @()
foreach ($case in $cases) {
  $results += Invoke-TestCase -Filter $case
}

$hasFailures = $results | Where-Object { $_.status -ne "passed" }
$finished = Get-Date

$artifact = [ordered]@{
  harness = "h02-multi-node-handoff-matrix"
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  status = if ($hasFailures.Count -eq 0) { "passed" } else { "failed" }
  matrix = $results
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($hasFailures.Count -gt 0) {
  Write-Error "H-02 multi-node handoff matrix failed."
  exit 1
}

Write-Host "H-02 multi-node handoff matrix passed. Artifact: $OutputPath"