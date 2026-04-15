param(
  [string]$OutputPath = "tests/kpi/results/ws5/ws5-closure-gate-summary.json",
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [switch]$IncludeRuntimeSmokes
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

$start = Get-Date
$ws5SummaryPath = "tests/kpi/results/ws5/ws5-gate-summary.json"
$ws5BadgePath = "tests/kpi/results/gates/ws5-gate-badge.json"
$ws5OperatorSmokePath = "tests/kpi/results/ws5/operator-auth-smoke.json"
$ws5TenantAuditPath = "tests/kpi/results/ws5/tenant-audit-runtime-smoke.json"

$runs = @()
$status = "passed"

try {
  $global:LASTEXITCODE = 0
  & "tests/kpi/scripts/run-ws5-gate.ps1" -OutputPath $ws5SummaryPath -BaseUrl $BaseUrl -IncludeRuntimeSmokes:$IncludeRuntimeSmokes 2>&1 | Out-Null
  if (-not $?) {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws5-gate"; status = "failed"; detail = "script_invocation_failed"; artifact = $ws5SummaryPath }
  } elseif ($global:LASTEXITCODE -ne 0) {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws5-gate"; status = "failed"; detail = "exit_code=$global:LASTEXITCODE"; artifact = $ws5SummaryPath }
  } else {
    $runs += [ordered]@{ pack = "ws5-gate"; status = "passed"; detail = "ok"; artifact = $ws5SummaryPath }
  }
} catch {
  $status = "failed"
  $runs += [ordered]@{ pack = "ws5-gate"; status = "failed"; detail = $_.Exception.Message; artifact = $ws5SummaryPath }
}

if ($status -eq "passed") {
  try {
    $global:LASTEXITCODE = 0
    & "tests/kpi/scripts/run-ws5-gate-badge.ps1" -SummaryPath $ws5SummaryPath -OutputPath $ws5BadgePath 2>&1 | Out-Null
    if (-not $?) {
      $status = "failed"
      $runs += [ordered]@{ pack = "ws5-gate-badge"; status = "failed"; detail = "script_invocation_failed"; artifact = $ws5BadgePath }
    } elseif ($global:LASTEXITCODE -ne 0) {
      $status = "failed"
      $runs += [ordered]@{ pack = "ws5-gate-badge"; status = "failed"; detail = "exit_code=$global:LASTEXITCODE"; artifact = $ws5BadgePath }
    } else {
      $runs += [ordered]@{ pack = "ws5-gate-badge"; status = "passed"; detail = "ok"; artifact = $ws5BadgePath }
    }
  } catch {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws5-gate-badge"; status = "failed"; detail = $_.Exception.Message; artifact = $ws5BadgePath }
  }
}

$checks = [ordered]@{
  ws5_gate_passed = $false
  ws5_security_smoke_passed = $false
  ws5_tenant_audit_runtime_passed = $false
  ws5_badge_green = $false
}

if ($status -eq "passed") {
  foreach ($path in @($ws5SummaryPath, $ws5BadgePath, $ws5OperatorSmokePath)) {
    if (!(Test-Path -Path $path)) {
      $status = "failed"
      $runs += [ordered]@{ pack = "ws5-artifact-presence"; status = "failed"; detail = "missing:$path"; artifact = $path }
    }
  }
  if ($IncludeRuntimeSmokes -and !(Test-Path -Path $ws5TenantAuditPath)) {
    $status = "failed"
    $runs += [ordered]@{ pack = "ws5-artifact-presence"; status = "failed"; detail = "missing:$ws5TenantAuditPath"; artifact = $ws5TenantAuditPath }
  }
}

if ($status -eq "passed") {
  $summary = Get-Content -Raw -Path $ws5SummaryPath | ConvertFrom-Json
  $badge = Get-Content -Raw -Path $ws5BadgePath | ConvertFrom-Json
  $securitySmoke = Get-Content -Raw -Path $ws5OperatorSmokePath | ConvertFrom-Json

  $checks.ws5_gate_passed = ([string]$summary.status -eq "passed")
  $checks.ws5_security_smoke_passed = ([string]$securitySmoke.status -eq "passed")
  $checks.ws5_badge_green = ([string]$badge.color -eq "green")

  if ($IncludeRuntimeSmokes) {
    $tenantAudit = Get-Content -Raw -Path $ws5TenantAuditPath | ConvertFrom-Json
    $checks.ws5_tenant_audit_runtime_passed = ([string]$tenantAudit.status -eq "passed")
  } else {
    $checks.ws5_tenant_audit_runtime_passed = $true
  }

  if (($checks.Values | Where-Object { $_ -eq $false }).Count -gt 0) {
    $status = "failed"
  }
}

$finished = Get-Date
$summaryOut = [ordered]@{
  gate = "ws5-closure-gate"
  status = $status
  validation_posture = if ($status -eq "passed") { "ready_for_validation" } else { "blocked" }
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  artifacts = [ordered]@{
    ws5_gate = $ws5SummaryPath
    ws5_badge = $ws5BadgePath
    ws5_security_smoke = $ws5OperatorSmokePath
    ws5_tenant_audit_runtime = if ($IncludeRuntimeSmokes) { $ws5TenantAuditPath } else { $null }
  }
  checks = $checks
  runs = $runs
}

$summaryOut | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "WS5 closure gate summary: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }