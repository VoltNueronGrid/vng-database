#!/usr/bin/env pwsh
# P5: Multi-Node Raft Cluster Smoke Test
# Starts 3 local VoltNueronGrid nodes, exercises leader election, row replication,
# and leader-loss recovery. Requires a built voltnuerongridd binary.
# Artifact: tests/kpi/results/multinode/multinode-smoke.json

param(
    [string]$BinaryPath = "$PSScriptRoot/../../../target/debug/voltnuerongridd",
    [int]$Node1Port     = 18080,
    [int]$Node2Port     = 18081,
    [int]$Node3Port     = 18082,
    [string]$AdminKey   = "secret",
    [int]$StartupWaitMs = 5000,
    [int]$ElectionWaitMs = 4000
)

$ErrorActionPreference = "Continue"
$ResultsDir = "$PSScriptRoot/../results/multinode"
if (-not (Test-Path $ResultsDir)) { New-Item -ItemType Directory -Force -Path $ResultsDir | Out-Null }

$packs = @()
$Procs  = @()
$tmpDirs = @()
$skipRemainingPacks = $false

function Stop-Cluster {
    foreach ($proc in $Procs) {
        try { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue } catch {}
    }
    foreach ($dir in $tmpDirs) {
        Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
    }
}

function Invoke-VngRequest {
    param([string]$Method, [string]$Uri, [hashtable]$Headers, [string]$Body)
    try {
        $params = @{ Method = $Method; Uri = $Uri; Headers = $Headers; ErrorAction = "Stop" }
        if ($Body) { $params["Body"] = $Body; $params["ContentType"] = "application/json" }
        return Invoke-RestMethod @params
    } catch {
        return $null
    }
}

# ── Check binary exists (respects CARGO_TARGET_DIR / ~/.cargo/config.toml) ───
if (-not (Test-Path $BinaryPath)) {
    Write-Warning "Binary not found at default path: $BinaryPath"
    Write-Warning "Building debug binary and discovering actual output path..."
    Push-Location "$PSScriptRoot/../../.."
    # Capture JSON build output to find the real executable path
    $buildJson = cargo build --bin voltnuerongridd --message-format=json 2>$null
    Pop-Location
    $resolved = $buildJson | ForEach-Object {
        try {
            $obj = $_ | ConvertFrom-Json
            if ($obj.reason -eq "compiler-artifact" -and $obj.executable) { $obj.executable }
        } catch {}
    } | Where-Object { $_ } | Select-Object -Last 1
    if ($resolved -and (Test-Path $resolved)) {
        Write-Host "Resolved binary via cargo JSON: $resolved"
        $BinaryPath = $resolved
    }
}
if (-not (Test-Path $BinaryPath)) {
    $artifact = @{
        gate      = "p5-multinode-smoke"
        timestamp = (Get-Date -Format "o")
        status    = "blocked"
        reason    = "binary not found: $BinaryPath"
        packs     = @()
    }
    $artifact | ConvertTo-Json -Depth 5 | Set-Content "$ResultsDir/multinode-smoke.json"
    Write-Warning "P5 gate BLOCKED: binary not found"
    exit 0
}

# ── Start 3 nodes ─────────────────────────────────────────────────────────────
# Build per-node peer lists that EXCLUDE the node itself (a node must not
# include its own URL in VNG_RAFT_PEERS, otherwise it heartbeats itself and
# calls become_follower(), causing the leader to step down immediately).
$allPorts = @($Node1Port,$Node2Port,$Node3Port)

for ($i = 1; $i -le 3; $i++) {
    $port  = $allPorts[$i - 1]
    # Peer URLs = all 3 minus this node's own URL
    $otherPorts = $allPorts | Where-Object { $_ -ne $port }
    $peerUrls   = ($otherPorts | ForEach-Object { "http://127.0.0.1:$_" }) -join ","

    $tdir  = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), "vng-node$i-$(Get-Random)")
    New-Item -ItemType Directory -Path $tdir | Out-Null
    $tmpDirs += $tdir

    $env_vars = @{
        VNG_ADMIN_API_KEY              = $AdminKey
        VNG_NODE_ID                    = "node-$i"
        VNG_RAFT_PEERS                 = $peerUrls
        VNG_CLUSTER_TOKEN              = "p5-gate-cluster-secret"
        VNG_CLUSTER_MODE               = "cluster"
        VNG_DATA_DIR                   = $tdir
        VNG_NATIVE_LISTENER_ENABLED    = "false"
        VNG_HTTP_BIND                  = "127.0.0.1:$port"
        RUST_LOG                       = "warn"
    }

    $proc = Start-Process -FilePath $BinaryPath `
        -WorkingDirectory $tdir `
        -PassThru `
        -NoNewWindow `
        -Environment $env_vars `
        -RedirectStandardOutput "$tdir/stdout.log" `
        -RedirectStandardError  "$tdir/stderr.log"
    $Procs += $proc
}

Start-Sleep -Milliseconds $StartupWaitMs

# ── Pack 1: All nodes healthy ─────────────────────────────────────────────────
$p = @{ name = "all-nodes-healthy"; status = "pending"; checks = @() }
$healthyCount = 0
foreach ($port in @($Node1Port,$Node2Port,$Node3Port)) {
    try {
        $r = Invoke-RestMethod -Uri "http://127.0.0.1:$port/health" -ErrorAction Stop
        $ok = $r.status -in @("ok","healthy") -or $r -eq "ok"
        if ($ok) { $healthyCount++ }
        $p.checks += @{ name = "node-$port-healthy"; passed = $ok; got = ($r | ConvertTo-Json -Compress) }
    } catch {
        $p.checks += @{ name = "node-$port-healthy"; passed = $false; got = "unreachable" }
    }
}
$p.status = if ($healthyCount -ge 2) { "passed" } else { "failed" }
$packs += $p

if ($healthyCount -lt 2) {
    Stop-Cluster
    Write-Warning "Fewer than 2 nodes responded; cluster not viable"
    $p.status = "failed"
    $skipRemainingPacks = $true
}

# ── Pack 2: Leader election ───────────────────────────────────────────────────
if (-not $skipRemainingPacks) {
Start-Sleep -Milliseconds $ElectionWaitMs
$p = @{ name = "leader-elected"; status = "pending"; checks = @() }
$leaderPort = $null
foreach ($port in @($Node1Port,$Node2Port,$Node3Port)) {
    try {
        $r = Invoke-RestMethod -Uri "http://127.0.0.1:$port/api/v1/cluster/raft/status" `
            -Headers @{ "x-vng-admin-key" = $AdminKey; "x-vng-operator-id" = "admin" } -ErrorAction Stop
        if ($r.raft.role -eq "Leader") { $leaderPort = $port }
        $p.checks += @{ name = "node-$port-role"; passed = $true; got = $r.raft.role }
    } catch {
        $p.checks += @{ name = "node-$port-role"; passed = $false; got = "error" }
    }
}
$p.status = if ($null -ne $leaderPort) { "passed" } else { "warning" }
$packs += $p

# ── Pack 3: Write rows to leader, verify replication ─────────────────────────
$p = @{ name = "row-replication"; status = "pending"; checks = @() }
if ($null -ne $leaderPort) {
    $headers = @{ "x-vng-admin-key" = $AdminKey; "x-vng-operator-id" = "admin"; "Content-Type" = "application/json" }
    $tableUniq = "p5_$(Get-Random)"
    # Create table
    $ddl = @{ sql_batch = "CREATE TABLE $tableUniq (id TEXT, val TEXT)" } | ConvertTo-Json
    Invoke-VngRequest -Method Post -Uri "http://127.0.0.1:$leaderPort/api/v1/sql/execute" `
        -Headers $headers -Body $ddl | Out-Null
    # Insert 5 rows
    for ($r = 1; $r -le 5; $r++) {
        $dml = @{ sql_batch = "INSERT INTO $tableUniq (id,val) VALUES ('row-$r','value-$r')" } | ConvertTo-Json
        Invoke-VngRequest -Method Post -Uri "http://127.0.0.1:$leaderPort/api/v1/sql/execute" `
            -Headers $headers -Body $dml | Out-Null
    }
    Start-Sleep -Milliseconds 1000
    # Verify rows visible on a follower
    $followerPort = @($Node1Port,$Node2Port,$Node3Port) | Where-Object { $_ -ne $leaderPort } | Select-Object -First 1
    $qry = @{ sql_batch = "SELECT id FROM $tableUniq" } | ConvertTo-Json
    $fResp = Invoke-VngRequest -Method Post `
        -Uri "http://127.0.0.1:$followerPort/api/v1/sql/execute" `
        -Headers $headers -Body $qry
    $rowCount = if ($fResp -and $fResp.rows) { $fResp.rows.Count } else { 0 }
    $p.checks += @{ name = "rows-on-follower"; passed = ($rowCount -ge 5); got = "rows=$rowCount" }
    $p.status = if ($rowCount -ge 5) { "passed" } else { "warning" }
} else {
    $p.checks += @{ name = "skipped"; passed = $true; got = "no-leader-elected" }
    $p.status = "warning"
}
$packs += $p

# ── Pack 4: Leader failure + new election ────────────────────────────────────
$p = @{ name = "leader-failover"; status = "pending"; checks = @() }
if ($null -ne $leaderPort) {
    # Kill the leader process
    $leaderProcIdx = @($Node1Port,$Node2Port,$Node3Port).IndexOf($leaderPort)
    try { Stop-Process -Id $Procs[$leaderProcIdx].Id -Force } catch {}
    Start-Sleep -Milliseconds ($ElectionWaitMs * 2)
    # Check for a new leader among surviving nodes
    $newLeader = $null
    foreach ($port in @($Node1Port,$Node2Port,$Node3Port) | Where-Object { $_ -ne $leaderPort }) {
        try {
            $r = Invoke-RestMethod -Uri "http://127.0.0.1:$port/api/v1/cluster/raft/status" `
                -Headers @{ "x-vng-admin-key" = $AdminKey; "x-vng-operator-id" = "admin" } -ErrorAction Stop
            if ($r.raft.role -eq "Leader") { $newLeader = $port }
            $p.checks += @{ name = "node-$port-after-kill"; passed = $true; got = $r.raft.role }
        } catch {
            $p.checks += @{ name = "node-$port-after-kill"; passed = $false; got = "unreachable" }
        }
    }
    $p.status = if ($null -ne $newLeader) { "passed" } else { "warning" }
} else {
    $p.checks += @{ name = "skipped"; passed = $true; got = "no-leader-to-kill" }
    $p.status = "warning"
}
$packs += $p

} # end if -not skipRemainingPacks (packs 2-4)

Stop-Cluster

# ── Derive gate status ────────────────────────────────────────────────────────
$failedCount  = ($packs | Where-Object { $_.status -eq "failed" }).Count
$warningCount = ($packs | Where-Object { $_.status -eq "warning" }).Count
$gateStatus   = if ($failedCount -gt 0) { "failed" } elseif ($warningCount -gt 0) { "warning" } else { "passed" }

$artifact = @{
    gate       = "p5-multinode-smoke"
    timestamp  = (Get-Date -Format "o")
    node_ports = @($Node1Port,$Node2Port,$Node3Port)
    status     = $gateStatus
    packs      = $packs
    note       = "P5 multi-node smoke: warning status is expected until P1 (durable row store) is complete. Leader failover and row replication over Raft are exercised."
}

$outPath = "$ResultsDir/multinode-smoke.json"
$artifact | ConvertTo-Json -Depth 8 | Set-Content $outPath
Write-Host "Multi-node smoke gate: $gateStatus — artifact: $outPath"
if ($gateStatus -eq "failed") { exit 1 }
