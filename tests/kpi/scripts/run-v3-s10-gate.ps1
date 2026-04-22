param(
  [string]$OutputPath = "tests/kpi/results/s10/s10-gate-summary.json"
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

$checks += [ordered]@{ check = "s10-001-java-driver-baseline"; passed = (Test-Path "drivers/voltnuerongrid-driver-java/pom.xml"); detail = "java driver baseline scaffold exists" }
$checks += [ordered]@{ check = "s10-002-node-driver-baseline"; passed = (Test-Path "drivers/voltnuerongrid-driver-typescript/package.json"); detail = "node/typescript driver baseline exists" }
$checks += [ordered]@{ check = "s10-003-cffi-poc"; passed = (Test-Path "drivers/voltnuerongrid-driver-cffi-poc/README.md"); detail = "C/C++ FFI strategy/PoC artifact exists" }
$checks += [ordered]@{ check = "s10-004-deno-adapter"; passed = (Test-Path "drivers/voltnuerongrid-driver-typescript/src/denoAdapter.ts"); detail = "Deno adapter source exists" }
$checks += [ordered]@{ check = "s10-005-perl-feasibility"; passed = (Test-Path "services/voltnuerongridd/reference/perl-binding-feasibility-s10-v1.md"); detail = "Perl feasibility report exists" }

$status = if ((@($checks | Where-Object { -not $_.passed }).Count) -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  gate = "v3-s10"
  status = $status
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  cloud_validation = "deferred"
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "V3 S10 gate: $status -> $OutputPath"
if ($status -ne "passed") { exit 1 }
