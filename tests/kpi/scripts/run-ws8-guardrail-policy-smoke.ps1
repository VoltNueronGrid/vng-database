param(
  [string]$RuntimePath = "services/voltnuerongridd/src/main.rs",
  [string]$OutputPath = "tests/kpi/results/ws8/ws8-guardrail-policy-smoke.json"
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

$checks = [ordered]@{
  autonomous_guardrails_route = (
    ($runtimeRaw -match '/api/v1/autonomous/guardrails') -and
    ($runtimeRaw -match 'get\(autonomous_guardrails\)')
  )
  autonomous_emergency_stop_route = (
    ($runtimeRaw -match '/api/v1/autonomous/emergency-stop') -and
    ($runtimeRaw -match 'post\(autonomous_emergency_stop\)')
  )
  autonomous_authorize_route = (
    ($runtimeRaw -match '/api/v1/autonomous/actions/authorize') -and
    ($runtimeRaw -match 'post\(authorize_autonomous_action\)')
  )
  emergency_stop_enforced = ($runtimeRaw -match 'if state\.emergency_stop\.get\(\)')
  guardrail_rules_present = ($runtimeRaw -match 'default_guardrail_rules\(\)')
}

$testOutput = Invoke-CargoTestCapture -Arguments @("test", "-p", "voltnuerongridd", "action_trace_id_is_generated", "--", "--nocapture")
$checks.trace_id_generation_test = $testOutput.Ok

$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  smoke = "ws8-guardrail-policy"
  status = $status
  runtime_path = $RuntimePath
  command = "cargo test -p voltnuerongridd action_trace_id_is_generated -- --nocapture"
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
  output_excerpt = (($testOutput.Text -split "`n" | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS8 guardrail policy smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
