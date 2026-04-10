param(
  [string]$OutputPath = "tests/kpi/results/h03/h03-cluster-runtime-linkage-smoke.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Get-ArtifactStatus {
  param([string]$PathValue)
  if (!(Test-Path $PathValue)) { return "missing_artifact" }
  try {
    $j = Get-Content -Raw $PathValue | ConvertFrom-Json
    if ($null -ne $j.status) { return [string]$j.status }
    return "present"
  } catch {
    return "invalid_artifact"
  }
}

Ensure-OutputDir -PathValue $OutputPath

$checks = @(
  @{ name = "ws6_gate_summary_passed"; artifact = "tests/kpi/results/ws6/ws6-gate-summary.json" },
  @{ name = "ws6_chaos_fault_matrix_present"; artifact = "tests/kpi/results/ws6/ws6-chaos-fault-matrix.json" },
  @{ name = "ws6_release_readiness_ready"; artifact = "tests/kpi/results/gates/ws6-release-readiness.json" }
)

$results = @()
foreach ($check in $checks) {
  $status = Get-ArtifactStatus -PathValue $check.artifact
  $passed = $false
  if ($check.name -eq "ws6_release_readiness_ready") {
    if ($status -eq "passed") {
      $j = Get-Content -Raw $check.artifact | ConvertFrom-Json
      $passed = ([string]$j.release_readiness -eq "ready_for_validation")
      $status = "status=$($j.status);release_readiness=$($j.release_readiness)"
    }
  } elseif ($check.name -eq "ws6_chaos_fault_matrix_present") {
    $passed = ($status -eq "present" -or $status -eq "passed")
  } else {
    $passed = ($status -eq "passed")
  }

  $results += [ordered]@{
    name = $check.name
    passed = $passed
    detail = "$status :: $($check.artifact)"
  }
}

$passedCount = @($results | Where-Object { $_.passed }).Count
$totalCount = $results.Count
$overallStatus = if ($passedCount -eq $totalCount) { "passed" } else { "failed" }

$artifact = [ordered]@{
  smoke = "h03-cluster-runtime-linkage"
  status = $overallStatus
  checks_passed = $passedCount
  checks_total = $totalCount
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $results
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "H-03 cluster runtime linkage smoke: $OutputPath ($overallStatus)"
if ($overallStatus -ne "passed") { exit 1 }
