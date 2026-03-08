param(
  [string]$OutputPath = "tests/kpi/results/gates/release-dx-api-readiness.json",
  [string]$Ws5SummaryPath = "",
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [switch]$IncludeWs5RuntimeSmokes
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
    Name = "ws5-security-gate"
    Script = "tests/kpi/scripts/run-ws5-gate.ps1"
    Artifact = "tests/kpi/results/ws5/ws5-gate-summary.json"
  },
  @{
    Name = "ws9-studio-gate"
    Script = "tests/kpi/scripts/run-ws9-gate.ps1"
    Artifact = "tests/kpi/results/ws9/ws9-gate-summary.json"
  },
  @{
    Name = "ws9a-ide-contract-smoke"
    Script = "tests/kpi/scripts/run-ws9a-ide-contract-smoke.ps1"
    Artifact = "tests/kpi/results/ws9a/ide-contract-smoke.json"
  },
  @{
    Name = "ws10-driver-gate"
    Script = "tests/kpi/scripts/run-ws10-gate.ps1"
    Artifact = "tests/kpi/results/ws10/ws10-gate-summary.json"
  }
)

foreach ($pack in $packs) {
  $packStatus = "passed"
  $detail = "ok"
  $artifactPath = $pack.Artifact
  try {
    if ($pack.Name -eq "ws5-security-gate" -and -not [string]::IsNullOrWhiteSpace($Ws5SummaryPath)) {
      if (!(Test-Path -Path $Ws5SummaryPath)) {
        throw "WS5 summary not found at $Ws5SummaryPath"
      }
      $artifactPath = $Ws5SummaryPath
      $existing = Get-Content -Raw -Path $Ws5SummaryPath | ConvertFrom-Json
      $packStatus = [string]$existing.status
      $detail = if ($packStatus -eq "passed") { "reused_existing_summary" } else { "reused_existing_summary_status=$packStatus" }
    } else {
      $global:LASTEXITCODE = 0
      if ($pack.Name -eq "ws5-security-gate" -and $IncludeWs5RuntimeSmokes) {
        & $pack.Script -OutputPath $pack.Artifact -BaseUrl $BaseUrl -IncludeRuntimeSmokes 2>&1 | Out-Null
      } else {
        & $pack.Script -OutputPath $pack.Artifact 2>&1 | Out-Null
      }
      if (-not $?) {
        $packStatus = "failed"
        $detail = "script_invocation_failed"
      } elseif ($global:LASTEXITCODE -ne 0) {
        $packStatus = "failed"
        $detail = "exit_code=$global:LASTEXITCODE"
      }
    }
  } catch {
    $packStatus = "failed"
    $detail = $_.Exception.Message
  }
  if ($packStatus -ne "passed") {
    $status = "failed"
  }
  $runs += [ordered]@{
    pack = $pack.Name
    status = $packStatus
    detail = $detail
    artifact = $artifactPath
  }
}

$ws5SummaryArtifact = if (![string]::IsNullOrWhiteSpace($Ws5SummaryPath)) { $Ws5SummaryPath } else { "tests/kpi/results/ws5/ws5-gate-summary.json" }
$ws5RuntimePack = $null
if (Test-Path -Path $ws5SummaryArtifact) {
  try {
    $ws5Summary = Get-Content -Raw -Path $ws5SummaryArtifact | ConvertFrom-Json
    $ws5RuntimePack = @($ws5Summary.packs | Where-Object { $_.pack -eq "ws5-tenant-audit-runtime" }) | Select-Object -First 1
  } catch {
    $ws5RuntimePack = $null
  }
}

$finished = Get-Date
$summary = [ordered]@{
  gate = "release-dx-api-cluster"
  status = $status
  release_readiness = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  scope = @("WS5", "WS9", "WS9A", "WS10")
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  packs = $runs
  highlights = [ordered]@{
    ws5_runtime_pack_included = ($null -ne $ws5RuntimePack)
    ws5_runtime_pack_status = if ($null -ne $ws5RuntimePack) { [string]$ws5RuntimePack.status } else { "not_included" }
  }
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

Write-Host "Release DX/API gate summary: $OutputPath ($status)"
if ($status -ne "passed") {
  exit 1
}
