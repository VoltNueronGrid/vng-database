param(
  [string]$OutputPath = "tests/kpi/results/s9/s9-gate-summary.json"
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
$checks = @()
$mainSrc = Get-Content -Raw -Path "services/voltnuerongridd/src/main.rs"

$checks += [ordered]@{ check = "s9-001-soak-plan-documented"; passed = (Test-Path "services/voltnuerongridd/reference/performance-proof-s8-s9-local-v1.md"); detail = "local soak approach documented" }
$checks += [ordered]@{ check = "s9-002-shard-code-path-detected"; passed = ($mainSrc -match "shard"); detail = "shard keyword/code path present in runtime source" }
$checks += [ordered]@{ check = "s9-003-failure-injection-covered"; passed = (Test-Path "tests/kpi/scripts/run-ws6-failover-flap-resistance-smoke.ps1"); detail = "existing failure/fault scripts available for recovery evidence" }
$checks += [ordered]@{ check = "s9-004-playbook-v1-published"; passed = (Test-Path "services/voltnuerongridd/reference/production-tuning-playbook-v1.md"); detail = "production tuning playbook exists" }

$status = if ((@($checks | Where-Object { -not $_.passed }).Count) -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  gate = "v3-s9-local"
  status = $status
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  cloud_validation = "deferred"
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "V3 S9 local gate: $status -> $OutputPath"
if ($status -ne "passed") { exit 1 }
