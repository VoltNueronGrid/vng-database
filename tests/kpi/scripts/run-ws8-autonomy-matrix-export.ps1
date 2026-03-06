param(
  [string]$SummaryPath = "tests/kpi/results/ws8/ws8-gate-summary.json",
  [string]$OutputPath = "tests/kpi/results/ws8/ws8-autonomy-matrix.json"
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
if (!(Test-Path -Path $SummaryPath)) { throw "WS8 gate summary not found at $SummaryPath" }

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$packByName = @{}
foreach ($pack in $summary.packs) { $packByName[[string]$pack.pack] = [string]$pack.status }

$matrix = @(
  [ordered]@{ control = "typed_action_record_baseline"; evidence_pack = "ws8-control-plane"; status = $packByName["ws8-control-plane"]; artifact = "tests/kpi/results/ws8/control-plane-smoke.json" },
  [ordered]@{ control = "guardrail_policy_and_emergency_stop"; evidence_pack = "ws8-guardrail-policy"; status = $packByName["ws8-guardrail-policy"]; artifact = "tests/kpi/results/ws8/ws8-guardrail-policy-smoke.json" },
  [ordered]@{ control = "mode_governance_and_blast_radius_policy"; evidence_pack = "ws8-mode-governance"; status = $packByName["ws8-mode-governance"]; artifact = "tests/kpi/results/ws8/ws8-mode-governance-smoke.json" },
  [ordered]@{ control = "audit_trail_linkage"; evidence_pack = "ws8a-audit-trail"; status = $packByName["ws8a-audit-trail"]; artifact = "tests/kpi/results/ws8a/audit-trail-smoke.json" },
  [ordered]@{ control = "audit_companion_trace_filter"; evidence_pack = "ws8a-audit-companion"; status = $packByName["ws8a-audit-companion"]; artifact = "tests/kpi/results/ws8a/audit-companion-smoke.json" }
)

$failed = @($matrix | Where-Object { $_.status -ne "passed" })
$status = if ($failed.Count -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  report = "ws8-autonomous-control-plane-matrix"
  status = $status
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  source_summary = $SummaryPath
  total_controls = $matrix.Count
  passed_controls = ($matrix | Where-Object { $_.status -eq "passed" }).Count
  failed_controls = $failed.Count
  matrix = $matrix
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS8 autonomy matrix artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
