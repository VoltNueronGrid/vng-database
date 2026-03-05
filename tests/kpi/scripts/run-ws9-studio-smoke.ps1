param(
  [string]$OutputPath = "tests/kpi/results/ws9/studio-smoke.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

Ensure-OutputDir -PathValue $OutputPath

$start = Get-Date
$checks = @()
$outputLines = @()
$command = "node ui/voltnuerongrid-studio/scripts/check-contracts.mjs + static Studio API contract checks"
$exitCode = 1

function Add-Check {
  param([string]$Name, [bool]$Ok, [string]$Detail)
  $script:checks += [ordered]@{
    check = $Name
    ok = $Ok
    detail = $Detail
  }
}

$typesPath = "ui/voltnuerongrid-studio/src/api/types.ts"
$clientPath = "ui/voltnuerongrid-studio/src/api/client.ts"
$scriptPath = "ui/voltnuerongrid-studio/scripts/check-contracts.mjs"

Add-Check -Name "types_file_exists" -Ok (Test-Path $typesPath) -Detail $typesPath
Add-Check -Name "client_file_exists" -Ok (Test-Path $clientPath) -Detail $clientPath
Add-Check -Name "contract_script_exists" -Ok (Test-Path $scriptPath) -Detail $scriptPath
$typesContent = ""
$clientContent = ""

try {
  if (Test-Path $scriptPath) {
    $scriptOutput = & node $scriptPath 2>&1
    $scriptExit = $LASTEXITCODE
    $outputLines += $scriptOutput
    Add-Check -Name "contract_script_executes" -Ok ($scriptExit -eq 0) -Detail "check-contracts.mjs exit_code=$scriptExit"
  } else {
    Add-Check -Name "contract_script_executes" -Ok $false -Detail "script missing"
  }

  if (Test-Path $typesPath) {
    $typesContent = Get-Content -Raw -Path $typesPath
    Add-Check -Name "types_has_trace_id" -Ok ($typesContent -match "trace_id") -Detail "trace_id in UI types"
    Add-Check -Name "types_has_sql_execute_response" -Ok ($typesContent -match "interface\s+SqlExecuteResponse") -Detail "SqlExecuteResponse contract exists"
    Add-Check -Name "types_has_route_path_union" -Ok ($typesContent -match 'type\s+RoutePath\s*=\s*"oltp"\s*\|\s*"olap"\s*\|\s*"hybrid"') -Detail "RoutePath includes oltp/olap/hybrid"
  }
  if (Test-Path $clientPath) {
    $clientContent = Get-Content -Raw -Path $clientPath
    Add-Check -Name "client_has_sql_execute_endpoint" -Ok ($clientContent -match "/api/v1/sql/execute") -Detail "sql execute endpoint"
    Add-Check -Name "client_has_authorize_endpoint" -Ok ($clientContent -match "/api/v1/autonomous/actions/authorize") -Detail "authorize endpoint"
    Add-Check -Name "client_has_audit_events_endpoint" -Ok ($clientContent -match "/api/v1/audit/events") -Detail "audit events endpoint"
    Add-Check -Name "client_has_action_records_endpoint" -Ok ($clientContent -match "/api/v1/autonomous/actions/records") -Detail "action records endpoint"
    Add-Check -Name "client_sets_admin_header" -Ok ($clientContent -match "x-vng-admin-key") -Detail "admin header wiring"
    Add-Check -Name "client_sets_operator_header" -Ok ($clientContent -match "x-vng-operator-id") -Detail "operator header wiring"
    Add-Check -Name "client_sets_session_header" -Ok ($clientContent -match "x-vng-session-id") -Detail "session header wiring"
  }

  $status = if (($checks | Where-Object { -not $_.ok }).Count -eq 0) { "passed" } else { "failed" }
  $exitCode = if ($status -eq "passed") { 0 } else { 1 }
} catch {
  $outputLines += $_.Exception.Message
  $status = "failed"
  $exitCode = 1
}
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws9-studio-api-contract"
  status = $status
  command = $command
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS9 studio smoke failed."
  exit 1
}

Write-Host "WS9 studio smoke passed. Artifact: $OutputPath"
