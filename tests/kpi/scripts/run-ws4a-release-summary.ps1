param(
  [string]$SummaryPath  = "tests/kpi/results/ws4a/ws4a-gate-summary.json",
  [string]$StreamPath   = "tests/kpi/results/ws4a/streaming-event-path-smoke.json",
  [string]$ReplayPath   = "tests/kpi/results/ws4a/replay-cursor-smoke.json",
  [string]$OutputPath   = "tests/kpi/results/gates/ws4a-release-readiness.json"
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

foreach ($path in @($SummaryPath, $StreamPath, $ReplayPath)) {
  if (!(Test-Path -Path $path)) { throw "Required WS4A artifact missing at $path" }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$stream  = Get-Content -Raw -Path $StreamPath  | ConvertFrom-Json
$replay  = Get-Content -Raw -Path $ReplayPath  | ConvertFrom-Json

$checks = [ordered]@{
  ws4a_gate_passed          = ([string]$summary.status -eq "passed")
  ws4a_streaming_passed     = ([string]$stream.status  -eq "passed")
  ws4a_replay_cursor_passed = ([string]$replay.status  -eq "passed")
}

$failCount = @($checks.Values | Where-Object { $_ -eq $false }).Count
$status = if ($failCount -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate              = "ws4a-release-readiness"
  status            = $status
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  release_targets   = @("R1")
  scope             = @("WS4A", "REQ-18", "REQ-26")
  generated_at_utc  = (Get-Date).ToUniversalTime().ToString("o")
  sources           = [ordered]@{
    summary        = $SummaryPath
    streaming_path = $StreamPath
    replay_cursor  = $ReplayPath
  }
  checks            = $checks
  highlights        = [ordered]@{
    pack_count              = @($summary.packs).Count
    streaming_path_status   = [string]$stream.status
    replay_cursor_status    = [string]$replay.status
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS4A release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
