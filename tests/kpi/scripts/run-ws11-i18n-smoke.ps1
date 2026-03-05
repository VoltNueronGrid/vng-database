param(
  [string]$OutputPath = "tests/kpi/results/ws11/i18n-smoke.json"
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
$command = "cargo test -p voltnuerongrid-sql ws11_; cargo test -p voltnuerongridd ws11_"
$outputLines = @()
$exitCode = 1
$fallbackPolicyChecks = [ordered]@{
  sql_locale_default = $false
  sql_message_default = $false
  runtime_header_default = $false
}

try {
  $first = & cargo test -p voltnuerongrid-sql ws11_ 2>&1
  $firstExit = $LASTEXITCODE
  $second = & cargo test -p voltnuerongridd ws11_ 2>&1
  $secondExit = $LASTEXITCODE
  $outputLines = @($first + $second)
  $joined = $outputLines -join "`n"
  $fallbackPolicyChecks.sql_locale_default = ($joined -match "ws11_locale_fallback_defaults_to_en_us")
  $fallbackPolicyChecks.sql_message_default = ($joined -match "ws11_unknown_message_key_uses_safe_fallback")
  $fallbackPolicyChecks.runtime_header_default = ($joined -match "ws11_locale_header_falls_back_to_en_us_for_unknown_locale")
  $policyExit = if (
    $fallbackPolicyChecks.sql_locale_default -and
    $fallbackPolicyChecks.sql_message_default -and
    $fallbackPolicyChecks.runtime_header_default
  ) { 0 } else { 1 }
  $exitCode = if ($firstExit -eq 0 -and $secondExit -eq 0 -and $policyExit -eq 0) { 0 } else { 1 }
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$status = if ($exitCode -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws11-i18n-utf8-baseline"
  status = $status
  command = $command
  fallback_policy_checks = $fallbackPolicyChecks
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS11 i18n smoke failed."
  exit 1
}

Write-Host "WS11 i18n smoke passed. Artifact: $OutputPath"
