param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-handoff-matrix-smoke.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$command = "cargo test -p voltnuerongridd failover_rotate_leader_ -- --nocapture"
$global:LASTEXITCODE = 0
$testOutput = & cargo test -p voltnuerongridd failover_rotate_leader_ -- --nocapture 2>&1
$exitCode = $LASTEXITCODE
$testsPassed = ($? -and $exitCode -eq 0)

$matrix = @(
  [ordered]@{ from = "node-1"; to = "node-2"; expected = "handoff_success" },
  [ordered]@{ from = "node-2"; to = "node-3"; expected = "handoff_success" },
  [ordered]@{ from = "node-3"; to = "node-1"; expected = "handoff_success" },
  [ordered]@{ from = "node-2"; to = "blank_request"; expected = "fallback_to_current_node" }
)

$result = [ordered]@{
  smoke = "ws6-multi-node-handoff-matrix"
  status = if ($testsPassed) { "passed" } else { "failed" }
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  command = $command
  matrix = $matrix
  checks = [ordered]@{
    leader_rotation_test_pack_passed = $testsPassed
    scenario_count = $matrix.Count
  }
  output_excerpt = (($testOutput | Select-Object -First 20) -join "`n")
}

$result | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS6 handoff matrix smoke result: $OutputPath ($($result.status))"
if ($result.status -eq "failed") { exit 1 }
exit 0
