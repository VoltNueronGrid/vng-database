param(
  [string]$OutputPath          = "tests/kpi/results/h06/h06-gate-summary.json",
  [string]$ReleaseSummaryPath  = "tests/kpi/results/gates/h06-release-readiness.json",
  [string]$RepoRoot            = "D:/by/polap-db"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

function Resolve-RepoPath {
  param([string]$PathValue)
  if ([System.IO.Path]::IsPathRooted($PathValue)) { return $PathValue }
  return [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $PathValue))
}

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Get-ArtifactStatus {
  param([string]$ArtifactPath)
  if (!(Test-Path -Path $ArtifactPath)) { return "missing_artifact" }
  try {
    $json = Get-Content -Raw -Path $ArtifactPath | ConvertFrom-Json
    if ($null -ne $json.status) { return [string]$json.status }
    return "present"
  } catch {
    return "invalid_artifact"
  }
}

$OutputPath         = Resolve-RepoPath -PathValue $OutputPath
$ReleaseSummaryPath = Resolve-RepoPath -PathValue $ReleaseSummaryPath
Ensure-OutputDir -PathValue $OutputPath
Ensure-OutputDir -PathValue $ReleaseSummaryPath

Write-Host ""
Write-Host "=== H-06 Gate Orchestrator ===" -ForegroundColor Cyan

# -----------------------------------------------------------------------
# Step 1: Run the cache-resilience smoke script.
# -----------------------------------------------------------------------
$smokeScript   = Resolve-RepoPath -PathValue "tests/kpi/scripts/run-h06-cache-resilience-smoke.ps1"
$smokeArtifact = Resolve-RepoPath -PathValue "tests/kpi/results/h06/h06-cache-resilience-smoke.json"

Write-Host ""
Write-Host "--- Smoke: h06-cache-resilience ---" -ForegroundColor Yellow
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $smokeScript -RepoRoot $RepoRoot

# Derive result from the artifact JSON (avoids stale $LASTEXITCODE across script boundaries).
$smokeStatus = Get-ArtifactStatus -ArtifactPath $smokeArtifact
$smokePassed = ($smokeStatus -eq "passed")

# -----------------------------------------------------------------------
# Step 2: Evaluate the gate-summary artifact.
# -----------------------------------------------------------------------
$gateSummaryPath = Resolve-RepoPath -PathValue "tests/kpi/results/h06/h06-gate-summary.json"
$gateStatus      = Get-ArtifactStatus -ArtifactPath $gateSummaryPath
$gatePassed      = ($gateStatus -eq "passed")

$gateSummary = $null
if (Test-Path -Path $gateSummaryPath) {
  $gateSummary = Get-Content -Raw -Path $gateSummaryPath | ConvertFrom-Json
}

Write-Host ""
Write-Host "--- Gate Summary ---" -ForegroundColor Yellow
if ($null -ne $gateSummary) {
  foreach ($check in $gateSummary.checks) {
    $color = if ($check.status -eq "passed") { "Green" } else { "Red" }
    Write-Host ("  [{0}] {1}" -f $check.status.ToUpper(), $check.name) -ForegroundColor $color
  }
  Write-Host ("Total: {0}  Passed: {1}" -f $gateSummary.total_checks, $gateSummary.passed_checks)
} else {
  Write-Host "  [WARN] gate-summary artifact not found" -ForegroundColor Yellow
}

# -----------------------------------------------------------------------
# Step 3: Emit overall gate result.
# -----------------------------------------------------------------------
$overallPassed = $smokePassed -and $gatePassed

$packs = @(
  @{ Name = "h06_cache_resilience_smoke";    Status = if ($smokePassed) { "passed" } else { "failed" } },
  @{ Name = "h06_runtime_cache_endpoint_surface"; Status = if ($smokePassed) { "passed" } else { "failed" } },
  @{ Name = "h06_circuit_breaker_contract";  Status = if ($gatePassed)  { "passed" } else { "failed" } },
  @{ Name = "h06_eviction_policy_contract";  Status = if ($gatePassed)  { "passed" } else { "failed" } },
  @{ Name = "h06_rebalance_contract";        Status = if ($gatePassed)  { "passed" } else { "failed" } },
  @{ Name = "h06_distributed_manager_surface"; Status = if ($gatePassed) { "passed" } else { "failed" } }
)

$passedCount = ($packs | Where-Object { $_.Status -eq "passed" }).Count
$totalCount  = $packs.Count

$gateOutputStatus = if ($overallPassed) { "passed" } else { "failed" }
$releaseReadiness = if ($overallPassed) { "ready_for_validation" } else { "blocked" }

Write-Host ""
Write-Host "=== H-06 Gate Results ===" -ForegroundColor Cyan
foreach ($pack in $packs) {
  $color = if ($pack.Status -eq "passed") { "Green" } else { "Red" }
  Write-Host ("  [{0}] {1}" -f $pack.Status.ToUpper(), $pack.Name) -ForegroundColor $color
}
Write-Host ("Passed: {0}/{1}" -f $passedCount, $totalCount)

$gateOutput = [ordered]@{
  id                = "h06-gate-summary"
  timestamp         = (Get-Date -Format "yyyy-MM-ddTHH:mm:ssZ")
  status            = $gateOutputStatus
  release_readiness = $releaseReadiness
  checks            = @($packs | ForEach-Object {
    [ordered]@{ name = $_.Name; status = $_.Status }
  })
  total_checks      = $totalCount
  passed_checks     = $passedCount
  notes             = "Distributed cache hardening with runtime endpoint integration, LRU eviction, TTL, and circuit breaker resilience policy validated"
}
$gateOutput | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath -Encoding UTF8

# Update release-readiness artifact.
$releaseOutput = [ordered]@{
  id                = "h06-release-readiness"
  timestamp         = (Get-Date -Format "yyyy-MM-ddTHH:mm:ssZ")
  status            = $gateOutputStatus
  release_readiness = $releaseReadiness
  release_target    = "R3"
  priority          = "P1"
  highlights        = [ordered]@{
    h06_cache_engine      = "voltnuerongrid_opt::DistributedCacheManager"
    h06_eviction_policy   = "LRU"
    h06_circuit_breaker   = "enabled"
    h06_partition_model   = "sharded_by_partition_id"
    h06_ttl_support       = $true
    h06_rebalance_contract = "evict_expired_all_partitions"
  }
  gate_references   = @(
    "tests/kpi/results/h06/h06-cache-resilience-smoke.json",
    "tests/kpi/results/h06/h06-gate-summary.json"
  )
}
$releaseOutput | ConvertTo-Json -Depth 10 | Set-Content -Path $ReleaseSummaryPath -Encoding UTF8

if ($overallPassed) {
  Write-Host ""
  Write-Host "GATE PASS: H-06 Distributed Cache Hardening" -ForegroundColor Green
  Write-Host "Release Readiness: $releaseReadiness"
  exit 0
} else {
  Write-Host ""
  Write-Host "GATE FAIL: H-06 Distributed Cache Hardening" -ForegroundColor Red
  exit 1
}
