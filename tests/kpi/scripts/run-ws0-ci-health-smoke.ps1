param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$OutputPath = "tests/kpi/results/ws0/ws0-ci-health-smoke.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

Ensure-OutputDir -PathValue $OutputPath

$checks = [ordered]@{}
$status = "passed"

# Check 1: CI workflow file exists
$ciWorkflow = Join-Path $RepoRoot ".github/workflows/ci.yml"
$checks["ci_workflow_exists"] = Test-Path -Path $ciWorkflow
if (-not $checks["ci_workflow_exists"]) { $status = "failed" }

# Check 2: KPI scripts scaffold exists
$kpiScripts = Join-Path $RepoRoot "tests/kpi/scripts"
$checks["kpi_scripts_dir_exists"] = Test-Path -Path $kpiScripts
if (-not $checks["kpi_scripts_dir_exists"]) { $status = "failed" }

# Check 3: KPI results scaffold exists
$kpiResults = Join-Path $RepoRoot "tests/kpi/results"
$checks["kpi_results_dir_exists"] = Test-Path -Path $kpiResults
if (-not $checks["kpi_results_dir_exists"]) { $status = "failed" }

# Check 4: Deploy scaffold exists
$deployLocal = Join-Path $RepoRoot "deploy/local"
$checks["deploy_local_dir_exists"] = Test-Path -Path $deployLocal
if (-not $checks["deploy_local_dir_exists"]) { $status = "failed" }

# Check 5: reference directory exists
$refDir = Join-Path $RepoRoot "reference"
$checks["reference_dir_exists"] = Test-Path -Path $refDir
if (-not $checks["reference_dir_exists"]) { $status = "failed" }

# Check 6: Cargo.toml workspace manifest exists
$cargoToml = Join-Path $RepoRoot "Cargo.toml"
$checks["cargo_toml_exists"] = Test-Path -Path $cargoToml
if (-not $checks["cargo_toml_exists"]) { $status = "failed" }

# Check 7: Services directory exists
$servicesDir = Join-Path $RepoRoot "services"
$checks["services_dir_exists"] = Test-Path -Path $servicesDir
if (-not $checks["services_dir_exists"]) { $status = "failed" }

# Check 8: crates directory exists
$cratesDir = Join-Path $RepoRoot "crates"
$checks["crates_dir_exists"] = Test-Path -Path $cratesDir
if (-not $checks["crates_dir_exists"]) { $status = "failed" }

$checksTotal = $checks.Count
$checksPassed = @($checks.Values | Where-Object { $_ -eq $true }).Count

$artifact = [ordered]@{
  smoke            = "ws0-ci-health"
  status           = $status
  checks_passed    = $checksPassed
  checks_total     = $checksTotal
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks           = ($checks.Keys | ForEach-Object {
    [ordered]@{ name = $_; passed = $checks[$_]; detail = if ($checks[$_]) { "ok" } else { "missing" } }
  })
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS0 CI health smoke: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
