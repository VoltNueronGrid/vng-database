param(
  [string]$OutputPath = "tests/kpi/results/ws8a/ws8a-gate-summary.json",
  [string]$ReleaseSummaryOutputPath = "tests/kpi/results/gates/ws8a-release-readiness.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

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
    if ($null -ne $json.release_readiness) {
      return [string]$json.release_readiness
    }
    return "present"
  } catch {
    return "invalid_artifact"
  }
}

Ensure-OutputDir -PathValue $OutputPath
Ensure-OutputDir -PathValue $ReleaseSummaryOutputPath

$summaryPath = "tests/kpi/results/ws8a/ws8a-gate-summary.json"
$priorSummaryPath = "tests/kpi/results/ws8a/ws8a-gate-summary.previous.json"
if (Test-Path -Path $summaryPath) { Copy-Item -Path $summaryPath -Destination $priorSummaryPath -Force }

$start = Get-Date
$runs = @()
$status = "passed"
$packs = @(
  @{ Name = "ws8a-audit-trail"; Script = "tests/kpi/scripts/run-ws8a-audit-smoke.ps1"; Artifact = "tests/kpi/results/ws8a/audit-trail-smoke.json" },
  @{ Name = "ws8a-audit-companion"; Script = "tests/kpi/scripts/run-ws8a-audit-companion-smoke.ps1"; Artifact = "tests/kpi/results/ws8a/audit-companion-smoke.json" },
  @{ Name = "ws8a-agent-authoring"; Script = "tests/kpi/scripts/run-ws8a-agent-authoring-smoke.ps1"; Artifact = "tests/kpi/results/ws8a/ws8a-agent-authoring-smoke.json" }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  try {
    & $pack.Script -OutputPath $pack.Artifact 2>&1 | Out-Null
    $artifactStatus = Get-ArtifactStatus -ArtifactPath $pack.Artifact
    if ($artifactStatus -ne "passed") { $packStatus = "failed"; $detail = $artifactStatus }
  } catch { $packStatus = "failed"; $detail = $_.Exception.Message }
  if ($packStatus -ne "passed") { $status = "failed" }
  $runs += [ordered]@{ pack = $pack.Name; status = $packStatus; detail = $detail; artifact = $pack.Artifact }
}

$finished = Get-Date
$summary = [ordered]@{
  gate = "ws8a"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $summaryPath
$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

$postArtifacts = @(
  @{
    Name = "ws8a-agent-authoring-matrix"
    Script = "tests/kpi/scripts/run-ws8a-agent-authoring-matrix-export.ps1"
    Runner = { & "tests/kpi/scripts/run-ws8a-agent-authoring-matrix-export.ps1" -SummaryPath $summaryPath -OutputPath "tests/kpi/results/ws8a/ws8a-agent-authoring-matrix.json" }
  },
  @{
    Name = "ws8a-gate-trend-comparison"
    Script = "tests/kpi/scripts/run-ws8a-gate-trend-compare.ps1"
    Runner = { & "tests/kpi/scripts/run-ws8a-gate-trend-compare.ps1" -CurrentSummaryPath $summaryPath -PriorSummaryPath $priorSummaryPath -OutputPath "tests/kpi/results/ws8a/ws8a-gate-trend-comparison.json" }
  },
  @{
    Name = "ws8a-agent-stability-badge"
    Script = "tests/kpi/scripts/run-ws8a-agent-stability-badge.ps1"
    Runner = { & "tests/kpi/scripts/run-ws8a-agent-stability-badge.ps1" -SummaryPath $summaryPath -TrendPath "tests/kpi/results/ws8a/ws8a-gate-trend-comparison.json" -OutputPath "tests/kpi/results/ws8a/ws8a-agent-stability-badge.json" }
  },
  @{
    Name = "ws8a-release-summary"
    Script = "tests/kpi/scripts/run-ws8a-release-summary.ps1"
    Runner = { & "tests/kpi/scripts/run-ws8a-release-summary.ps1" -SummaryPath $summaryPath -MatrixPath "tests/kpi/results/ws8a/ws8a-agent-authoring-matrix.json" -TrendPath "tests/kpi/results/ws8a/ws8a-gate-trend-comparison.json" -BadgePath "tests/kpi/results/ws8a/ws8a-agent-stability-badge.json" -OutputPath $ReleaseSummaryOutputPath }
  }
)

foreach ($artifact in $postArtifacts) {
  try {
    & $artifact.Runner 2>&1 | Out-Null
    if (-not $?) {
      $status = "failed"
      $runs += [ordered]@{ pack = $artifact.Name; status = "failed"; detail = "script_invocation_failed"; artifact = $artifact.Script }
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
$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $summaryPath
$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "WS8A gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
