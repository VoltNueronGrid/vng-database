param(
  [string]$OutputPath = "tests/kpi/results/ws2a/ws2a-gate-summary.json"
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

$packStatus = "passed"
$detail = "ok"
try {
  $global:LASTEXITCODE = 0
  & "tests/kpi/scripts/run-ws2a-row-sync-origin-smoke.ps1" -OutputPath "tests/kpi/results/ws2a/row-sync-origin-smoke.json" 2>&1 | Out-Null
  if (-not $?) { $packStatus = "failed"; $detail = "script_invocation_failed" }
  elseif ($global:LASTEXITCODE -ne 0) { $packStatus = "failed"; $detail = "exit_code=$global:LASTEXITCODE" }
} catch { $packStatus = "failed"; $detail = $_.Exception.Message }
if ($packStatus -ne "passed") { $status = "failed" }
$runs += [ordered]@{
  pack = "ws2a-row-sync-origin"
  status = $packStatus
  detail = $detail
  artifact = "tests/kpi/results/ws2a/row-sync-origin-smoke.json"
}

$finished = Get-Date
$summary = [ordered]@{
  gate = "ws2a"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "WS2A gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
