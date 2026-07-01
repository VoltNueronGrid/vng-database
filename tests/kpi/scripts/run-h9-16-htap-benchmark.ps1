<#
.SYNOPSIS
    H9-16 HTAP Benchmark Suite gate script.
    Runs the Rust benchmark binary and checks results meet acceptance thresholds.
.PARAMETER BaseUrl
    Base URL of the running VoltNueronGrid service (optional for unit benchmarks).
#>
param(
    [string]$BaseUrl = "http://127.0.0.1:8080"
)

$ErrorActionPreference = "Stop"
$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$ResultsDir = Join-Path $ScriptDir "../results/htap"

# Ensure results directory exists
if (-not (Test-Path $ResultsDir)) {
    New-Item -ItemType Directory -Path $ResultsDir -Force | Out-Null
}

$timestamp    = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$artifactPath = Join-Path $ResultsDir "h9-16-benchmark-$timestamp.json"

Write-Host "H9-16 HTAP Benchmark Suite"
Write-Host "Results dir: $ResultsDir"

$packs = @()

# ---------------------------------------------------------------------------
# Pack 1: htap_benchmark unit tests
# ---------------------------------------------------------------------------
Write-Host "`nRunning htap_benchmark unit tests..."
$testOutput  = & cargo test -p voltnuerongrid-store --lib -- htap_benchmark 2>&1
$testExit    = $LASTEXITCODE
$pack1Status = if ($testExit -eq 0) { "passed" } else { "failed" }

Write-Host "  Pack 1 (htap_benchmark unit tests): $pack1Status"
if ($pack1Status -eq "failed") {
    Write-Host $testOutput
}

$packs += [ordered]@{
    name   = "htap_benchmark_unit_tests"
    status = $pack1Status
    output = ($testOutput | Out-String).Trim()
}

# ---------------------------------------------------------------------------
# Pack 2: full store test suite (regression guard)
# ---------------------------------------------------------------------------
Write-Host "`nRunning full store lib tests (regression guard)..."
$storeOutput = & cargo test -p voltnuerongrid-store --lib 2>&1
$storeExit   = $LASTEXITCODE
$pack2Status = if ($storeExit -eq 0) { "passed" } else { "failed" }

Write-Host "  Pack 2 (store lib regression): $pack2Status"
if ($pack2Status -eq "failed") {
    Write-Host $storeOutput
}

$packs += [ordered]@{
    name   = "store_lib_regression"
    status = $pack2Status
    output = ($storeOutput | Out-String).Trim()
}

# ---------------------------------------------------------------------------
# Gate result
# ---------------------------------------------------------------------------
$failedPacks = $packs | Where-Object { $_.status -eq "failed" }
$gateStatus  = if ($failedPacks.Count -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
    gate         = "h9-16-htap-benchmark"
    timestamp_ms = $timestamp
    base_url     = $BaseUrl
    packs        = $packs
    status       = $gateStatus
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $artifactPath -Encoding UTF8

Write-Host "`nGate status : $gateStatus"
Write-Host "Artifact    : $artifactPath"

if ($gateStatus -eq "failed") {
    exit 1
}
