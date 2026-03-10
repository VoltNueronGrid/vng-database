param(
  [string]$RepoRoot = "D:/by/polap-db",
  [int]$Cycles = 5,
  [int]$MaxCycleMs = 6000,
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-failover-flap-resistance-smoke.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

function Invoke-CargoCapture {
  param([scriptblock]$Command)

  $previousPreference = $ErrorActionPreference
  try {
    $ErrorActionPreference = "Continue"
    $global:LASTEXITCODE = 0
    $output = & $Command 2>&1
    $exitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previousPreference
  }

  return [pscustomobject]@{
    Output = @($output)
    ExitCode = $exitCode
  }
}

$cycleResults = @()
$overallPassed = $true

for ($i = 1; $i -le $Cycles; $i++) {
  $started = Get-Date
  $result = Invoke-CargoCapture -Command { cargo test -p voltnuerongridd failover_rotate_leader_ -- --nocapture }
  $output = $result.Output
  $exitCode = $result.ExitCode
  $finished = Get-Date
  $durationMs = [int](($finished - $started).TotalMilliseconds)
  $passed = ($exitCode -eq 0 -and $durationMs -le $MaxCycleMs)
  if (-not $passed) { $overallPassed = $false }
  $cycleResults += [ordered]@{
    cycle = $i
    status = if ($passed) { "passed" } else { "failed" }
    duration_ms = $durationMs
    max_cycle_ms = $MaxCycleMs
    output_excerpt = (($output | Select-Object -First 6) -join "`n")
  }
}

$durations = @($cycleResults | ForEach-Object { [int]$_.duration_ms } | Sort-Object)
$p95Index = [Math]::Min($durations.Count - 1, [Math]::Floor(($durations.Count - 1) * 0.95))
$p95 = if ($durations.Count -gt 0) { $durations[$p95Index] } else { 0 }
$maxObserved = if ($durations.Count -gt 0) { ($durations | Measure-Object -Maximum).Maximum } else { 0 }

$status = if ($overallPassed) { "passed" } else { "failed" }
$result = [ordered]@{
  smoke = "ws6-failover-flap-resistance"
  status = $status
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  command = "cargo test -p voltnuerongridd failover_rotate_leader_ -- --nocapture"
  cycles = $Cycles
  cycle_max_ms = $MaxCycleMs
  latency_envelope = [ordered]@{
    p95_cycle_ms = $p95
    max_cycle_ms_observed = $maxObserved
  }
  cycle_results = $cycleResults
}

$result | ConvertTo-Json -Depth 12 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS6 failover flap-resistance smoke result: $OutputPath ($status)"
if ($status -eq "failed") { exit 1 }
exit 0
