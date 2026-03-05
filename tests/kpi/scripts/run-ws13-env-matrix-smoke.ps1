param(
  [string]$OutputPath = "tests/kpi/results/ws13/env-matrix-smoke.json"
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

$providers = @(
  @{
    Name = "aws"
    Readme = "deploy/cloud/aws/README.md"
    Vars = @("VNG_AWS_REGION","VNG_AWS_IMAGE_TAG","VNG_AWS_ADMIN_API_KEY","VNG_AWS_BASE_URL","VNG_AWS_SQL_URL","VNG_AWS_BEARER_TOKEN")
  },
  @{
    Name = "azure"
    Readme = "deploy/cloud/azure/README.md"
    Vars = @("VNG_AZURE_REGION","VNG_AZURE_IMAGE_TAG","VNG_AZURE_ADMIN_API_KEY","VNG_AZURE_BASE_URL","VNG_AZURE_SQL_URL","VNG_AZURE_BEARER_TOKEN")
  },
  @{
    Name = "gcp"
    Readme = "deploy/cloud/gcp/README.md"
    Vars = @("VNG_GCP_REGION","VNG_GCP_IMAGE_TAG","VNG_GCP_ADMIN_API_KEY","VNG_GCP_BASE_URL","VNG_GCP_SQL_URL","VNG_GCP_BEARER_TOKEN")
  }
)

foreach ($provider in $providers) {
  $readmeExists = Test-Path $provider.Readme
  Add-Check -Name ("runbook_exists_" + $provider.Name) -Ok $readmeExists -Detail $provider.Readme
  if ($readmeExists) {
    $content = Get-Content -Raw -Path $provider.Readme
    Add-Check -Name ("runbook_has_matrix_" + $provider.Name) -Ok ($content -match "Environment variable matrix") -Detail "matrix heading"
    foreach ($var in $provider.Vars) {
      Add-Check -Name ("runbook_var_" + $provider.Name + "_" + $var) -Ok ($content -match [regex]::Escape($var)) -Detail $var
    }
  }
}

$commonReadme = "deploy/cloud/common/README.md"
Add-Check -Name "common_runbook_exists" -Ok (Test-Path $commonReadme) -Detail $commonReadme
if (Test-Path $commonReadme) {
  $content = Get-Content -Raw -Path $commonReadme
  Add-Check -Name "common_contract_ref" -Ok ($content -match "profile-contract\.yaml") -Detail "contract reference"
}

$status = if (($checks | Where-Object { -not $_.ok }).Count -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws13-env-matrix-validation"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS13 env matrix smoke failed."
  exit 1
}

Write-Host "WS13 env matrix smoke passed. Artifact: $OutputPath"
