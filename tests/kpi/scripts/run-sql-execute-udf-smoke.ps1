param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$OutputPath = "tests/kpi/results/ws1/sql-execute-udf-smoke.json"
)

$ErrorActionPreference = "Stop"

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$okRequest = @{
  sql_batch = "SELECT udf_rust('hello'); SELECT udf_js('abc'); SELECT udf_python('delta');"
  max_rows = 10
}
$okResponse = Invoke-RestMethod `
  -Method Post `
  -Uri "$BaseUrl/api/v1/sql/execute" `
  -Body ($okRequest | ConvertTo-Json -Depth 8) `
  -ContentType "application/json" `
  -TimeoutSec 20

$guardRequest = @{
  sql_batch = "SELECT udf_python('x'); import os"
  max_rows = 10
}
$guardHttp = Invoke-WebRequest `
  -Method Post `
  -Uri "$BaseUrl/api/v1/sql/execute" `
  -Body ($guardRequest | ConvertTo-Json -Depth 8) `
  -ContentType "application/json" `
  -TimeoutSec 20 -UseBasicParsing -SkipHttpErrorCheck
$guardBlocked = ([int]$guardHttp.StatusCode -eq 400)
$guardReasonMatches = $false
if ($guardHttp.Content) {
  try {
    $guardJson = $guardHttp.Content | ConvertFrom-Json
    $guardReasonMatches = ($guardJson.reason -eq "udf_guardrail_blocked_python_payload")
  } catch {
    $guardReasonMatches = $false
  }
}

$checks = [ordered]@{
  execute_status_ok = ($okResponse.status -eq "ok")
  udf_results_present = ($okResponse.udf_results.Count -eq 3)
  udf_catalog_contract_present = ($okResponse.udf_function_catalog.Count -eq 3)
  udf_guard_policy_contract_present = ($okResponse.udf_guard_policies.Count -eq 3)
  udf_execution_plan_present = ($okResponse.udf_execution_plan.Count -ge 1)
  udf_execution_plan_has_invocations = (($okResponse.udf_execution_plan | Where-Object { $_.udf_invocations.Count -gt 0 }).Count -eq 3)
  rust_udf_output_expected = ($okResponse.udf_results[0].output -eq "HELLO")
  js_udf_output_expected = ($okResponse.udf_results[1].output -eq "cba")
  python_udf_output_expected = ($okResponse.udf_results[2].output -eq "5")
  guardrail_blocks_unsafe_payload = $guardBlocked
  guardrail_returns_language_policy_reason = $guardReasonMatches
}
$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  smoke = "sql-execute-udf-runtime"
  status = $status
  base_url = $BaseUrl
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  udf_results = $okResponse.udf_results
  udf_function_catalog = $okResponse.udf_function_catalog
  udf_guard_policies = $okResponse.udf_guard_policies
  udf_execution_plan = $okResponse.udf_execution_plan
}

$artifact | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "SQL execute UDF smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
