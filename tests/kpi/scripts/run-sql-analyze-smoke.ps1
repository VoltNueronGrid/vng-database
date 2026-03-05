param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$OutputPath = "tests/kpi/results/20260305-ws1/sql-analyze-smoke.json"
)

$ErrorActionPreference = "Stop"

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$request = @{
  sql_batch = "BEGIN; CREATE TABLE t(id int); SELECT * FROM t; nonsense command;"
}

$response = Invoke-RestMethod `
  -Method Post `
  -Uri "$BaseUrl/api/v1/sql/analyze" `
  -Body ($request | ConvertTo-Json -Depth 8) `
  -ContentType "application/json" `
  -TimeoutSec 15

$response | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "SQL analyze smoke result: $OutputPath"
