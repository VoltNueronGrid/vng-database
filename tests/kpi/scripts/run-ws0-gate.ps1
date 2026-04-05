param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$OutputPath = "tests/kpi/results/ws0/ws0-gate-summary.json"
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

$start = Get-Date
$runs = @()
$status = "passed"

$packs = @(
  @{
    Name     = "ws0-ci-health"
    Script   = "tests/kpi/scripts/run-ws0-ci-health-smoke.ps1"
    Artifact = "tests/kpi/results/ws0/ws0-ci-health-smoke.json"
    Args     = @{ RepoRoot = $RepoRoot }
  },
  @{
    Name     = "ws0-governance"
    Script   = "tests/kpi/scripts/run-ws0-governance-smoke.ps1"
    Artifact = "tests/kpi/results/ws0/ws0-governance-smoke.json"
    Args     = @{ RepoRoot = $RepoRoot }
  }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  try {
    $global:LASTEXITCODE = 0
    & $pack.Script -RepoRoot $pack.Args.RepoRoot -OutputPath $pack.Artifact 2>&1 | Out-Null
    if (-not $?) { $packStatus = "failed"; $detail = "script_invocation_failed" }
  } catch { $packStatus = "failed"; $detail = $_.Exception.Message }
  # Derive gate status from artifact — never rely on $LASTEXITCODE alone
  if ($packStatus -eq "passed" -and (Test-Path -Path $pack.Artifact)) {
    $artifactObj = Get-Content -Raw -Path $pack.Artifact | ConvertFrom-Json
    if ([string]$artifactObj.status -ne "passed") {
      $packStatus = "failed"
      $detail = "artifact_status=$($artifactObj.status)"
    }
  } elseif ($packStatus -eq "passed") {
    $packStatus = "failed"
    $detail = "artifact_not_written"
  }
  if ($packStatus -ne "passed") { $status = "failed" }
  $runs += [ordered]@{
    pack     = $pack.Name
    status   = $packStatus
    detail   = $detail
    artifact = $pack.Artifact
  }
}

$finished = Get-Date
$summary = [ordered]@{
  gate            = "ws0"
  status          = $status
  started_at_utc  = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms     = [int](($finished - $start).TotalMilliseconds)
  packs           = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS0 gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
