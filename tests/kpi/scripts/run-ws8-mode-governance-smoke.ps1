param(
  [string]$RuntimePath = "services/voltnuerongridd/src/main.rs",
  [string]$OutputPath = "tests/kpi/results/ws8/ws8-mode-governance-smoke.json"
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
$runtimeRaw = Get-Content -Raw -Path $RuntimePath

$global:LASTEXITCODE = 0
$testOutput = & cargo test -p voltnuerongridd ws12_dr_hook_denies_when_mode_below_policy -- --nocapture 2>&1
$modePolicyTestPassed = ($? -and $LASTEXITCODE -eq 0)

$checks = [ordered]@{
  dr_hook_denies_when_mode_below_policy_test = $modePolicyTestPassed
  autonomous_mode_disabled_path_present = ($runtimeRaw -match 'autonomous_mode_disabled')
  deny_mode_policy_decision_present = ($runtimeRaw -match 'deny_mode')
  fast_failover_requires_autonomous_mode = ($runtimeRaw -match 'Fast autonomous failover is allowed only in full autonomous mode')
}

$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  smoke = "ws8-mode-governance"
  status = $status
  runtime_path = $RuntimePath
  command = "cargo test -p voltnuerongridd ws12_dr_hook_denies_when_mode_below_policy -- --nocapture"
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  output_excerpt = (($testOutput | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS8 mode-governance smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
