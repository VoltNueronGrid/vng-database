param(
  [string]$SummaryPath    = "tests/kpi/results/ws4/ws4-gate-summary.json",
  [string]$PluginPath     = "tests/kpi/results/ws4/ingest-plugin-smoke.json",
  [string]$ParserPath     = "tests/kpi/results/ws4/ws4-ingest-parser-smoke.json",
  [string]$ChunkedPath    = "tests/kpi/results/ws4/ws4-chunked-loader-smoke.json",
  [string]$OutputPath     = "tests/kpi/results/gates/ws4-release-readiness.json"
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

foreach ($path in @($SummaryPath, $PluginPath, $ParserPath, $ChunkedPath)) {
  if (!(Test-Path -Path $path)) { throw "Required WS4 artifact missing at $path" }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$plugin  = Get-Content -Raw -Path $PluginPath  | ConvertFrom-Json
$parser  = Get-Content -Raw -Path $ParserPath  | ConvertFrom-Json
$chunked = Get-Content -Raw -Path $ChunkedPath | ConvertFrom-Json

$checks = [ordered]@{
  ws4_gate_passed           = ([string]$summary.status -eq "passed")
  ws4_ingest_plugin_passed  = ([string]$plugin.status  -eq "passed")
  ws4_ingest_parser_passed  = ([string]$parser.status  -eq "passed")
  ws4_chunked_loader_passed = ([string]$chunked.status -eq "passed")
}

$failCount = @($checks.Values | Where-Object { $_ -eq $false }).Count
$status = if ($failCount -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate              = "ws4-release-readiness"
  status            = $status
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  release_targets   = @("R1")
  scope             = @("WS4", "REQ-06")
  generated_at_utc  = (Get-Date).ToUniversalTime().ToString("o")
  sources           = [ordered]@{
    summary        = $SummaryPath
    ingest_plugin  = $PluginPath
    ingest_parser  = $ParserPath
    chunked_loader = $ChunkedPath
  }
  checks            = $checks
  highlights        = [ordered]@{
    pack_count            = @($summary.packs).Count
    ingest_plugin_status  = [string]$plugin.status
    ingest_parser_status  = [string]$parser.status
    chunked_loader_status = [string]$chunked.status
  }
}

$artifact | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS4 release summary artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
