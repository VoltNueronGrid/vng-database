param(
  [string]$OutputPath = "tests/kpi/results/ws13/overlay-schema-smoke.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Add-Check {
  param([string]$Name, [bool]$Ok, [string]$Detail)
  $script:checks += [ordered]@{
    check = $Name
    ok = $Ok
    detail = $Detail
  }
}

Ensure-OutputDir -PathValue $OutputPath
$start = Get-Date
$checks = @()

$providers = @("aws", "azure", "gcp")
foreach ($provider in $providers) {
  $singlePath = "deploy/cloud/$provider/single-node-overlay.yaml"
  $multiPath = "deploy/cloud/$provider/multi-node-overlay.yaml"
  $helmPath = "deploy/cloud/$provider/helm-values.yaml"

  Add-Check -Name ("single_overlay_exists_" + $provider) -Ok (Test-Path $singlePath) -Detail $singlePath
  Add-Check -Name ("multi_overlay_exists_" + $provider) -Ok (Test-Path $multiPath) -Detail $multiPath
  Add-Check -Name ("helm_values_exists_" + $provider) -Ok (Test-Path $helmPath) -Detail $helmPath

  if (Test-Path $singlePath) {
    $single = Get-Content -Raw -Path $singlePath
    Add-Check -Name ("single_mode_" + $provider) -Ok ($single -match "deployment_mode:\s*single-node") -Detail "deployment_mode single-node"
    Add-Check -Name ("single_replica_" + $provider) -Ok ($single -match "replica_count:\s*1") -Detail "replica_count 1"
    Add-Check -Name ("single_cluster_mode_" + $provider) -Ok ($single -match "cluster_mode:\s*single") -Detail "runtime cluster_mode single"
  }

  if (Test-Path $multiPath) {
    $multi = Get-Content -Raw -Path $multiPath
    Add-Check -Name ("multi_mode_" + $provider) -Ok ($multi -match "deployment_mode:\s*multi-node") -Detail "deployment_mode multi-node"
    Add-Check -Name ("multi_replica_" + $provider) -Ok ($multi -match "replica_count:\s*3") -Detail "replica_count 3"
    Add-Check -Name ("multi_seed_nodes_" + $provider) -Ok ($multi -match "seed_nodes:") -Detail "seed_nodes present"
  }

  if (Test-Path $helmPath) {
    $helm = Get-Content -Raw -Path $helmPath
    Add-Check -Name ("helm_provider_" + $provider) -Ok ($helm -match ("provider:\s*" + [regex]::Escape($provider))) -Detail "cloud provider"
    Add-Check -Name ("helm_loadbalancer_" + $provider) -Ok ($helm -match "type:\s*LoadBalancer") -Detail "service type"
    Add-Check -Name ("helm_cluster_mode_multi_" + $provider) -Ok ($helm -match "VNG_CLUSTER_MODE:\s*multi") -Detail "env cluster mode"
  }
}

$status = if (($checks | Where-Object { -not $_.ok }).Count -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws13-overlay-schema-validation"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS13 overlay/schema smoke failed."
  exit 1
}

Write-Host "WS13 overlay/schema smoke passed. Artifact: $OutputPath"
