param(
  [string]$OutputPath = "tests/kpi/results/ws4a/ws4a-closure-gate-summary.json"
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
$ws4aSummaryPath = "tests/kpi/results/ws4a/ws4a-gate-summary.json"
$ws4aStreamPath  = "tests/kpi/results/ws4a/streaming-event-path-smoke.json"
$ws4aReplayPath  = "tests/kpi/results/ws4a/replay-cursor-smoke.json"

$runs = @()
$status = "passed"

$checks = [ordered]@{
  ws4a_gate_passed         = $false
  ws4a_streaming_passed    = $false
  ws4a_replay_cursor_passed = $false
  ws4a_all_packs_present   = $false
}

# Validate existing artifacts -- do not re-run live HTTP packs
$allArtifacts = @($ws4aSummaryPath, $ws4aStreamPath, $ws4aReplayPath)
$allPresent = $true
foreach ($path in $allArtifacts) {
  if (!(Test-Path -Path $path)) {
    $status = "failed"
    $allPresent = $false
    $runs += [ordered]@{ pack = "ws4a-artifact-presence"; status = "failed"; detail = "missing:$path"; artifact = $path }
  }
}
$checks["ws4a_all_packs_present"] = $allPresent

if ($allPresent) {
  $summary = Get-Content -Raw -Path $ws4aSummaryPath | ConvertFrom-Json
  $stream  = Get-Content -Raw -Path $ws4aStreamPath  | ConvertFrom-Json
  $replay  = Get-Content -Raw -Path $ws4aReplayPath  | ConvertFrom-Json

  $checks["ws4a_gate_passed"]          = ([string]$summary.status -eq "passed")
  $checks["ws4a_streaming_passed"]     = ([string]$stream.status  -eq "passed")
  $checks["ws4a_replay_cursor_passed"] = ([string]$replay.status  -eq "passed")

  if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) { $status = "failed" }
  $runs += [ordered]@{ pack = "ws4a-artifact-validation"; status = $status; detail = "checked_existing_artifacts"; artifact = $ws4aSummaryPath }
}

$finished = Get-Date
$summaryOut = [ordered]@{
  gate               = "ws4a-closure-gate"
  status             = $status
  validation_posture = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  started_at_utc     = $start.ToUniversalTime().ToString("o")
  finished_at_utc    = $finished.ToUniversalTime().ToString("o")
  duration_ms        = [int](($finished - $start).TotalMilliseconds)
  artifacts          = [ordered]@{
    ws4a_gate      = $ws4aSummaryPath
    streaming_path = $ws4aStreamPath
    replay_cursor  = $ws4aReplayPath
  }
  checks             = $checks
  runs               = $runs
}

$summaryOut | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS4A closure gate summary: $OutputPath ($status)"
if ($status -eq "passed") {
  $outDir   = Split-Path -Parent $OutputPath
  $ciMirror = Join-Path $outDir "ci-ws4a-closure-gate-summary.json"
  if ($ciMirror -ne $OutputPath) {
    Copy-Item -LiteralPath $OutputPath -Destination $ciMirror -Force
    Write-Host "CI mirror: $ciMirror"
  }
}
if ($status -ne "passed") { exit 1 }
