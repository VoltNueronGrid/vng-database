param(
  [string]$OutputPath = "tests/kpi/results/ws1a/ws1a-closure-gate-summary.json"
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
$ws1aSummaryPath    = "tests/kpi/results/ws1a/ws1a-gate-summary.json"
$ws1aUdfBridgePath  = "tests/kpi/results/ws1a/ws1a-udf-contract-bridge-smoke.json"
$ws1aLegacyEvalPath = "tests/kpi/results/ws1a/ws1a-legacy-numeric-eval-smoke.json"
$ws1aParityPath     = "tests/kpi/results/parity/legacy-aggregation-parity.json"
$ws1aGapReportPath  = "tests/kpi/results/parity/legacy-aggregation-gap-report.json"

$runs = @()
$status = "passed"

$checks = [ordered]@{
  ws1a_gate_passed          = $false
  ws1a_udf_bridge_passed    = $false
  ws1a_legacy_eval_passed   = $false
  ws1a_parity_passed        = $false
  ws1a_gap_report_passed    = $false
  ws1a_all_packs_present    = $false
}

# Validate existing artifacts -- do not re-run live HTTP packs
$allArtifacts = @($ws1aSummaryPath, $ws1aUdfBridgePath, $ws1aLegacyEvalPath, $ws1aParityPath, $ws1aGapReportPath)
$allPresent = $true
foreach ($path in $allArtifacts) {
  if (!(Test-Path -Path $path)) {
    $status = "failed"
    $allPresent = $false
    $runs += [ordered]@{ pack = "ws1a-artifact-presence"; status = "failed"; detail = "missing:$path"; artifact = $path }
  }
}
$checks["ws1a_all_packs_present"] = $allPresent

if ($allPresent) {
  $summary    = Get-Content -Raw -Path $ws1aSummaryPath    | ConvertFrom-Json
  $udfBridge  = Get-Content -Raw -Path $ws1aUdfBridgePath  | ConvertFrom-Json
  $legacyEval = Get-Content -Raw -Path $ws1aLegacyEvalPath | ConvertFrom-Json
  $parity     = Get-Content -Raw -Path $ws1aParityPath     | ConvertFrom-Json
  $gapReport  = Get-Content -Raw -Path $ws1aGapReportPath  | ConvertFrom-Json

  $checks["ws1a_gate_passed"]        = ([string]$summary.status    -eq "passed")
  $checks["ws1a_udf_bridge_passed"]  = ([string]$udfBridge.status  -eq "passed")
  $checks["ws1a_legacy_eval_passed"] = ([string]$legacyEval.status -eq "passed")
  $checks["ws1a_parity_passed"]      = ([string]$parity.status     -eq "passed")
  $checks["ws1a_gap_report_passed"]  = ([string]$gapReport.status  -eq "passed")

  if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) { $status = "failed" }
  $runs += [ordered]@{ pack = "ws1a-artifact-validation"; status = $status; detail = "checked_existing_artifacts"; artifact = $ws1aSummaryPath }
}

$finished = Get-Date
$summaryOut = [ordered]@{
  gate               = "ws1a-closure-gate"
  status             = $status
  validation_posture = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  started_at_utc     = $start.ToUniversalTime().ToString("o")
  finished_at_utc    = $finished.ToUniversalTime().ToString("o")
  duration_ms        = [int](($finished - $start).TotalMilliseconds)
  artifacts          = [ordered]@{
    ws1a_gate      = $ws1aSummaryPath
    udf_bridge     = $ws1aUdfBridgePath
    legacy_eval    = $ws1aLegacyEvalPath
    parity         = $ws1aParityPath
    gap_report     = $ws1aGapReportPath
  }
  checks             = $checks
  runs               = $runs
}

$summaryOut | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS1A closure gate summary: $OutputPath ($status)"
if ($status -eq "passed") {
  $outDir   = Split-Path -Parent $OutputPath
  $ciMirror = Join-Path $outDir "ci-ws1a-closure-gate-summary.json"
  if ($ciMirror -ne $OutputPath) {
    Copy-Item -LiteralPath $OutputPath -Destination $ciMirror -Force
    Write-Host "CI mirror: $ciMirror"
  }
}
if ($status -ne "passed") { exit 1 }
