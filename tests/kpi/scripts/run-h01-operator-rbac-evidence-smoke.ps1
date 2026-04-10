param(
  [string]$OutputPath = "tests/kpi/results/h01/h01-operator-rbac-evidence-smoke.json"
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

$src = "tests/kpi/results/ws5/operator-auth-smoke.json"
$checks = @()
$status = "failed"

if (Test-Path $src) {
  $j = Get-Content -Raw $src | ConvertFrom-Json
  $checksTotal = 0
  $hasContractEvidence = $false
  if ($null -ne $j.checks_total) {
    $checksTotal = [int]$j.checks_total
  } elseif ($null -ne $j.checks) {
    $checksTotal = @($j.checks).Count
  }
  if (($null -ne $j.command -and -not [string]::IsNullOrWhiteSpace([string]$j.command)) -or ($null -ne $j.security_contract_checks)) {
    $hasContractEvidence = $true
  }
  $checks += [ordered]@{ name = "ws5_operator_auth_smoke_present"; passed = $true; detail = $src }
  $checks += [ordered]@{ name = "ws5_operator_auth_smoke_status_passed"; passed = ([string]$j.status -eq "passed"); detail = "status=$($j.status)" }
  $checks += [ordered]@{ name = "ws5_operator_auth_smoke_evidence_present"; passed = (($checksTotal -ge 1) -or $hasContractEvidence); detail = "checks_total=$checksTotal;contract_evidence=$hasContractEvidence" }
  if ((@($checks | Where-Object { -not $_.passed }).Count) -eq 0) { $status = "passed" }
} else {
  $checks += [ordered]@{ name = "ws5_operator_auth_smoke_present"; passed = $false; detail = "missing:$src" }
}

$artifact = [ordered]@{
  smoke = "h01-operator-rbac-evidence"
  status = $status
  checks_passed = @($checks | Where-Object { $_.passed }).Count
  checks_total = $checks.Count
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "H-01 operator RBAC evidence smoke: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
