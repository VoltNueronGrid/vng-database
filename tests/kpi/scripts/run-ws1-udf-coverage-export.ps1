param(
  [string]$SummaryPath = "tests/kpi/results/ws1/ws1-gate-summary.json",
  [string]$UdfSmokePath = "tests/kpi/results/ws1/ws1-udf-contract-smoke.json",
  [string]$RuntimeSmokePath = "tests/kpi/results/ws1/sql-execute-udf-smoke.json",
  [string]$OutputPath = "tests/kpi/results/ws1/ws1-udf-coverage-matrix.json"
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
foreach ($path in @($SummaryPath, $UdfSmokePath, $RuntimeSmokePath)) {
  if (!(Test-Path -Path $path)) { throw "WS1 UDF coverage source missing at $path" }
}

$summary = Get-Content -Raw -Path $SummaryPath | ConvertFrom-Json
$udfSmoke = Get-Content -Raw -Path $UdfSmokePath | ConvertFrom-Json
$runtimeSmoke = Get-Content -Raw -Path $RuntimeSmokePath | ConvertFrom-Json

$packByName = @{}
foreach ($pack in $summary.packs) { $packByName[[string]$pack.pack] = [string]$pack.status }

$matrix = @(
  [ordered]@{ control = "polyglot_contract_declarations"; source = "ws1-udf-contract-smoke"; status = [string]$udfSmoke.status; artifact = $UdfSmokePath },
  [ordered]@{ control = "runtime_polyglot_execute_path"; source = "sql-execute-udf-smoke"; status = [string]$runtimeSmoke.status; artifact = $RuntimeSmokePath },
  [ordered]@{ control = "runtime_guardrail_rejection"; source = "sql-execute-udf-smoke"; status = if ($runtimeSmoke.checks.guardrail_blocks_unsafe_payload -and $runtimeSmoke.checks.guardrail_returns_language_policy_reason) { "passed" } else { "failed" }; artifact = $RuntimeSmokePath },
  [ordered]@{ control = "ws1_gate_runtime_test_pack"; source = "ws1-gate-summary"; status = $packByName["ws1-udf-runtime-scaffold-tests"]; artifact = $SummaryPath }
)

$failed = @($matrix | Where-Object { $_.status -ne "passed" })
$status = if ($failed.Count -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  report = "ws1-udf-coverage-matrix"
  status = $status
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  source_summary = $SummaryPath
  total_controls = $matrix.Count
  passed_controls = ($matrix | Where-Object { $_.status -eq "passed" }).Count
  failed_controls = $failed.Count
  matrix = $matrix
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS1 UDF coverage matrix artifact: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
