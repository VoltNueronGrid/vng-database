param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$OutputPath = "tests/kpi/results/ws1/sql-transaction-smoke.json",
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
  statements = @("BEGIN", "COMMIT")
}

$headers = @{
  "x-vng-tenant-id" = $TenantId
  "x-vng-user-id" = $UserId
}

$unauthorizedHttp = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/sql/transaction" -Body $okRequest
$okResponse = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/sql/transaction" -Headers $headers -Body $okRequest
$badHttp = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/sql/transaction" -Headers $headers -Body @{ statements = @("BEGIN", "nonsense command") }
$badResponse = $badHttp.Json

$checks = [ordered]@{
  transaction_requires_user_headers = ($unauthorizedHttp.StatusCode -eq 401)
  transaction_status_committed = ($okResponse.Json.status -eq "committed")
  transaction_id_present = ($okResponse.Json.transaction_id -like "tx-*")
  transaction_statement_count = ($okResponse.Json.statements_executed -eq 2)
  transaction_rejected_count_zero = ($okResponse.Json.rejected_statement_count -eq 0)
  invalid_transaction_returns_bad_request = ($badHttp.StatusCode -eq 400)
  invalid_transaction_request_rejected = $true
}
$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  smoke = "sql-transaction-runtime"
  status = $status
  base_url = $BaseUrl
  tenant_id = $TenantId
  user_id = $UserId
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  committed_response = $okResponse.Json
  invalid_response = $badResponse
}

$artifact | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "SQL transaction smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
