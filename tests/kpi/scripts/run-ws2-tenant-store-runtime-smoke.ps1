param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$OutputPath = "tests/kpi/results/ws2/ws2-tenant-store-runtime-smoke.json",
  [string]$TenantId = "acme",
  [string]$AdminUserId = "admin-acme",
  [string]$AnalystUserId = "analyst-acme"
)

$ErrorActionPreference = "Stop"

$kpiScriptsRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $kpiScriptsRoot "kpi-http-helpers.ps1")

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$suffix = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$tableName = "tenant/$TenantId/orders"
$indexName = "idx_${TenantId}_orders_$suffix"
$constraintName = "pk_${TenantId}_orders_$suffix"

$adminHeaders = @{
  "x-vng-tenant-id" = $TenantId
  "x-vng-user-id" = $AdminUserId
}
$analystHeaders = @{
  "x-vng-tenant-id" = $TenantId
  "x-vng-user-id" = $AnalystUserId
}

$createRequest = @{
  name = $indexName
  table = $tableName
  column = "customer_id"
  unique = $false
}

$unauthorizedCreate = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/store/indexes/create" -Body $createRequest
$analystCreate = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/store/indexes/create" -Headers $analystHeaders -Body $createRequest
$crossTenantCreate = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/store/indexes/create" -Headers $adminHeaders -Body @{
  name = "idx_globex_$suffix"
  table = "tenant/globex/orders"
  column = "customer_id"
  unique = $false
}
$createResponse = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/store/indexes/create" -Headers $adminHeaders -Body $createRequest
$listResponse = Invoke-HttpJson -Method Get -Uri "$BaseUrl/api/v1/store/indexes" -Headers $analystHeaders
$lookupResponse = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/store/indexes/lookup" -Headers $analystHeaders -Body @{ index_name = $indexName; value = "C100" }
$constraintResponse = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/store/constraints/add" -Headers $adminHeaders -Body @{
  name = $constraintName
  table = $tableName
  column = "id"
  kind = "primary_key"
}
$validateResponse = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/store/constraints/validate" -Headers $analystHeaders -Body @{
  table = $tableName
  column = "id"
  value = "ord-$suffix"
}
$dropResponse = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/store/indexes/drop" -Headers $adminHeaders -Body @{ name = $indexName }

$checks = [ordered]@{
  create_requires_auth_headers = ($unauthorizedCreate.StatusCode -eq 401)
  analyst_cannot_create_index = ($analystCreate.StatusCode -eq 403)
  tenant_admin_cannot_create_cross_tenant_index = ($crossTenantCreate.StatusCode -eq 403)
  tenant_admin_create_index_created = ($createResponse.Json.status -eq "created")
  analyst_list_indexes_ok = ($listResponse.Json.status -eq "ok")
  analyst_list_contains_tenant_index = (($listResponse.Content -match [regex]::Escape($indexName)))
  analyst_lookup_ok = ($lookupResponse.Json.status -eq "ok")
  analyst_lookup_targets_created_index = ($lookupResponse.Json.index_name -eq $indexName)
  tenant_admin_add_constraint_created = ($constraintResponse.Json.status -eq "created")
  analyst_validate_constraint_ok = ($validateResponse.Json.status -eq "ok")
  analyst_validate_constraint_valid = ($validateResponse.Json.valid -eq $true)
  tenant_admin_drop_index_ok = ($dropResponse.Json.status -eq "dropped")
}
$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  smoke = "ws2-tenant-store-runtime"
  status = $status
  base_url = $BaseUrl
  tenant_id = $TenantId
  admin_user_id = $AdminUserId
  analyst_user_id = $AnalystUserId
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  table_name = $tableName
  index_name = $indexName
  constraint_name = $constraintName
  checks = $checks
  list_response = $listResponse.Json
  lookup_response = $lookupResponse.Json
  validate_response = $validateResponse.Json
  drop_response = $dropResponse.Json
}

$artifact | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS2 tenant store runtime smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
