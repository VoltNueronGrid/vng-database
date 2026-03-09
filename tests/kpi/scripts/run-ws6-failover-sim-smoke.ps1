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

function Invoke-CargoTestCapture {
  param([string[]]$Arguments)

  $tempFile = [System.IO.Path]::GetTempFileName()
  try {
    $commandText = "cargo " + (($Arguments | ForEach-Object {
      if ($_ -match "\s") { '"' + $_ + '"' } else { $_ }
    }) -join " ")
    $process = Start-Process -FilePath "cmd.exe" -ArgumentList "/c", "$commandText > `"$tempFile`" 2>&1" -Wait -PassThru -NoNewWindow
    $text = if (Test-Path -Path $tempFile) { Get-Content -Path $tempFile -Raw } else { "" }
    $ok = ($text -match "test result: ok\." -and $text -notmatch "test result: FAILED" -and $text -notmatch "(?m)^error:")
    return [pscustomobject]@{
      Ok = $ok
      Text = $text
      ExitCode = $process.ExitCode
    }
  } finally {
    if (Test-Path -Path $tempFile) {
      Remove-Item -Path $tempFile -Force -ErrorAction SilentlyContinue
    }
  }
}

$start = Get-Date
$command = "cargo test -p voltnuerongridd failover_rotate_leader -- --nocapture"
$outputLines = @()
$exitCode = 1

try {
  $testRun = Invoke-CargoTestCapture -Arguments @("test", "-p", "voltnuerongridd", "failover_rotate_leader", "--", "--nocapture")
  $outputLines = @($testRun.Text)
  $exitCode = if ($testRun.Ok -and $testRun.ExitCode -eq 0) { 0 } else { 1 }
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
