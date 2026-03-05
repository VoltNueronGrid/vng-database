param(
  [string]$OutputPath = "tests/kpi/results/ws15/competitive-parity-smoke.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Add-Check {
  param([string]$Name, [bool]$Ok, [string]$Detail)
  $script:checks += [ordered]@{
    check = $Name
    ok = $Ok
    detail = $Detail
  }
}

Ensure-OutputDir -PathValue $OutputPath
$start = Get-Date
$checks = @()

$matrixPath = "reference/competitive/ws15-feature-adoption-matrix.json"
Add-Check -Name "matrix_exists" -Ok (Test-Path $matrixPath) -Detail $matrixPath

if (Test-Path $matrixPath) {
  try {
    $matrix = Get-Content -Raw -Path $matrixPath | ConvertFrom-Json

    $requiredCompetitors = @("mysql","postgresql","cockroachdb","oracle","neo4j","pinecone")
    foreach ($c in $requiredCompetitors) {
      Add-Check -Name ("competitor_" + $c) -Ok (@($matrix.competitors) -contains $c) -Detail "competitor listed"
    }

    $featureCount = @($matrix.features).Count
    Add-Check -Name "minimum_feature_count" -Ok ($featureCount -ge 8) -Detail "features=$featureCount expected>=8"

    $allHaveRelease = @($matrix.features | Where-Object { [string]::IsNullOrWhiteSpace($_.target_release) }).Count -eq 0
    Add-Check -Name "all_features_have_target_release" -Ok $allHaveRelease -Detail "target_release required"

    $allHaveWsLink = @($matrix.features | Where-Object { [string]::IsNullOrWhiteSpace($_.ws_epic_link) }).Count -eq 0
    Add-Check -Name "all_features_have_ws_link" -Ok $allHaveWsLink -Detail "ws_epic_link required"

    $allPlannedOrBetter = @($matrix.features | Where-Object { $_.status -notin @("planned_stub","in_progress","implemented") }).Count -eq 0
    Add-Check -Name "feature_status_values_supported" -Ok $allPlannedOrBetter -Detail "status domain check"
  } catch {
    Add-Check -Name "matrix_json_parse" -Ok $false -Detail $_.Exception.Message
  }
}

$status = if (($checks | Where-Object { -not $_.ok }).Count -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws15-competitive-parity-baseline"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS15 competitive parity smoke failed."
  exit 1
}

Write-Host "WS15 competitive parity smoke passed. Artifact: $OutputPath"
