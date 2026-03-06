param(
  [string]$OutputPath = "tests/kpi/results/gates/release-r3-plugin-readiness.json"
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
    Name = "ws7-closure-gate"
    Script = "tests/kpi/scripts/run-ws7-closure-gate.ps1"
    Artifact = "tests/kpi/results/ws7/ws7-closure-gate-summary.json"
  },
  @{
    Name = "ws9a-ide-extension-gate"
    Script = "tests/kpi/scripts/run-ws9a-gate.ps1"
    Artifact = "tests/kpi/results/ws9a/ws9a-gate-summary.json"
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
  if ($packStatus -ne "passed") { $status = "failed" }
  $runs += [ordered]@{ pack = $pack.Name; status = $packStatus; detail = $detail; artifact = $pack.Artifact }
}

$finished = Get-Date
$artifact = [ordered]@{
  gate = "release-r3-plugin-readiness"
  status = $status
  release_target = "R3"
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  scope = @("WS7", "REQ-09", "REQ-26", "WS9A")
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "Release R3 plugin gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
