param(
  [string]$OutputPath = "tests/kpi/results/ws7/ws7-manifest-integrity-smoke.json"
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
$runs = @()
$status = "passed"

$tests = @(
  "rejects_package_when_manifest_checksum_is_tampered",
  "rejects_package_when_manifest_key_is_unknown",
  "rejects_package_when_manifest_key_is_revoked",
  "rejects_package_when_manifest_key_trust_too_low"
)

foreach ($testName in $tests) {
  $global:LASTEXITCODE = 0
  $output = & cargo test -p voltnuerongrid-plugins $testName -- --nocapture 2>&1
  $exitCode = $LASTEXITCODE
  $testStatus = if ($? -and $exitCode -eq 0) { "passed" } else { "failed" }
  if ($testStatus -ne "passed") { $status = "failed" }
  $runs += [ordered]@{
    test = $testName
    status = $testStatus
    output_excerpt = (($output | Select-Object -First 8) -join "`n")
  }
}

$finished = Get-Date
$artifact = [ordered]@{
  smoke = "ws7-manifest-integrity"
  status = $status
  command = "cargo test -p voltnuerongrid-plugins <manifest_integrity_tests> -- --nocapture"
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  tests = $runs
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS7 manifest integrity smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
