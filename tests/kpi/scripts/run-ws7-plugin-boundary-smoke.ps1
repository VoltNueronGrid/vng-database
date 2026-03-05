param(
  [string]$OutputPath = "tests/kpi/results/ws7/plugin-boundary-smoke.json"
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
$command = "cargo test -p voltnuerongrid-plugins registers_valid_package; cargo test -p voltnuerongrid-plugins rejects_package_when_manifest_key_is_revoked"
$outputLines = @()
$exitCode = 1

try {
  $first = & cargo test -p voltnuerongrid-plugins registers_valid_package 2>&1
  $firstExit = $LASTEXITCODE
  $second = & cargo test -p voltnuerongrid-plugins rejects_package_when_manifest_key_is_revoked 2>&1
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
  smoke = "ws7-plugin-registration-boundary"
  status = $status
  command = $command
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS7 plugin boundary smoke failed."
  exit 1
}

Write-Host "WS7 plugin boundary smoke passed. Artifact: $OutputPath"
