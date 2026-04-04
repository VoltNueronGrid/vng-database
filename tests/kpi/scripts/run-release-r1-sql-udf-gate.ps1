param(
  [string]$OutputPath = "tests/kpi/results/gates/release-r1-sql-udf-readiness.json"
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
$runs = @()
$status = "passed"

$packs = @(
  @{
    Name = "r1-baseline-checklist"
    Script = "tests/kpi/scripts/check-r1-gate.ps1"
    Artifact = "tests/kpi/results/gates/r1-gate-check.json"
    Runner = { & "tests/kpi/scripts/check-r1-gate.ps1" -RepoRoot "." -OutputPath "tests/kpi/results/gates/r1-gate-check.json" }
  },
  @{
    Name = "ws1-closure-gate"
    Script = "tests/kpi/scripts/run-ws1-closure-gate.ps1"
    Artifact = "tests/kpi/results/ws1/ws1-closure-gate-summary.json"
    Runner = { & "tests/kpi/scripts/run-ws1-closure-gate.ps1" -OutputPath "tests/kpi/results/ws1/ws1-closure-gate-summary.json" }
  },
  @{
    Name = "ws22-closure-gate"
    Script = "tests/kpi/scripts/run-ws22-closure-gate.ps1"
    Artifact = "tests/kpi/results/ws22/ws22-closure-gate-summary.json"
    Runner = { & "tests/kpi/scripts/run-ws22-closure-gate.ps1" -OutputPath "tests/kpi/results/ws22/ws22-closure-gate-summary.json" }
  }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  try {
    $global:LASTEXITCODE = 0
    & $pack.Runner 2>&1 | Out-Null
    if (-not $?) {
      $packStatus = "failed"
      $detail = "script_invocation_failed"
    } elseif ($global:LASTEXITCODE -ne 0) {
      $packStatus = "failed"
      $detail = "exit_code=$global:LASTEXITCODE"
    }
  } catch {
    $packStatus = "failed"
    $detail = $_.Exception.Message
  }
  if ($packStatus -ne "passed") { $status = "failed" }
  $runs += [ordered]@{
    pack = $pack.Name
    status = $packStatus
    detail = $detail
    artifact = $pack.Artifact
  }
}

$finished = Get-Date
$artifact = [ordered]@{
  gate = "release-r1-sql-udf-readiness"
  status = $status
  release_target = "R1"
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  scope = @("WS1", "WS22", "REQ-03", "REQ-22", "PR-004", "PR-007")
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "Release R1 SQL/UDF/locking gate summary: $OutputPath ($status)"
if ($status -eq "passed") {
  $outDir = Split-Path -Parent $OutputPath
  $ciMirror = Join-Path $outDir "ci-release-r1-sql-udf-readiness.json"
  if ($ciMirror -ne $OutputPath) {
    Copy-Item -LiteralPath $OutputPath -Destination $ciMirror -Force
    Write-Host "CI mirror: $ciMirror"
  }
}
if ($status -ne "passed") { exit 1 }
