param(
  [string]$OutputPath = "tests/kpi/results/ws1/ws1-gate-summary.json",
  [string]$ReleaseSummaryOutputPath = "tests/kpi/results/gates/ws1-release-readiness.json",
  [string]$BaseUrl = "http://127.0.0.1:8080"
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

function Get-OutputText {
  param([object[]]$Lines)
  return ((@($Lines) | ForEach-Object { "$_" }) -join "`n")
}

function Invoke-CargoPack {
  param(
    [string[]]$Arguments,
    [ValidateSet("cargo-test", "cargo-check")]
    [string]$Mode,
    [string]$DefaultDetail
  )

    $tempFile = [System.IO.Path]::GetTempFileName()
    try {
      $commandText = "cargo " + (($Arguments | ForEach-Object {
        if ($_ -match "\s") { '"' + $_ + '"' } else { $_ }
      }) -join " ")
      $redirectedCommand = "$commandText > `"$tempFile`" 2>&1"
      $process = Start-Process -FilePath "cmd.exe" -ArgumentList "/c", $redirectedCommand -Wait -PassThru -NoNewWindow
      $text = if (Test-Path -Path $tempFile) { Get-Content -Path $tempFile -Raw } else { "" }
    } finally {
      if (Test-Path -Path $tempFile) {
        Remove-Item -Path $tempFile -Force -ErrorAction SilentlyContinue
      }
    }
  $ok = $false

  if ($Mode -eq "cargo-test") {
    $ok = ($text -match "test result: ok\." -and $text -notmatch "test result: FAILED" -and $text -notmatch "(?m)^error:")
  } else {
    $ok = ($text -match "Finished" -and $text -notmatch "(?m)^error:")
  }

  $detail = if ($ok) { $DefaultDetail } else { (($text -split "`n") | Select-Object -First 5) -join "`n" }
  return [pscustomobject]@{
    Ok = $ok
    Detail = $detail
  }
}

function Invoke-ScriptPack {
  param(
    [scriptblock]$Runner,
    [string]$DefaultDetail
  )

  $global:LASTEXITCODE = 0
  try {
    & $Runner
    $ok = ($? -and $global:LASTEXITCODE -eq 0)
    return [pscustomobject]@{
      Ok = $ok
      Detail = if ($ok) { $DefaultDetail } else { $DefaultDetail }
    }
  } catch {
    return [pscustomobject]@{
      Ok = $false
      Detail = $_.Exception.Message
    }
  }
}

$packs = @(
  @{ Name = "ws1-sql-core-tests"; Type = "cargo-test"; Detail = "cargo test -p voltnuerongrid-sql"; Arguments = @("test", "-p", "voltnuerongrid-sql") },
  @{ Name = "ws1-udf-contract-smoke"; Type = "script"; Detail = "run-ws1-udf-contract-smoke.ps1"; Runner = { & "tests/kpi/scripts/run-ws1-udf-contract-smoke.ps1" -OutputPath "tests/kpi/results/ws1/ws1-udf-contract-smoke.json" } },
  @{ Name = "ws1-udf-runtime-scaffold-tests"; Type = "cargo-test"; Detail = "cargo test -p voltnuerongridd ws1_udf_ -- --nocapture"; Arguments = @("test", "-p", "voltnuerongridd", "ws1_udf_", "--", "--nocapture") },
  @{ Name = "ws1-runtime-sql-check"; Type = "cargo-check"; Detail = "cargo check -p voltnuerongridd"; Arguments = @("check", "-p", "voltnuerongridd") },
  @{ Name = "ws1-sql-analyze-runtime-smoke"; Type = "script"; Detail = "run-sql-analyze-smoke.ps1"; Runner = { & "tests/kpi/scripts/run-sql-analyze-smoke.ps1" -BaseUrl $BaseUrl -OutputPath "tests/kpi/results/20260305-ws1/sql-analyze-smoke.json" } },
  @{ Name = "ws1-sql-route-runtime-smoke"; Type = "script"; Detail = "run-sql-route-smoke.ps1"; Runner = { & "tests/kpi/scripts/run-sql-route-smoke.ps1" -BaseUrl $BaseUrl -OutputPath "tests/kpi/results/ws1/sql-route-smoke.json" } },
  @{ Name = "ws1-sql-execute-runtime-smoke"; Type = "script"; Detail = "run-sql-execute-udf-smoke.ps1"; Runner = { & "tests/kpi/scripts/run-sql-execute-udf-smoke.ps1" -BaseUrl $BaseUrl -OutputPath "tests/kpi/results/ws1/sql-execute-udf-smoke.json" } },
  @{ Name = "ws1-sql-transaction-runtime-smoke"; Type = "script"; Detail = "run-sql-transaction-smoke.ps1"; Runner = { & "tests/kpi/scripts/run-sql-transaction-smoke.ps1" -BaseUrl $BaseUrl -OutputPath "tests/kpi/results/ws1/sql-transaction-smoke.json" } }
)

foreach ($pack in $packs) {
  try {
    if ($pack.Type -eq "cargo-test" -or $pack.Type -eq "cargo-check") {
      $result = Invoke-CargoPack -Arguments $pack.Arguments -Mode $pack.Type -DefaultDetail $pack.Detail
    } else {
      $result = Invoke-ScriptPack -Runner $pack.Runner -DefaultDetail $pack.Detail
    }
    Add-Run -Name $pack.Name -Ok $result.Ok -Detail $result.Detail
  } catch {
    Add-Run -Name $pack.Name -Ok $false -Detail $_.Exception.Message
  }
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
