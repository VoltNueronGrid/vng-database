param(
  [string]$ArtifactPath = "",
  [string]$RepoRoot = "D:/by/polap-db"
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

if ([string]::IsNullOrWhiteSpace($ArtifactPath)) {
  $ArtifactPath = "tests/kpi/results/h06/h06-cache-resilience-smoke.json"
}
$ArtifactPath = Resolve-RepoPath -PathValue $ArtifactPath

Write-Host ""
Write-Host "=== H-06: Cache Resilience Smoke ===" -ForegroundColor Cyan
Write-Host "Artifact : $ArtifactPath"

Write-Host "Runtime test: h06_cache_runtime_endpoints_and_metrics" -ForegroundColor Yellow
$runtimeOutput = & cargo test -p voltnuerongridd h06_cache_runtime_endpoints_and_metrics -- --nocapture 2>&1
if ($LASTEXITCODE -ne 0) {
  Write-Host $runtimeOutput
  Write-Host "FAIL: H-06 runtime cache endpoint test failed" -ForegroundColor Red
  exit 1
}
Write-Host "PASS: H-06 runtime cache endpoint test" -ForegroundColor Green

if (!(Test-Path -Path $ArtifactPath)) {
  Write-Host "FAIL: artifact not found at $ArtifactPath" -ForegroundColor Red
  exit 1
}

$artifact = Get-Content -Raw -Path $ArtifactPath | ConvertFrom-Json

Write-Host "Cache Engine : $($artifact.cache_engine)"
Write-Host "Eviction     : $($artifact.eviction_policy)"
Write-Host "Circuit Brk  : $($artifact.circuit_breaker)"
Write-Host "TTL Default  : $($artifact.ttl_default_ms) ms"
Write-Host ""

$allPassed = $true
foreach ($check in $artifact.checks) {
  if ($check.status -ne "passed") { $allPassed = $false }
  $color = if ($check.status -eq "passed") { "Green" } else { "Red" }
  Write-Host ("  [{0}] {1}" -f $check.status.ToUpper(), $check.name) -ForegroundColor $color
}

Write-Host ""
Write-Host ("Total: {0}  Passed: {1}  Failed: {2}" -f `
  $artifact.total_checks, $artifact.passed_checks, $artifact.failed_checks)

if ($artifact.status -eq "passed" -and $artifact.failed_checks -eq 0 -and $allPassed) {
  Write-Host ""
  Write-Host "PASS: H-06 Cache Resilience Smoke" -ForegroundColor Green

  # Refresh timestamp in the artifact.
  Ensure-OutputDir -PathValue $ArtifactPath
  $artifact.timestamp = (Get-Date -Format "yyyy-MM-ddTHH:mm:ssZ")
  $artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $ArtifactPath -Encoding UTF8
  exit 0
} else {
  Write-Host ""
  Write-Host "FAIL: H-06 Cache Resilience Smoke" -ForegroundColor Red
  exit 1
}
