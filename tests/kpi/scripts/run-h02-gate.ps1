param(
  [string]$OutputPath = "tests/kpi/results/h02/h02-gate-summary.json",
  [string]$ReleaseSummaryOutputPath = "tests/kpi/results/gates/h02-release-readiness.json",
  [string]$RepoRoot = "D:/by/polap-db"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

function Resolve-RepoPath {
  param([string]$PathValue)
  if ([System.IO.Path]::IsPathRooted($PathValue)) { return $PathValue }
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
  if (!(Test-Path -Path $ArtifactPath)) { return "missing_artifact" }
  try {
    $json = Get-Content -Raw -Path $ArtifactPath | ConvertFrom-Json
    if ($null -ne $json.status) { return [string]$json.status }
    return "present"
  } catch { return "invalid_artifact" }
}

function Invoke-PowerShellScript {
  param([string]$ScriptPath, [string[]]$ArgumentList = @())
  $resolvedScriptPath = Resolve-RepoPath -PathValue $ScriptPath
  $process = Start-Process -FilePath "powershell.exe" `
    -ArgumentList (@("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $resolvedScriptPath) + $ArgumentList) `
    -WorkingDirectory $RepoRoot -NoNewWindow -PassThru -Wait
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
    Name     = "h02-restart-replay-matrix"
    Script   = "tests/kpi/scripts/run-h02-restart-replay-matrix.ps1"
    Artifact = Resolve-RepoPath -PathValue "tests/kpi/results/h02/h02-restart-replay-matrix.json"
  },
  @{
    Name     = "h02-multi-node-handoff-matrix"
    Script   = "tests/kpi/scripts/run-h02-multi-node-handoff-matrix.ps1"
    Artifact = Resolve-RepoPath -PathValue "tests/kpi/results/h02/h02-multi-node-handoff-matrix.json"
  },
  @{
    Name     = "h02-sync-fault-injection"
    Script   = "tests/kpi/scripts/run-h02-sync-fault-injection.ps1"
    Artifact = Resolve-RepoPath -PathValue "tests/kpi/results/h02/htap-sync-fault-injection.json"
  },
  @{
    Name     = "h02-reorder-duplicate-faults"
    Script   = "tests/kpi/scripts/run-h02-reorder-duplicate-faults.ps1"
    Artifact = Resolve-RepoPath -PathValue "tests/kpi/results/h02/htap-sync-reorder-duplicate-faults.json"
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
  if ($packStatus -ne "passed") { $status = "failed" }
  $runs += [ordered]@{ pack = $pack.Name; status = $packStatus; detail = $detail; artifact = $pack.Artifact }
}

$finished = Get-Date
$summary = [ordered]@{
  gate            = "h02"
  status          = $status
  started_at_utc  = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms     = [int](($finished - $start).TotalMilliseconds)
  packs           = $runs
}
$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

# Write release readiness summary inline
$checks = [ordered]@{
  h02_gate_passed        = ($status -eq "passed")
  h02_all_packs_passed   = ((@($runs | Where-Object { $_.status -ne "passed" }).Count) -eq 0)
}
$allPassed = ($checks.Values | Where-Object { $_ -eq $false }).Count -eq 0
$releaseReadiness = if ($allPassed) { "ready_for_validation" } else { "blocked" }

$releaseArtifact = [ordered]@{
  gate                = "h02-release-htap-sync-correctness-readiness"
  status              = if ($allPassed) { "passed" } else { "failed" }
  release_readiness   = $releaseReadiness
  release_targets     = @("R2")
  scope               = @("WS2", "WS2A", "WS6", "REQ-05", "REQ-17", "H-02")
  generated_at_utc    = (Get-Date).ToUniversalTime().ToString("o")
  sources             = [ordered]@{
    gate_summary              = $OutputPath
    restart_replay_matrix     = Resolve-RepoPath "tests/kpi/results/h02/h02-restart-replay-matrix.json"
    multi_node_handoff_matrix = Resolve-RepoPath "tests/kpi/results/h02/h02-multi-node-handoff-matrix.json"
    sync_fault_injection      = Resolve-RepoPath "tests/kpi/results/h02/htap-sync-fault-injection.json"
    reorder_duplicate_faults  = Resolve-RepoPath "tests/kpi/results/h02/htap-sync-reorder-duplicate-faults.json"
  }
  checks    = $checks
  highlights = [ordered]@{
    pack_count                         = $runs.Count
    htap_sync_fault_injection_status   = [string](($runs | Where-Object { $_.pack -eq "h02-sync-fault-injection" } | Select-Object -First 1).status)
    restart_replay_matrix_status       = [string](($runs | Where-Object { $_.pack -eq "h02-restart-replay-matrix" } | Select-Object -First 1).status)
    multi_node_handoff_matrix_status   = [string](($runs | Where-Object { $_.pack -eq "h02-multi-node-handoff-matrix" } | Select-Object -First 1).status)
    reorder_duplicate_faults_status    = [string](($runs | Where-Object { $_.pack -eq "h02-reorder-duplicate-faults" } | Select-Object -First 1).status)
    ordered_transport_covered          = $true
    unapplied_only_replay_covered      = $true
    dropped_sequence_detection_covered = $true
    duplicate_sequence_detection       = $true
    out_of_order_sequence_detection    = $true
  }
}
$releaseArtifact | ConvertTo-Json -Depth 12 | Set-Content -Path $ReleaseSummaryOutputPath

Write-Host "H02 gate summary: $OutputPath ($status)"
Write-Host "H02 release readiness: $ReleaseSummaryOutputPath ($releaseReadiness)"
if ($status -ne "passed") { exit 1 }
