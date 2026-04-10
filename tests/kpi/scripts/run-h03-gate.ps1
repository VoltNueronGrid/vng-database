param(
  [string]$OutputPath = "tests/kpi/results/h03/h03-gate-summary.json",
  [string]$ReleaseSummaryOutputPath = "tests/kpi/results/gates/h03-release-readiness.json"
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
  if (!(Test-Path -Path $ArtifactPath)) { return "missing_artifact" }
  try {
    $json = Get-Content -Raw -Path $ArtifactPath | ConvertFrom-Json
    if ($null -ne $json.status) { return [string]$json.status }
    return "present"
  } catch {
    return "invalid_artifact"
  }
}

Ensure-OutputDir -PathValue $OutputPath
Ensure-OutputDir -PathValue $ReleaseSummaryOutputPath

$start = Get-Date
$runs = @()
$status = "passed"

$packs = @(
  @{ Name = "h03-control-plane-chaos-evidence"; Script = "tests/kpi/scripts/run-h03-control-plane-chaos-evidence-smoke.ps1"; Artifact = "tests/kpi/results/h03/h03-control-plane-chaos-evidence-smoke.json" },
  @{ Name = "h03-cluster-runtime-linkage"; Script = "tests/kpi/scripts/run-h03-cluster-runtime-linkage-smoke.ps1"; Artifact = "tests/kpi/results/h03/h03-cluster-runtime-linkage-smoke.json" },
  @{ Name = "h03-degraded-failover-evidence"; Script = "tests/kpi/scripts/run-h03-degraded-failover-evidence-smoke.ps1"; Artifact = "tests/kpi/results/h03/h03-degraded-failover-evidence-smoke.json" }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  try {
    $global:LASTEXITCODE = 0
    & $pack.Script -OutputPath $pack.Artifact 2>&1 | Out-Null
    $artifactStatus = Get-ArtifactStatus -ArtifactPath $pack.Artifact
    if ($artifactStatus -ne "passed") {
      $packStatus = "failed"
      $detail = $artifactStatus
    } elseif ($global:LASTEXITCODE -ne 0) {
      $packStatus = "failed"
      $detail = "exit_code=$global:LASTEXITCODE"
    }
  } catch {
    $packStatus = "failed"
    $detail = $_.Exception.Message
  }

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
  gate = "h03"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath -Encoding UTF8

try {
  $global:LASTEXITCODE = 0
  & "tests/kpi/scripts/run-h03-release-summary.ps1" -SummaryPath $OutputPath -OutputPath $ReleaseSummaryOutputPath 2>&1 | Out-Null
  if (-not $?) { $status = "failed" }
  elseif ($global:LASTEXITCODE -ne 0) { $status = "failed" }
} catch {
  $status = "failed"
}

$summary.status = $status
$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "H-03 gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
