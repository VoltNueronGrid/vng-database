#!/usr/bin/env pwsh
# Refresh WS1A, WS2, WS2A, WS4, WS4A release-readiness artifacts
param(
    [string]$ScriptDir = "D:\by\polap-db\tests\kpi\scripts",
    [string]$GatesDir = "D:\by\polap-db\tests\kpi\results\gates"
)

$scripts = @(
    @{ script = "run-ws1a-release-summary.ps1"; out = "$GatesDir\ws1a-release-readiness.json" },
    @{ script = "run-ws2-release-summary.ps1";  out = "$GatesDir\ws2-release-readiness.json" },
    @{ script = "run-ws2a-release-summary.ps1"; out = "$GatesDir\ws2a-release-readiness.json" },
    @{ script = "run-ws4-release-summary.ps1";  out = "$GatesDir\ws4-release-readiness.json" },
    @{ script = "run-ws4a-release-summary.ps1"; out = "$GatesDir\ws4a-release-readiness.json" }
)

foreach ($entry in $scripts) {
    $scriptPath = "$ScriptDir\$($entry.script)"
    if (-not (Test-Path $scriptPath)) {
        Write-Warning "Script not found: $scriptPath"
        continue
    }
    Write-Host "Running: $($entry.script) ..."
    & $scriptPath -OutputPath $entry.out 2>&1 | Out-Null
    if (Test-Path $entry.out) {
        $result = Get-Content $entry.out | ConvertFrom-Json
        Write-Host "  -> status=$($result.status) readiness=$($result.release_readiness)"
    } else {
        # Script may use different param name; try without OutputPath
        & $scriptPath 2>&1 | Out-Null
        if (Test-Path $entry.out) {
            $result = Get-Content $entry.out | ConvertFrom-Json
            Write-Host "  -> status=$($result.status) readiness=$($result.release_readiness)"
        } else {
            Write-Warning "  -> artifact not found at $($entry.out)"
        }
    }
}

Write-Host ""
Write-Host "WS release summary refresh complete."
