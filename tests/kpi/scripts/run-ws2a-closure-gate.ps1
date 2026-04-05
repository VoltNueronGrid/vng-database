param(
  [string]$OutputPath = "tests/kpi/results/ws2a/ws2a-closure-gate-summary.json"
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
$ws2aSummaryPath = "tests/kpi/results/ws2a/ws2a-gate-summary.json"
$ws2aSmokePath   = "tests/kpi/results/ws2a/row-sync-origin-smoke.json"

$runs = @()
$status = "passed"

# Run the WS2A gate
try {
  $global:LASTEXITCODE = 0
  & "tests/kpi/scripts/run-ws2a-gate.ps1" -OutputPath $ws2aSummaryPath 2>&1 | Out-Null
  if (-not $?) {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws2a-gate"; status = "failed"; detail = "script_invocation_failed"; artifact = $ws2aSummaryPath }
  } else {
    $runs += [ordered]@{ pack = "ws2a-gate"; status = "passed"; detail = "ok"; artifact = $ws2aSummaryPath }
  }
} catch {
  $status = "failed"
  $runs += [ordered]@{ pack = "ws2a-gate"; status = "failed"; detail = $_.Exception.Message; artifact = $ws2aSummaryPath }
}

$checks = [ordered]@{
  ws2a_gate_passed            = $false
  ws2a_row_sync_origin_passed = $false
  ws2a_all_artifacts_present  = $false
}

$allArtifacts = @($ws2aSummaryPath, $ws2aSmokePath)
$allPresent = $true
foreach ($path in $allArtifacts) {
  if (!(Test-Path -Path $path)) {
    $status = "failed"
    $allPresent = $false
    $runs += [ordered]@{ pack = "ws2a-artifact-presence"; status = "failed"; detail = "missing:$path"; artifact = $path }
  }
}
$checks["ws2a_all_artifacts_present"] = $allPresent

if ($status -eq "passed") {
  $summary = Get-Content -Raw -Path $ws2aSummaryPath | ConvertFrom-Json
  $smoke   = Get-Content -Raw -Path $ws2aSmokePath   | ConvertFrom-Json

  $checks["ws2a_gate_passed"]            = ([string]$summary.status -eq "passed")
  $checks["ws2a_row_sync_origin_passed"] = ([string]$smoke.status   -eq "passed")

  if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) { $status = "failed" }
}

$finished = Get-Date
$summaryOut = [ordered]@{
  gate               = "ws2a-closure-gate"
  status             = $status
  validation_posture = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  started_at_utc     = $start.ToUniversalTime().ToString("o")
  finished_at_utc    = $finished.ToUniversalTime().ToString("o")
  duration_ms        = [int](($finished - $start).TotalMilliseconds)
  artifacts          = [ordered]@{
    ws2a_gate       = $ws2aSummaryPath
    row_sync_origin = $ws2aSmokePath
  }
  checks             = $checks
  runs               = $runs
}

$summaryOut | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS2A closure gate summary: $OutputPath ($status)"
if ($status -eq "passed") {
  $outDir   = Split-Path -Parent $OutputPath
  $ciMirror = Join-Path $outDir "ci-ws2a-closure-gate-summary.json"
  if ($ciMirror -ne $OutputPath) {
    Copy-Item -LiteralPath $OutputPath -Destination $ciMirror -Force
    Write-Host "CI mirror: $ciMirror"
  }
}
if ($status -ne "passed") { exit 1 }
