param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$OutputPath = "tests/kpi/results/ws8/tenant-autonomous-runtime-smoke.json",
  [string]$TenantId = "acme",
  [string]$TenantUserId = "analyst-acme",
  [string]$OtherTenantId = "globex",
  [string]$OtherTenantUserId = "analyst-globex",
  [string]$OperatorId = "platform-admin",
  [string]$AdminApiKey = ""
)

$ErrorActionPreference = "Stop"

$kpiScriptsRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $kpiScriptsRoot "kpi-http-helpers.ps1")

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

if ([string]::IsNullOrWhiteSpace($AdminApiKey)) {
  $AdminApiKey = $env:VNG_ADMIN_API_KEY
}

$operatorHeaders = @{
  "x-vng-operator-id" = $OperatorId
}
if (-not [string]::IsNullOrWhiteSpace($AdminApiKey)) {
  $operatorHeaders["x-vng-admin-key"] = $AdminApiKey
}

$tenantHeaders = @{
  "x-vng-tenant-id" = $TenantId
  "x-vng-user-id" = $TenantUserId
}

$otherTenantHeaders = @{
  "x-vng-tenant-id" = $OtherTenantId
  "x-vng-user-id" = $OtherTenantUserId
}

$tenantScope = "tenants/$TenantId/autonomous/records"
$otherTenantScope = "tenants/$OtherTenantId/autonomous/records"

$unauthorizedRecords = Invoke-HttpJson -Method Get -Uri "$BaseUrl/api/v1/autonomous/actions/records?max_items=10"

$tenantAuthorize = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/autonomous/actions/authorize" -Headers $operatorHeaders -Body @{
  action = "performance_tune"
  scope = $tenantScope
}

$otherTenantAuthorize = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/autonomous/actions/authorize" -Headers $operatorHeaders -Body @{
  action = "performance_tune"
  scope = $otherTenantScope
}

$operatorRecords = Invoke-HttpJson -Method Get -Uri "$BaseUrl/api/v1/autonomous/actions/records?max_items=10" -Headers $operatorHeaders
$tenantRecords = Invoke-HttpJson -Method Get -Uri "$BaseUrl/api/v1/autonomous/actions/records?max_items=10" -Headers $tenantHeaders
$otherTenantRecords = Invoke-HttpJson -Method Get -Uri "$BaseUrl/api/v1/autonomous/actions/records?max_items=10" -Headers $otherTenantHeaders

$tenantTraceId = if ($tenantAuthorize.Json) { [string]$tenantAuthorize.Json.trace_id } else { "" }
$otherTenantTraceId = if ($otherTenantAuthorize.Json) { [string]$otherTenantAuthorize.Json.trace_id } else { "" }

$tenantRecordTenantIds = @($tenantRecords.Json.records | ForEach-Object { $_.tenant_id })
$otherTenantRecordTenantIds = @($otherTenantRecords.Json.records | ForEach-Object { $_.tenant_id })

$checks = [ordered]@{
  records_require_auth_headers = ($unauthorizedRecords.StatusCode -eq 401)
  tenant_authorize_request_allowed = ($tenantAuthorize.StatusCode -eq 200 -and $tenantAuthorize.Json.decision -eq "allow")
  other_tenant_authorize_request_allowed = ($otherTenantAuthorize.StatusCode -eq 200 -and $otherTenantAuthorize.Json.decision -eq "allow")
  operator_records_status_ok = ($operatorRecords.Json.status -eq "ok")
  operator_records_include_both_trace_ids = (($operatorRecords.Content -match [regex]::Escape($tenantTraceId)) -and ($operatorRecords.Content -match [regex]::Escape($otherTenantTraceId)))
  tenant_records_status_ok = ($tenantRecords.Json.status -eq "ok")
  tenant_records_include_own_trace = ($tenantRecords.Content -match [regex]::Escape($tenantTraceId))
  tenant_records_exclude_other_trace = (-not ($tenantRecords.Content -match [regex]::Escape($otherTenantTraceId)))
  tenant_records_only_include_own_tenant_id = ((@($tenantRecordTenantIds | Where-Object { $_ -and $_ -ne $TenantId }).Count) -eq 0 -and (@($tenantRecordTenantIds | Where-Object { $_ -eq $TenantId }).Count) -ge 1)
  other_tenant_records_status_ok = ($otherTenantRecords.Json.status -eq "ok")
  other_tenant_records_include_own_trace = ($otherTenantRecords.Content -match [regex]::Escape($otherTenantTraceId))
  other_tenant_records_exclude_first_tenant_trace = (-not ($otherTenantRecords.Content -match [regex]::Escape($tenantTraceId)))
  other_tenant_records_only_include_own_tenant_id = ((@($otherTenantRecordTenantIds | Where-Object { $_ -and $_ -ne $OtherTenantId }).Count) -eq 0 -and (@($otherTenantRecordTenantIds | Where-Object { $_ -eq $OtherTenantId }).Count) -ge 1)
}
$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  smoke = "ws8-tenant-autonomous-runtime"
  status = $status
  base_url = $BaseUrl
  operator_id = $OperatorId
  tenant_id = $TenantId
  tenant_user_id = $TenantUserId
  other_tenant_id = $OtherTenantId
  other_tenant_user_id = $OtherTenantUserId
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  authorize_responses = [ordered]@{
    tenant = $tenantAuthorize.Json
    other_tenant = $otherTenantAuthorize.Json
  }
  operator_records = $operatorRecords.Json
  tenant_records = $tenantRecords.Json
  other_tenant_records = $otherTenantRecords.Json
}

$artifact | ConvertTo-Json -Depth 12 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS8 tenant autonomous runtime smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }