param(
  [string]$OutputPath = "tests/kpi/results/h01/h01-emergency-stop-evidence-smoke.json"
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

$legacySrc = "tests/kpi/results/20260305-h01/autonomous-guardrail-smoke.json"
$ws8Src = "tests/kpi/results/ws8/ws8-guardrail-policy-smoke.json"
$checks = @()
$status = "failed"

if (Test-Path $legacySrc) {
  $l = Get-Content -Raw $legacySrc | ConvertFrom-Json
  $checksTotal = 0
  if ($null -ne $l.checks_total) {
    $checksTotal = [int]$l.checks_total
  } elseif ($null -ne $l.checks) {
    $checksTotal = @($l.checks).Count
  }
  $checks += [ordered]@{ name = "legacy_h01_guardrail_smoke_status_passed"; passed = ([string]$l.status -eq "passed"); detail = "status=$($l.status)" }
  $checks += [ordered]@{ name = "legacy_h01_guardrail_checks_nonzero"; passed = ($checksTotal -ge 1); detail = "checks_total=$checksTotal" }
} else {
  $checks += [ordered]@{ name = "legacy_h01_guardrail_smoke_present"; passed = $false; detail = "missing:$legacySrc" }
}

if (Test-Path $ws8Src) {
  $w = Get-Content -Raw $ws8Src | ConvertFrom-Json
  $checks += [ordered]@{ name = "ws8_guardrail_policy_smoke_status_passed"; passed = ([string]$w.status -eq "passed"); detail = "status=$($w.status)" }
} else {
  $checks += [ordered]@{ name = "ws8_guardrail_policy_smoke_present"; passed = $false; detail = "missing:$ws8Src" }
}

if ((@($checks | Where-Object { -not $_.passed }).Count) -eq 0) { $status = "passed" }

$artifact = [ordered]@{
  smoke = "h01-emergency-stop-evidence"
  status = $status
  checks_passed = @($checks | Where-Object { $_.passed }).Count
  checks_total = $checks.Count
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "H-01 emergency stop evidence smoke: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
