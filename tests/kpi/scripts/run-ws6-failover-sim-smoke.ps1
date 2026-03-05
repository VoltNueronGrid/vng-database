param(
  [string]$OutputPath = "tests/kpi/results/ws6/failover-sim-smoke.json"
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
$command = "cargo test -p voltnuerongridd failover_rotate_leader"
$outputLines = @()
$exitCode = 1

try {
  $outputLines = & cargo test -p voltnuerongridd failover_rotate_leader 2>&1
  $exitCode = $LASTEXITCODE
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$status = if ($exitCode -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws6-failover-simulation"
  status = $status
  command = $command
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS6 failover simulation smoke failed."
  exit 1
}

Write-Host "WS6 failover simulation smoke passed. Artifact: $OutputPath"
