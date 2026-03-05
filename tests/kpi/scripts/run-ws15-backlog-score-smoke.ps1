param(
  [string]$OutputPath = "tests/kpi/results/ws15/backlog-score-smoke.json"
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
$backlogPath = "reference/competitive/ws15-implementation-backlog.json"

Add-Check -Name "matrix_exists" -Ok (Test-Path $matrixPath) -Detail $matrixPath
Add-Check -Name "backlog_exists" -Ok (Test-Path $backlogPath) -Detail $backlogPath

if ((Test-Path $matrixPath) -and (Test-Path $backlogPath)) {
  try {
    $matrix = Get-Content -Raw -Path $matrixPath | ConvertFrom-Json
    $backlog = Get-Content -Raw -Path $backlogPath | ConvertFrom-Json
    $matrixIds = @($matrix.features | ForEach-Object { $_.id })
    $backlogIds = @($backlog.items | ForEach-Object { $_.feature_id })

    $allMapped = @($matrixIds | Where-Object { $backlogIds -contains $_ }).Count -eq $matrixIds.Count
    Add-Check -Name "all_matrix_features_scored" -Ok $allMapped -Detail "all matrix ids mapped in backlog"

    $missingIds = @($backlogIds | Where-Object { $matrixIds -notcontains $_ })
    Add-Check -Name "no_unknown_backlog_features" -Ok ($missingIds.Count -eq 0) -Detail ("unknown_feature_ids=" + ($missingIds -join ","))

    $scoreDomainOk = @($backlog.items | Where-Object { $_.impact_score -lt 1 -or $_.impact_score -gt 5 -or $_.effort_score -lt 1 -or $_.effort_score -gt 5 }).Count -eq 0
    Add-Check -Name "score_domain_valid" -Ok $scoreDomainOk -Detail "impact/effort scores within 1..5"

    $formulaOk = $true
    foreach ($item in $backlog.items) {
      $expected = [math]::Round((([double]$item.impact_score * 2) - [double]$item.effort_score), 2)
      if ([double]$item.priority_score -ne $expected) {
        $formulaOk = $false
        break
      }
    }
    Add-Check -Name "priority_formula_consistent" -Ok $formulaOk -Detail "priority_score follows formula"

    $allHaveOwner = @($backlog.items | Where-Object { [string]::IsNullOrWhiteSpace($_.owner_team) }).Count -eq 0
    Add-Check -Name "all_items_have_owner" -Ok $allHaveOwner -Detail "owner_team required"
  } catch {
    Add-Check -Name "backlog_parse_or_validation" -Ok $false -Detail $_.Exception.Message
  }
}

$status = if (($checks | Where-Object { -not $_.ok }).Count -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws15-backlog-score-validation"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS15 backlog score smoke failed."
  exit 1
}

Write-Host "WS15 backlog score smoke passed. Artifact: $OutputPath"
