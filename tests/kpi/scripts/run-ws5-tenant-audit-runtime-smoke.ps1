param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$OutputPath = "tests/kpi/results/ws5/tenant-audit-runtime-smoke.json",
  [string]$TenantId = "acme",
  [string]$AdminUserId = "admin-acme",
  [string]$TenantUserId = "analyst-acme",
  [string]$OtherTenantId = "globex",
  [string]$OtherTenantUserId = "analyst-globex"
)

$ErrorActionPreference = "Stop"

$kpiScriptsRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $kpiScriptsRoot "kpi-http-helpers.ps1")

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$suffix = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()

$adminHeaders = @{
  "x-vng-tenant-id" = $TenantId
  "x-vng-user-id" = $AdminUserId
}
$tenantHeaders = @{
  "x-vng-tenant-id" = $TenantId
  "x-vng-user-id" = $TenantUserId
}
$otherTenantHeaders = @{
  "x-vng-tenant-id" = $OtherTenantId
  "x-vng-user-id" = $OtherTenantUserId
}

$unauthorizedAudit = Invoke-HttpJson -Method Get -Uri "$BaseUrl/api/v1/audit/events?max_items=10"

$routeRequest = @{
  sql_batch = "BEGIN; SELECT region, SUM(amount) FROM orders GROUP BY region;"
}

$storeRequest = @{
  name = "idx_${TenantId}_audit_$suffix"
  table = "tenant/$TenantId/orders"
  column = "customer_id"
  unique = $false
}

$ingestRequest = @{
  connector_id = "orders-csv-$suffix"
  csv_data = "id,amount`n1,42`n"
}

$tenantRoute = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/sql/route" -Headers $tenantHeaders -Body $routeRequest
$tenantStore = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/store/indexes/create" -Headers $adminHeaders -Body $storeRequest
$tenantIngest = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/ingest/csv" -Headers $adminHeaders -Body $ingestRequest
$otherTenantRoute = Invoke-HttpJson -Method Post -Uri "$BaseUrl/api/v1/sql/route" -Headers $otherTenantHeaders -Body $routeRequest
$tenantAudit = Invoke-HttpJson -Method Get -Uri "$BaseUrl/api/v1/audit/events?max_items=20" -Headers $tenantHeaders

$parsedEvents = @()
foreach ($event in @($tenantAudit.Json.events)) {
  $details = $null
  if ($event.details_json) {
    try {
      $details = $event.details_json | ConvertFrom-Json
    } catch {
      $details = $null
    }
  }
  $parsedEvents += [pscustomobject]@{
    actor = $event.actor
    action = $event.action
    kind = $event.kind
    details = $details
  }
}

$tenantEventTenantIds = @($parsedEvents | ForEach-Object { if ($_.details) { $_.details.tenant_id } })
$tenantEventActions = @($parsedEvents | ForEach-Object { $_.action })

$checks = [ordered]@{
  audit_requires_user_headers = ($unauthorizedAudit.StatusCode -eq 401)
  tenant_route_generates_event = ($tenantRoute.Json.status -eq "ok")
  tenant_store_generates_event = ($tenantStore.Json.status -eq "created")
  tenant_ingest_generates_event = ($tenantIngest.Json.status -eq "ok")
  other_tenant_route_generates_event = ($otherTenantRoute.Json.status -eq "ok")
  tenant_audit_status_ok = ($tenantAudit.Json.status -eq "ok")
  tenant_audit_has_events = (@($tenantAudit.Json.events).Count -ge 1)
  tenant_audit_contains_tenant_actor = ($tenantAudit.Content -match [regex]::Escape($TenantUserId))
  tenant_audit_contains_tenant_id = ($tenantEventTenantIds -contains $TenantId)
  tenant_audit_contains_storage_action = ($tenantEventActions -contains "store_create_index")
  tenant_audit_contains_ingest_action = ($tenantEventActions -contains "ingest_csv")
  tenant_audit_excludes_other_tenant_actor = (-not ($tenantAudit.Content -match [regex]::Escape($OtherTenantUserId)))
  tenant_audit_excludes_other_tenant_id = (-not ($tenantEventTenantIds -contains $OtherTenantId))
}
$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  smoke = "ws5-tenant-audit-runtime"
  status = $status
  base_url = $BaseUrl
  tenant_id = $TenantId
  admin_user_id = $AdminUserId
  tenant_user_id = $TenantUserId
  other_tenant_id = $OtherTenantId
  other_tenant_user_id = $OtherTenantUserId
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  generated_actions = [ordered]@{
    sql_route = $tenantRoute.Json
    store_create_index = $tenantStore.Json
    ingest_csv = $tenantIngest.Json
  }
  audit_response = $tenantAudit.Json
}

$artifact | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS5 tenant audit runtime smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }