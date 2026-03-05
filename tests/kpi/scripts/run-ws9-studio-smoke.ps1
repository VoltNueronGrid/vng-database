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

if (Test-Path $typesPath) {
  $typesContent = Get-Content -Raw -Path $typesPath
  Add-Check -Name "types_has_trace_id" -Ok ($typesContent -match "trace_id") -Detail "trace_id in UI types"
}
if (Test-Path $clientPath) {
  $clientContent = Get-Content -Raw -Path $clientPath
  Add-Check -Name "client_has_sql_execute_endpoint" -Ok ($clientContent -match "/api/v1/sql/execute") -Detail "sql execute endpoint"
  Add-Check -Name "client_has_action_records_endpoint" -Ok ($clientContent -match "/api/v1/autonomous/actions/records") -Detail "action records endpoint"
}

$status = if (($checks | Where-Object { -not $_.ok }).Count -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws9-studio-api-contract"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS9 studio smoke failed."
  exit 1
}

Write-Host "WS9 studio smoke passed. Artifact: $OutputPath"
