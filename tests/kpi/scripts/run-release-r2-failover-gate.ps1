param(
  [string]$OutputPath = "tests/kpi/results/gates/release-r2-failover-readiness.json"
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
    Name = "ws6-closure-gate"
    Script = "tests/kpi/scripts/run-ws6-closure-gate.ps1"
    Artifact = "tests/kpi/results/ws6/ws6-closure-gate-summary.json"
  },
  @{
    Name = "release-ops-resilience-cluster"
    Script = "tests/kpi/scripts/run-release-ops-resilience-gate.ps1"
    Artifact = "tests/kpi/results/gates/release-ops-resilience-readiness.json"
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
$artifact = [ordered]@{
  gate = "release-r2-failover-readiness"
  status = $status
  release_target = "R2"
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  scope = @("WS6", "WS12", "WS13", "WS14", "REQ-17")
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "Release R2 failover gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
