param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$OutputPath = "tests/kpi/results/ws0/ws0-governance-smoke.json"
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

# Check 1: status_tracker.md exists
$trackerPath = Join-Path $RepoRoot "status_tracker.md"
$checks["status_tracker_exists"] = Test-Path -Path $trackerPath
if (-not $checks["status_tracker_exists"]) { $status = "failed" }

# Check 2: status_tracker mentions WS0
if ($checks["status_tracker_exists"]) {
  $trackerContent = Get-Content -Raw -Path $trackerPath
  $checks["status_tracker_has_ws0"] = ($trackerContent -match "WS0")
  if (-not $checks["status_tracker_has_ws0"]) { $status = "failed" }
} else {
  $checks["status_tracker_has_ws0"] = $false
  $status = "failed"
}

# Check 3: Cargo workspace has voltnuerongridd service
$cargoPath = Join-Path $RepoRoot "Cargo.toml"
if (Test-Path -Path $cargoPath) {
  $cargoContent = Get-Content -Raw -Path $cargoPath
  $checks["cargo_workspace_has_service"] = ($cargoContent -match "voltnuerongridd")
  if (-not $checks["cargo_workspace_has_service"]) { $status = "failed" }
} else {
  $checks["cargo_workspace_has_service"] = $false
  $status = "failed"
}

# Check 4: main.rs service entry point exists
$mainRs = Join-Path $RepoRoot "services/voltnuerongridd/src/main.rs"
$checks["main_rs_exists"] = Test-Path -Path $mainRs
if (-not $checks["main_rs_exists"]) { $status = "failed" }

# Check 5: KPI gate scripts present (at least 10)
$scriptsDir = Join-Path $RepoRoot "tests/kpi/scripts"
if (Test-Path -Path $scriptsDir) {
  $scriptCount = @(Get-ChildItem -Path $scriptsDir -Filter "*.ps1").Count
  $checks["kpi_scripts_count_sufficient"] = ($scriptCount -ge 10)
  if (-not $checks["kpi_scripts_count_sufficient"]) { $status = "failed" }
} else {
  $checks["kpi_scripts_count_sufficient"] = $false
  $status = "failed"
}

# Check 6: LICENSE file present
$licensePath = Join-Path $RepoRoot "LICENSE"
$checks["license_exists"] = Test-Path -Path $licensePath
if (-not $checks["license_exists"]) { $status = "failed" }

# Check 7: README.md present
$readmePath = Join-Path $RepoRoot "README.md"
$checks["readme_exists"] = Test-Path -Path $readmePath
if (-not $checks["readme_exists"]) { $status = "failed" }

$checksTotal = $checks.Count
$checksPassed = @($checks.Values | Where-Object { $_ -eq $true }).Count

$artifact = [ordered]@{
  smoke            = "ws0-governance"
  status           = $status
  checks_passed    = $checksPassed
  checks_total     = $checksTotal
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks           = ($checks.Keys | ForEach-Object {
    [ordered]@{ name = $_; passed = $checks[$_]; detail = if ($checks[$_]) { "ok" } else { "missing" } }
  })
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS0 governance smoke: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
