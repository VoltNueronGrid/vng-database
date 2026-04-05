param(
  [string]$SummaryPath = "tests/kpi/results/ws2a/ws2a-gate-summary.json",
  [string]$SmokePath   = "tests/kpi/results/ws2a/row-sync-origin-smoke.json",
  [string]$OutputPath  = "tests/kpi/results/gates/ws2a-release-readiness.json"
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

foreach ($path in @($SummaryPath, $SmokePath)) {
  if (!(Test-Path -Path $path)) { throw "Required WS2A artifact missing at $path" }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$smoke   = Get-Content -Raw -Path $SmokePath   | ConvertFrom-Json

$checks = [ordered]@{
  ws2a_gate_passed            = ([string]$summary.status -eq "passed")
  ws2a_row_sync_origin_passed = ([string]$smoke.status   -eq "passed")
}

$failCount = @($checks.Values | Where-Object { $_ -eq $false }).Count
$status = if ($failCount -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate              = "ws2a-release-readiness"
  status            = $status
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  release_targets   = @("R1")
  scope             = @("WS2A", "REQ-02")
  generated_at_utc  = (Get-Date).ToUniversalTime().ToString("o")
  sources           = [ordered]@{
    summary         = $SummaryPath
    row_sync_origin = $SmokePath
  }
  checks            = $checks
  highlights        = [ordered]@{
    pack_count              = @($summary.packs).Count
    row_sync_origin_status  = [string]$smoke.status
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS2A release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
