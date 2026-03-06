param(
  [string]$SummaryPath = "tests/kpi/results/ws8a/ws8a-gate-summary.json",
  [string]$OutputPath = "tests/kpi/results/ws8a/ws8a-agent-authoring-matrix.json"
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
if (!(Test-Path -Path $SummaryPath)) { throw "WS8A gate summary not found at $SummaryPath" }

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$packByName = @{}
foreach ($pack in $summary.packs) { $packByName[[string]$pack.pack] = [string]$pack.status }

$matrix = @(
  [ordered]@{ control = "audit_trail_append_only"; evidence_pack = "ws8a-audit-trail"; status = $packByName["ws8a-audit-trail"]; artifact = "tests/kpi/results/ws8a/audit-trail-smoke.json" },
  [ordered]@{ control = "audit_companion_trace_action_filter"; evidence_pack = "ws8a-audit-companion"; status = $packByName["ws8a-audit-companion"]; artifact = "tests/kpi/results/ws8a/audit-companion-smoke.json" },
  [ordered]@{ control = "agent_authoring_object_plugin_workflow"; evidence_pack = "ws8a-agent-authoring"; status = $packByName["ws8a-agent-authoring"]; artifact = "tests/kpi/results/ws8a/ws8a-agent-authoring-smoke.json" }
)

$failed = @($matrix | Where-Object { $_.status -ne "passed" })
$status = if ($failed.Count -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  report = "ws8a-agent-authoring-matrix"
  status = $status
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  source_summary = $SummaryPath
  total_controls = $matrix.Count
  passed_controls = ($matrix | Where-Object { $_.status -eq "passed" }).Count
  failed_controls = $failed.Count
  matrix = $matrix
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS8A agent authoring matrix artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
