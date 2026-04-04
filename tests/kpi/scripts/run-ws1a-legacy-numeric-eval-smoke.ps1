param(
  [string]$RepoRoot = ".",
  [string]$OutputPath = "tests/kpi/results/ws1a/ws1a-legacy-numeric-eval-smoke.json"
)

$ErrorActionPreference = "Stop"
Push-Location $RepoRoot
try {
  $start = Get-Date
  $checks = @()
  $status = "passed"

  $rustSrc = Get-Content -Path "crates/voltnuerongrid-sql/src/legacy_aggregations.rs" -Raw
  $c1 = if ($rustSrc -match "pub fn eval_legacy_numeric_aggregation") { "passed" } else { "failed" }
  $checks += [ordered]@{ check = "eval_legacy_numeric_aggregation_declared"; status = $c1 }

  $global:LASTEXITCODE = 0
  $null = & cargo test -p voltnuerongrid-sql "legacy_aggregations::numeric_tests" -- --quiet 2>&1
  $c2 = if ($global:LASTEXITCODE -eq 0) { "passed" } else { "failed" }
  $checks += [ordered]@{ check = "legacy_numeric_unit_tests_pass"; status = $c2 }

  foreach ($c in $checks) {
    if ($c.status -ne "passed") { $status = "failed" }
  }

  $finished = Get-Date
  $parent = Split-Path -Parent $OutputPath
  if ($parent -and !(Test-Path $parent)) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
  }

  $artifact = [ordered]@{
    smoke = "ws1a-legacy-numeric-eval"
    status = $status
    started_at_utc = $start.ToUniversalTime().ToString("o")
    finished_at_utc = $finished.ToUniversalTime().ToString("o")
    duration_ms = [int](($finished - $start).TotalMilliseconds)
    total_checks = $checks.Count
    passed_checks = ($checks | Where-Object { $_.status -eq "passed" }).Count
    checks = $checks
  }

  $artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
  Write-Host "WS1A legacy numeric eval smoke: $OutputPath ($status)"
  if ($status -ne "passed") { exit 1 }
}
finally {
  Pop-Location
}
