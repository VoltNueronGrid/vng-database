param(
  [string]$OutputPath = "tests/kpi/results/ws4/ws4-closure-gate-summary.json"
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
$ws4SummaryPath      = "tests/kpi/results/ws4/ws4-gate-summary.json"
$ws4PluginPath       = "tests/kpi/results/ws4/ingest-plugin-smoke.json"
$ws4ParserPath       = "tests/kpi/results/ws4/ws4-ingest-parser-smoke.json"
$ws4ChunkedPath      = "tests/kpi/results/ws4/ws4-chunked-loader-smoke.json"

$runs = @()
$status = "passed"

$checks = [ordered]@{
  ws4_gate_passed            = $false
  ws4_ingest_plugin_passed   = $false
  ws4_ingest_parser_passed   = $false
  ws4_chunked_loader_passed  = $false
  ws4_all_packs_present      = $false
}

# Validate existing artifacts -- do not re-run live HTTP packs
$allArtifacts = @($ws4SummaryPath, $ws4PluginPath, $ws4ParserPath, $ws4ChunkedPath)
$allPresent = $true
foreach ($path in $allArtifacts) {
  if (!(Test-Path -Path $path)) {
    $status = "failed"
    $allPresent = $false
    $runs += [ordered]@{ pack = "ws4-artifact-presence"; status = "failed"; detail = "missing:$path"; artifact = $path }
  }
}
$checks["ws4_all_packs_present"] = $allPresent

if ($allPresent) {
  $summary = Get-Content -Raw -Path $ws4SummaryPath  | ConvertFrom-Json
  $plugin  = Get-Content -Raw -Path $ws4PluginPath   | ConvertFrom-Json
  $parser  = Get-Content -Raw -Path $ws4ParserPath   | ConvertFrom-Json
  $chunked = Get-Content -Raw -Path $ws4ChunkedPath  | ConvertFrom-Json

  $checks["ws4_gate_passed"]           = ([string]$summary.status -eq "passed")
  $checks["ws4_ingest_plugin_passed"]  = ([string]$plugin.status  -eq "passed")
  $checks["ws4_ingest_parser_passed"]  = ([string]$parser.status  -eq "passed")
  $checks["ws4_chunked_loader_passed"] = ([string]$chunked.status -eq "passed")

  if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) { $status = "failed" }
  $runs += [ordered]@{ pack = "ws4-artifact-validation"; status = $status; detail = "checked_existing_artifacts"; artifact = $ws4SummaryPath }
}

$finished = Get-Date
$summaryOut = [ordered]@{
  gate               = "ws4-closure-gate"
  status             = $status
  validation_posture = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  started_at_utc     = $start.ToUniversalTime().ToString("o")
  finished_at_utc    = $finished.ToUniversalTime().ToString("o")
  duration_ms        = [int](($finished - $start).TotalMilliseconds)
  artifacts          = [ordered]@{
    ws4_gate       = $ws4SummaryPath
    ingest_plugin  = $ws4PluginPath
    ingest_parser  = $ws4ParserPath
    chunked_loader = $ws4ChunkedPath
  }
  checks             = $checks
  runs               = $runs
}

$summaryOut | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS4 closure gate summary: $OutputPath ($status)"
if ($status -eq "passed") {
  $outDir   = Split-Path -Parent $OutputPath
  $ciMirror = Join-Path $outDir "ci-ws4-closure-gate-summary.json"
  if ($ciMirror -ne $OutputPath) {
    Copy-Item -LiteralPath $OutputPath -Destination $ciMirror -Force
    Write-Host "CI mirror: $ciMirror"
  }
}
if ($status -ne "passed") { exit 1 }
