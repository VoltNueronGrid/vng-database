param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$OutputPath = "tests/kpi/results/gates/r1-gate-check.json"
)

$ErrorActionPreference = "Stop"

Set-Location $RepoRoot

function Test-RequiredPath {
  param([string]$Path)
  return [PSCustomObject]@{
    path = $Path
    exists = (Test-Path $Path)
  }
}

$requiredPaths = @(
  "deploy/local/single-node.yml",
  "deploy/local/multi-node.yml",
  "deploy/helm/voltnuerongrid/Chart.yaml",
  "Cargo.toml",
  "services/voltnuerongridd/src/main.rs",
  "tests/kpi/scripts/run-kpi.ps1",
  "tests/kpi/scenarios/oltp-latency.yaml",
  "tests/kpi/scenarios/olap-latency.yaml",
  "tests/kpi/scenarios/htap-mixed-throughput.yaml",
  "tests/kpi/scenarios/failover-rto-rpo.yaml"
)

$checks = @()
foreach ($path in $requiredPaths) {
  $checks += Test-RequiredPath -Path $path
}

$baselineResultPaths = @(
  "tests/kpi/results/20260304-pr007/single-node-local/oltp-latency-result.json",
  "tests/kpi/results/20260304-pr007/single-node-local/olap-latency-result.json",
  "tests/kpi/results/20260304-pr007/multi-node-local/oltp-latency-result.json",
  "tests/kpi/results/20260304-pr007/multi-node-local/failover-rto-rpo-result.json"
)

$baselineChecks = @()
foreach ($path in $baselineResultPaths) {
  $baselineChecks += Test-RequiredPath -Path $path
}

$failedPaths = @($checks | Where-Object { -not $_.exists })
$failedBaseline = @($baselineChecks | Where-Object { -not $_.exists })

$status = if ($failedPaths.Count -eq 0 -and $failedBaseline.Count -eq 0) { "passed" } else { "failed" }
$result = @{
  gate = "R1"
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  status = $status
  criteria = @(
    "PR-002 deploy scaffolds present",
    "PR-004 KPI harness present",
    "PR-005 workspace/runtime scaffolds present",
    "KPI smoke baseline artifacts present"
  )
  required_artifacts = $checks
  baseline_artifacts = $baselineChecks
}

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}
$result | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "R1 gate check result: $OutputPath ($status)"

if ($status -eq "failed") {
  exit 1
}
exit 0
