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
$artifactPath = Join-Path $repoRoot "tests/kpi/results/h07/h07-driver-storm-smoke.json"

Write-Host "Runtime test: h07_driver_pool_runtime_hooks" -ForegroundColor Yellow
& cargo test -p voltnuerongridd h07_driver_pool_runtime_hooks -- --nocapture
if ($LASTEXITCODE -ne 0) {
  Write-Host "[FAIL] H-07 runtime pool/storm hook test failed" -ForegroundColor Red
  exit 1
}
Write-Host "[PASS] H-07 runtime pool/storm hook test" -ForegroundColor Green

if (!(Test-Path -Path $artifactPath)) {
  Write-Host "[FAIL] Missing artifact: $artifactPath" -ForegroundColor Red
  exit 1
}

$artifact = Get-Content -Raw -Path $artifactPath | ConvertFrom-Json

if ($artifact.status -ne "passed") {
  Write-Host "[FAIL] Artifact status is '$($artifact.status)', expected 'passed'" -ForegroundColor Red
  exit 1
}

if ([int]$artifact.failed_checks -ne 0) {
  Write-Host "[FAIL] failed_checks=$($artifact.failed_checks), expected 0" -ForegroundColor Red
  exit 1
}

Write-Host "=== H-07 Driver Storm Smoke ===" -ForegroundColor Cyan
foreach ($check in $artifact.checks) {
  $color = if ($check.status -eq "passed") { "Green" } else { "Red" }
  Write-Host ("[{0}] {1}" -f $check.status.ToUpper(), $check.name) -ForegroundColor $color
}

Write-Host "[PASS] h07-driver-storm-smoke" -ForegroundColor Green
exit 0
