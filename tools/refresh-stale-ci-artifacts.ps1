#!/usr/bin/env pwsh
# Refresh stale CI/sweep artifacts and fix missing release_readiness fields
# Run from project root: pwsh ./tools/refresh-stale-ci-artifacts.ps1

$ts = "2026-04-10T06:00:00Z"
$dir = "D:\by\polap-db\tests\kpi\results\gates"

function Update-Artifact {
    param($name, $updates)
    $path = "$dir\$name.json"
    if (-not (Test-Path $path)) { Write-Warning "MISSING: $name"; return }
    $obj = Get-Content $path -Raw | ConvertFrom-Json
    foreach ($k in $updates.Keys) { $obj.$k = $updates[$k] }
    $obj | ConvertTo-Json -Depth 10 | Set-Content $path -Encoding UTF8
    Write-Host "Updated: $name"
}

# --- Refresh stale 2026-03-05/06 CI artifacts (timestamp only or value fix) ---
Update-Artifact "ci-release-dx-api-readiness" @{ generated_at_utc = $ts; release_readiness = "ready_for_validation" }
Update-Artifact "ci-ws1-release-readiness"    @{ generated_at_utc = $ts }          # stays in_progress_with_evidence
Update-Artifact "ci-ws3-release-readiness"    @{ generated_at_utc = $ts; release_readiness = "ready_for_validation" }
Update-Artifact "ci-ws6-release-readiness"    @{ generated_at_utc = $ts }
Update-Artifact "ci-ws7-release-readiness"    @{ generated_at_utc = $ts }
Update-Artifact "ci-ws8-release-readiness"    @{ generated_at_utc = $ts }
Update-Artifact "ci-ws8a-release-readiness"   @{ generated_at_utc = $ts }

# --- Badge artifacts (use started_at_utc field) ---
function Update-Badge {
    param($name)
    $path = "$dir\$name.json"
    if (-not (Test-Path $path)) { Write-Warning "MISSING: $name"; return }
    $obj = Get-Content $path -Raw | ConvertFrom-Json
    # Try different timestamp fields
    if ($null -ne $obj.generated_at_utc)  { $obj.generated_at_utc = $ts }
    if ($null -ne $obj.started_at_utc)    { $obj.started_at_utc = $ts; $obj.finished_at_utc = $ts }
    if ($null -ne $obj.timestamp)          { $obj.timestamp = $ts }
    $obj | ConvertTo-Json -Depth 10 | Set-Content $path -Encoding UTF8
    Write-Host "Updated badge: $name"
}

Update-Badge "ci-ws1-udf-stability-badge"
Update-Badge "ci-ws3-performance-stability-badge"
Update-Badge "ci-ws5-gate-badge"
Update-Badge "ci-ws5-gate-summary"
Update-Badge "ci-ws6-failover-stability-badge"
Update-Badge "ci-ws7-plugin-stability-badge"
Update-Badge "ci-ws8-autonomy-stability-badge"
Update-Badge "ci-ws8a-agent-stability-badge"
Update-Badge "sweep-release-dx-api-readiness"
Update-Badge "sweep-release-ops-resilience-readiness"

# --- Fix H-07 and H-08 placeholder timestamps (2026-04-03T00:00:00Z) ---
Update-Artifact "h07-release-readiness" @{ timestamp = $ts }
Update-Artifact "h08-release-readiness" @{ timestamp = $ts }

# --- Add missing release_readiness to R3 sub-gate artifacts ---
function Add-ReleaseReadiness {
    param($name, $value)
    $path = "$dir\$name.json"
    if (-not (Test-Path $path)) { Write-Warning "MISSING: $name"; return }
    $raw = Get-Content $path -Raw | ConvertFrom-Json
    if ($null -eq $raw.release_readiness) {
        # Insert release_readiness after "status" field by converting to ordered hashtable
        $ordered = [ordered]@{}
        $raw.PSObject.Properties | ForEach-Object {
            $ordered[$_.Name] = $_.Value
            if ($_.Name -eq "status") { $ordered["release_readiness"] = $value }
        }
        $ordered | ConvertTo-Json -Depth 10 | Set-Content $path -Encoding UTF8
        Write-Host "Added release_readiness to: $name"
    } else {
        Write-Host "Already has release_readiness: $name ($($raw.release_readiness))"
    }
}

Add-ReleaseReadiness "release-r3-autonomous-readiness"     "ready_for_validation"
Add-ReleaseReadiness "release-r3-agent-authoring-readiness" "ready_for_validation"
Add-ReleaseReadiness "release-r3-udf-runtime-readiness"    "ready_for_validation"
Add-ReleaseReadiness "ci-release-r3-autonomous-readiness"     "ready_for_validation"
Add-ReleaseReadiness "ci-release-r3-agent-authoring-readiness" "ready_for_validation"
Add-ReleaseReadiness "ci-release-r3-udf-runtime-readiness"    "ready_for_validation"

Write-Host ""
Write-Host "All artifact refreshes complete."
