param(
  [string]$SummaryPath = "tests/kpi/results/gates/ci-ws5-gate-summary.json",
  [string]$OutputPath = "tests/kpi/results/gates/ci-ws5-gate-badge.json"
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

if (!(Test-Path -Path $SummaryPath)) {
  throw "WS5 summary not found at $SummaryPath"
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$status = [string]$summary.status
$packs = @($summary.packs)
$runtimePack = @($packs | Where-Object { $_.pack -eq "ws5-tenant-audit-runtime" }) | Select-Object -First 1
$passedCount = @($packs | Where-Object { $_.status -eq "passed" }).Count
$totalCount = $packs.Count

$badge = [ordered]@{
  label = if ($null -ne $runtimePack) { "ws5-security-runtime" } else { "ws5-security-gate" }
  message = if ($totalCount -gt 0) {
    if ($null -ne $runtimePack) {
      "$passedCount/$totalCount $status + tenant-audit"
    } else {
      "$passedCount/$totalCount $status"
    }
  } else {
    $status
  }
  color = if ($status -eq "passed") { "green" } else { "red" }
  source_summary = $SummaryPath
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
}

$badge | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "WS5 gate badge artifact: $OutputPath ($status)"
