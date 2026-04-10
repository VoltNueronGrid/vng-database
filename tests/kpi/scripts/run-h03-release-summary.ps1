param(
  [string]$SummaryPath = "tests/kpi/results/h03/h03-gate-summary.json",
  [string]$OutputPath = "tests/kpi/results/gates/h03-release-readiness.json",
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

$SummaryPath = Resolve-RepoPath -PathValue $SummaryPath
$OutputPath = Resolve-RepoPath -PathValue $OutputPath
Ensure-OutputDir -PathValue $OutputPath

if (!(Test-Path -Path $SummaryPath)) {
  throw "Required H03 summary missing at $SummaryPath"
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$checks = [ordered]@{
  h03_gate_passed = ([string]$summary.status -eq "passed")
  h03_all_packs_passed = ((@($summary.packs | Where-Object { $_.status -ne "passed" }).Count) -eq 0)
  h03_cluster_runtime_cert_complete = $false
}

$status = if (($checks.h03_gate_passed -and $checks.h03_all_packs_passed)) { "passed" } else { "failed" }
$releaseReadiness = if ($status -eq "passed") { "in_progress_with_evidence" } else { "blocked" }

$artifact = [ordered]@{
  gate = "h03-release-control-plane-resilience-readiness"
  status = $status
  release_readiness = $releaseReadiness
  release_targets = @("R2")
  scope = @("WS6", "REQ-17", "H-03")
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  sources = [ordered]@{
    summary = "tests/kpi/results/h03/h03-gate-summary.json"
  }
  checks = $checks
  highlights = [ordered]@{
    pack_count = @($summary.packs).Count
    control_plane_chaos_pack_status = [string](($summary.packs | Where-Object { $_.pack -eq "h03-control-plane-chaos-evidence" } | Select-Object -First 1).status)
    runtime_linkage_pack_status = [string](($summary.packs | Where-Object { $_.pack -eq "h03-cluster-runtime-linkage" } | Select-Object -First 1).status)
    degraded_failover_pack_status = [string](($summary.packs | Where-Object { $_.pack -eq "h03-degraded-failover-evidence" } | Select-Object -First 1).status)
    release_blocker = "full_inter_process_transport_backed_cluster_runtime_certification_pending"
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "H03 release summary artifact: $OutputPath ($status, $releaseReadiness)"
if ($status -ne "passed") { exit 1 }
