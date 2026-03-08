param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$OutputPath = "tests/kpi/results/ws1/sql-route-smoke.json",
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

$request = @{
  sql_batch = "BEGIN; SELECT region, SUM(amount) FROM orders GROUP BY region;"
}

$unauthorizedHttp = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/sql/route" -Body $request

$headers = @{
  "x-vng-tenant-id" = $TenantId
  "x-vng-user-id" = $UserId
}

$response = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/sql/route" -Headers $headers -Body $request

$checks = [ordered]@{
  route_requires_user_headers = ($unauthorizedHttp.StatusCode -eq 401)
  route_status_ok = ($response.Json.status -eq "ok")
  route_path_hybrid = ($response.Json.route_path -eq "hybrid")
  route_reason_matches = ($response.Json.reason -eq "mixed transactional and analytical workload")
  route_statement_count = (@($response.Json.statements).Count -eq 2)
  route_contains_oltp_statement = ($response.Content -match '"path"\s*:\s*"oltp"')
  route_contains_olap_statement = ($response.Content -match '"path"\s*:\s*"olap"')
}
$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  smoke = "sql-route-runtime"
  status = $status
  base_url = $BaseUrl
  tenant_id = $TenantId
  user_id = $UserId
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  response = $response.Json
}

$artifact | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "SQL route smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
