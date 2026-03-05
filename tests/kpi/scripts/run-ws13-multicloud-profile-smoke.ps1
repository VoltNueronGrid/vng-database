param(
  [string]$OutputPath = "tests/kpi/results/ws13/multicloud-profile-smoke.json"
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

$contractPath = "deploy/cloud/common/profile-contract.yaml"
$profiles = @(
  @{ Provider = "aws"; Path = "deploy/cloud/aws/profile.yaml"; RegionEnv = "VNG_AWS_REGION"; AdminEnv = "VNG_AWS_ADMIN_API_KEY" },
  @{ Provider = "azure"; Path = "deploy/cloud/azure/profile.yaml"; RegionEnv = "VNG_AZURE_REGION"; AdminEnv = "VNG_AZURE_ADMIN_API_KEY" },
  @{ Provider = "gcp"; Path = "deploy/cloud/gcp/profile.yaml"; RegionEnv = "VNG_GCP_REGION"; AdminEnv = "VNG_GCP_ADMIN_API_KEY" }
)

Add-Check -Name "profile_contract_exists" -Ok (Test-Path $contractPath) -Detail $contractPath

foreach ($profile in $profiles) {
  $exists = Test-Path $profile.Path
  Add-Check -Name ("profile_exists_" + $profile.Provider) -Ok $exists -Detail $profile.Path
  if ($exists) {
    $content = Get-Content -Raw -Path $profile.Path
    $hasProvider = $content -match ("provider:\s*" + [regex]::Escape($profile.Provider))
    $hasRegionEnv = $content -match ("region_env:\s*" + [regex]::Escape($profile.RegionEnv))
    $hasAdminEnv = $content -match ("admin_key_env:\s*" + [regex]::Escape($profile.AdminEnv))
    $hasDrState = $content -match "dr_hook_state_path:\s*/var/lib/vng/state/dr-hook-runtime\.json"
    Add-Check -Name ("profile_provider_" + $profile.Provider) -Ok $hasProvider -Detail "provider key"
    Add-Check -Name ("profile_region_env_" + $profile.Provider) -Ok $hasRegionEnv -Detail $profile.RegionEnv
    Add-Check -Name ("profile_admin_env_" + $profile.Provider) -Ok $hasAdminEnv -Detail $profile.AdminEnv
    Add-Check -Name ("profile_dr_state_path_" + $profile.Provider) -Ok $hasDrState -Detail "dr_hook_state_path"
  }
}

$kpiCloudProfiles = "tests/kpi/config/cloud-profiles-real.yaml"
$kpiExists = Test-Path $kpiCloudProfiles
Add-Check -Name "kpi_cloud_profiles_real_exists" -Ok $kpiExists -Detail $kpiCloudProfiles
if ($kpiExists) {
  $content = Get-Content -Raw -Path $kpiCloudProfiles
  Add-Check -Name "kpi_cloud_profiles_contains_aws" -Ok ($content -match "VNG_AWS_BASE_URL") -Detail "aws env bindings"
  Add-Check -Name "kpi_cloud_profiles_contains_azure" -Ok ($content -match "VNG_AZURE_BASE_URL") -Detail "azure env bindings"
  Add-Check -Name "kpi_cloud_profiles_contains_gcp" -Ok ($content -match "VNG_GCP_BASE_URL") -Detail "gcp env bindings"
}

$status = if (($checks | Where-Object { -not $_.ok }).Count -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws13-multicloud-deployment-profile-baseline"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS13 multicloud profile smoke failed."
  exit 1
}

Write-Host "WS13 multicloud profile smoke passed. Artifact: $OutputPath"
