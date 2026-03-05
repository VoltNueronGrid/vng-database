param(
  [Parameter(Mandatory = $true)][string]$OutputRootDir,
  [string]$CloudProfilesPath = "",
  [string]$TargetsPath = "",
  [int]$RequestTimeoutSec = 10,
  [string]$DefaultAuthToken = "",
  [switch]$AllowMissingEnv = $false
)

$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$kpiRunner = Join-Path $scriptRoot "run-kpi.ps1"
$configDir = Join-Path (Split-Path -Parent $scriptRoot) "config"
if ([string]::IsNullOrWhiteSpace($CloudProfilesPath)) {
  $CloudProfilesPath = Join-Path $configDir "cloud-profiles.yaml"
}
if ([string]::IsNullOrWhiteSpace($TargetsPath)) {
  $TargetsPath = Join-Path $configDir "targets.yaml"
}

if (!(Test-Path $OutputRootDir)) {
  New-Item -ItemType Directory -Path $OutputRootDir -Force | Out-Null
}

function Parse-CloudProfilesYaml {
  param([string]$Path)

  $lines = Get-Content -Path $Path
  $profiles = @()
  $currentProfile = $null
  $inHeaders = $false
  foreach ($line in $lines) {
    if ($line -match "^\s*-\s*name:\s*(.+)\s*$") {
      if ($currentProfile) {
        $profiles += [PSCustomObject]$currentProfile
      }
      $currentProfile = @{
        name = $Matches[1].Trim()
        headers = @{}
      }
      $inHeaders = $false
      continue
    }

    if (-not $currentProfile) { continue }

    if ($line -match "^\s{4}headers:\s*$") {
      $inHeaders = $true
      continue
    }
    if ($line -match "^\s{4}[a-zA-Z0-9_]+:\s*") {
      $inHeaders = $false
    }
    if ($inHeaders -and $line -match "^\s{6}([A-Za-z0-9\-_]+):\s*(.+)\s*$") {
      $currentProfile.headers[$Matches[1].Trim()] = $Matches[2].Trim()
      continue
    }

    if ($line -match "^\s{4}base_url:\s*(.+)\s*$") { $currentProfile.base_url = $Matches[1].Trim(); continue }
    if ($line -match "^\s{4}sql_url:\s*(.+)\s*$") { $currentProfile.sql_url = $Matches[1].Trim(); continue }
    if ($line -match "^\s{4}base_url_env:\s*(.+)\s*$") { $currentProfile.base_url_env = $Matches[1].Trim(); continue }
    if ($line -match "^\s{4}sql_url_env:\s*(.+)\s*$") { $currentProfile.sql_url_env = $Matches[1].Trim(); continue }
    if ($line -match "^\s{4}auth_mode:\s*(.+)\s*$") { $currentProfile.auth_mode = $Matches[1].Trim(); continue }
    if ($line -match "^\s{4}auth_token:\s*(.+)\s*$") { $currentProfile.auth_token = $Matches[1].Trim(); continue }
    if ($line -match "^\s{4}auth_token_env:\s*(.+)\s*$") { $currentProfile.auth_token_env = $Matches[1].Trim(); continue }
    if ($line -match "^\s{4}api_key_header_name:\s*(.+)\s*$") { $currentProfile.api_key_header_name = $Matches[1].Trim(); continue }
  }

  if ($currentProfile) {
    $profiles += [PSCustomObject]$currentProfile
  }
  return $profiles
}

$profiles = Parse-CloudProfilesYaml -Path $CloudProfilesPath
if (-not $profiles -or $profiles.Count -eq 0) {
  throw "No cloud profiles found in $CloudProfilesPath"
}

$profileSummaries = @()
$readinessChecks = @()
foreach ($profile in $profiles) {
  $profileName = [string]$profile.name
  if ([string]::IsNullOrWhiteSpace($profileName)) {
    throw "A cloud profile is missing 'name' in $CloudProfilesPath"
  }

  $baseUrl = [string]$profile.base_url
  $sqlUrl = [string]$profile.sql_url
  $missingRequirements = @()
  if ($profile.base_url_env) {
    $baseFromEnv = [Environment]::GetEnvironmentVariable([string]$profile.base_url_env)
    if (-not [string]::IsNullOrWhiteSpace($baseFromEnv)) {
      $baseUrl = $baseFromEnv
    }
    else {
      if (-not $AllowMissingEnv) {
        throw "Profile '$profileName' requires environment variable '$($profile.base_url_env)' for base_url."
      }
      $missingRequirements += [string]$profile.base_url_env
    }
  }
  if ($profile.sql_url_env) {
    $sqlFromEnv = [Environment]::GetEnvironmentVariable([string]$profile.sql_url_env)
    if (-not [string]::IsNullOrWhiteSpace($sqlFromEnv)) {
      $sqlUrl = $sqlFromEnv
    }
    else {
      if (-not $AllowMissingEnv) {
        throw "Profile '$profileName' requires environment variable '$($profile.sql_url_env)' for sql_url."
      }
      $missingRequirements += [string]$profile.sql_url_env
    }
  }
  if (([string]::IsNullOrWhiteSpace($baseUrl) -or [string]::IsNullOrWhiteSpace($sqlUrl)) -and -not $AllowMissingEnv) {
    throw "Cloud profile '$profileName' must define base_url and sql_url."
  }

  $authMode = if ($profile.auth_mode) { [string]$profile.auth_mode } else { "none" }
  $authToken = $DefaultAuthToken
  if ($profile.auth_token_env) {
    $envToken = [Environment]::GetEnvironmentVariable([string]$profile.auth_token_env)
    if (-not [string]::IsNullOrWhiteSpace($envToken)) {
      $authToken = $envToken
    }
    elseif ($AllowMissingEnv -and $authMode -ne "none") {
      $missingRequirements += [string]$profile.auth_token_env
    }
  }
  if ($profile.auth_token) {
    $authToken = [string]$profile.auth_token
  }

  $isReady = ($missingRequirements.Count -eq 0)
  $readinessChecks += [PSCustomObject]@{
    profile = $profileName
    ready = $isReady
    missing_requirements = $missingRequirements
  }

  if (-not $isReady -and $AllowMissingEnv) {
    $profileSummaries += [PSCustomObject]@{
      profile = $profileName
      status = "pending_config"
      passed = 0
      failed = 0
      total_scenarios = 0
      rollup_path = ""
      missing_requirements = $missingRequirements
    }
    continue
  }

  $apiKeyHeaderName = if ($profile.api_key_header_name) { [string]$profile.api_key_header_name } else { "X-API-Key" }
  $headersJson = ""
  if ($profile.headers -and $profile.headers.Keys.Count -gt 0) {
    $headersJson = ($profile.headers | ConvertTo-Json -Compress -Depth 6)
  }

  $profileOutputDir = Join-Path $OutputRootDir $profileName
  & $kpiRunner `
    -BaseUrl $baseUrl `
    -SqlUrl $sqlUrl `
    -OutputDir $profileOutputDir `
    -TargetsPath $TargetsPath `
    -AuthMode $authMode `
    -AuthToken $authToken `
    -ApiKeyHeaderName $apiKeyHeaderName `
    -RequestTimeoutSec $RequestTimeoutSec `
    -ExtraHeadersJson $headersJson `
    -ProfileName $profileName

  $rollupPath = Join-Path $profileOutputDir "rollup-summary.json"
  $rollup = Get-Content -Raw -Path $rollupPath | ConvertFrom-Json
  $profileSummaries += [PSCustomObject]@{
    profile = $rollup.profile
    status = $rollup.status
    passed = $rollup.passed
    failed = $rollup.failed
    total_scenarios = $rollup.total_scenarios
    rollup_path = $rollupPath
    missing_requirements = @()
  }
}

$failedProfiles = @($profileSummaries | Where-Object { $_.status -eq "failed" }).Count
$pendingProfiles = @($profileSummaries | Where-Object { $_.status -eq "pending_config" }).Count
$cloudRollup = @{
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  output_root = $OutputRootDir
  cloud_profiles_total = $profileSummaries.Count
  cloud_profiles_passed = @($profileSummaries | Where-Object { $_.status -eq "passed" }).Count
  cloud_profiles_failed = $failedProfiles
  cloud_profiles_pending_config = $pendingProfiles
  status = if ($failedProfiles -gt 0) { "failed" } elseif ($pendingProfiles -gt 0) { "pending_config" } else { "passed" }
  profiles = $profileSummaries
}

$cloudRollupPath = Join-Path $OutputRootDir "cloud-rollup-summary.json"
$cloudRollup | ConvertTo-Json -Depth 10 | Out-File -FilePath $cloudRollupPath -Encoding utf8

$readinessReport = @{
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  cloud_profiles_path = $CloudProfilesPath
  allow_missing_env = [bool]$AllowMissingEnv
  checks = $readinessChecks
}
$readinessPath = Join-Path $OutputRootDir "cloud-readiness-report.json"
$readinessReport | ConvertTo-Json -Depth 10 | Out-File -FilePath $readinessPath -Encoding utf8

Write-Host "Cloud smoke pack completed. Summary: $cloudRollupPath"
Write-Host "Cloud readiness report: $readinessPath"
