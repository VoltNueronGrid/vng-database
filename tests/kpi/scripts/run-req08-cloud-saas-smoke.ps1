# REQ-08: Cloud SaaS deployment model smoke check
# Verifies that all mandatory cloud deployment artefacts exist and are well-formed
# for the three supported providers (AWS, Azure, GCP).
#
# Usage:
#   pwsh ./tests/kpi/scripts/run-req08-cloud-saas-smoke.ps1
#   pwsh ./tests/kpi/scripts/run-req08-cloud-saas-smoke.ps1 -OutputPath tests/kpi/results/req08/cloud-saas-smoke.json

param(
  [string]$OutputPath = "tests/kpi/results/req08/cloud-saas-smoke.json"
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
    check  = $Name
    ok     = $Ok
    detail = $Detail
  }
}

Ensure-OutputDir -PathValue $OutputPath
$start  = Get-Date
$checks = @()

# ── 1. Per-provider deploy artefacts ──────────────────────────────────────────
$providers = @("aws", "azure", "gcp")
$requiredFiles = @(
  "profile.yaml",
  "helm-values.yaml",
  "single-node-overlay.yaml",
  "multi-node-overlay.yaml",
  "README.md"
)

foreach ($provider in $providers) {
  $base = "deploy/cloud/$provider"

  $dirExists = Test-Path $base
  Add-Check -Name "cloud_dir_exists_$provider" -Ok $dirExists -Detail $base

  if ($dirExists) {
    foreach ($file in $requiredFiles) {
      $filePath = "$base/$file"
      $fileExists = Test-Path $filePath
      Add-Check -Name "cloud_file_$($provider)_$($file -replace '[^a-z0-9]','_')" `
                -Ok $fileExists -Detail $filePath
    }

    # profile.yaml must declare the correct provider key
    $profilePath = "$base/profile.yaml"
    if (Test-Path $profilePath) {
      $content = Get-Content -Raw -Path $profilePath
      $hasProvider  = $content -match "provider:\s*$provider"
      $hasAdminEnv  = $content -match "admin_key_env:\s*VNG_"
      $hasRegionEnv = $content -match "region_env:\s*VNG_"
      Add-Check -Name "cloud_profile_provider_key_$provider"  -Ok $hasProvider  -Detail "provider: $provider"
      Add-Check -Name "cloud_profile_admin_env_$provider"     -Ok $hasAdminEnv  -Detail "admin_key_env present"
      Add-Check -Name "cloud_profile_region_env_$provider"    -Ok $hasRegionEnv -Detail "region_env present"
    }

    # multi-node overlay must reference replica/ha configuration
    $multiNodePath = "$base/multi-node-overlay.yaml"
    if (Test-Path $multiNodePath) {
      $content     = Get-Content -Raw -Path $multiNodePath
      $hasReplica  = $content -match "replica|ha_mode|cluster_mode|replication"
      Add-Check -Name "cloud_multi_node_ha_keys_$provider" -Ok $hasReplica -Detail "replica/ha_mode/cluster_mode/replication"
    }
  }
}

# ── 2. Common deploy artefacts ────────────────────────────────────────────────
$commonDir = "deploy/cloud/common"
Add-Check -Name "cloud_common_dir_exists" -Ok (Test-Path $commonDir) -Detail $commonDir

# ── 3. Helm chart directory ───────────────────────────────────────────────────
$helmDir      = "deploy/helm"
$helmExists   = Test-Path $helmDir
Add-Check -Name "deploy_helm_dir_exists" -Ok $helmExists -Detail $helmDir

# ── 4. Redis-compat port documented in profiles ────────────────────────────────
# REQ-27 requires port 6380 to be referenced in at least one deployment artefact.
$redisPortFound = $false
foreach ($provider in $providers) {
  $profilePath = "deploy/cloud/$provider/profile.yaml"
  if (Test-Path $profilePath) {
    $content = Get-Content -Raw -Path $profilePath
    if ($content -match "6380|redis_compat_port") {
      $redisPortFound = $true
      break
    }
  }
}
# Also check helm values
if (!$redisPortFound -and (Test-Path $helmDir)) {
  $helmValues = Get-ChildItem -Path $helmDir -Filter "*.yaml" -Recurse -ErrorAction SilentlyContinue
  foreach ($hv in $helmValues) {
    $content = Get-Content -Raw -Path $hv.FullName
    if ($content -match "6380|redis_compat_port") {
      $redisPortFound = $true
      break
    }
  }
}
Add-Check -Name "redis_compat_port_6380_documented" -Ok $redisPortFound `
          -Detail "port 6380 referenced in a cloud/helm artefact"

# ── 5. SRE cache endpoint documented in profile or README ─────────────────────
$cacheEndpointFound = $false
foreach ($provider in $providers) {
  $readmePath = "deploy/cloud/$provider/README.md"
  if (Test-Path $readmePath) {
    $content = Get-Content -Raw -Path $readmePath
    if ($content -match "cache|redis|6380") {
      $cacheEndpointFound = $true
      break
    }
  }
}
Add-Check -Name "sre_cache_endpoint_referenced_in_docs" -Ok $cacheEndpointFound `
          -Detail "cache/redis/6380 present in a provider README"

# ── 6. P2 aggregation keywords documented in design reference ─────────────────
$designDocPath = "reference/voltnuerongrid-db-design.md"
if (Test-Path $designDocPath) {
  $content = Get-Content -Raw -Path $designDocPath
  $hasApprox   = $content -match "APPROX_COUNT_DISTINCT|approx_count_distinct"
  $hasTopN     = $content -match "TOP_N|top_n"
  $hasBottomN  = $content -match "BOTTOM_N|bottom_n"
  Add-Check -Name "design_doc_approx_count_distinct" -Ok $hasApprox  -Detail $designDocPath
  Add-Check -Name "design_doc_top_n"                 -Ok $hasTopN    -Detail $designDocPath
  Add-Check -Name "design_doc_bottom_n"              -Ok $hasBottomN -Detail $designDocPath
} else {
  # Not blocking — design doc optional at this stage
  Add-Check -Name "design_doc_present" -Ok $false -Detail $designDocPath
}

# ── Derive gate status from checks ────────────────────────────────────────────
# Critical checks: cloud artefact presence per provider (non-blocking: Redis port + docs)
$criticalFailed = $checks |
  Where-Object { -not $_.ok -and $_.check -notmatch "redis_compat_port|sre_cache|design_doc" }

$status   = if ($criticalFailed.Count -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke                = "req08-cloud-saas-deployment-baseline"
  status               = $status
  started_at_utc       = $start.ToUniversalTime().ToString("o")
  finished_at_utc      = $finished.ToUniversalTime().ToString("o")
  duration_ms          = [int](($finished - $start).TotalMilliseconds)
  providers_checked    = $providers
  critical_failed      = $criticalFailed.Count
  total_checks         = $checks.Count
  checks               = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "REQ-08 cloud SaaS smoke FAILED — $($criticalFailed.Count) critical check(s) failed. Artifact: $OutputPath"
  exit 1
}

Write-Host "REQ-08 cloud SaaS smoke PASSED. Artifact written to: $OutputPath"
