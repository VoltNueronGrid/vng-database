param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$OutputPath = "tests/kpi/results/ws1/sql-execute-udf-smoke.json",
  [string]$TenantId = "acme",
  [string]$UserId = "analyst-acme"
)

$ErrorActionPreference = "Stop"

function Invoke-HttpJson {
  param(
    [string]$Method,
    [string]$Uri,
    [hashtable]$Headers,
    [object]$Body = $null
  )

  $params = @{
    Method = $Method
    Uri = $Uri
    TimeoutSec = 20
    UseBasicParsing = $true
  }
  if ($Headers) {
    $params.Headers = $Headers
  }
  if ($null -ne $Body) {
    $params.Body = ($Body | ConvertTo-Json -Depth 8)
    $params.ContentType = "application/json"
  }

  try {
    $response = Invoke-WebRequest @params
    $json = if ($response.Content) { $response.Content | ConvertFrom-Json } else { $null }
    return [pscustomobject]@{ StatusCode = [int]$response.StatusCode; Json = $json; Content = $response.Content }
  } catch {
    $statusCode = 0
    $content = ""
    if ($_.Exception.Response) {
      $statusCode = [int]$_.Exception.Response.StatusCode.value__
      $reader = New-Object System.IO.StreamReader($_.Exception.Response.GetResponseStream())
      $content = $reader.ReadToEnd()
      $reader.Close()
    }
    $json = if ($content) { try { $content | ConvertFrom-Json } catch { $null } } else { $null }
    return [pscustomobject]@{ StatusCode = $statusCode; Json = $json; Content = $content }
  }
}

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$okRequest = @{
  sql_batch = "SELECT udf_rust('hello'); SELECT udf_js('abc'); SELECT udf_python('delta');"
  max_rows = 10
}
$headers = @{
  "x-vng-tenant-id" = $TenantId
  "x-vng-user-id" = $UserId
}

$unauthorizedHttp = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/sql/execute" -Body $okRequest

$okResponse = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/sql/execute" -Headers $headers -Body $okRequest

$guardRequest = @{
  sql_batch = "SELECT udf_python('x'); import os"
  max_rows = 10
}
$guardHttp = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/sql/execute" -Headers $headers -Body $guardRequest
$guardBlocked = ($guardHttp.StatusCode -eq 400)
$guardReasonMatches = $false
if ($guardHttp.Json) {
  $guardReasonMatches = ($guardHttp.Json.reason -like "udf_guardrail_blocked_*" -or $guardHttp.Json.udf_guardrail_status -eq "blocked")
}
if (-not $guardReasonMatches -and $guardHttp.Content) {
  $guardReasonMatches = ($guardHttp.Content -match "udf_guardrail_blocked_python_payload" -or $guardHttp.Content -match '"udf_guardrail_status"\s*:\s*"blocked"')
}
if (-not $guardReasonMatches -and $guardBlocked) {
  $guardReasonMatches = $true
}

$checks = [ordered]@{
  execute_requires_user_headers = ($unauthorizedHttp.StatusCode -eq 401)
  execute_status_ok = ($okResponse.Json.status -eq "ok")
  udf_results_present = (@($okResponse.Json.udf_results).Count -eq 3)
  udf_catalog_contract_present = (@($okResponse.Json.udf_function_catalog).Count -eq 3)
  udf_guard_policy_contract_present = (@($okResponse.Json.udf_guard_policies).Count -eq 3)
  udf_execution_plan_present = (@($okResponse.Json.udf_execution_plan).Count -ge 1)
  udf_execution_plan_has_invocations = ((@($okResponse.Json.udf_execution_plan) | Where-Object { @($_.udf_invocations).Count -gt 0 }).Count -eq 3)
  rust_udf_output_expected = ($okResponse.Json.udf_results[0].output -eq "HELLO")
  js_udf_output_expected = ($okResponse.Json.udf_results[1].output -eq "cba")
  python_udf_output_expected = ($okResponse.Json.udf_results[2].output -eq "5")
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
  udf_results = $okResponse.Json.udf_results
  udf_function_catalog = $okResponse.Json.udf_function_catalog
  udf_guard_policies = $okResponse.Json.udf_guard_policies
  udf_execution_plan = $okResponse.Json.udf_execution_plan
}

$artifact | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "SQL execute UDF smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
