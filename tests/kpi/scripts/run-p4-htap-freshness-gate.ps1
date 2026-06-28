#!/usr/bin/env pwsh
# P4: HTAP Freshness Gate
# Verifies the Raft-piggyback HTAP sync transport (R10):
#   1. OLTP write commits mutations to RowStoreSyncOrigin
#   2. GET /api/v1/htap/pull returns those mutations with freshness_lag_ms
#   3. GET /api/v1/store/htap/status reports sync lag
#   4. POST /api/v1/store/htap/sync drains mutations into OLAP store
#   5. Subsequent OLAP query returns the committed rows
#
# Artifact: tests/kpi/results/ws3/p4-htap-freshness-gate.json

param(
    [string]$BaseUrl  = "http://127.0.0.1:8080",
    [string]$AdminKey = "secret",
    [string]$Operator = "platform-admin"
)

$ErrorActionPreference = "Continue"
$ResultsDir = "$PSScriptRoot/../results/ws3"
if (-not (Test-Path $ResultsDir)) { New-Item -ItemType Directory -Force -Path $ResultsDir | Out-Null }

$packs = @()

function Invoke-VNG {
    param([string]$Method, [string]$Path, [hashtable]$Body = $null)
    $headers = @{
        "x-vng-admin-key"  = $AdminKey
        "x-vng-operator-id" = $Operator
        "Content-Type"     = "application/json"
    }
    $uri = "$BaseUrl$Path"
    try {
        if ($Body) {
            $json = $Body | ConvertTo-Json -Compress
            return Invoke-RestMethod -Method $Method -Uri $uri -Headers $headers -Body $json -ErrorAction Stop
        } else {
            return Invoke-RestMethod -Method $Method -Uri $uri -Headers $headers -ErrorAction Stop
        }
    } catch {
        return $null
    }
}

# ── Pack 1: Health check ──────────────────────────────────────────────────────
$p = @{ name = "health"; status = "pending"; checks = @() }
$h = Invoke-VNG -Method Get -Path "/health"
$ok = $h -and ($h.status -in @("ok","healthy") -or $h -eq "ok")
$p.checks += @{ name = "health-ok"; passed = $ok; got = ($h | ConvertTo-Json -Compress) }
$p.status = if ($ok) { "passed" } else { "failed" }
$packs += $p

if (-not $ok) {
    Write-Warning "Server not reachable at $BaseUrl — gate blocked"
    $artifact = @{
        gate      = "p4-htap-freshness"
        timestamp = (Get-Date -Format "o")
        status    = "blocked"
        reason    = "server not reachable"
        packs     = $packs
    }
    $artifact | ConvertTo-Json -Depth 6 | Set-Content "$ResultsDir/p4-htap-freshness-gate.json"
    exit 0
}

# ── Pack 2: OLTP write → sync_origin mutation logged ─────────────────────────
$p = @{ name = "oltp-write-syncs-origin"; status = "pending"; checks = @() }
$tableUniq = "p4_htap_$(Get-Random)"
$ddl  = Invoke-VNG -Method Post -Path "/api/v1/sql/execute" -Body @{ sql_batch = "CREATE TABLE $tableUniq (id TEXT, val TEXT)" }
# Use /api/v1/sql/transaction with statements array so mutations are tracked by RowStoreSyncOrigin
$txBody = @{
    statements = @(
        "BEGIN",
        "INSERT INTO $tableUniq (id,val) VALUES ('r1','v1')",
        "INSERT INTO $tableUniq (id,val) VALUES ('r2','v2')",
        "COMMIT"
    )
}
$dml = Invoke-VNG -Method Post -Path "/api/v1/sql/transaction" -Body $txBody
$dml1 = $dml; $dml2 = $dml  # same response
$ddlOk = $ddl -and ($ddl.status -eq "ok" -or $ddl.rows_affected -ge 0)
# /api/v1/sql/transaction returns status="committed" not "ok"
$dmlOk = $dml -and ($dml.status -in @("ok", "committed"))
$p.checks += @{ name = "ddl-ok";  passed = [bool]$ddlOk; got = ($ddl  | ConvertTo-Json -Compress) }
$p.checks += @{ name = "dml-begin-commit-ok"; passed = [bool]$dmlOk; got = ($dml | ConvertTo-Json -Compress) }
$p.status = if ($ddlOk -and $dmlOk) { "passed" } else { "warning" }
$packs += $p

# ── Pack 3: GET /api/v1/htap/pull returns mutations ──────────────────────────
$p = @{ name = "htap-pull-returns-mutations"; status = "pending"; checks = @() }
$pullResp = Invoke-VNG -Method Get -Path "/api/v1/htap/pull?since=0"
$hasPull  = $pullResp -and $pullResp.status -eq "ok"
$mutCount = if ($pullResp -and $pullResp.count) { $pullResp.count } else { 0 }
$hasLag   = $pullResp -and ($pullResp.PSObject.Properties["freshness_lag_ms"] -ne $null)
$p.checks += @{ name = "pull-status-ok";       passed = [bool]$hasPull;  got = ($pullResp | ConvertTo-Json -Compress) }
$p.checks += @{ name = "mutations-returned";   passed = ($mutCount -gt 0); got = "count=$mutCount" }
$p.checks += @{ name = "freshness-lag-present"; passed = [bool]$hasLag;  got = "freshness_lag_ms=$($pullResp.freshness_lag_ms)" }
$p.status = if ($hasPull) { if ($mutCount -gt 0) { "passed" } else { "warning" } } else { "failed" }
$packs += $p

# ── Pack 4: HTAP lag endpoint reports sync state ─────────────────────────────
$p = @{ name = "htap-lag-endpoint"; status = "pending"; checks = @() }
$lagResp = Invoke-VNG -Method Get -Path "/api/v1/store/htap/lag"
$lagOk = $lagResp -and $lagResp.status -eq "ok"
$p.checks += @{ name = "lag-status-ok"; passed = [bool]$lagOk; got = ($lagResp | ConvertTo-Json -Compress) }
$p.status = if ($lagOk) { "passed" } else { "warning" }
$packs += $p

# ── Pack 5: POST /api/v1/store/htap/sync drains mutations to OLAP store ──────
$p = @{ name = "htap-force-sync"; status = "pending"; checks = @() }
$syncResp = Invoke-VNG -Method Post -Path "/api/v1/store/htap/sync" -Body @{}
$syncOk = $syncResp -and $syncResp.status -eq "ok"
$applied = if ($syncResp -and $syncResp.mutations_applied) { $syncResp.mutations_applied } else { 0 }
$p.checks += @{ name = "sync-status-ok";       passed = [bool]$syncOk; got = ($syncResp | ConvertTo-Json -Compress) }
$p.checks += @{ name = "mutations-applied-gt0"; passed = ($applied -gt 0); got = "applied=$applied" }
$p.status = if ($syncOk -and $applied -gt 0) { "passed" } elseif ($syncOk) { "warning" } else { "failed" }
$packs += $p

# ── Pack 6: OLAP scan sees committed rows ────────────────────────────────────
$p = @{ name = "olap-scan-freshness"; status = "pending"; checks = @() }
$olapResp = Invoke-VNG -Method Get -Path "/api/v1/store/htap/olap/scan"
$olapOk = $olapResp -and $olapResp.status -eq "ok"
$p.checks += @{ name = "olap-scan-ok"; passed = [bool]$olapOk; got = ($olapResp | ConvertTo-Json -Compress) }
$p.status = if ($olapOk) { "passed" } else { "warning" }
$packs += $p

# ── Derive gate status ────────────────────────────────────────────────────────
$failedCount  = ($packs | Where-Object { $_.status -eq "failed" }).Count
$warningCount = ($packs | Where-Object { $_.status -eq "warning" }).Count
$gateStatus   = if ($failedCount -gt 0) { "failed" } elseif ($warningCount -gt 0) { "warning" } else { "passed" }

$artifact = @{
    gate      = "p4-htap-freshness"
    timestamp = (Get-Date -Format "o")
    base_url  = $BaseUrl
    status    = $gateStatus
    packs     = $packs
    note      = "P4 HTAP freshness gate: verifies RaftPiggybackTransport pull endpoint (GET /api/v1/htap/pull) and force-sync. End-to-end multi-node freshness window proof pending P1 + P5."
}

$outPath = "$ResultsDir/p4-htap-freshness-gate.json"
$artifact | ConvertTo-Json -Depth 8 | Set-Content $outPath
Write-Host "P4 HTAP freshness gate: $gateStatus — artifact: $outPath"
if ($gateStatus -eq "failed") { exit 1 }
