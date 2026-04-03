param(
  [string]$OutputPath = "tests/kpi/results/h07/h07-data-plane-pool-orchestration-smoke.json"
)

$ErrorActionPreference = "Stop"

function Resolve-RepoRoot {
  param([string]$StartPath)

  $current = [System.IO.Path]::GetFullPath($StartPath)
  while ($true) {
    if (Test-Path (Join-Path $current "Cargo.toml")) {
      return $current
    }

    $parent = Split-Path -Parent $current
    if ([string]::IsNullOrWhiteSpace($parent) -or $parent -eq $current) {
      throw "Unable to locate repository root from $StartPath"
    }
    $current = $parent
  }
}

$repoRoot = Resolve-RepoRoot -StartPath $PSScriptRoot
Set-Location $repoRoot

if (![System.IO.Path]::IsPathRooted($OutputPath)) {
  $OutputPath = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $OutputPath))
}
$parent = Split-Path -Parent $OutputPath
if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
  New-Item -Path $parent -ItemType Directory -Force | Out-Null
}

Write-Host "Runtime test: h07_sql_data_plane_pool_acquire_release_on_sql_handlers" -ForegroundColor Yellow
& cargo test -p voltnuerongridd h07_sql_data_plane_pool_acquire_release_on_sql_handlers -- --nocapture
$testOnePassed = ($LASTEXITCODE -eq 0)

Write-Host "Runtime test: h07_sql_data_plane_pool_rejects_when_pool_exhausted" -ForegroundColor Yellow
& cargo test -p voltnuerongridd h07_sql_data_plane_pool_rejects_when_pool_exhausted -- --nocapture
$testTwoPassed = ($LASTEXITCODE -eq 0)

$checks = @(
  [ordered]@{ name = "h07_sql_data_plane_pool_acquire_release"; status = if ($testOnePassed) { "passed" } else { "failed" } },
  [ordered]@{ name = "h07_sql_data_plane_pool_exhaustion_rejection"; status = if ($testTwoPassed) { "passed" } else { "failed" } }
)
$passedChecks = ($checks | Where-Object { $_.status -eq "passed" }).Count
$totalChecks = $checks.Count
$status = if ($passedChecks -eq $totalChecks) { "passed" } else { "failed" }

$artifact = [ordered]@{
  id = "h07-data-plane-pool-orchestration-smoke"
  timestamp = (Get-Date -Format "yyyy-MM-ddTHH:mm:ssZ")
  status = $status
  checks = $checks
  total_checks = $totalChecks
  passed_checks = $passedChecks
  failed_checks = ($totalChecks - $passedChecks)
  coverage = "sql_route_sql_transaction_sql_execute_data_plane_pool_orchestration"
}
$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath -Encoding UTF8

if ($status -eq "passed") {
  Write-Host "[PASS] h07-data-plane-pool-orchestration-smoke" -ForegroundColor Green
  exit 0
}

Write-Host "[FAIL] h07-data-plane-pool-orchestration-smoke" -ForegroundColor Red
exit 1
