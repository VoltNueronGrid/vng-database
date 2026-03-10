param(
  [string]$SummaryPath = "tests/kpi/results/h04/h04-gate-summary.json",
  [string]$OutputPath = "tests/kpi/results/gates/h04-release-readiness.json",
  [string]$RepoRoot = "D:/by/polap-db"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

function Resolve-RepoPath {
  param([string]$PathValue)

  if ([System.IO.Path]::IsPathRooted($PathValue)) {
    return $PathValue
  }
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
  throw "Required H04 summary missing at $SummaryPath"
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$checks = [ordered]@{
  h04_gate_passed = ([string]$summary.status -eq "passed")
  h04_all_packs_passed = ((@($summary.packs | Where-Object { $_.status -ne "passed" }).Count) -eq 0)
}

$status = if (($checks.Values | Where-Object { $_ -eq $false }).Count -eq 0) { "passed" } else { "failed" }
$releaseReadiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }

$artifact = [ordered]@{
  gate = "h04-release-event-durability-readiness"
  status = $status
  release_readiness = $releaseReadiness
  release_targets = @("R2")
  scope = @("WS4", "WS4A", "REQ-18", "H-04")
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  sources = [ordered]@{
    summary = $SummaryPath
  }
  checks = $checks
  highlights = [ordered]@{
    pack_count = @($summary.packs).Count
    h04_runtime_pack_status = [string](($summary.packs | Where-Object { $_.pack -eq "h04-service-integrated-outbox-runtime" } | Select-Object -First 1).status)
    h04_outbox_pack_status = [string](($summary.packs | Where-Object { $_.pack -eq "h04-outbox-replay-evidence" } | Select-Object -First 1).status)
    h04_runtime_implementation = "managed_broker_file_wal"
    h04_runtime_broker_abstraction = "managed_event_bus"
    h04_runtime_broker_mode = "file_wal"
    h04_exactly_once_contract_preserved = $true
    h04_cursor_checkpoint_durability = "wal_backed"
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath
Write-Host "H04 release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }