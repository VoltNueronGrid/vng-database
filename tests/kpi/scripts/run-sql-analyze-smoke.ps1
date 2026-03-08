param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$OutputPath = "tests/kpi/results/20260305-ws1/sql-analyze-smoke.json",
  [string]$TenantId = "acme",
  [string]$UserId = "analyst-acme"
)

$ErrorActionPreference = "Stop"

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$request = @{
  sql_batch = "BEGIN; CREATE TABLE t(id int); SELECT * FROM t; nonsense command;"
}

$unauthorizedHttp = Invoke-WebRequest `
  -Method Post `
  -Uri "$BaseUrl/api/v1/sql/analyze" `
  -Body ($request | ConvertTo-Json -Depth 8) `
  -ContentType "application/json" `
  -TimeoutSec 15 -UseBasicParsing -SkipHttpErrorCheck

$headers = @{
  "x-vng-tenant-id" = $TenantId
  "x-vng-user-id" = $UserId
}

$response = Invoke-RestMethod `
  -Method Post `
  -Uri "$BaseUrl/api/v1/sql/analyze" `
  -Headers $headers `
  -Body ($request | ConvertTo-Json -Depth 8) `
  -ContentType "application/json" `
  -TimeoutSec 15

$checks = [ordered]@{
  analyze_requires_user_headers = ([int]$unauthorizedHttp.StatusCode -eq 401)
  analyze_status_ok = ($response.status -eq "ok")
  statements_reported = ($response.total_statements -eq 4)
  rejected_statement_count_present = ($response.rejected_statements -ge 1)
}
$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  smoke = "sql-analyze-runtime"
  status = $status
  base_url = $BaseUrl
  tenant_id = $TenantId
  user_id = $UserId
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  response = $response
}

$artifact | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "SQL analyze smoke result: $OutputPath"
if ($status -ne "passed") { exit 1 }
