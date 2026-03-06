param(
  [string]$OutputPath = "tests/kpi/results/ws22/ws22-gate-summary.json",
  [string]$ReleaseSummaryOutputPath = "tests/kpi/results/gates/ws22-release-readiness.json"
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
Ensure-OutputDir -PathValue $ReleaseSummaryOutputPath
$priorSummaryPath = "tests/kpi/results/ws22/ws22-gate-summary.previous.json"
if (Test-Path -Path $OutputPath) { Copy-Item -Path $OutputPath -Destination $priorSummaryPath -Force }

$start = Get-Date
$runs = @()
$status = "passed"

$packs = @(
  @{
    Name = "ws22-pessimistic-lock-smoke"
    Script = "tests/kpi/scripts/run-ws22-pessimistic-lock-smoke.ps1"
    Artifact = "tests/kpi/results/ws22/ws22-pessimistic-lock-smoke.json"
  },
  @{
    Name = "ws22-lock-contention-metrics-smoke"
    Script = "tests/kpi/scripts/run-ws22-lock-contention-metrics-smoke.ps1"
    Artifact = "tests/kpi/results/ws22/ws22-lock-contention-metrics-smoke.json"
  }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  try {
    $global:LASTEXITCODE = 0
    & $pack.Script -OutputPath $pack.Artifact 2>&1 | Out-Null
    if (-not $?) {
      $packStatus = "failed"
      $detail = "script_invocation_failed"
    } elseif ($global:LASTEXITCODE -ne 0) {
      $packStatus = "failed"
      $detail = "exit_code=$global:LASTEXITCODE"
    }
  } catch {
    $packStatus = "failed"
    $detail = $_.Exception.Message
  }
  if ($packStatus -ne "passed") {
    $status = "failed"
  }
  $runs += [ordered]@{
    pack = $pack.Name
    status = $packStatus
    detail = $detail
    artifact = $pack.Artifact
  }
}

$finished = Get-Date
$summary = [ordered]@{
  gate = "ws22"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

$postArtifacts = @(
  @{
    Name = "ws22-gate-trend-comparison"
    Script = "tests/kpi/scripts/run-ws22-gate-trend-compare.ps1"
    Runner = { & "tests/kpi/scripts/run-ws22-gate-trend-compare.ps1" -CurrentSummaryPath $OutputPath -PriorSummaryPath $priorSummaryPath -OutputPath "tests/kpi/results/ws22/ws22-gate-trend-comparison.json" }
  },
  @{
    Name = "ws22-lock-stability-badge"
    Script = "tests/kpi/scripts/run-ws22-pessimistic-lock-stability-badge.ps1"
    Runner = { & "tests/kpi/scripts/run-ws22-pessimistic-lock-stability-badge.ps1" -SummaryPath $OutputPath -TrendPath "tests/kpi/results/ws22/ws22-gate-trend-comparison.json" -OutputPath "tests/kpi/results/ws22/ws22-pessimistic-lock-stability-badge.json" }
  },
  @{
    Name = "ws22-release-summary"
    Script = "tests/kpi/scripts/run-ws22-release-summary.ps1"
    Runner = { & "tests/kpi/scripts/run-ws22-release-summary.ps1" -SummaryPath $OutputPath -SmokePath "tests/kpi/results/ws22/ws22-pessimistic-lock-smoke.json" -TrendPath "tests/kpi/results/ws22/ws22-gate-trend-comparison.json" -BadgePath "tests/kpi/results/ws22/ws22-pessimistic-lock-stability-badge.json" -OutputPath $ReleaseSummaryOutputPath }
  }
)

foreach ($artifact in $postArtifacts) {
  $global:LASTEXITCODE = 0
  & $artifact.Runner 2>&1 | Out-Null
  $artifactOk = ($? -and $global:LASTEXITCODE -eq 0)
  if (-not $artifactOk) { $status = "failed" }
  $runs += [ordered]@{
    pack = $artifact.Name
    status = if ($artifactOk) { "passed" } else { "failed" }
    detail = $artifact.Script
    artifact = if ($artifact.Name -eq "ws22-release-summary") { $ReleaseSummaryOutputPath } elseif ($artifact.Name -eq "ws22-lock-stability-badge") { "tests/kpi/results/ws22/ws22-pessimistic-lock-stability-badge.json" } else { "tests/kpi/results/ws22/ws22-gate-trend-comparison.json" }
  }
}

$summary.status = $status
$summary.packs = $runs
$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

Write-Host "WS22 gate summary: $OutputPath ($status)"
if ($status -ne "passed") {
  exit 1
}
