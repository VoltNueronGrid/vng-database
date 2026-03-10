param(
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-gate-summary.json",
  [string]$ReleaseSummaryOutputPath = "tests/kpi/results/gates/ws6-release-readiness.json"
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

function Get-ArtifactStatus {
  param([string]$ArtifactPath)

  if (!(Test-Path -Path $ArtifactPath)) {
    return "missing_artifact"
  }

  try {
    $json = Get-Content -Raw -Path $ArtifactPath | ConvertFrom-Json
    if ($null -ne $json.status) {
      return [string]$json.status
    }
    return "present"
  } catch {
    return "invalid_artifact"
  }
}

function Invoke-PowerShellScript {
  param(
    [string]$ScriptPath,
    [string[]]$ArgumentList = @()
  )

  $process = Start-Process -FilePath "powershell.exe" `
    -ArgumentList (@("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $ScriptPath) + $ArgumentList) `
    -NoNewWindow `
    -PassThru `
    -Wait
  return $process.ExitCode
}

$priorSummaryPath = "tests/kpi/results/ws6/ws6-gate-summary.previous.json"
if (Test-Path -Path $OutputPath) {
  Copy-Item -Path $OutputPath -Destination $priorSummaryPath -Force
}

$start = Get-Date
$runs = @()
$status = "passed"

$packs = @(
  @{
    Name = "ws6-failover-simulation"
    Script = "tests/kpi/scripts/run-ws6-failover-sim-smoke.ps1"
    Artifact = "tests/kpi/results/ws6/failover-sim-smoke.json"
  },
  @{
    Name = "ws6-failover-contract"
    Script = "tests/kpi/scripts/run-ws6-failover-contract-smoke.ps1"
    Artifact = "tests/kpi/results/ws6/failover-contract-smoke.json"
  },
  @{
    Name = "ws6-dr-failover-path"
    Script = "tests/kpi/scripts/run-ws6-dr-failover-smoke.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-dr-failover-smoke.json"
  },
  @{
    Name = "ws6-multi-node-handoff-matrix"
    Script = "tests/kpi/scripts/run-ws6-handoff-matrix-smoke.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-handoff-matrix-smoke.json"
  },
  @{
    Name = "ws6-replication-lag-failure-scenarios"
    Script = "tests/kpi/scripts/run-ws6-replication-lag-scenarios-smoke.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-replication-lag-scenarios-smoke.json"
  },
  @{
    Name = "ws6-rto-rpo-threshold-score"
    Script = "tests/kpi/scripts/run-ws6-rto-rpo-threshold-score.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-rto-rpo-threshold-score.json"
  },
  @{
    Name = "ws6-node-loss-rejoin-sequence"
    Script = "tests/kpi/scripts/run-ws6-node-loss-rejoin-smoke.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-node-loss-rejoin-smoke.json"
  },
  @{
    Name = "ws6-failover-flap-resistance"
    Script = "tests/kpi/scripts/run-ws6-failover-flap-resistance-smoke.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-failover-flap-resistance-smoke.json"
  },
  @{
    Name = "ws6-reconcile-latency-envelope"
    Script = "tests/kpi/scripts/run-ws6-reconcile-latency-envelope-smoke.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-reconcile-latency-envelope-smoke.json"
  },
  @{
    Name = "ws6-control-plane-chaos-certification"
    Script = "tests/kpi/scripts/run-ws6-control-plane-chaos-smoke.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-control-plane-chaos-smoke.json"
  },
  @{
    Name = "ws6-multi-node-cluster-runtime-chaos"
    Script = "tests/kpi/scripts/run-ws6-multi-node-cluster-chaos-smoke.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-multi-node-cluster-chaos-smoke.json"
  },
  @{
    Name = "ws6-process-isolated-cluster-runtime-chaos"
    Script = "tests/kpi/scripts/run-ws6-process-isolated-cluster-chaos-smoke.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-process-isolated-cluster-chaos-smoke.json"
  }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  try {
    $exitCode = Invoke-PowerShellScript -ScriptPath $pack.Script -ArgumentList @("-OutputPath", $pack.Artifact)
    $artifactStatus = Get-ArtifactStatus -ArtifactPath $pack.Artifact
    if ($artifactStatus -eq "passed") {
      $packStatus = "passed"
    } else {
      $packStatus = "failed"
      $detail = if ($exitCode -ne 0) { "exit_code=$exitCode;$artifactStatus" } else { $artifactStatus }
    }
  } catch { $packStatus = "failed"; $detail = $_.Exception.Message }
  if ($packStatus -ne "passed") { $status = "failed" }
  $runs += [ordered]@{ pack = $pack.Name; status = $packStatus; detail = $detail; artifact = $pack.Artifact }
}

$finished = Get-Date
$summary = [ordered]@{
  gate = "ws6"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

$postArtifacts = @(
  @{
    Name = "ws6-chaos-fault-matrix"
    Script = "tests/kpi/scripts/run-ws6-chaos-fault-matrix-export.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-chaos-fault-matrix.json"
    Arguments = @(
      "-SummaryPath", $OutputPath,
      "-OutputPath", "tests/kpi/results/ws6/ws6-chaos-fault-matrix.json"
    )
  },
  @{
    Name = "ws6-gate-trend-comparison"
    Script = "tests/kpi/scripts/run-ws6-gate-trend-compare.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-gate-trend-comparison.json"
    Arguments = @(
      "-CurrentSummaryPath", $OutputPath,
      "-PriorSummaryPath", $priorSummaryPath,
      "-OutputPath", "tests/kpi/results/ws6/ws6-gate-trend-comparison.json"
    )
  },
  @{
    Name = "ws6-failover-stability-badge"
    Script = "tests/kpi/scripts/run-ws6-failover-stability-badge.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-failover-stability-badge.json"
    Arguments = @(
      "-SummaryPath", $OutputPath,
      "-TrendPath", "tests/kpi/results/ws6/ws6-gate-trend-comparison.json",
      "-OutputPath", "tests/kpi/results/ws6/ws6-failover-stability-badge.json"
    )
  },
  @{
    Name = "ws6-release-summary"
    Script = "tests/kpi/scripts/run-ws6-release-summary.ps1"
    Artifact = $ReleaseSummaryOutputPath
    Arguments = @(
      "-SummaryPath", $OutputPath,
      "-ChaosMatrixPath", "tests/kpi/results/ws6/ws6-chaos-fault-matrix.json",
      "-TrendPath", "tests/kpi/results/ws6/ws6-gate-trend-comparison.json",
      "-BadgePath", "tests/kpi/results/ws6/ws6-failover-stability-badge.json",
      "-OutputPath", $ReleaseSummaryOutputPath
    )
  }
)

foreach ($artifact in $postArtifacts) {
  try {
    $exitCode = Invoke-PowerShellScript -ScriptPath $artifact.Script -ArgumentList $artifact.Arguments
    $artifactStatus = Get-ArtifactStatus -ArtifactPath $artifact.Artifact
    if ($artifactStatus -eq "passed") {
      $runs += [ordered]@{ pack = $artifact.Name; status = "passed"; detail = "ok"; artifact = $artifact.Script }
    } else {
      $status = "failed"
      $detail = if ($exitCode -ne 0) { "exit_code=$exitCode;$artifactStatus" } else { $artifactStatus }
      $runs += [ordered]@{ pack = $artifact.Name; status = "failed"; detail = $detail; artifact = $artifact.Script }
    }
  } catch {
    $status = "failed"
    $runs += [ordered]@{ pack = $artifact.Name; status = "failed"; detail = $_.Exception.Message; artifact = $artifact.Script }
  }
}

$summary.status = $status
$summary.packs = $runs
$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "WS6 gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
