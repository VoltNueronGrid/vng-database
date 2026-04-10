param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$OutputPath = "tests/kpi/results/gates/release-r4-saas-maturity-readiness.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Read-JsonArtifact {
  param([string]$PathValue)
  if (!(Test-Path -Path $PathValue)) {
    throw "Required artifact missing at $PathValue"
  }
  return Get-Content -Raw -Path $PathValue | ConvertFrom-Json
}

Ensure-OutputDir -PathValue $OutputPath

$start = Get-Date
$runs = @()
$status = "passed"

$packs = @(
  @{
    Name = "release-ops-resilience-gate"
    Script = "tests/kpi/scripts/run-release-ops-resilience-gate.ps1"
    Params = @{ OutputPath = "tests/kpi/results/gates/release-ops-resilience-readiness.json" }
    Artifact = "tests/kpi/results/gates/release-ops-resilience-readiness.json"
  },
  @{
    Name = "req08-cloud-saas-smoke"
    Script = "tests/kpi/scripts/run-req08-cloud-saas-smoke.ps1"
    Params = @{ OutputPath = "tests/kpi/results/req08/cloud-saas-smoke.json" }
    Artifact = "tests/kpi/results/req08/cloud-saas-smoke.json"
  },
  @{
    Name = "req10-benchmark-smoke"
    Script = "tests/kpi/scripts/run-req10-benchmark-smoke.ps1"
    Params = @{ BaseUrl = $BaseUrl; OutputPath = "tests/kpi/results/req10/benchmark-smoke.json" }
    Artifact = "tests/kpi/results/req10/benchmark-smoke.json"
  },
  @{
    Name = "h09-gate"
    Script = "tests/kpi/scripts/run-h09-gate.ps1"
    Params = @{ OutputPath = "tests/kpi/results/h09/h09-gate-summary.json"; ReleaseSummaryOutputPath = "tests/kpi/results/gates/h09-release-readiness.json" }
    Artifact = "tests/kpi/results/gates/h09-release-readiness.json"
  },
  @{
    Name = "h10-gate"
    Script = "tests/kpi/scripts/run-h10-gate.ps1"
    Params = @{ OutputPath = "tests/kpi/results/h10/h10-gate-summary.json"; ReleaseSummaryOutputPath = "tests/kpi/results/gates/h10-release-readiness.json" }
    Artifact = "tests/kpi/results/gates/h10-release-readiness.json"
  }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  try {
    $global:LASTEXITCODE = 0
    $packParams = $pack.Params
    & $pack.Script @packParams 2>&1 | Out-Null
    if (-not $?) {
      $packStatus = "failed"
      $detail = "script_invocation_failed"
    } elseif ($global:LASTEXITCODE -ne 0) {
      $packStatus = "failed"
      $detail = "exit_code=$global:LASTEXITCODE"
    }
  } catch {
    $packStatus = "failed"
    $detail = $_.Exception.Message
  }

  if ($packStatus -ne "passed") {
    $status = "failed"
  }

  $runs += [ordered]@{
    pack = $pack.Name
    status = $packStatus
    detail = $detail
    artifact = $pack.Artifact
  }
}

$opsRelease = Read-JsonArtifact -PathValue "tests/kpi/results/gates/release-ops-resilience-readiness.json"
$req08 = Read-JsonArtifact -PathValue "tests/kpi/results/req08/cloud-saas-smoke.json"
$req10 = Read-JsonArtifact -PathValue "tests/kpi/results/req10/benchmark-smoke.json"
$h09 = Read-JsonArtifact -PathValue "tests/kpi/results/gates/h09-release-readiness.json"
$h10 = Read-JsonArtifact -PathValue "tests/kpi/results/gates/h10-release-readiness.json"

$checks = [ordered]@{
  ops_resilience_ready = ([string]$opsRelease.release_readiness -eq "ready_for_validation")
  req08_cloud_saas_passed = ([string]$req08.status -eq "passed")
  req10_benchmark_passed = ([string]$req10.status -eq "passed")
  h09_gate_passed = ([string]$h09.status -eq "passed")
  h10_gate_passed = ([string]$h10.status -eq "passed")
  h09_release_ready = ([string]$h09.release_readiness -eq "ready_for_validation")
  h10_release_ready = ([string]$h10.release_readiness -eq "ready_for_validation")
}

$releaseReadiness = if ($status -eq "passed" -and (@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) {
  "ready_for_validation"
} else {
  "blocked"
}

$finished = Get-Date
$artifact = [ordered]@{
  gate = "release-r4-saas-maturity-readiness"
  status = $status
  release_target = "R4"
  release_readiness = $releaseReadiness
  scope = @("H-09", "H-10", "REQ-08", "REQ-10", "REQ-24", "REQ-28", "WS12", "WS13", "WS14")
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
  checks = $checks
  highlights = [ordered]@{
    h09_release_readiness = [string]$h09.release_readiness
    h10_release_readiness = [string]$h10.release_readiness
    req08_total_checks = [int]$req08.total_checks
    req10_checks_passed = [int]$req10.checks_passed
    req10_checks_total = [int]$req10.checks_total
  }
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "Release R4 SaaS maturity gate summary: $OutputPath ($status / $releaseReadiness)"
if ($status -eq "passed") {
  $outDir = Split-Path -Parent $OutputPath
  $ciMirror = Join-Path $outDir "ci-release-r4-saas-maturity-readiness.json"
  if ($ciMirror -ne $OutputPath) {
    Copy-Item -LiteralPath $OutputPath -Destination $ciMirror -Force
    Write-Host "CI mirror: $ciMirror"
  }
}
if ($status -ne "passed") { exit 1 }