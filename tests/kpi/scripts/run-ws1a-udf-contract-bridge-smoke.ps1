param(
  [string]$OutputPath = "tests/kpi/results/ws1a/ws1a-udf-contract-bridge-smoke.json"
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

$outputLines = @()
$checks = [ordered]@{}

$global:LASTEXITCODE = 0
$parityOutput = & cargo test -p voltnuerongrid-sql p2_stub_hooks_cover_expected_aggregations -- --nocapture 2>&1
$outputLines += $parityOutput
$checks.ws1a_p2_stub_hook_test_passes = ($? -and $LASTEXITCODE -eq 0)

$global:LASTEXITCODE = 0
$udfOutput = & cargo test -p voltnuerongrid-sql function_registry_supports_polyglot_udf_contract -- --nocapture 2>&1
$outputLines += $udfOutput
$checks.ws1_polyglot_udf_contract_test_passes = ($? -and $LASTEXITCODE -eq 0)

$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  smoke = "ws1a-udf-contract-bridge"
  status = $status
  commands = @(
    "cargo test -p voltnuerongrid-sql p2_stub_hooks_cover_expected_aggregations -- --nocapture",
    "cargo test -p voltnuerongrid-sql function_registry_supports_polyglot_udf_contract -- --nocapture"
  )
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  output_excerpt = (($outputLines | Select-Object -First 40) -join "`n")
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS1A UDF bridge smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
