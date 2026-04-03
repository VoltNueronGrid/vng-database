$ErrorActionPreference = "Stop"

# ── Resolve repo root ────────────────────────────────────────────────────────
function Resolve-RepoRoot {
    $dir = $PSScriptRoot
    while ($dir -and !(Test-Path (Join-Path $dir "Cargo.toml"))) {
        $dir = Split-Path $dir -Parent
    }
    if (!$dir) { throw "Could not locate repo root (Cargo.toml not found)" }
    $dir
}

$repoRoot    = Resolve-RepoRoot
$artifactPath = Join-Path $repoRoot "tests/kpi/results/h08/h08-plugin-supply-chain-smoke.json"

Write-Host "Runtime test: h08_signed_provenance_enforcement_endpoint_path" -ForegroundColor Yellow
$runtimeOutput = & cargo test -p voltnuerongridd h08_signed_provenance_enforcement_endpoint_path -- --nocapture 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Host $runtimeOutput
    Write-Host "[FAIL] H-08 runtime signed provenance endpoint test failed" -ForegroundColor Red
    exit 1
}
Write-Host "[PASS] H-08 runtime signed provenance endpoint test" -ForegroundColor Green

Write-Host ""
Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host " H-08 · Plugin Supply-Chain Smoke" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

if (!(Test-Path $artifactPath)) {
    Write-Host "[FAIL] Artifact not found: $artifactPath" -ForegroundColor Red
    exit 1
}

$artifact = Get-Content $artifactPath -Raw | ConvertFrom-Json

$status      = $artifact.status
$failedChecks = $artifact.failed_checks

Write-Host "Supply-chain engine : $($artifact.supply_chain_engine)"
Write-Host "Provenance chain    : $($artifact.provenance_chain)"
Write-Host "SBOM inspection     : $($artifact.sbom_inspection)"
Write-Host "Audit trail         : $($artifact.audit_trail)"
Write-Host ""
Write-Host "Checks:"
foreach ($check in $artifact.checks) {
    $color = if ($check.status -eq "passed") { "Green" } else { "Red" }
    Write-Host ("  [{0,-6}] {1}" -f $check.status.ToUpper(), $check.name) -ForegroundColor $color
}
Write-Host ""
Write-Host "Total: $($artifact.total_checks)  Passed: $($artifact.passed_checks)  Failed: $failedChecks"
Write-Host ""

if ($status -eq "passed" -and $failedChecks -eq 0) {
    Write-Host "[PASS] h08-plugin-supply-chain-smoke" -ForegroundColor Green
    exit 0
} else {
    Write-Host "[FAIL] h08-plugin-supply-chain-smoke (status=$status, failed=$failedChecks)" -ForegroundColor Red
    exit 1
}
