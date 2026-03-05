param(
  [string]$OutputPath = "tests/kpi/results/gates/release-dx-api-readiness.json"
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
    Name = "ws5-security-gate"
    Script = "tests/kpi/scripts/run-ws5-gate.ps1"
    Artifact = "tests/kpi/results/ws5/ws5-gate-summary.json"
  },
  @{
    Name = "ws9-studio-gate"
    Script = "tests/kpi/scripts/run-ws9-gate.ps1"
    Artifact = "tests/kpi/results/ws9/ws9-gate-summary.json"
  },
  @{
    Name = "ws9a-ide-contract-smoke"
    Script = "tests/kpi/scripts/run-ws9a-ide-contract-smoke.ps1"
    Artifact = "tests/kpi/results/ws9a/ide-contract-smoke.json"
  },
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
  gate = "release-dx-api-cluster"
  status = $status
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  scope = @("WS5", "WS9", "WS9A", "WS10")
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

Write-Host "Release DX/API gate summary: $OutputPath ($status)"
if ($status -ne "passed") {
  exit 1
}
