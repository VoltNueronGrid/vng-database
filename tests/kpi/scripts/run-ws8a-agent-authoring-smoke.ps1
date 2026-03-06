param(
  [string]$RuntimePath = "services/voltnuerongridd/src/main.rs",
  [string]$OutputPath = "tests/kpi/results/ws8a/ws8a-agent-authoring-smoke.json"
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

$outputs = @()
$checks = [ordered]@{
  schema_change_guardrail_supervised = (
    ($runtimeRaw -match 'action:\s*"schema_change"') -and
    ($runtimeRaw -match 'action:\s*"schema_change"[\s\S]*?required_mode:\s*AutonomousMode::Supervised')
  )
  plugin_install_guardrail_supervised = (
    ($runtimeRaw -match 'action:\s*"plugin_install"') -and
    ($runtimeRaw -match 'action:\s*"plugin_install"[\s\S]*?required_mode:\s*AutonomousMode::Supervised')
  )
}

$global:LASTEXITCODE = 0
$runtimeTestOutput = & cargo test -p voltnuerongridd append_action_record_writes_to_history -- --nocapture 2>&1
$outputs += $runtimeTestOutput
$checks.action_record_history_test = ($? -and $LASTEXITCODE -eq 0)

$global:LASTEXITCODE = 0
$pluginPositiveOutput = & cargo test -p voltnuerongrid-plugins registers_valid_package -- --nocapture 2>&1
$outputs += $pluginPositiveOutput
$checks.plugin_authoring_registers_valid_package = ($? -and $LASTEXITCODE -eq 0)

$global:LASTEXITCODE = 0
$pluginPolicyOutput = & cargo test -p voltnuerongrid-plugins rejects_package_when_custom_hook_fails -- --nocapture 2>&1
$outputs += $pluginPolicyOutput
$checks.plugin_authoring_guardrail_hook_rejects_invalid_owner = ($? -and $LASTEXITCODE -eq 0)

$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  smoke = "ws8a-agent-authoring-workflow"
  status = $status
  runtime_path = $RuntimePath
  commands = @(
    "cargo test -p voltnuerongridd append_action_record_writes_to_history -- --nocapture",
    "cargo test -p voltnuerongrid-plugins registers_valid_package -- --nocapture",
    "cargo test -p voltnuerongrid-plugins rejects_package_when_custom_hook_fails -- --nocapture"
  )
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  output_excerpt = (($outputs | Select-Object -First 40) -join "`n")
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS8A agent authoring smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
