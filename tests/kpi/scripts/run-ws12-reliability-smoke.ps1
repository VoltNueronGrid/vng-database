param(
  [string]$OutputPath = "tests/kpi/results/ws12/reliability-sre-smoke.json"
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
$command = "cargo test -p voltnuerongridd ws12_"
$outputLines = @()
$exitCode = 1

try {
  $result = & cargo test -p voltnuerongridd ws12_ 2>&1
  $resultExit = $LASTEXITCODE
  $outputLines = @($result)
  $exitCode = if ($resultExit -eq 0) { 0 } else { 1 }
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$status = if ($exitCode -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws12-reliability-sre-baseline"
  status = $status
  command = $command
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS12 reliability smoke failed."
  exit 1
}

Write-Host "WS12 reliability smoke passed. Artifact: $OutputPath"
