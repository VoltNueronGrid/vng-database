param(
  [string]$SummaryPath = "tests/kpi/results/h10/h10-gate-summary.json",
  [string]$ChecklistPath = "tests/kpi/results/h10/h10-governance-checklist.json",
  [string]$OutputPath = "tests/kpi/results/gates/h10-release-readiness.json"
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
foreach ($path in @($SummaryPath, $ChecklistPath)) {
  if (!(Test-Path -Path $path)) { throw "Required H-10 artifact missing at $path" }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$checklist = Get-Content -Raw -Path $ChecklistPath | ConvertFrom-Json

$programSignoffApproved = ([string]$env:VNG_PROGRAM_SIGNOFF_APPROVED).ToLowerInvariant() -in @("1", "true", "yes", "y")

$checks = [ordered]@{
  h10_gate_passed = ([string]$summary.status -eq "passed")
  h10_checklist_passed = ([string]$checklist.status -eq "passed")
}

$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate = "h10-governance-readiness"
  status = $status
  release_readiness = if ($status -ne "passed") { "blocked" } elseif ($programSignoffApproved) { "ready_for_validation" } else { "in_progress_with_evidence" }
  release_targets = @("R4")
  scope = @("H-10")
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  sources = [ordered]@{
    summary = $SummaryPath
    checklist = $ChecklistPath
  }
  checks = $checks
  highlights = [ordered]@{
    checklist_total = @($checklist.checks).Count
    checklist_passed = @($checklist.checks | Where-Object { $_.ok }).Count
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "H-10 release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
