param(
  [string]$SummaryPath = "tests/kpi/results/h09/h09-gate-summary.json",
  [string]$ParityPath = "tests/kpi/results/h09/h09-ide-parity-matrix.json",
  [string]$OutputPath = "tests/kpi/results/gates/h09-release-readiness.json"
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
foreach ($path in @($SummaryPath, $ParityPath)) {
  if (!(Test-Path -Path $path)) { throw "Required H-09 artifact missing at $path" }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$parity = Get-Content -Raw -Path $ParityPath | ConvertFrom-Json

$programSignoffApproved = ([string]$env:VNG_PROGRAM_SIGNOFF_APPROVED).ToLowerInvariant() -in @("1", "true", "yes", "y")

$checks = [ordered]@{
  h09_gate_passed = ([string]$summary.status -eq "passed")
  h09_parity_smoke_passed = ([string]$parity.status -eq "passed")
}

$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate = "h09-ide-parity-readiness"
  status = $status
  release_readiness = if ($status -ne "passed") { "blocked" } elseif ($programSignoffApproved) { "ready_for_validation" } else { "in_progress_with_evidence" }
  release_targets = @("R4")
  scope = @("H-09", "REQ-28")
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  sources = [ordered]@{
    summary = $SummaryPath
    parity = $ParityPath
  }
  checks = $checks
  highlights = [ordered]@{
    check_count = @($parity.checks).Count
    passed_checks = (@($parity.checks | Where-Object { $_.ok }).Count)
  }
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "H-09 release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
