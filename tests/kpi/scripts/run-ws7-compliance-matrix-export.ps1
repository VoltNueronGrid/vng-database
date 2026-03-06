param(
  [string]$SummaryPath = "tests/kpi/results/ws7/ws7-gate-summary.json",
  [string]$OutputPath = "tests/kpi/results/ws7/ws7-compliance-matrix.json"
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
if (!(Test-Path -Path $SummaryPath)) {
  throw "WS7 gate summary not found at $SummaryPath"
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$packByName = @{}
foreach ($pack in $summary.packs) {
  $packByName[[string]$pack.pack] = [string]$pack.status
}

$matrix = @(
  [ordered]@{ control = "plugin_registration_boundary"; evidence_pack = "ws7-plugin-boundary"; status = $packByName["ws7-plugin-boundary"]; artifact = "tests/kpi/results/ws7/plugin-boundary-smoke.json" },
  [ordered]@{ control = "manifest_integrity_and_revocation"; evidence_pack = "ws7-manifest-integrity"; status = $packByName["ws7-manifest-integrity"]; artifact = "tests/kpi/results/ws7/ws7-manifest-integrity-smoke.json" },
  [ordered]@{ control = "registration_policy_and_capabilities"; evidence_pack = "ws7-registration-policy"; status = $packByName["ws7-registration-policy"]; artifact = "tests/kpi/results/ws7/ws7-registration-policy-smoke.json" }
)

$failed = @($matrix | Where-Object { $_.status -ne "passed" })
$status = if ($failed.Count -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  report = "ws7-plugin-compliance-matrix"
  status = $status
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  source_summary = $SummaryPath
  total_controls = $matrix.Count
  passed_controls = ($matrix | Where-Object { $_.status -eq "passed" }).Count
  failed_controls = $failed.Count
  matrix = $matrix
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS7 compliance matrix artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
