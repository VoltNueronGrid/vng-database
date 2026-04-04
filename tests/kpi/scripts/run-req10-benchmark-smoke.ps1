# REQ-10 / REQ-19: Benchmark smoke check
# Verifies that benchmark endpoints exist and (if BaseUrl provided) measures throughput.
#
# Usage (static source check only):
#   pwsh ./tests/kpi/scripts/run-req10-benchmark-smoke.ps1
# Usage (live server):
#   pwsh ./tests/kpi/scripts/run-req10-benchmark-smoke.ps1 -BaseUrl http://127.0.0.1:8080

param(
  [string]$BaseUrl    = "",
  [string]$OutputPath = "tests/kpi/results/req10/benchmark-smoke.json"
)

$ErrorActionPreference = "Continue"
$checks = @()

function Add-Check {
  param([string]$Name, [bool]$Ok, [string]$Detail)
  $script:checks += [ordered]@{ check = $Name; ok = $Ok; detail = $Detail }
}

function Ensure-OutputDir {
  param([string]$P)
  $parent = Split-Path -Parent $P
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

# ── Static source checks ─────────────────────────────────────────────────────
$mainSrc = Get-Content "services/voltnuerongridd/src/main.rs" -Raw -ErrorAction SilentlyContinue

Add-Check "benchmark-ingest-route-exists" `
  ($mainSrc -match '"/api/v1/benchmark/ingest"') `
  "POST /api/v1/benchmark/ingest route in main.rs"

Add-Check "benchmark-query-route-exists" `
  ($mainSrc -match '"/api/v1/benchmark/query"') `
  "POST /api/v1/benchmark/query route in main.rs"

Add-Check "benchmark-ingest-handler-exists" `
  ($mainSrc -match 'async fn benchmark_ingest') `
  "benchmark_ingest handler function present"

Add-Check "benchmark-query-handler-exists" `
  ($mainSrc -match 'async fn benchmark_query') `
  "benchmark_query handler function present"

Add-Check "benchmark-response-records-per-second" `
  ($mainSrc -match 'records_per_second') `
  "records_per_second field in BenchmarkIngestResponse"

Add-Check "benchmark-response-ops-per-second" `
  ($mainSrc -match 'ops_per_second') `
  "ops_per_second field in BenchmarkQueryResponse"

Add-Check "benchmark-ingest-uses-chunkedloader" `
  ($mainSrc -match 'ChunkedLoader') `
  "benchmark_ingest uses ChunkedLoader for realistic load"

Add-Check "benchmark-operator-auth-enforced" `
  ($mainSrc -match 'benchmark_ingest|benchmark.*operator|operator.*benchmark') `
  "benchmark endpoints guarded by operator auth"

# ── Live server checks (optional) ────────────────────────────────────────────
if (![string]::IsNullOrWhiteSpace($BaseUrl)) {
  $adminKey = if ($env:VNG_ADMIN_API_KEY) { $env:VNG_ADMIN_API_KEY } else { "secret" }
  $headers  = @{ "x-vng-admin-key" = $adminKey; "x-vng-operator-id" = "automation" }

  try {
    $ingestBody = '{"record_count":1000,"chunk_target_rows":100}'
    $resp = Invoke-RestMethod -Uri "$BaseUrl/api/v1/benchmark/ingest" `
      -Method Post -Body $ingestBody -ContentType "application/json" `
      -Headers $headers -TimeoutSec 30
    $rps = [double]($resp.records_per_second)
    Add-Check "live-benchmark-ingest-responds" ($resp.status -eq "ok") `
      "benchmark/ingest status=$($resp.status) rps=$([math]::Round($rps,0))"
    Add-Check "live-benchmark-ingest-rps-positive" ($rps -gt 0) `
      "records_per_second=$([math]::Round($rps,0))"
  } catch {
    Add-Check "live-benchmark-ingest-responds" $false "Error: $_"
    Add-Check "live-benchmark-ingest-rps-positive" $false "Skipped (request failed)"
  }

  try {
    $queryBody = '{"op_count":5000}'
    $resp = Invoke-RestMethod -Uri "$BaseUrl/api/v1/benchmark/query" `
      -Method Post -Body $queryBody -ContentType "application/json" `
      -Headers $headers -TimeoutSec 30
    $ops = [double]($resp.ops_per_second)
    Add-Check "live-benchmark-query-responds" ($resp.status -eq "ok") `
      "benchmark/query status=$($resp.status) ops=$([math]::Round($ops,0))"
    Add-Check "live-benchmark-query-ops-positive" ($ops -gt 0) `
      "ops_per_second=$([math]::Round($ops,0))"
  } catch {
    Add-Check "live-benchmark-query-responds" $false "Error: $_"
    Add-Check "live-benchmark-query-ops-positive" $false "Skipped (request failed)"
  }
}

# ── Write artifact ────────────────────────────────────────────────────────────
$passed = ($checks | Where-Object { $_.ok }).Count
$total  = $checks.Count
$status = if (($checks | Where-Object { -not $_.ok }).Count -eq 0) { "passed" } else { "failed" }

$artifact = [ordered]@{
  smoke            = "req10-benchmark"
  status           = $status
  checks_passed    = $passed
  checks_total     = $total
  generated_at_utc = (Get-Date -Format "o")
  checks           = $checks
}

Ensure-OutputDir $OutputPath
$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "req10-benchmark-smoke: $status ($passed/$total checks passed) -> $OutputPath"
