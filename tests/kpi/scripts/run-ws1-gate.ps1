param(
  [string]$OutputPath = "tests/kpi/results/ws1/ws1-gate-summary.json",
  [string]$ReleaseSummaryOutputPath = "tests/kpi/results/gates/ws1-release-readiness.json"
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
Ensure-OutputDir -PathValue $ReleaseSummaryOutputPath
$priorSummaryPath = "tests/kpi/results/ws1/ws1-gate-summary.previous.json"
if (Test-Path -Path $OutputPath) { Copy-Item -Path $OutputPath -Destination $priorSummaryPath -Force }

$start = Get-Date
$runs = @()
$status = "passed"

function Add-Run {
  param([string]$Name, [bool]$Ok, [string]$Detail)
  $script:runs += [ordered]@{
    pack = $Name
    status = if ($Ok) { "passed" } else { "failed" }
    detail = $Detail
  }
  if (-not $Ok) {
    $script:status = "failed"
  }
}

try {
  $global:LASTEXITCODE = 0
  & cargo test -p voltnuerongrid-sql 2>&1 | Out-Null
  Add-Run -Name "ws1-sql-core-tests" -Ok ($? -and $global:LASTEXITCODE -eq 0) -Detail "cargo test -p voltnuerongrid-sql"

  $global:LASTEXITCODE = 0
  & "tests/kpi/scripts/run-ws1-udf-contract-smoke.ps1" -OutputPath "tests/kpi/results/ws1/ws1-udf-contract-smoke.json" 2>&1 | Out-Null
  Add-Run -Name "ws1-udf-contract-smoke" -Ok ($? -and $global:LASTEXITCODE -eq 0) -Detail "run-ws1-udf-contract-smoke.ps1"

  $global:LASTEXITCODE = 0
  & cargo test -p voltnuerongridd ws1_udf_ -- --nocapture 2>&1 | Out-Null
  Add-Run -Name "ws1-udf-runtime-scaffold-tests" -Ok ($? -and $global:LASTEXITCODE -eq 0) -Detail "cargo test -p voltnuerongridd ws1_udf_ -- --nocapture"

  $global:LASTEXITCODE = 0
  & cargo check -p voltnuerongridd 2>&1 | Out-Null
  Add-Run -Name "ws1-runtime-sql-check" -Ok ($? -and $global:LASTEXITCODE -eq 0) -Detail "cargo check -p voltnuerongridd"
} catch {
  Add-Run -Name "ws1-gate-exception" -Ok $false -Detail $_.Exception.Message
}

$finished = Get-Date
$summary = [ordered]@{
  gate = "ws1"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

$postArtifacts = @(
  @{
    Name = "ws1-udf-coverage-matrix"
    Script = "tests/kpi/scripts/run-ws1-udf-coverage-export.ps1"
    Runner = { & "tests/kpi/scripts/run-ws1-udf-coverage-export.ps1" -SummaryPath $OutputPath -UdfSmokePath "tests/kpi/results/ws1/ws1-udf-contract-smoke.json" -RuntimeSmokePath "tests/kpi/results/ws1/sql-execute-udf-smoke.json" -OutputPath "tests/kpi/results/ws1/ws1-udf-coverage-matrix.json" }
  },
  @{
    Name = "ws1-gate-trend-comparison"
    Script = "tests/kpi/scripts/run-ws1-gate-trend-compare.ps1"
    Runner = { & "tests/kpi/scripts/run-ws1-gate-trend-compare.ps1" -CurrentSummaryPath $OutputPath -PriorSummaryPath $priorSummaryPath -OutputPath "tests/kpi/results/ws1/ws1-gate-trend-comparison.json" }
  },
  @{
    Name = "ws1-udf-stability-badge"
    Script = "tests/kpi/scripts/run-ws1-udf-stability-badge.ps1"
    Runner = { & "tests/kpi/scripts/run-ws1-udf-stability-badge.ps1" -SummaryPath $OutputPath -TrendPath "tests/kpi/results/ws1/ws1-gate-trend-comparison.json" -OutputPath "tests/kpi/results/ws1/ws1-udf-stability-badge.json" }
  },
  @{
    Name = "ws1-release-summary"
    Script = "tests/kpi/scripts/run-ws1-release-summary.ps1"
    Runner = { & "tests/kpi/scripts/run-ws1-release-summary.ps1" -SummaryPath $OutputPath -CoverageMatrixPath "tests/kpi/results/ws1/ws1-udf-coverage-matrix.json" -TrendPath "tests/kpi/results/ws1/ws1-gate-trend-comparison.json" -BadgePath "tests/kpi/results/ws1/ws1-udf-stability-badge.json" -OutputPath $ReleaseSummaryOutputPath }
  }
)

foreach ($artifact in $postArtifacts) {
  $global:LASTEXITCODE = 0
  & $artifact.Runner 2>&1 | Out-Null
  Add-Run -Name $artifact.Name -Ok ($? -and $global:LASTEXITCODE -eq 0) -Detail $artifact.Script
}

$summary.status = $status
$summary.packs = $runs
$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "WS1 gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
