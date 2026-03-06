param(
  [string]$SummaryPath = "tests/kpi/results/ws3/ws3-gate-summary.json",
  [string]$OutputPath = "tests/kpi/results/ws3/ws3-performance-score.json"
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
if (!(Test-Path -Path $SummaryPath)) { throw "WS3 summary missing at $SummaryPath" }

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$byPack = @{}
foreach ($pack in $summary.packs) { $byPack[[string]$pack.pack] = [string]$pack.status }

$controls = @(
  [ordered]@{ name = "query_routing_pack"; weight = 40; passed = ($byPack["ws3-query-routing"] -eq "passed") },
  [ordered]@{ name = "htap_target_contract_pack"; weight = 40; passed = ($byPack["ws3-htap-target-contract"] -eq "passed") },
  [ordered]@{ name = "gate_summary_status"; weight = 20; passed = ([string]$summary.status -eq "passed") }
)

$score = 0
foreach ($c in $controls) { if ($c.passed) { $score += [int]$c.weight } }
$status = if ($score -ge 100) { "passed" } else { "failed" }

$artifact = [ordered]@{
  report = "ws3-performance-score"
  status = $status
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  source_summary = $SummaryPath
  score = $score
  max_score = 100
  threshold = 100
  controls = $controls
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS3 performance score artifact: $OutputPath ($status, score=$score)"
if ($status -ne "passed") { exit 1 }
