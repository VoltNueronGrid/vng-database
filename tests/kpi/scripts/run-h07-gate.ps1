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
$smokeScript = Join-Path $repoRoot "tests/kpi/scripts/run-h07-driver-storm-smoke.ps1"
$dataPlaneScript = Join-Path $repoRoot "tests/kpi/scripts/run-h07-data-plane-pool-orchestration-smoke.ps1"
$summaryPath = Join-Path $repoRoot "tests/kpi/results/h07/h07-gate-summary.json"
$releasePath = Join-Path $repoRoot "tests/kpi/results/gates/h07-release-readiness.json"
$dataPlaneArtifactPath = Join-Path $repoRoot "tests/kpi/results/h07/h07-data-plane-pool-orchestration-smoke.json"

& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $smokeScript
$smokePassed = ($LASTEXITCODE -eq 0)

& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $dataPlaneScript
$dataPlanePassed = ($LASTEXITCODE -eq 0)

if (Test-Path -Path $summaryPath) {
  $summary = Get-Content -Raw -Path $summaryPath | ConvertFrom-Json
  $summaryPassed = ($summary.status -eq "passed")
} else {
  $summaryPassed = $false
}

if (Test-Path -Path $releasePath) {
  $release = Get-Content -Raw -Path $releasePath | ConvertFrom-Json
  $releasePassed = ($release.status -eq "passed")
} else {
  $releasePassed = $false
}

if (Test-Path -Path $dataPlaneArtifactPath) {
  $dataPlaneArtifact = Get-Content -Raw -Path $dataPlaneArtifactPath | ConvertFrom-Json
  $dataPlaneArtifactPassed = ($dataPlaneArtifact.status -eq "passed")
  $dataPlanePassed = ($dataPlanePassed -and $dataPlaneArtifactPassed)
}

$total = 4
$passed = @($smokePassed, $dataPlanePassed, $summaryPassed, $releasePassed | Where-Object { $_ }).Count

Write-Host "=== H-07 Gate ===" -ForegroundColor Cyan
Write-Host (("[{0}] h07_driver_storm_smoke_and_runtime_hooks") -f ($(if ($smokePassed) { "PASSED" } else { "FAILED" })))
Write-Host (("[{0}] h07_data_plane_pool_orchestration") -f ($(if ($dataPlanePassed) { "PASSED" } else { "FAILED" })))
Write-Host (("[{0}] h07_gate_summary") -f ($(if ($summaryPassed) { "PASSED" } else { "FAILED" })))
Write-Host (("[{0}] h07_release_readiness") -f ($(if ($releasePassed) { "PASSED" } else { "FAILED" })))
Write-Host ("Passed: {0}/{1}" -f $passed, $total)

if ($smokePassed -and $dataPlanePassed -and $summaryPassed -and $releasePassed) {
  exit 0
}

exit 1
