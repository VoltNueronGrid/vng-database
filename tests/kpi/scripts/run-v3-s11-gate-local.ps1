param(
  [string]$OutputPath = "tests/kpi/results/s11/s11-gate-summary.json"
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

$checks += [ordered]@{ check = "s11-001-e2e-scenario-pack"; passed = (Test-Path "tests/kpi/scenarios/s11-e2e-pack/core-local-e2e.yaml"); detail = "end-to-end scenario pack exists" }
$checks += [ordered]@{ check = "s11-002-compatibility-matrix"; passed = (Test-Path "services/voltnuerongridd/reference/versioned-compatibility-matrix-s11-v1.md"); detail = "versioned compatibility matrix exists" }
$checks += [ordered]@{ check = "s11-003-security-closure"; passed = (Test-Path "services/voltnuerongridd/reference/security-compliance-closure-s11-v1.md"); detail = "security/compliance closure checklist exists" }
$checks += [ordered]@{ check = "s11-004-rc-packaging-guides"; passed = (Test-Path "services/voltnuerongridd/reference/rc-packaging-installation-guides-s11-v1.md"); detail = "RC packaging and install guide exists" }

$status = if ((@($checks | Where-Object { -not $_.passed }).Count) -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  gate = "v3-s11-local"
  status = $status
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  cloud_validation = "deferred"
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "V3 S11 local gate: $status -> $OutputPath"
if ($status -ne "passed") { exit 1 }
