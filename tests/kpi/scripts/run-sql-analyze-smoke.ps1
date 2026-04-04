param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$OutputPath = "tests/kpi/results/20260305-ws1/sql-analyze-smoke.json",
  [string]$TenantId = "acme",
  [string]$UserId = "analyst-acme"
)

$ErrorActionPreference = "Stop"

$kpiScriptsRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $kpiScriptsRoot "kpi-http-helpers.ps1")
$PSDefaultParameterValues['Invoke-HttpJson:TimeoutSec'] = 15

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$request = @{
  sql_batch = "BEGIN; CREATE TABLE t(id int); SELECT * FROM t; nonsense command;"
}

$unauthorizedHttp = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/sql/analyze" -Body $request

$headers = @{
  "x-vng-tenant-id" = $TenantId
  "x-vng-user-id" = $UserId
}

$response = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/sql/analyze" -Headers $headers -Body $request

$checks = [ordered]@{
  analyze_requires_user_headers = ($unauthorizedHttp.StatusCode -eq 401)
  analyze_status_ok = ($response.Json.status -eq "ok")
  statements_reported = ($response.Json.total_statements -eq 4)
  rejected_statement_count_present = ($response.Json.rejected_statements -ge 1)
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
  response = $response.Json
}

$artifact | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "SQL analyze smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
