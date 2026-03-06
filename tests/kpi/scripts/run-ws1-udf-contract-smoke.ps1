param(
  [string]$SqlLibPath = "crates/voltnuerongrid-sql/src/lib.rs",
  [string]$OutputPath = "tests/kpi/results/ws1/ws1-udf-contract-smoke.json"
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
if (!(Test-Path -Path $SqlLibPath)) { throw "SQL library file not found at $SqlLibPath" }

$content = Get-Content -Raw -Path $SqlLibPath
$checks = [ordered]@{
  function_language_rust_declared = ($content -match 'FunctionLanguage\s*\{[\s\S]*Rust')
  function_language_javascript_declared = ($content -match 'FunctionLanguage\s*\{[\s\S]*JavaScript')
  function_language_python_declared = ($content -match 'FunctionLanguage\s*\{[\s\S]*Python')
  create_function_classifier_present = ($content -match 'CREATE"\),\s*Some\("FUNCTION"\)')
}

$output = @()
$global:LASTEXITCODE = 0
$testOutput = & cargo test -p voltnuerongrid-sql function_registry_supports_polyglot_udf_contract -- --nocapture 2>&1
$output += $testOutput
$checks.polyglot_udf_registry_test_passes = ($? -and $LASTEXITCODE -eq 0)

$global:LASTEXITCODE = 0
$classifyOutput = & cargo test -p voltnuerongrid-sql classifies_core_statements -- --nocapture 2>&1
$output += $classifyOutput
$checks.create_function_classification_test_passes = ($? -and $LASTEXITCODE -eq 0)

$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  smoke = "ws1-udf-contract"
  status = $status
  sql_lib_path = $SqlLibPath
  commands = @(
    "cargo test -p voltnuerongrid-sql function_registry_supports_polyglot_udf_contract -- --nocapture",
    "cargo test -p voltnuerongrid-sql classifies_core_statements -- --nocapture"
  )
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  output_excerpt = (($output | Select-Object -First 40) -join "`n")
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS1 UDF contract smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
