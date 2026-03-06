param(
  [string]$OutputPath = "tests/kpi/results/ws7/ws7-gate-summary.json",
  [string]$ReleaseSummaryOutputPath = "tests/kpi/results/gates/ws7-release-readiness.json"
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
$priorSummaryPath = "tests/kpi/results/ws7/ws7-gate-summary.previous.json"
if (Test-Path -Path $OutputPath) { Copy-Item -Path $OutputPath -Destination $priorSummaryPath -Force }

$start = Get-Date
$runs = @()
$status = "passed"

$packs = @(
  @{ Name = "ws7-plugin-boundary"; Script = "tests/kpi/scripts/run-ws7-plugin-boundary-smoke.ps1"; Artifact = "tests/kpi/results/ws7/plugin-boundary-smoke.json" },
  @{ Name = "ws7-manifest-integrity"; Script = "tests/kpi/scripts/run-ws7-manifest-integrity-smoke.ps1"; Artifact = "tests/kpi/results/ws7/ws7-manifest-integrity-smoke.json" },
  @{ Name = "ws7-registration-policy"; Script = "tests/kpi/scripts/run-ws7-registration-policy-smoke.ps1"; Artifact = "tests/kpi/results/ws7/ws7-registration-policy-smoke.json" }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  try {
    $global:LASTEXITCODE = 0
    & $pack.Script -OutputPath $pack.Artifact 2>&1 | Out-Null
    if (-not $?) { $packStatus = "failed"; $detail = "script_invocation_failed" }
    elseif ($global:LASTEXITCODE -ne 0) { $packStatus = "failed"; $detail = "exit_code=$global:LASTEXITCODE" }
  } catch { $packStatus = "failed"; $detail = $_.Exception.Message }
  if ($packStatus -ne "passed") { $status = "failed" }
  $runs += [ordered]@{ pack = $pack.Name; status = $packStatus; detail = $detail; artifact = $pack.Artifact }
}

$finished = Get-Date
$summary = [ordered]@{
  gate = "ws7"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

$postArtifacts = @(
  @{
    Name = "ws7-compliance-matrix"
    Script = "tests/kpi/scripts/run-ws7-compliance-matrix-export.ps1"
    Runner = { & "tests/kpi/scripts/run-ws7-compliance-matrix-export.ps1" -SummaryPath $OutputPath -OutputPath "tests/kpi/results/ws7/ws7-compliance-matrix.json" }
  },
  @{
    Name = "ws7-gate-trend-comparison"
    Script = "tests/kpi/scripts/run-ws7-gate-trend-compare.ps1"
    Runner = { & "tests/kpi/scripts/run-ws7-gate-trend-compare.ps1" -CurrentSummaryPath $OutputPath -PriorSummaryPath $priorSummaryPath -OutputPath "tests/kpi/results/ws7/ws7-gate-trend-comparison.json" }
  },
  @{
    Name = "ws7-plugin-stability-badge"
    Script = "tests/kpi/scripts/run-ws7-plugin-stability-badge.ps1"
    Runner = { & "tests/kpi/scripts/run-ws7-plugin-stability-badge.ps1" -SummaryPath $OutputPath -TrendPath "tests/kpi/results/ws7/ws7-gate-trend-comparison.json" -OutputPath "tests/kpi/results/ws7/ws7-plugin-stability-badge.json" }
  },
  @{
    Name = "ws7-release-summary"
    Script = "tests/kpi/scripts/run-ws7-release-summary.ps1"
    Runner = { & "tests/kpi/scripts/run-ws7-release-summary.ps1" -SummaryPath $OutputPath -ComplianceMatrixPath "tests/kpi/results/ws7/ws7-compliance-matrix.json" -TrendPath "tests/kpi/results/ws7/ws7-gate-trend-comparison.json" -BadgePath "tests/kpi/results/ws7/ws7-plugin-stability-badge.json" -OutputPath $ReleaseSummaryOutputPath }
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
Write-Host "WS7 gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
