param(
  [string]$OutputPath = "tests/kpi/results/gates/release-ops-resilience-readiness.json"
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
    Name = "ws12-reliability-smoke"
    Script = "tests/kpi/scripts/run-ws12-reliability-smoke.ps1"
    Artifact = "tests/kpi/results/ws12/reliability-sre-smoke.json"
  },
  @{
    Name = "ws13-multicloud-gate"
    Script = "tests/kpi/scripts/run-ws13-gate.ps1"
    Artifact = "tests/kpi/results/ws13/ws13-gate-summary.json"
  },
  @{
    Name = "ws14-config-gate"
    Script = "tests/kpi/scripts/run-ws14-gate.ps1"
    Artifact = "tests/kpi/results/ws14/ws14-gate-summary.json"
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
  gate = "release-ops-resilience-cluster"
  status = $status
  release_targets = @("R2", "R3")
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  scope = @("WS12", "WS13", "WS14")
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

Write-Host "Release Ops/Resilience gate summary: $OutputPath ($status)"
if ($status -ne "passed") {
  exit 1
}
