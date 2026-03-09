param(
  [string]$SummaryPath = "tests/kpi/results/ws6/ws6-gate-summary.json",
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-chaos-fault-matrix.json"
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

if (!(Test-Path -Path $SummaryPath)) {
  throw "WS6 gate summary not found at $SummaryPath"
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$packByName = @{}
foreach ($pack in $summary.packs) {
  $packByName[[string]$pack.pack] = [string]$pack.status
}

$matrix = @(
  [ordered]@{
    fault_mode = "node_loss_and_rejoin_sequence"
    evidence_pack = "ws6-node-loss-rejoin-sequence"
    status = $packByName["ws6-node-loss-rejoin-sequence"]
    artifact = "tests/kpi/results/ws6/ws6-node-loss-rejoin-smoke.json"
  },
  [ordered]@{
    fault_mode = "failover_flapping_under_repeated_handoffs"
    evidence_pack = "ws6-failover-flap-resistance"
    status = $packByName["ws6-failover-flap-resistance"]
    artifact = "tests/kpi/results/ws6/ws6-failover-flap-resistance-smoke.json"
  },
  [ordered]@{
    fault_mode = "replication_lag_critical_signal_and_reconcile"
    evidence_pack = "ws6-replication-lag-failure-scenarios"
    status = $packByName["ws6-replication-lag-failure-scenarios"]
    artifact = "tests/kpi/results/ws6/ws6-replication-lag-scenarios-smoke.json"
  },
  [ordered]@{
    fault_mode = "multi_node_handoff_matrix"
    evidence_pack = "ws6-multi-node-handoff-matrix"
    status = $packByName["ws6-multi-node-handoff-matrix"]
    artifact = "tests/kpi/results/ws6/ws6-handoff-matrix-smoke.json"
  },
  [ordered]@{
    fault_mode = "reconcile_latency_envelope"
    evidence_pack = "ws6-reconcile-latency-envelope"
    status = $packByName["ws6-reconcile-latency-envelope"]
    artifact = "tests/kpi/results/ws6/ws6-reconcile-latency-envelope-smoke.json"
  },
  [ordered]@{
    fault_mode = "control_plane_leader_churn_and_reconcile"
    evidence_pack = "ws6-control-plane-chaos-certification"
    status = $packByName["ws6-control-plane-chaos-certification"]
    artifact = "tests/kpi/results/ws6/ws6-control-plane-chaos-smoke.json"
  },
  [ordered]@{
    fault_mode = "multi_node_cluster_runtime_targeted_handoff_churn"
    evidence_pack = "ws6-multi-node-cluster-runtime-chaos"
    status = $packByName["ws6-multi-node-cluster-runtime-chaos"]
    artifact = "tests/kpi/results/ws6/ws6-multi-node-cluster-chaos-smoke.json"
  }
)

$missing = @($matrix | Where-Object { [string]::IsNullOrWhiteSpace($_.status) })
$failed = @($matrix | Where-Object { $_.status -ne "passed" })
$status = if ($missing.Count -eq 0 -and $failed.Count -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  report = "ws6-chaos-fault-injection-matrix"
  status = $status
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  source_summary = $SummaryPath
  total_fault_modes = $matrix.Count
  passed_modes = ($matrix | Where-Object { $_.status -eq "passed" }).Count
  failed_modes = $failed.Count
  matrix = $matrix
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS6 chaos fault matrix artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
