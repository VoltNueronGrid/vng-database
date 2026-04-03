param(
  [string]$OutputPath = "tests/kpi/results/h10/h10-gate-summary.json",
  [string]$ReleaseSummaryOutputPath = "tests/kpi/results/gates/h10-release-readiness.json"
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

$start = Get-Date
$runs = @()
$status = "passed"

$packs = @(
  @{
    Name = "h10-governance-checklist"
    Script = "tests/kpi/scripts/run-h10-governance-checklist.ps1"
    Artifact = "tests/kpi/results/h10/h10-governance-checklist.json"
  }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  try {
    $global:LASTEXITCODE = 0
    & $pack.Script -OutputPath $pack.Artifact 2>&1 | Out-Null
    if (-not (Test-Path -Path $pack.Artifact)) {
      $packStatus = "failed"
      $detail = "artifact_not_generated"
    } elseif (-not $?) {
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
  gate = "h10"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

try {
  $global:LASTEXITCODE = 0
  & "tests/kpi/scripts/run-h10-release-summary.ps1" -SummaryPath $OutputPath -ChecklistPath "tests/kpi/results/h10/h10-governance-checklist.json" -OutputPath $ReleaseSummaryOutputPath 2>&1 | Out-Null
  if (-not $?) {
    $status = "failed"
  } elseif ($global:LASTEXITCODE -ne 0) {
    $status = "failed"
  }
} catch {
  $status = "failed"
}

$summary.status = $status
$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "H-10 gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
