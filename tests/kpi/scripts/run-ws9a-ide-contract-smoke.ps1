param(
  [string]$OutputPath = "tests/kpi/results/ws9a/ide-contract-smoke.json"
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

$commonPath = "ui/ide-extensions/contracts/common-api-contract.json"
$manifestPaths = @(
  "ui/ide-extensions/contracts/visual-studio.manifest.json",
  "ui/ide-extensions/contracts/cursor.manifest.json",
  "ui/ide-extensions/contracts/antigravity.manifest.json",
  "ui/ide-extensions/contracts/jetbrains.manifest.json",
  "ui/ide-extensions/contracts/eclipse.manifest.json"
)

Add-Check -Name "common_contract_exists" -Ok (Test-Path $commonPath) -Detail $commonPath
foreach ($manifest in $manifestPaths) {
  Add-Check -Name ("manifest_exists_" + [IO.Path]::GetFileNameWithoutExtension($manifest)) -Ok (Test-Path $manifest) -Detail $manifest
}

$schemaOk = $false
if (Test-Path $commonPath) {
  $common = Get-Content -Raw -Path $commonPath | ConvertFrom-Json
  $paths = @($common.endpoints | ForEach-Object { $_.path })
  $schemaOk = (
    $paths -contains "/api/v1/sql/execute" -and
    $paths -contains "/api/v1/autonomous/actions/authorize" -and
    $paths -contains "/api/v1/audit/events" -and
    $paths -contains "/api/v1/i18n/messages"
  )
}
Add-Check -Name "common_contract_endpoints" -Ok $schemaOk -Detail "Core runtime endpoints in shared IDE contract"

$status = if (($checks | Where-Object { -not $_.ok }).Count -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws9a-ide-extension-contract"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS9A IDE contract smoke failed."
  exit 1
}

Write-Host "WS9A IDE contract smoke passed. Artifact: $OutputPath"
