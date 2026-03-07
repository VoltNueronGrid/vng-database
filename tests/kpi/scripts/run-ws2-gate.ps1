param(
  [string]$OutputPath = "tests/kpi/results/ws2/ws2-gate-summary.json"
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
  @{ Name = "ws2-store-durability"; Script = "tests/kpi/scripts/run-store-durability-smoke.ps1"; Artifact = "tests/kpi/results/ws2/store-durability-smoke.json" },
  @{ Name = "ws2-disk-wal"; Script = "tests/kpi/scripts/run-ws2-disk-wal-smoke.ps1"; Artifact = "tests/kpi/results/ws2/disk-wal-adapter-smoke.json" },
  @{ Name = "ws2-checkpoint-restart"; Script = "tests/kpi/scripts/run-ws2-checkpoint-restart-smoke.ps1"; Artifact = "tests/kpi/results/ws2/ws2-checkpoint-restart-smoke.json" },
  @{ Name = "ws2-index-constraint"; Script = "tests/kpi/scripts/run-ws2-index-constraint-smoke.ps1"; Artifact = "tests/kpi/results/ws2/ws2-index-constraint-smoke.json" }
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
  gate = "ws2"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "WS2 gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
