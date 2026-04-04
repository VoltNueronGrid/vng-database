param(
  [string]$OutputPath = "tests/kpi/results/ws3/query-routing-smoke.json",
  [int]$TimeoutSeconds = 600
)

$ErrorActionPreference = "Stop"

function New-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Invoke-CargoWithCapture {
  param(
    [string[]]$Arguments,
    [int]$TimeoutSecondsValue
  )

  $stdoutFile = [System.IO.Path]::GetTempFileName()
  $stderrFile = [System.IO.Path]::GetTempFileName()
  try {
    $process = Start-Process -FilePath "cargo" `
      -ArgumentList $Arguments `
      -RedirectStandardOutput $stdoutFile `
      -RedirectStandardError $stderrFile `
      -NoNewWindow `
      -PassThru

    $timedOut = -not $process.WaitForExit($TimeoutSecondsValue * 1000)
    if ($timedOut) {
      try { $process.Kill() } catch {}
      return [ordered]@{
        ExitCode = 124
        TimedOut = $true
        OutputLines = @("timeout after ${TimeoutSecondsValue}s")
      }
    }

    $lines = @()
    if (Test-Path -Path $stdoutFile) {
      $lines += @(Get-Content -Path $stdoutFile)
    }
    if (Test-Path -Path $stderrFile) {
      $lines += @(Get-Content -Path $stderrFile)
    }

    return [ordered]@{
      ExitCode = $process.ExitCode
      TimedOut = $false
      OutputLines = $lines
    }
  }
  finally {
    if (Test-Path -Path $stdoutFile) {
      Remove-Item -Path $stdoutFile -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -Path $stderrFile) {
      Remove-Item -Path $stderrFile -Force -ErrorAction SilentlyContinue
    }
  }
}

New-OutputDir -PathValue $OutputPath

if (!(Get-Command cargo -ErrorAction SilentlyContinue)) {
  throw "cargo not found in PATH"
}

$start = Get-Date
$command = "cargo test -p voltnuerongridd ws3_ -- --test-threads=1 --nocapture"
$outputLines = @()
$exitCode = 1
$testExitCode = -1
$timedOut = $false
$testsExecuted = 0
$expectedTests = 11

try {
  $result = Invoke-CargoWithCapture -Arguments @("test", "-p", "voltnuerongridd", "ws3_", "--", "--test-threads=1", "--nocapture") -TimeoutSecondsValue $TimeoutSeconds
  $testExitCode = $result.ExitCode
  $timedOut = $result.TimedOut
  $outputLines = @($result.OutputLines)

  $testsExecuted = @($outputLines | Where-Object { $_ -match "test\s+tests::ws3_" }).Count

  if ($testExitCode -ne 0) {
    $errorLines = @($outputLines | Where-Object { $_ -match "error\[E|error:|FAILED|panicked|test result: FAILED|could not compile" })
    if ($errorLines.Count -gt 0) {
      $outputLines += "=== MATCHED ERROR LINES ==="
      $outputLines += @($errorLines | Select-Object -First 30)
    }
    $exitCode = 1
  }
  else {
    $exitCode = 0
  }
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$status = if ($exitCode -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws3-query-routing"
  status = $status
  command = $command
  test_exit_code = $testExitCode
  timed_out = $timedOut
  tests_executed = $testsExecuted
  tests_expected = $expectedTests
  test_count_match = ($testsExecuted -eq $expectedTests)
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -Last 80) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS3 query-routing smoke failed. exit=$testExitCode tests=$testsExecuted/$expectedTests timeout=$timedOut artifact=$OutputPath"
  exit 1
}

Write-Host "WS3 query-routing smoke passed. Artifact: $OutputPath"
