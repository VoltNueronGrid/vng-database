param(
  [string]$OutputPath = "tests/kpi/results/h01/h01-autonomy-policy-evidence-smoke.json"
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

$policySrc = "tests/kpi/results/ws8/ws8-mode-governance-smoke.json"
$guardrailSrc = "tests/kpi/results/ws8/ws8-guardrail-policy-smoke.json"
$checks = @()
$status = "failed"

if (Test-Path $policySrc) {
  $p = Get-Content -Raw $policySrc | ConvertFrom-Json
  $checks += [ordered]@{ name = "ws8_mode_governance_smoke_status_passed"; passed = ([string]$p.status -eq "passed"); detail = "status=$($p.status)" }
} else {
  $checks += [ordered]@{ name = "ws8_mode_governance_smoke_present"; passed = $false; detail = "missing:$policySrc" }
}

if (Test-Path $guardrailSrc) {
  $g = Get-Content -Raw $guardrailSrc | ConvertFrom-Json
  $checks += [ordered]@{ name = "ws8_guardrail_policy_smoke_status_passed"; passed = ([string]$g.status -eq "passed"); detail = "status=$($g.status)" }
} else {
  $checks += [ordered]@{ name = "ws8_guardrail_policy_smoke_present"; passed = $false; detail = "missing:$guardrailSrc" }
}

$checks += [ordered]@{ name = "ws8_policy_artifacts_present"; passed = ((Test-Path $policySrc) -and (Test-Path $guardrailSrc)); detail = "$policySrc,$guardrailSrc" }

if ((@($checks | Where-Object { -not $_.passed }).Count) -eq 0) { $status = "passed" }

$artifact = [ordered]@{
  smoke = "h01-autonomy-policy-evidence"
  status = $status
  checks_passed = @($checks | Where-Object { $_.passed }).Count
  checks_total = $checks.Count
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "H-01 autonomy policy evidence smoke: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
