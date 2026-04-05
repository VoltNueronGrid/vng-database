param(
  [string]$SummaryPath    = "tests/kpi/results/ws2/ws2-gate-summary.json",
  [string]$DurabilityPath = "tests/kpi/results/ws2/store-durability-smoke.json",
  [string]$WalPath        = "tests/kpi/results/ws2/disk-wal-adapter-smoke.json",
  [string]$CheckpointPath = "tests/kpi/results/ws2/ws2-checkpoint-restart-smoke.json",
  [string]$IndexPath      = "tests/kpi/results/ws2/ws2-index-constraint-smoke.json",
  [string]$TenantPath     = "tests/kpi/results/ws2/ws2-tenant-store-runtime-smoke.json",
  [string]$OutputPath     = "tests/kpi/results/gates/ws2-release-readiness.json"
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

foreach ($path in @($SummaryPath, $DurabilityPath, $WalPath, $CheckpointPath, $IndexPath, $TenantPath)) {
  if (!(Test-Path -Path $path)) { throw "Required WS2 artifact missing at $path" }
}

$summary    = Get-Content -Raw -Path $SummaryPath    | ConvertFrom-Json
$durability = Get-Content -Raw -Path $DurabilityPath | ConvertFrom-Json
$wal        = Get-Content -Raw -Path $WalPath        | ConvertFrom-Json
$checkpoint = Get-Content -Raw -Path $CheckpointPath | ConvertFrom-Json
$index      = Get-Content -Raw -Path $IndexPath      | ConvertFrom-Json
$tenant     = Get-Content -Raw -Path $TenantPath     | ConvertFrom-Json

$checks = [ordered]@{
  ws2_gate_passed               = ([string]$summary.status    -eq "passed")
  ws2_store_durability_passed   = ([string]$durability.status -eq "passed")
  ws2_wal_passed                = ([string]$wal.status        -eq "passed")
  ws2_checkpoint_restart_passed = ([string]$checkpoint.status -eq "passed")
  ws2_index_constraint_passed   = ([string]$index.status      -eq "passed")
  ws2_tenant_store_passed       = ([string]$tenant.status     -eq "passed")
}

$failCount = @($checks.Values | Where-Object { $_ -eq $false }).Count
$status = if ($failCount -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate              = "ws2-release-readiness"
  status            = $status
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  release_targets   = @("R1")
  scope             = @("WS2", "REQ-02")
  generated_at_utc  = (Get-Date).ToUniversalTime().ToString("o")
  sources           = [ordered]@{
    summary          = $SummaryPath
    store_durability = $DurabilityPath
    disk_wal         = $WalPath
    checkpoint       = $CheckpointPath
    index_constraint = $IndexPath
    tenant_store     = $TenantPath
  }
  checks            = $checks
  highlights        = [ordered]@{
    pack_count         = @($summary.packs).Count
    durability_status  = [string]$durability.status
    wal_status         = [string]$wal.status
    checkpoint_status  = [string]$checkpoint.status
    index_status       = [string]$index.status
    tenant_store_status = [string]$tenant.status
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS2 release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
