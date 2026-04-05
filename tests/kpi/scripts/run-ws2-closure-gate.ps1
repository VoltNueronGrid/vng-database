param(
  [string]$OutputPath = "tests/kpi/results/ws2/ws2-closure-gate-summary.json"
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

$start = Get-Date
$ws2SummaryPath    = "tests/kpi/results/ws2/ws2-gate-summary.json"
$ws2DurabilityPath = "tests/kpi/results/ws2/store-durability-smoke.json"
$ws2WalPath        = "tests/kpi/results/ws2/disk-wal-adapter-smoke.json"
$ws2CheckpointPath = "tests/kpi/results/ws2/ws2-checkpoint-restart-smoke.json"
$ws2IndexPath      = "tests/kpi/results/ws2/ws2-index-constraint-smoke.json"
$ws2TenantPath     = "tests/kpi/results/ws2/ws2-tenant-store-runtime-smoke.json"

$runs = @()
$status = "passed"

$checks = [ordered]@{
  ws2_gate_passed               = $false
  ws2_store_durability_passed   = $false
  ws2_wal_passed                = $false
  ws2_checkpoint_restart_passed = $false
  ws2_index_constraint_passed   = $false
  ws2_tenant_store_passed       = $false
  ws2_all_packs_present         = $false
}

# Validate existing artifacts -- do not re-run live HTTP packs
$allArtifacts = @($ws2SummaryPath, $ws2DurabilityPath, $ws2WalPath, $ws2CheckpointPath, $ws2IndexPath, $ws2TenantPath)
$allPresent = $true
foreach ($path in $allArtifacts) {
  if (!(Test-Path -Path $path)) {
    $status = "failed"
    $allPresent = $false
    $runs += [ordered]@{ pack = "ws2-artifact-presence"; status = "failed"; detail = "missing:$path"; artifact = $path }
  }
}
$checks["ws2_all_packs_present"] = $allPresent

if ($allPresent) {
  $summary    = Get-Content -Raw -Path $ws2SummaryPath    | ConvertFrom-Json
  $durability = Get-Content -Raw -Path $ws2DurabilityPath | ConvertFrom-Json
  $wal        = Get-Content -Raw -Path $ws2WalPath         | ConvertFrom-Json
  $checkpoint = Get-Content -Raw -Path $ws2CheckpointPath  | ConvertFrom-Json
  $index      = Get-Content -Raw -Path $ws2IndexPath       | ConvertFrom-Json
  $tenant     = Get-Content -Raw -Path $ws2TenantPath      | ConvertFrom-Json

  $checks["ws2_gate_passed"]               = ([string]$summary.status    -eq "passed")
  $checks["ws2_store_durability_passed"]   = ([string]$durability.status -eq "passed")
  $checks["ws2_wal_passed"]                = ([string]$wal.status         -eq "passed")
  $checks["ws2_checkpoint_restart_passed"] = ([string]$checkpoint.status  -eq "passed")
  $checks["ws2_index_constraint_passed"]   = ([string]$index.status       -eq "passed")
  $checks["ws2_tenant_store_passed"]       = ([string]$tenant.status      -eq "passed")

  if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) { $status = "failed" }
  $runs += [ordered]@{ pack = "ws2-artifact-validation"; status = $status; detail = "checked_existing_artifacts"; artifact = $ws2SummaryPath }
}

$finished = Get-Date
$summaryOut = [ordered]@{
  gate               = "ws2-closure-gate"
  status             = $status
  validation_posture = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  started_at_utc     = $start.ToUniversalTime().ToString("o")
  finished_at_utc    = $finished.ToUniversalTime().ToString("o")
  duration_ms        = [int](($finished - $start).TotalMilliseconds)
  artifacts          = [ordered]@{
    ws2_gate         = $ws2SummaryPath
    store_durability = $ws2DurabilityPath
    disk_wal         = $ws2WalPath
    checkpoint       = $ws2CheckpointPath
    index            = $ws2IndexPath
    tenant_store     = $ws2TenantPath
  }
  checks             = $checks
  runs               = $runs
}

$summaryOut | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS2 closure gate summary: $OutputPath ($status)"
if ($status -eq "passed") {
  $outDir   = Split-Path -Parent $OutputPath
  $ciMirror = Join-Path $outDir "ci-ws2-closure-gate-summary.json"
  if ($ciMirror -ne $OutputPath) {
    Copy-Item -LiteralPath $OutputPath -Destination $ciMirror -Force
    Write-Host "CI mirror: $ciMirror"
  }
}
if ($status -ne "passed") { exit 1 }
