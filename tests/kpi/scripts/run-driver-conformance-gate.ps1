##
## Driver Conformance Gate — P9
##
## Validates that the voltnuerongrid-driver-rust crate passes all conformance
## fixture cases defined in drivers/conformance/fixtures/.
## Runs the driver Rust tests, validates config-validation-cases.json and
## transport-mode-cases.json fixture coverage, and emits a structured artifact.
##
## Usage:
##   pwsh ./tests/kpi/scripts/run-driver-conformance-gate.ps1
##   pwsh ./tests/kpi/scripts/run-driver-conformance-gate.ps1 -OutputPath custom.json
##

param(
  [string]$OutputPath = "tests/kpi/results/ws10/driver-conformance-gate.json"
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

# ── Cross-platform cargo runner ───────────────────────────────────────────────
function Invoke-CargoTestCapture {
  param([string[]]$Arguments)
  if ($IsWindows) {
    $tempFile = [System.IO.Path]::GetTempFileName()
    try {
      $commandText = "cargo " + (($Arguments | ForEach-Object {
        if ($_ -match "\s") { '"' + $_ + '"' } else { $_ }
      }) -join " ")
      $process = Start-Process -FilePath "cmd.exe" -ArgumentList "/c", "$commandText > `"$tempFile`" 2>&1" -Wait -PassThru -NoNewWindow
      $text = if (Test-Path -Path $tempFile) { Get-Content -Path $tempFile -Raw } else { "" }
      $ok = ($text -match "test result: ok\." -and $text -notmatch "test result: FAILED" -and $text -notmatch "(?m)^error:")
      return [pscustomobject]@{ Ok = $ok; Text = $text; ExitCode = $process.ExitCode }
    } finally {
      if (Test-Path -Path $tempFile) { Remove-Item -Path $tempFile -Force -ErrorAction SilentlyContinue }
    }
  } else {
    $allArgs = @("cargo") + $Arguments
    $text = (& $allArgs[0] $allArgs[1..($allArgs.Length - 1)] 2>&1) -join "`n"
    $ok = ($text -match "test result: ok\." -and $text -notmatch "test result: FAILED" -and $text -notmatch "(?m)^error:")
    return [pscustomobject]@{ Ok = $ok; Text = $text; ExitCode = if ($ok) { 0 } else { 1 } }
  }
}

# ── Pack definitions ──────────────────────────────────────────────────────────
$packs = @(
  [ordered]@{
    id          = "cargo-driver-tests"
    description = "cargo test -p voltnuerongrid-driver-rust"
    type        = "cargo"
    arguments   = @("test", "-p", "voltnuerongrid-driver-rust")
  },
  [ordered]@{
    id          = "config-validation-fixture"
    description = "Config validation fixture: drivers/conformance/fixtures/config-validation-cases.json"
    type        = "fixture"
    path        = "drivers/conformance/fixtures/config-validation-cases.json"
    requiredCases = @("admin mode requires admin key", "operator mode requires admin key and operator id", "tenant mode requires tenant id")
  },
  [ordered]@{
    id          = "transport-mode-fixture"
    description = "Transport mode fixture: drivers/conformance/fixtures/transport-mode-cases.json"
    type        = "fixture"
    path        = "drivers/conformance/fixtures/transport-mode-cases.json"
    requiredFields = @("cases")
  },
  [ordered]@{
    id          = "request-building-fixture"
    description = "Request building fixture: drivers/conformance/fixtures/request-building-cases.json"
    type        = "fixture"
    path        = "drivers/conformance/fixtures/request-building-cases.json"
    requiredFields = @("operatorExecuteCase")
  }
)

$start = Get-Date
$packResults = @()

foreach ($pack in $packs) {
  $packStart = Get-Date
  $packStatus = "unknown"
  $packDetail = ""

  if ($pack.type -eq "cargo") {
    try {
      $result = Invoke-CargoTestCapture -Arguments $pack.arguments
      $packStatus = if ($result.Ok) { "passed" } else { "failed" }
      $packDetail = ($result.Text -split "`n" | Select-Object -Last 5) -join "`n"
    } catch {
      $packStatus = "failed"
      $packDetail = $_.Exception.Message
    }
  } elseif ($pack.type -eq "fixture") {
    try {
      if (!(Test-Path -Path $pack.path)) {
        $packStatus = "failed"
        $packDetail = "Fixture file not found: $($pack.path)"
      } else {
        $fixtureRaw = Get-Content -Raw -Path $pack.path
        $fixture = $fixtureRaw | ConvertFrom-Json -ErrorAction Stop
        # Validate required cases by name
        if ($pack.requiredCases) {
          $caseNames = $fixture.cases | ForEach-Object { $_.name }
          $missing = $pack.requiredCases | Where-Object { $_ -notin $caseNames }
          if ($missing.Count -gt 0) {
            $packStatus = "failed"
            $packDetail = "Missing required cases: $($missing -join ', ')"
          } else {
            $packStatus = "passed"
            $packDetail = "All $($fixture.cases.Count) cases present; required cases verified."
          }
        } elseif ($pack.requiredFields) {
          $missingFields = $pack.requiredFields | Where-Object { $null -eq $fixture.$_ }
          if ($missingFields.Count -gt 0) {
            $packStatus = "failed"
            $packDetail = "Missing required fields: $($missingFields -join ', ')"
          } else {
            $packStatus = "passed"
            $packDetail = "All required fields present; $($fixture.cases.Count) cases found."
          }
        } else {
          $packStatus = "passed"
          $packDetail = "Fixture loaded successfully."
        }
      }
    } catch {
      $packStatus = "failed"
      $packDetail = $_.Exception.Message
    }
  }

  $packResults += [ordered]@{
    id          = $pack.id
    description = $pack.description
    status      = $packStatus
    detail      = $packDetail
    duration_ms = [int](((Get-Date) - $packStart).TotalMilliseconds)
  }
}

$finished = Get-Date
$allPassed = ($packResults | Where-Object { $_.status -ne "passed" }).Count -eq 0
$gateStatus = if ($allPassed) { "passed" } else { "failed" }

$artifact = [ordered]@{
  gate           = "driver-conformance"
  status         = $gateStatus
  total_packs    = $packResults.Count
  passed_packs   = ($packResults | Where-Object { $_.status -eq "passed" }).Count
  failed_packs   = ($packResults | Where-Object { $_.status -ne "passed" }).Count
  packs          = $packResults
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms    = [int](($finished - $start).TotalMilliseconds)
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($gateStatus -ne "passed") {
  Write-Error "Driver conformance gate FAILED. See $OutputPath for details."
  exit 1
}

Write-Host "Driver conformance gate PASSED. Artifact: $OutputPath"
