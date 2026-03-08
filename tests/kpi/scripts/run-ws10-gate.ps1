param(
  [string]$OutputPath = "tests/kpi/results/ws10/ws10-gate-summary.json"
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

$start = Get-Date
$runs = @()
$status = "passed"

$packs = @(
  @{
    Name = "ws10-driver-contract-smoke"
    Script = "tests/kpi/scripts/run-ws10-driver-smoke.ps1"
    Artifact = "tests/kpi/results/ws10/driver-smoke.json"
  }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  try {
    & $pack.Script -OutputPath $pack.Artifact 2>&1 | Out-Null
    $artifactStatus = Get-ArtifactStatus -ArtifactPath $pack.Artifact
    if ($artifactStatus -eq "passed") {
      $packStatus = "passed"
    } else {
      $packStatus = "failed"
      $detail = $artifactStatus
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
  gate = "ws10"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

Write-Host "WS10 gate summary: $OutputPath ($status)"
if ($status -ne "passed") {
  exit 1
}
