param(
  [string]$OutputPath = "tests/kpi/results/h03/h03-degraded-failover-evidence-smoke.json"
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
  @{ name = "ws6_failover_contract_smoke_passed"; artifact = "tests/kpi/results/ws6/failover-contract-smoke.json" },
  @{ name = "ws6_dr_failover_smoke_passed"; artifact = "tests/kpi/results/ws6/ws6-dr-failover-smoke.json" },
  @{ name = "ws6_reconcile_latency_smoke_passed"; artifact = "tests/kpi/results/ws6/ws6-reconcile-latency-envelope-smoke.json" }
)

$results = @()
foreach ($check in $checks) {
  $status = Get-ArtifactStatus -PathValue $check.artifact
  $results += [ordered]@{
    name = $check.name
    passed = ($status -eq "passed")
    detail = "$status :: $($check.artifact)"
  }
}

$passedCount = @($results | Where-Object { $_.passed }).Count
$totalCount = $results.Count
$overallStatus = if ($passedCount -eq $totalCount) { "passed" } else { "failed" }

$artifact = [ordered]@{
  smoke = "h03-degraded-failover-evidence"
  status = $overallStatus
  checks_passed = $passedCount
  checks_total = $totalCount
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $results
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "H-03 degraded failover evidence smoke: $OutputPath ($overallStatus)"
if ($overallStatus -ne "passed") { exit 1 }
