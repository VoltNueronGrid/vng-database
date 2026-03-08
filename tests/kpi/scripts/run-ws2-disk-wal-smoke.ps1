param(
  [string]$OutputPath = "tests/kpi/results/ws2/disk-wal-adapter-smoke.json"
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
$command = "cmd.exe /c cargo test -p voltnuerongrid-store wal_adapter -- --nocapture"
$outputLines = @()
$exitCode = 1

try {
  $tempFile = [System.IO.Path]::GetTempFileName()
  try {
    $process = Start-Process -FilePath "cmd.exe" -ArgumentList "/c", "cargo test -p voltnuerongrid-store wal_adapter -- --nocapture > `"$tempFile`" 2>&1" -Wait -PassThru -NoNewWindow
    $outputLines = if (Test-Path -Path $tempFile) { Get-Content -Path $tempFile } else { @() }
    $exitCode = $process.ExitCode
  } finally {
    if (Test-Path -Path $tempFile) {
      Remove-Item -Path $tempFile -Force -ErrorAction SilentlyContinue
    }
  }
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$outputText = ((@($outputLines) | ForEach-Object { "$_" }) -join "`n")
$status = if ($outputText -match "test result: ok\." -and $outputText -notmatch "test result: FAILED" -and $outputText -notmatch "(?m)^error:") { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws2-disk-wal-adapter"
  status = $status
  command = $command
  exit_code = $exitCode
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS2 disk WAL adapter smoke failed."
  exit 1
}

Write-Host "WS2 disk WAL adapter smoke passed. Artifact: $OutputPath"
