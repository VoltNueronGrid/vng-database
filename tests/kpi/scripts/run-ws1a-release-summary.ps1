param(
  [string]$SummaryPath    = "tests/kpi/results/ws1a/ws1a-gate-summary.json",
  [string]$UdfBridgePath  = "tests/kpi/results/ws1a/ws1a-udf-contract-bridge-smoke.json",
  [string]$LegacyEvalPath = "tests/kpi/results/ws1a/ws1a-legacy-numeric-eval-smoke.json",
  [string]$ParityPath     = "tests/kpi/results/parity/legacy-aggregation-parity.json",
  [string]$GapReportPath  = "tests/kpi/results/parity/legacy-aggregation-gap-report.json",
  [string]$OutputPath     = "tests/kpi/results/gates/ws1a-release-readiness.json"
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

foreach ($path in @($SummaryPath, $UdfBridgePath, $LegacyEvalPath, $ParityPath, $GapReportPath)) {
  if (!(Test-Path -Path $path)) { throw "Required WS1A artifact missing at $path" }
}

$summary    = Get-Content -Raw -Path $SummaryPath    | ConvertFrom-Json
$udfBridge  = Get-Content -Raw -Path $UdfBridgePath  | ConvertFrom-Json
$legacyEval = Get-Content -Raw -Path $LegacyEvalPath | ConvertFrom-Json
$parity     = Get-Content -Raw -Path $ParityPath     | ConvertFrom-Json
$gapReport  = Get-Content -Raw -Path $GapReportPath  | ConvertFrom-Json

$checks = [ordered]@{
  ws1a_gate_passed        = ([string]$summary.status    -eq "passed")
  ws1a_udf_bridge_passed  = ([string]$udfBridge.status  -eq "passed")
  ws1a_legacy_eval_passed = ([string]$legacyEval.status -eq "passed")
  ws1a_parity_passed      = ([string]$parity.status     -eq "passed")
  ws1a_gap_report_passed  = ([string]$gapReport.status  -eq "passed")
}

$failCount = @($checks.Values | Where-Object { $_ -eq $false }).Count
$status = if ($failCount -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate              = "ws1a-release-readiness"
  status            = $status
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  release_targets   = @("R1")
  scope             = @("WS1A", "REQ-12")
  generated_at_utc  = (Get-Date).ToUniversalTime().ToString("o")
  sources           = [ordered]@{
    summary     = $SummaryPath
    udf_bridge  = $UdfBridgePath
    legacy_eval = $LegacyEvalPath
    parity      = $ParityPath
    gap_report  = $GapReportPath
  }
  checks            = $checks
  highlights        = [ordered]@{
    pack_count          = @($summary.packs).Count
    udf_bridge_status   = [string]$udfBridge.status
    legacy_eval_status  = [string]$legacyEval.status
    parity_status       = [string]$parity.status
    gap_report_status   = [string]$gapReport.status
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS1A release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
