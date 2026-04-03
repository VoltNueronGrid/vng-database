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

$repoRoot     = Resolve-RepoRoot
$smokeScript  = Join-Path $repoRoot "tests/kpi/scripts/run-h08-plugin-supply-chain-smoke.ps1"
$gateArtifact = Join-Path $repoRoot "tests/kpi/results/h08/h08-gate-summary.json"

Write-Host ""
Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host " H-08 · Plugin Supply-Chain Gate" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

# Run smoke sub-script (includes runtime signed provenance endpoint path test)
& $smokeScript
if ($LASTEXITCODE -ne 0) {
    Write-Host "[GATE FAIL] Supply-chain smoke check failed" -ForegroundColor Red
    exit 1
}

# Read gate summary
if (!(Test-Path $gateArtifact)) {
    Write-Host "[GATE FAIL] Gate artifact missing: $gateArtifact" -ForegroundColor Red
    exit 1
}

$gate = Get-Content $gateArtifact -Raw | ConvertFrom-Json

Write-Host ""
Write-Host "Gate summary checks:"
foreach ($check in $gate.checks) {
    $color = if ($check.status -eq "passed") { "Green" } else { "Red" }
    Write-Host ("  [{0,-6}] {1}" -f $check.status.ToUpper(), $check.name) -ForegroundColor $color
}
Write-Host ""
Write-Host "Release readiness: $($gate.release_readiness)"
Write-Host ""

if ($gate.status -eq "passed") {
    Write-Host "[GATE PASS] H-08 Plugin Supply-Chain Gate" -ForegroundColor Green
    exit 0
} else {
    Write-Host "[GATE FAIL] H-08 status=$($gate.status)" -ForegroundColor Red
    exit 1
}
