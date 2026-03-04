param(
  [Parameter(Mandatory = $true)][string]$BaseUrl,
  [Parameter(Mandatory = $true)][string]$SqlUrl,
  [Parameter(Mandatory = $true)][string]$OutputDir,
  [string]$TargetsPath = "",
  [ValidateSet("none", "bearer", "apiKey")][string]$AuthMode = "none",
  [string]$AuthToken = "",
  [string]$ApiKeyHeaderName = "X-API-Key",
  [int]$RequestTimeoutSec = 10,
  [string]$ExtraHeadersJson = "",
  [string]$ProfileName = "default"
)

$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$scenariosDir = Join-Path (Split-Path -Parent $scriptRoot) "scenarios"
$runner = Join-Path $scriptRoot "run-scenario.ps1"
if ([string]::IsNullOrWhiteSpace($TargetsPath)) {
  $TargetsPath = Join-Path (Join-Path (Split-Path -Parent $scriptRoot) "config") "targets.yaml"
}

if (!(Test-Path $OutputDir)) {
  New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
}

$scenarioFiles = Get-ChildItem -Path $scenariosDir -Filter "*.yaml" | Sort-Object Name
if ($scenarioFiles.Count -eq 0) {
  throw "No scenario files found in $scenariosDir"
}

$extraHeaders = @{}
if (-not [string]::IsNullOrWhiteSpace($ExtraHeadersJson)) {
  $rawHeaders = $ExtraHeadersJson | ConvertFrom-Json
  if ($rawHeaders) {
    foreach ($prop in $rawHeaders.PSObject.Properties) {
      $extraHeaders[$prop.Name] = [string]$prop.Value
    }
  }
}

foreach ($scenario in $scenarioFiles) {
  & $runner `
    -ScenarioPath $scenario.FullName `
    -BaseUrl $BaseUrl `
    -SqlUrl $SqlUrl `
    -OutputDir $OutputDir `
    -TargetsPath $TargetsPath `
    -AuthMode $AuthMode `
    -AuthToken $AuthToken `
    -ApiKeyHeaderName $ApiKeyHeaderName `
    -RequestTimeoutSec $RequestTimeoutSec `
    -ExtraHeaders $extraHeaders `
    -ProfileName $ProfileName
}

$resultFiles = Get-ChildItem -Path $OutputDir -Filter "*-result.json" | Sort-Object Name
$parsedResults = @()
foreach ($resultFile in $resultFiles) {
  $parsedResults += Get-Content -Raw -Path $resultFile.FullName | ConvertFrom-Json
}

$passed = @($parsedResults | Where-Object { $_.status -eq "passed" }).Count
$failed = @($parsedResults | Where-Object { $_.status -eq "failed" }).Count
$rollup = @{
  profile = $ProfileName
  base_url = $BaseUrl
  sql_url = $SqlUrl
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  total_scenarios = $parsedResults.Count
  passed = $passed
  failed = $failed
  status = if ($failed -eq 0) { "passed" } else { "failed" }
  scenarios = @($parsedResults | ForEach-Object {
      @{
        scenario = $_.scenario
        status = $_.status
      }
    })
}

$rollupPath = Join-Path $OutputDir "rollup-summary.json"
$rollup | ConvertTo-Json -Depth 8 | Out-File -FilePath $rollupPath -Encoding utf8
Write-Host "KPI harness run completed. Rollup: $rollupPath"
