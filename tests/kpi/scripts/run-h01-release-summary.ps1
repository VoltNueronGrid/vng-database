param(
  [string]$SummaryPath = "tests/kpi/results/h01/h01-gate-summary.json",
  [string]$OutputPath = "tests/kpi/results/gates/h01-release-readiness.json"
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

if (!(Test-Path $SummaryPath)) { throw "Missing H-01 gate summary: $SummaryPath" }

$summary = Get-Content -Raw $SummaryPath | ConvertFrom-Json
$checks = [ordered]@{
  h01_gate_passed = ([string]$summary.status -eq "passed")
  h01_all_packs_passed = ((@($summary.packs | Where-Object { $_.status -ne "passed" }).Count) -eq 0)
  h01_full_cross_channel_rbac_complete = $false
}

$status = if (($checks.h01_gate_passed -and $checks.h01_all_packs_passed)) { "passed" } else { "failed" }
$releaseReadiness = if ($status -eq "passed") { "in_progress_with_evidence" } else { "blocked" }

$artifact = [ordered]@{
  gate = "h01-release-autonomous-blast-radius-readiness"
  status = $status
  release_readiness = $releaseReadiness
  release_targets = @("R2")
  scope = @("WS5", "WS8", "WS12", "REQ-13", "REQ-29", "H-01")
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  sources = [ordered]@{
    summary = "tests/kpi/results/h01/h01-gate-summary.json"
    ws5_operator_auth = "tests/kpi/results/ws5/operator-auth-smoke.json"
    ws8_mode_governance = "tests/kpi/results/ws8/ws8-mode-governance-smoke.json"
    ws8_guardrail_policy = "tests/kpi/results/ws8/ws8-guardrail-policy-smoke.json"
    legacy_h01_guardrail = "tests/kpi/results/20260305-h01/autonomous-guardrail-smoke.json"
  }
  checks = $checks
  highlights = [ordered]@{
    pack_count = @($summary.packs).Count
    h01_operator_rbac_pack_status = [string](($summary.packs | Where-Object { $_.pack -eq "h01-operator-rbac-evidence" } | Select-Object -First 1).status)
    h01_autonomy_policy_pack_status = [string](($summary.packs | Where-Object { $_.pack -eq "h01-autonomy-policy-evidence" } | Select-Object -First 1).status)
    h01_emergency_stop_pack_status = [string](($summary.packs | Where-Object { $_.pack -eq "h01-emergency-stop-evidence" } | Select-Object -First 1).status)
    blocker = "full_resource_scoped_rbac_and_cross_channel_blast_radius_certification_pending"
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "H-01 release summary artifact: $OutputPath ($status, $releaseReadiness)"
if ($status -ne "passed") { exit 1 }
