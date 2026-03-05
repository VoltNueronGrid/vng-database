param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [string]$OutputPath = "tests/kpi/results/autonomous-guardrail-smoke.json"
)

$ErrorActionPreference = "Stop"

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

function Invoke-JsonPost {
  param([string]$Uri, [hashtable]$Body)
  $json = $Body | ConvertTo-Json -Depth 8
  return Invoke-RestMethod -Method Post -Uri $Uri -Body $json -ContentType "application/json" -TimeoutSec 15
}

$result = @{
  base_url = $BaseUrl
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  checks = @()
  status = "passed"
}

try {
  $guardrails = Invoke-RestMethod -Method Get -Uri "$BaseUrl/api/v1/autonomous/guardrails" -TimeoutSec 15
  $result.checks += @{
    check = "guardrails_endpoint"
    ok = ($guardrails.status -eq "ok" -and $guardrails.policy_matrix.Count -gt 0)
    emergency_stop_enabled = $guardrails.emergency_stop_enabled
    policy_rule_count = $guardrails.policy_matrix.Count
  }

  $allowResp = Invoke-JsonPost -Uri "$BaseUrl/api/v1/autonomous/actions/authorize" -Body @{
    action = "performance_tune"
    scope = "session"
  }
  $result.checks += @{
    check = "authorize_allowed_action"
    ok = ($allowResp.decision -eq "allow")
    decision = $allowResp.decision
    reason = $allowResp.reason
  }

  [void](Invoke-JsonPost -Uri "$BaseUrl/api/v1/autonomous/emergency-stop" -Body @{
      enabled = $true
      reason = "kpi_guardrail_smoke"
      requested_by = "automation"
    })

  $blocked = $false
  try {
    [void](Invoke-JsonPost -Uri "$BaseUrl/api/v1/autonomous/actions/authorize" -Body @{
        action = "schema_change"
        scope = "database"
      })
  }
  catch {
    if ($_.Exception.Response.StatusCode.value__ -eq 503) {
      $blocked = $true
    }
  }
  $result.checks += @{
    check = "emergency_stop_blocks_actions"
    ok = $blocked
  }
}
finally {
  try {
    [void](Invoke-JsonPost -Uri "$BaseUrl/api/v1/autonomous/emergency-stop" -Body @{
        enabled = $false
        reason = "smoke_cleanup"
        requested_by = "automation"
      })
  }
  catch {
    # Best effort cleanup.
  }
}

foreach ($check in $result.checks) {
  if (-not $check.ok) {
    $result.status = "failed"
    break
  }
}

$result | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "Autonomous guardrail smoke result: $OutputPath ($($result.status))"

if ($result.status -eq "failed") {
  exit 1
}
exit 0
