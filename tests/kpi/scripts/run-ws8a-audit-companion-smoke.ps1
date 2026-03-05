param(
  [string]$OutputPath = "tests/kpi/results/ws8a/audit-companion-smoke.json",
  [string]$ReportPath = "tests/kpi/results/ws8a/audit-companion-report.json"
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
Ensure-OutputDir -PathValue $ReportPath

$start = Get-Date
$command = "cargo run -p voltnuerongrid-audit-companion -- --audit-file tests/kpi/fixtures/ws8a/audit-events-sample.json --action-file tests/kpi/fixtures/ws8/control-plane-records-sample.json --trace-id atrace-101 --action autonomous_action_authorize --out $ReportPath"
$outputLines = @()
$exitCode = 1
$validationOk = $false

try {
  $outputLines = & cargo run -p voltnuerongrid-audit-companion -- --audit-file tests/kpi/fixtures/ws8a/audit-events-sample.json --action-file tests/kpi/fixtures/ws8/control-plane-records-sample.json --trace-id atrace-101 --action autonomous_action_authorize --out $ReportPath 2>&1
  $exitCode = $LASTEXITCODE
  if ($exitCode -eq 0 -and (Test-Path $ReportPath)) {
    $report = Get-Content -Raw -Path $ReportPath | ConvertFrom-Json
    $validationOk = (
      $report.status -eq "ok" -and
      $report.total_audit_events -eq 1 -and
      $report.total_action_records -eq 0 -and
      $report.linked_trace_matches -eq 0 -and
      $report.audit_events[0].action -eq "autonomous_action_authorize"
    )
  }
} catch {
  $outputLines += $_.Exception.Message
  $exitCode = 1
}

$status = if ($exitCode -eq 0 -and $validationOk) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "ws8a-audit-companion-flow"
  status = $status
  command = $command
  report_path = $ReportPath
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  validation_ok = $validationOk
  output_excerpt = (($outputLines | Select-Object -First 20) -join "`n")
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath

if ($status -ne "passed") {
  Write-Error "WS8A audit companion smoke failed."
  exit 1
}

Write-Host "WS8A audit companion smoke passed. Artifact: $OutputPath"
