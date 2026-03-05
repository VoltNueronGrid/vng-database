param(
  [string]$OutputPath = "tests/kpi/results/ws8a/audit-trail-smoke.json"
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
$command = "cargo test -p voltnuerongrid-audit; cargo test -p voltnuerongridd audit_append_event"
$outputLines = @()
$exitCode = 1

try {
  $first = & cargo test -p voltnuerongrid-audit 2>&1
  $firstExit = $LASTEXITCODE
  $second = & cargo test -p voltnuerongridd audit_append_event 2>&1
  $secondExit = $LASTEXITCODE
  $outputLines = @($first + $second)
  $exitCode = if ($firstExit -eq 0 -and $secondExit -eq 0) { 0 } else { 1 }
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$status = if ($exitCode -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws8a-audit-trail-baseline"
  status = $status
  command = $command
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS8A audit smoke failed."
  exit 1
}

Write-Host "WS8A audit smoke passed. Artifact: $OutputPath"
