param(
  [string]$OutputPath = "tests/kpi/results/ws3/ws3-gate-summary.json",
  [string]$ReleaseSummaryOutputPath = "tests/kpi/results/gates/ws3-release-readiness.json"
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
$priorSummaryPath = "tests/kpi/results/ws3/ws3-gate-summary.previous.json"
if (Test-Path -Path $OutputPath) { Copy-Item -Path $OutputPath -Destination $priorSummaryPath -Force }

$start = Get-Date
$runs = @()
$status = "passed"

$packs = @(
  @{ Name = "ws3-query-routing"; Script = "tests/kpi/scripts/run-ws3-query-routing-smoke.ps1"; Artifact = "tests/kpi/results/ws3/query-routing-smoke.json" },
  @{ Name = "ws3-htap-target-contract"; Script = "tests/kpi/scripts/run-ws3-htap-target-contract-smoke.ps1"; Artifact = "tests/kpi/results/ws3/ws3-htap-target-contract-smoke.json" }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  try {
    $global:LASTEXITCODE = 0
    Write-Host "[WS3 GATE] Running pack: $($pack.Name)"
    & $pack.Script -OutputPath $pack.Artifact 2>&1 | Out-Null
    if (-not (Test-Path -Path $pack.Artifact)) {
      $packStatus = "failed"
      $detail = "artifact_not_generated"
    }
    elseif (-not $?) { $packStatus = "failed"; $detail = "script_invocation_failed" }
    elseif ($global:LASTEXITCODE -ne 0) { $packStatus = "failed"; $detail = "exit_code=$global:LASTEXITCODE" }
    else {
      Write-Host "[WS3 GATE] Pack passed and artifact generated: $($pack.Artifact)"
    }
  } catch { $packStatus = "failed"; $detail = $_.Exception.Message }
  if ($packStatus -ne "passed") { $status = "failed" }
  $runs += [ordered]@{
    pack = $pack.Name
    status = $packStatus
    detail = $detail
    artifact = $pack.Artifact
  }
}

$finished = Get-Date
$summary = [ordered]@{
  gate = "ws3"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
$postArtifacts = @(
  @{
    Name = "ws3-performance-score"
    Script = "tests/kpi/scripts/run-ws3-performance-score.ps1"
    Runner = { & "tests/kpi/scripts/run-ws3-performance-score.ps1" -SummaryPath $OutputPath -OutputPath "tests/kpi/results/ws3/ws3-performance-score.json" }
  },
  @{
    Name = "ws3-gate-trend-comparison"
    Script = "tests/kpi/scripts/run-ws3-gate-trend-compare.ps1"
    Runner = { & "tests/kpi/scripts/run-ws3-gate-trend-compare.ps1" -CurrentSummaryPath $OutputPath -PriorSummaryPath $priorSummaryPath -OutputPath "tests/kpi/results/ws3/ws3-gate-trend-comparison.json" }
  },
  @{
    Name = "ws3-performance-stability-badge"
    Script = "tests/kpi/scripts/run-ws3-performance-stability-badge.ps1"
    Runner = { & "tests/kpi/scripts/run-ws3-performance-stability-badge.ps1" -SummaryPath $OutputPath -TrendPath "tests/kpi/results/ws3/ws3-gate-trend-comparison.json" -OutputPath "tests/kpi/results/ws3/ws3-performance-stability-badge.json" }
  },
  @{
    Name = "ws3-release-summary"
    Script = "tests/kpi/scripts/run-ws3-release-summary.ps1"
    Runner = { & "tests/kpi/scripts/run-ws3-release-summary.ps1" -SummaryPath $OutputPath -ScorePath "tests/kpi/results/ws3/ws3-performance-score.json" -TrendPath "tests/kpi/results/ws3/ws3-gate-trend-comparison.json" -BadgePath "tests/kpi/results/ws3/ws3-performance-stability-badge.json" -OutputPath $ReleaseSummaryOutputPath }
  }
)

foreach ($artifact in $postArtifacts) {
  try {
    $global:LASTEXITCODE = 0
    & $artifact.Runner 2>&1 | Out-Null
    if (-not $?) {
      $status = "failed"
      $runs += [ordered]@{ pack = $artifact.Name; status = "failed"; detail = "script_invocation_failed"; artifact = $artifact.Script }
    } elseif ($global:LASTEXITCODE -ne 0) {
      $status = "failed"
      $runs += [ordered]@{ pack = $artifact.Name; status = "failed"; detail = "exit_code=$global:LASTEXITCODE"; artifact = $artifact.Script }
    } else {
      $runs += [ordered]@{ pack = $artifact.Name; status = "passed"; detail = "ok"; artifact = $artifact.Script }
    }
  } catch {
    $status = "failed"
    $runs += [ordered]@{ pack = $artifact.Name; status = "failed"; detail = $_.Exception.Message; artifact = $artifact.Script }
  }
}

$summary.status = $status
$summary.packs = $runs
$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "WS3 gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
