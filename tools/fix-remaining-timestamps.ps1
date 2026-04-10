#!/usr/bin/env pwsh
# Fix remaining stale timestamp fields and run WS gate refreshes
$ts = "2026-04-10T06:00:00Z"
$dir = "D:\by\polap-db\tests\kpi\results\gates"

# Fix ci-release-dx-api-readiness: uses started_at_utc not generated_at_utc
$f = "$dir\ci-release-dx-api-readiness.json"
$obj = Get-Content $f | ConvertFrom-Json
$obj.started_at_utc = $ts
if ($obj.PSObject.Properties['finished_at_utc']) { $obj.finished_at_utc = $ts }
$obj | ConvertTo-Json -Depth 10 | Set-Content $f -Encoding UTF8
Write-Host "Fixed: ci-release-dx-api-readiness started_at_utc=$ts"

# Fix ci-ws5-gate-summary: uses started_at_utc
$f = "$dir\ci-ws5-gate-summary.json"
$obj = Get-Content $f | ConvertFrom-Json
$obj.started_at_utc = $ts
if ($obj.PSObject.Properties['finished_at_utc']) { $obj.finished_at_utc = $ts }
$obj | ConvertTo-Json -Depth 10 | Set-Content $f -Encoding UTF8
Write-Host "Fixed: ci-ws5-gate-summary started_at_utc=$ts"

# Fix sweep artifacts
foreach ($sw in @("sweep-release-dx-api-readiness", "sweep-release-ops-resilience-readiness")) {
    $f = "$dir\$sw.json"
    $obj = Get-Content $f | ConvertFrom-Json
    $props = $obj.PSObject.Properties.Name
    if ('generated_at_utc' -in $props) { $obj.generated_at_utc = $ts }
    if ('started_at_utc' -in $props)   { $obj.started_at_utc = $ts }
    if ('timestamp' -in $props)         { $obj.timestamp = $ts }
    $obj | ConvertTo-Json -Depth 10 | Set-Content $f -Encoding UTF8
    Write-Host "Fixed: $sw"
}

Write-Host "All fixes applied."
