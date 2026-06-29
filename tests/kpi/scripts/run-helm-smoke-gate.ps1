#!/usr/bin/env pwsh
# DEPLOY-1: Helm Chart Smoke Gate
# Tests: helm lint → kubeconform schema validation → kind cluster install → health check
#
# Prerequisites (CI):
#   helm  >= 3.12          (https://helm.sh/docs/intro/install/)
#   kubeconform >= 0.6    (https://github.com/yannh/kubeconform)
#   kind  >= 0.20          (https://kind.sigs.k8s.io/) — only for --kind-smoke
#   kubectl >= 1.28        — only for --kind-smoke
#
# Usage:
#   pwsh ./tests/kpi/scripts/run-helm-smoke-gate.ps1
#   pwsh ./tests/kpi/scripts/run-helm-smoke-gate.ps1 --kind-smoke
#   pwsh ./tests/kpi/scripts/run-helm-smoke-gate.ps1 --base-url http://127.0.0.1:8080

param(
    [string]$BaseUrl = "",
    [switch]$KindSmoke,
    [string]$ChartDir = "deploy/helm/voltnuerongrid",
    [string]$ArtifactDir = "tests/kpi/results/helm"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$packs = [System.Collections.Generic.List[hashtable]]::new()
$overallStatus = "pass"

function Write-Step($msg) { Write-Host "  >> $msg" -ForegroundColor Cyan }
function Pass-Pack($name, $detail) {
    Write-Host "  [PASS] $name — $detail" -ForegroundColor Green
    $packs.Add(@{ pack = $name; status = "pass"; detail = $detail })
}
function Fail-Pack($name, $detail) {
    Write-Host "  [FAIL] $name — $detail" -ForegroundColor Red
    $packs.Add(@{ pack = $name; status = "fail"; detail = $detail })
    $script:overallStatus = "fail"
}
function Skip-Pack($name, $detail) {
    Write-Host "  [SKIP] $name — $detail" -ForegroundColor Yellow
    $packs.Add(@{ pack = $name; status = "skip"; detail = $detail })
}

Write-Host "`n=== DEPLOY-1: Helm Chart Smoke Gate ===" -ForegroundColor Magenta

# ── Pack 1: helm lint ─────────────────────────────────────────────────────────
Write-Step "Running helm lint on $ChartDir"
$helmCmd = Get-Command helm -ErrorAction SilentlyContinue
if (-not $helmCmd) {
    Fail-Pack "helm-lint" "helm not found on PATH"
} else {
    $lintOutput = helm lint $ChartDir 2>&1
    if ($LASTEXITCODE -eq 0) {
        Pass-Pack "helm-lint" "0 warnings, 0 errors"
    } else {
        $firstError = ($lintOutput | Where-Object { $_ -match "ERROR|WARNING" } | Select-Object -First 1) -as [string]
        Fail-Pack "helm-lint" "lint failed: $firstError"
    }
}

# ── Pack 2: helm template + kubeconform schema validation ─────────────────────
Write-Step "Running helm template | kubeconform"
$helmOk = (Get-Command helm -ErrorAction SilentlyContinue) -ne $null
$kcOk   = (Get-Command kubeconform -ErrorAction SilentlyContinue) -ne $null

if (-not $helmOk) {
    Skip-Pack "kubeconform" "helm not found"
} elseif (-not $kcOk) {
    Skip-Pack "kubeconform" "kubeconform not found on PATH (install: go install github.com/yannh/kubeconform/cmd/kubeconform@latest)"
} else {
    $templateOutput = helm template voltnuerongrid $ChartDir `
        --set "adminApiKey.secretName=voltnuerongrid-admin" `
        --set "adminApiKey.secretKey=api-key" 2>&1
    if ($LASTEXITCODE -ne 0) {
        Fail-Pack "kubeconform" "helm template failed: $($templateOutput | Select-Object -First 1)"
    } else {
        $kcResult = $templateOutput | kubeconform -strict -kubernetes-version 1.28.0 2>&1
        if ($LASTEXITCODE -eq 0) {
            Pass-Pack "kubeconform" "all manifests valid against k8s 1.28 schema"
        } else {
            $firstErr = ($kcResult | Select-Object -First 1) -as [string]
            Fail-Pack "kubeconform" "schema validation error: $firstErr"
        }
    }
}

# ── Pack 3: kind cluster smoke test (optional, gated by --kind-smoke) ─────────
if ($KindSmoke) {
    Write-Step "Creating kind cluster for smoke test"
    $kindOk    = (Get-Command kind -ErrorAction SilentlyContinue) -ne $null
    $kubectlOk = (Get-Command kubectl -ErrorAction SilentlyContinue) -ne $null

    if (-not $kindOk) {
        Fail-Pack "kind-smoke" "kind not found on PATH (https://kind.sigs.k8s.io/)"
    } elseif (-not $kubectlOk) {
        Fail-Pack "kind-smoke" "kubectl not found on PATH"
    } else {
        $clusterName = "vng-helm-smoke"
        try {
            # Create cluster
            kind create cluster --name $clusterName --wait 60s 2>&1 | Out-Null

            # Create admin secret
            kubectl create secret generic voltnuerongrid-admin `
                --from-literal=api-key=smoke-test-key `
                --namespace default 2>&1 | Out-Null

            # Install chart
            helm install voltnuerongrid $ChartDir `
                --namespace default `
                --set "replicaCount=1" `
                --set "adminApiKey.secretName=voltnuerongrid-admin" `
                --set "adminApiKey.secretKey=api-key" `
                --set "persistence.enabled=false" `
                --wait --timeout 120s 2>&1 | Out-Null

            # Port-forward and health check
            $pf = Start-Process kubectl -ArgumentList "port-forward svc/voltnuerongrid 18080:8080 -n default" -PassThru -WindowStyle Hidden
            Start-Sleep -Seconds 5

            try {
                $health = Invoke-RestMethod -Uri "http://127.0.0.1:18080/health" -Method GET -TimeoutSec 10
                if ($health.status -eq "ok") {
                    Pass-Pack "kind-smoke" "pod Ready, /health returned ok"
                } else {
                    Fail-Pack "kind-smoke" "/health returned unexpected body: $health"
                }
            } catch {
                Fail-Pack "kind-smoke" "/health request failed: $_"
            } finally {
                Stop-Process -Id $pf.Id -Force -ErrorAction SilentlyContinue
            }
        } catch {
            Fail-Pack "kind-smoke" "cluster/install failed: $_"
        } finally {
            Write-Step "Tearing down kind cluster $clusterName"
            kind delete cluster --name $clusterName 2>&1 | Out-Null
        }
    }
} else {
    Skip-Pack "kind-smoke" "pass --kind-smoke to run (requires kind + kubectl)"
}

# ── Pack 4: health endpoint smoke (pre-running server) ────────────────────────
if ($BaseUrl -ne "") {
    Write-Step "Checking /health on $BaseUrl"
    try {
        $resp = Invoke-RestMethod -Uri "$BaseUrl/health" -Method GET -TimeoutSec 10
        if ($resp.status -eq "ok") {
            Pass-Pack "health-endpoint" "/health returned ok at $BaseUrl"
        } else {
            Fail-Pack "health-endpoint" "/health returned: $($resp | ConvertTo-Json -Compress)"
        }
    } catch {
        Fail-Pack "health-endpoint" "request to $BaseUrl/health failed: $_"
    }
} else {
    Skip-Pack "health-endpoint" "pass --base-url to test against a live server"
}

# ── Artifact ──────────────────────────────────────────────────────────────────
$null = New-Item -ItemType Directory -Force -Path $ArtifactDir
$artifact = @{
    gate        = "DEPLOY-1"
    status      = $overallStatus
    generated_at = (Get-Date -Format "yyyy-MM-ddTHH:mm:ssZ" -AsUTC)
    chart_dir   = $ChartDir
    packs       = $packs
}
$artifact | ConvertTo-Json -Depth 5 | Set-Content "$ArtifactDir/helm-smoke-gate.json"

Write-Host ""
if ($overallStatus -eq "pass") {
    Write-Host "DEPLOY-1 GATE: PASS" -ForegroundColor Green
} else {
    Write-Host "DEPLOY-1 GATE: FAIL" -ForegroundColor Red
}
Write-Host "Artifact: $ArtifactDir/helm-smoke-gate.json"
