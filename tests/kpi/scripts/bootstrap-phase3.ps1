param(
  [string]$CloudProfilesPath = "",
  [string]$TargetsPath = "",
  [string]$LocalBaselineRoot = "",
  [string]$CloudOutputDir = "",
  [string]$ReportOutputDir = "",
  [int]$RequestTimeoutSec = 10,
  [string]$DefaultAuthToken = ""
)

$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$kpiRoot = Split-Path -Parent $scriptRoot
$configDir = Join-Path $kpiRoot "config"

if ([string]::IsNullOrWhiteSpace($CloudProfilesPath)) {
  $CloudProfilesPath = Join-Path $configDir "cloud-profiles-real.yaml"
}
if ([string]::IsNullOrWhiteSpace($TargetsPath)) {
  $TargetsPath = Join-Path $configDir "targets.yaml"
}
if ([string]::IsNullOrWhiteSpace($LocalBaselineRoot)) {
  $LocalBaselineRoot = Join-Path $kpiRoot "results/20260304-pr007"
}
if ([string]::IsNullOrWhiteSpace($CloudOutputDir)) {
  $CloudOutputDir = Join-Path $LocalBaselineRoot "cloud-profiles-real"
}
if ([string]::IsNullOrWhiteSpace($ReportOutputDir)) {
  $ReportOutputDir = Join-Path $LocalBaselineRoot "reports-real"
}

$cloudRunner = Join-Path $scriptRoot "run-cloud-smoke.ps1"
$gateRunner = Join-Path $scriptRoot "generate-gate-report.ps1"

if (!(Test-Path $cloudRunner)) { throw "Missing script: $cloudRunner" }
if (!(Test-Path $gateRunner)) { throw "Missing script: $gateRunner" }
if (!(Test-Path $CloudProfilesPath)) { throw "Missing cloud profiles: $CloudProfilesPath" }
if (!(Test-Path $TargetsPath)) { throw "Missing targets file: $TargetsPath" }
if (!(Test-Path $LocalBaselineRoot)) { throw "Missing local baseline root: $LocalBaselineRoot" }

function Get-RequiredEnvVarsFromCloudProfiles {
  param([string]$Path)
  $envVars = @()
  foreach ($line in (Get-Content -Path $Path)) {
    if ($line -match "^\s{4}(base_url_env|sql_url_env|auth_token_env):\s*(.+)\s*$") {
      $envName = $Matches[2].Trim()
      if (-not [string]::IsNullOrWhiteSpace($envName)) {
        $envVars += $envName
      }
    }
  }
  return @($envVars | Select-Object -Unique)
}

$requiredEnvVars = Get-RequiredEnvVarsFromCloudProfiles -Path $CloudProfilesPath
$missingEnvVars = @()
foreach ($envName in $requiredEnvVars) {
  $value = [Environment]::GetEnvironmentVariable($envName)
  if ([string]::IsNullOrWhiteSpace($value)) {
    $missingEnvVars += $envName
  }
}

$useAllowMissingEnv = ($missingEnvVars.Count -gt 0)
if ($useAllowMissingEnv) {
  Write-Host "Phase-3 bootstrap: missing environment variables detected, using deferred mode."
  foreach ($missing in $missingEnvVars) {
    Write-Host " - $missing"
  }
}
else {
  Write-Host "Phase-3 bootstrap: all required cloud env vars found; running real remote smoke."
}

if ($useAllowMissingEnv) {
  & $cloudRunner `
    -OutputRootDir $CloudOutputDir `
    -CloudProfilesPath $CloudProfilesPath `
    -TargetsPath $TargetsPath `
    -RequestTimeoutSec $RequestTimeoutSec `
    -DefaultAuthToken $DefaultAuthToken `
    -AllowMissingEnv
}
else {
  & $cloudRunner `
    -OutputRootDir $CloudOutputDir `
    -CloudProfilesPath $CloudProfilesPath `
    -TargetsPath $TargetsPath `
    -RequestTimeoutSec $RequestTimeoutSec `
    -DefaultAuthToken $DefaultAuthToken
}

$cloudRollupPath = Join-Path $CloudOutputDir "cloud-rollup-summary.json"
& $gateRunner `
  -LocalBaselineRoot $LocalBaselineRoot `
  -CloudRollupPath $cloudRollupPath `
  -OutputDir $ReportOutputDir

$finalGatePath = Join-Path $ReportOutputDir "final-gate-report.json"
$finalGate = Get-Content -Raw -Path $finalGatePath | ConvertFrom-Json

Write-Host "Phase-3 bootstrap complete. Final status: $($finalGate.overall_status)"
Write-Host "Cloud output: $CloudOutputDir"
Write-Host "Report output: $ReportOutputDir"

if ($finalGate.overall_status -eq "failed") {
  Write-Error "KPI gate failed."
  exit 1
}

exit 0
