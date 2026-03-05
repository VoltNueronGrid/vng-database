param(
  [Parameter(Mandatory = $true)][string]$LocalBaselineRoot,
  [Parameter(Mandatory = $true)][string]$CloudRollupPath,
  [Parameter(Mandatory = $true)][string]$OutputDir
)

$ErrorActionPreference = "Stop"

if (!(Test-Path $OutputDir)) {
  New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
}

function Read-Json {
  param([string]$Path)
  if (!(Test-Path $Path)) {
    throw "Missing required JSON file: $Path"
  }
  return (Get-Content -Raw -Path $Path | ConvertFrom-Json)
}

function Scenario-Map {
  param([string]$Root)
  $map = @{}
  $files = Get-ChildItem -Path $Root -Filter "*-result.json" -Recurse
  foreach ($f in $files) {
    $json = Read-Json -Path $f.FullName
    $map[$json.scenario] = $json
  }
  return $map
}

$cloudRollup = Read-Json -Path $CloudRollupPath
$baselineSingle = Scenario-Map -Root (Join-Path $LocalBaselineRoot "single-node-local")
$baselineMulti = Scenario-Map -Root (Join-Path $LocalBaselineRoot "multi-node-local")

$profiles = @()
foreach ($profileEntry in $cloudRollup.profiles) {
  $profileRollupPath = [string]$profileEntry.rollup_path
  $profileScenarios = @{}
  if (-not [string]::IsNullOrWhiteSpace($profileRollupPath) -and (Test-Path $profileRollupPath)) {
    $profileRoot = Split-Path -Parent $profileRollupPath
    $profileScenarios = Scenario-Map -Root $profileRoot
  }
  $scenarioDeltas = @()

  foreach ($scenarioName in $profileScenarios.Keys) {
    $remote = $profileScenarios[$scenarioName]
    $single = $baselineSingle[$scenarioName]
    $multi = $baselineMulti[$scenarioName]
    $delta = @{
      scenario = $scenarioName
      status_remote = $remote.status
      status_single_baseline = if ($single) { $single.status } else { "missing" }
      status_multi_baseline = if ($multi) { $multi.status } else { "missing" }
    }

    if ($remote.metrics.p95_latency_ms -ne $null -and $single -and $single.metrics.p95_latency_ms -ne $null) {
      $delta["delta_p95_vs_single_ms"] = [Math]::Round(([double]$remote.metrics.p95_latency_ms - [double]$single.metrics.p95_latency_ms), 3)
    }
    if ($remote.metrics.p95_latency_ms -ne $null -and $multi -and $multi.metrics.p95_latency_ms -ne $null) {
      $delta["delta_p95_vs_multi_ms"] = [Math]::Round(([double]$remote.metrics.p95_latency_ms - [double]$multi.metrics.p95_latency_ms), 3)
    }
    if ($remote.metrics.read_qps -ne $null -and $single -and $single.metrics.read_qps -ne $null) {
      $delta["delta_read_qps_vs_single"] = [Math]::Round(([double]$remote.metrics.read_qps - [double]$single.metrics.read_qps), 3)
    }
    if ($remote.metrics.write_tps -ne $null -and $single -and $single.metrics.write_tps -ne $null) {
      $delta["delta_write_tps_vs_single"] = [Math]::Round(([double]$remote.metrics.write_tps - [double]$single.metrics.write_tps), 3)
    }
    $scenarioDeltas += $delta
  }

  $profiles += @{
    profile = $profileEntry.profile
    status = $profileEntry.status
    passed = $profileEntry.passed
    failed = $profileEntry.failed
    total_scenarios = $profileEntry.total_scenarios
    missing_requirements = $profileEntry.missing_requirements
    scenario_deltas = $scenarioDeltas
  }
}

$gate = @{
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  local_baseline_root = $LocalBaselineRoot
  cloud_rollup_path = $CloudRollupPath
  overall_status = $cloudRollup.status
  profiles = $profiles
}

$jsonPath = Join-Path $OutputDir "final-gate-report.json"
$gate | ConvertTo-Json -Depth 12 | Out-File -FilePath $jsonPath -Encoding utf8

$mdPath = Join-Path $OutputDir "final-gate-report.md"
$lines = @()
$lines += "# PR-007 Final Gate Report"
$lines += ""
$lines += "- Timestamp (UTC): $($gate.timestamp_utc)"
$lines += "- Overall Status: **$($gate.overall_status)**"
$lines += "- Local Baseline: $($gate.local_baseline_root)"
$lines += "- Cloud Rollup: $($gate.cloud_rollup_path)"
$lines += ""
$lines += "## Profile Summary"
$lines += ""
$lines += "| Profile | Status | Passed | Failed | Total |"
$lines += "|---|---:|---:|---:|---:|"
foreach ($p in $profiles) {
  $lines += "| $($p.profile) | $($p.status) | $($p.passed) | $($p.failed) | $($p.total_scenarios) |"
}
$pending = @($profiles | Where-Object { $_.status -eq "pending_config" })
if ($pending.Count -gt 0) {
  $lines += ""
  $lines += "## Pending Configuration"
  $lines += ""
  foreach ($p in $pending) {
    $missing = if ($p.missing_requirements) { ($p.missing_requirements -join ", ") } else { "(none)" }
    $lines += "- $($p.profile): $missing"
  }
}
$lines += ""
$lines += "## Scenario Deltas vs Local Baseline"
$lines += ""
$lines += "| Profile | Scenario | Remote | Single Baseline | Multi Baseline | Delta P95 vs Single (ms) | Delta P95 vs Multi (ms) | Delta Read QPS vs Single | Delta Write TPS vs Single |"
$lines += "|---|---|---|---|---|---:|---:|---:|---:|"
foreach ($p in $profiles) {
  foreach ($d in $p.scenario_deltas) {
    $lines += "| $($p.profile) | $($d.scenario) | $($d.status_remote) | $($d.status_single_baseline) | $($d.status_multi_baseline) | $($d.delta_p95_vs_single_ms) | $($d.delta_p95_vs_multi_ms) | $($d.delta_read_qps_vs_single) | $($d.delta_write_tps_vs_single) |"
  }
}
$lines | Out-File -FilePath $mdPath -Encoding utf8

Write-Host "Generated gate reports:"
Write-Host " - $jsonPath"
Write-Host " - $mdPath"
