param(
  [string]$OutputPath = "tests/kpi/results/h04/h04-gate-summary.json",
  [string]$ReleaseSummaryOutputPath = "tests/kpi/results/gates/h04-release-readiness.json",
  [string]$RepoRoot = "D:/by/polap-db"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

function Resolve-RepoPath {
  param([string]$PathValue)

  if ([System.IO.Path]::IsPathRooted($PathValue)) {
    return $PathValue
  }
  return [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $PathValue))
}

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Get-ArtifactStatus {
  param([string]$ArtifactPath)

  if (!(Test-Path -Path $ArtifactPath)) {
    return "missing_artifact"
  }

  try {
    $json = Get-Content -Raw -Path $ArtifactPath | ConvertFrom-Json
    if ($null -ne $json.status) {
      return [string]$json.status
    }
    return "present"
  } catch {
    return "invalid_artifact"
  }
}

function Invoke-PowerShellScript {
  param(
    [string]$ScriptPath,
    [string[]]$ArgumentList = @()
  )

  $resolvedScriptPath = Resolve-RepoPath -PathValue $ScriptPath
  $process = Start-Process -FilePath "powershell.exe" `
    -ArgumentList (@("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $resolvedScriptPath) + $ArgumentList) `
    -WorkingDirectory $RepoRoot `
    -NoNewWindow `
    -PassThru `
    -Wait
  return $process.ExitCode
}

$OutputPath = Resolve-RepoPath -PathValue $OutputPath
$ReleaseSummaryOutputPath = Resolve-RepoPath -PathValue $ReleaseSummaryOutputPath
Ensure-OutputDir -PathValue $OutputPath
Ensure-OutputDir -PathValue $ReleaseSummaryOutputPath

$start = Get-Date
$runs = @()
$status = "passed"

$packs = @(
  @{
    Name = "h04-service-integrated-outbox-runtime"
    Script = "tests/kpi/scripts/run-h04-service-integrated-outbox-runtime-smoke.ps1"
    Artifact = Resolve-RepoPath -PathValue "tests/kpi/results/h04/h04-service-integrated-outbox-runtime.json"
  },
  @{
    Name = "h04-outbox-replay-evidence"
    Script = "tests/kpi/scripts/run-h04-outbox-replay-evidence.ps1"
    Artifact = Resolve-RepoPath -PathValue "tests/kpi/results/h04/h04-outbox-replay-evidence.json"
  }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  try {
    $exitCode = Invoke-PowerShellScript -ScriptPath $pack.Script -ArgumentList @("-OutputPath", $pack.Artifact)
    $artifactStatus = Get-ArtifactStatus -ArtifactPath $pack.Artifact
    if ($artifactStatus -eq "passed") {
      $packStatus = "passed"
    } else {
      $packStatus = "failed"
      $detail = if ($exitCode -ne 0) { "exit_code=$exitCode;$artifactStatus" } else { $artifactStatus }
    }
  } catch {
    $packStatus = "failed"
    $detail = $_.Exception.Message
  }
  if ($packStatus -ne "passed") {
    $status = "failed"
  }
  $runs += [ordered]@{ pack = $pack.Name; status = $packStatus; detail = $detail; artifact = $pack.Artifact }
}

$finished = Get-Date
$summary = [ordered]@{
  gate = "h04"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

$null = Invoke-PowerShellScript -ScriptPath "tests/kpi/scripts/run-h04-release-summary.ps1" -ArgumentList @(
  "-RepoRoot", $RepoRoot,
  "-SummaryPath", $OutputPath,
  "-OutputPath", $ReleaseSummaryOutputPath
)

Write-Host "H04 gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }