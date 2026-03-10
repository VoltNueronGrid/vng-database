param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$AuthPath = "crates/voltnuerongrid-auth/src/lib.rs",
  [string]$SecurityJsonPath = "reference/config-contracts/ws14/security-control-config.json",
  [string]$SecurityYamlPath = "reference/config-contracts/ws14/security-control-config.yaml",
  [string]$SecurityPropertiesPath = "reference/config-contracts/ws14/security-control-config.properties",
  [string]$RuntimeSmokeScriptPath = "tests/kpi/scripts/run-h05-kms-region-failover-runtime-smoke.ps1",
  [string]$RuntimeSmokeArtifactPath = "tests/kpi/results/h05/h05-kms-region-failover-runtime-smoke.json",
  [string]$OutputPath = "tests/kpi/results/h05/h05-kms-region-failover-evidence.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

function Resolve-RepoPath {
  param([string]$PathValue)

  if ([System.IO.Path]::IsPathRooted($PathValue)) {
    return $PathValue
  }
  return [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $PathValue))
}

$AuthPath = Resolve-RepoPath -PathValue $AuthPath
$SecurityJsonPath = Resolve-RepoPath -PathValue $SecurityJsonPath
$SecurityYamlPath = Resolve-RepoPath -PathValue $SecurityYamlPath
$SecurityPropertiesPath = Resolve-RepoPath -PathValue $SecurityPropertiesPath
$RuntimeSmokeScriptPath = Resolve-RepoPath -PathValue $RuntimeSmokeScriptPath
$RuntimeSmokeArtifactPath = Resolve-RepoPath -PathValue $RuntimeSmokeArtifactPath
$OutputPath = Resolve-RepoPath -PathValue $OutputPath

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

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
    return [pscustomobject]@{ Ok = $ok; Text = $text; ExitCode = $process.ExitCode }
  } finally {
    if (Test-Path -Path $tempFile) { Remove-Item -Path $tempFile -Force -ErrorAction SilentlyContinue }
  }
}

function Get-ArtifactStatus {
  param([string]$ArtifactPath)

  if (!(Test-Path -Path $ArtifactPath)) {
    return "missing_artifact"
  }

  try {
    $json = Get-Content -Raw -Path $ArtifactPath | ConvertFrom-Json
    if ($null -ne $json.status) {
      return [string]$json.status
    }
    return "present"
  } catch {
    return "invalid_artifact"
  }
}

function Invoke-PowerShellArtifact {
  param(
    [string]$ScriptPath,
    [string]$ArtifactPath
  )

  $process = Start-Process -FilePath "powershell.exe" `
    -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $ScriptPath, "-OutputPath", $ArtifactPath) `
    -WorkingDirectory $RepoRoot `
    -NoNewWindow `
    -PassThru `
    -Wait
  return $process.ExitCode
}

$cases = @(
  [ordered]@{
    step = "primary_region_resolution"
    arguments = @("test", "-p", "voltnuerongrid-auth", "h05_resolves_primary_kms_region_when_available", "--", "--nocapture")
    command = "cargo test -p voltnuerongrid-auth h05_resolves_primary_kms_region_when_available -- --nocapture"
  },
  [ordered]@{
    step = "secondary_region_failover_resolution"
    arguments = @("test", "-p", "voltnuerongrid-auth", "h05_falls_back_to_secondary_kms_region_when_primary_missing", "--", "--nocapture")
    command = "cargo test -p voltnuerongrid-auth h05_falls_back_to_secondary_kms_region_when_primary_missing -- --nocapture"
  },
  [ordered]@{
    step = "duplicate_failover_region_rejected"
    arguments = @("test", "-p", "voltnuerongrid-auth", "h05_rejects_duplicate_kms_failover_env_names", "--", "--nocapture")
    command = "cargo test -p voltnuerongrid-auth h05_rejects_duplicate_kms_failover_env_names -- --nocapture"
  },
  [ordered]@{
    step = "all_regions_unavailable_rejected"
    arguments = @("test", "-p", "voltnuerongrid-auth", "h05_fails_when_all_kms_regions_are_unavailable", "--", "--nocapture")
    command = "cargo test -p voltnuerongrid-auth h05_fails_when_all_kms_regions_are_unavailable -- --nocapture"
  }
)

$steps = @()
$overallPassed = $true

$runtimeStarted = Get-Date
$runtimeExitCode = Invoke-PowerShellArtifact -ScriptPath $RuntimeSmokeScriptPath -ArtifactPath $RuntimeSmokeArtifactPath
$runtimeFinished = Get-Date
$runtimeArtifactStatus = Get-ArtifactStatus -ArtifactPath $RuntimeSmokeArtifactPath
$runtimeArtifact = if (Test-Path -Path $RuntimeSmokeArtifactPath) {
  try { Get-Content -Raw -Path $RuntimeSmokeArtifactPath | ConvertFrom-Json } catch { $null }
} else { $null }
$runtimePassed = ($runtimeArtifactStatus -eq "passed")
if (-not $runtimePassed) { $overallPassed = $false }
$steps += [ordered]@{
  step = "runtime_region_outage_failover"
  command = "powershell -NoProfile -ExecutionPolicy Bypass -File $RuntimeSmokeScriptPath -OutputPath $RuntimeSmokeArtifactPath"
  status = if ($runtimePassed) { "passed" } else { "failed" }
  duration_ms = [int](($runtimeFinished - $runtimeStarted).TotalMilliseconds)
  output_excerpt = "artifact=$RuntimeSmokeArtifactPath; exit_code=$runtimeExitCode; status=$runtimeArtifactStatus"
}

foreach ($case in $cases) {
  $started = Get-Date
  $run = Invoke-CargoTestCapture -Arguments $case.arguments
  $finished = Get-Date
  $passed = ($run.Ok -and $run.ExitCode -eq 0)
  if (-not $passed) { $overallPassed = $false }
  $steps += [ordered]@{
    step = $case.step
    command = $case.command
    status = if ($passed) { "passed" } else { "failed" }
    duration_ms = [int](($finished - $started).TotalMilliseconds)
    output_excerpt = (($run.Text -split "`r?`n" | Select-Object -First 8) -join "`n")
  }
}

$authRaw = Get-Content -Path $AuthPath -Raw
$securityJsonRaw = Get-Content -Path $SecurityJsonPath -Raw
$securityYamlRaw = Get-Content -Path $SecurityYamlPath -Raw
$securityPropertiesRaw = Get-Content -Path $SecurityPropertiesPath -Raw

$contractChecks = [ordered]@{
  auth_contract_declares_failover_envs = ($authRaw -match 'kms_failover_key_ref_envs:\s*Vec<String>')
  auth_contract_declares_provider_trait = ($authRaw -match 'pub trait KmsKeyProvider')
  auth_contract_declares_provider_adapter = ($authRaw -match 'pub struct InMemoryKmsProviderAdapter')
  auth_contract_declares_provider_resolution_method = ($authRaw -match 'pub fn resolve_kms_key_ref_with_provider<')
  security_json_declares_failover_envs = ($securityJsonRaw -match '"kmsFailoverKeyRefEnvs"\s*:\s*\[')
  security_yaml_declares_failover_envs = ($securityYamlRaw -match '(?m)^\s*kmsFailoverKeyRefEnvs:\s*$')
  security_properties_declares_failover_envs = ($securityPropertiesRaw -match '(?m)^\s*security\.kmsFailoverKeyRefEnvs\s*=\s*.+$')
}

if (($contractChecks.Values | Where-Object { $_ -eq $false }).Count -gt 0) {
  $overallPassed = $false
}

$artifact = [ordered]@{
  pack = "h05-kms-region-failover-evidence"
  status = if ($overallPassed) { "passed" } else { "failed" }
  hardening_item = "H-05"
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  implementation = [ordered]@{
    kms_resolution_mode = "provider_backed"
    outage_simulation_contract = "preserved"
    runtime_provider_drill_mode = if ($null -ne $runtimeArtifact -and $null -ne $runtimeArtifact.observations.provider_drill_mode) { [string]$runtimeArtifact.observations.provider_drill_mode } else { "unknown" }
    configured_key_refs = if ($null -ne $runtimeArtifact -and $null -ne $runtimeArtifact.observations.configured_key_refs) { @($runtimeArtifact.observations.configured_key_refs) } else { @() }
  }
  evidence_scope = @(
    "primary_region_key_resolution",
    "secondary_region_failover",
    "duplicate_failover_guardrail",
    "all_regions_unavailable_guardrail"
  )
  contract_checks = $contractChecks
  steps = $steps
}

$artifact | ConvertTo-Json -Depth 12 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "H05 KMS region failover evidence result: $OutputPath ($($artifact.status))"
if ($artifact.status -eq "failed") { exit 1 }
exit 0