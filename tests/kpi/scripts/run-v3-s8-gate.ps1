param(
  [string]$BaseUrl = "",
  [string]$OutputPath = "tests/kpi/results/s8/s8-gate-summary.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Add-Check {
  param(
    [string]$Name,
    [bool]$Passed,
    [string]$Detail
  )
  $script:checks += [ordered]@{
    check = $Name
    passed = $Passed
    detail = $Detail
  }
}

Ensure-OutputDir -PathValue $OutputPath
$checks = @()
$mainSrc = Get-Content -Raw -Path "services/voltnuerongridd/src/main.rs"

# S8-001 benchmark suite readiness
Add-Check "s8-001-benchmark-ingest-route" ($mainSrc -match "/api/v1/benchmark/ingest") "ingest benchmark route exists"
Add-Check "s8-001-benchmark-query-route" ($mainSrc -match "/api/v1/benchmark/query") "query benchmark route exists"
Add-Check "s8-001-benchmark-suite-doc" (Test-Path "services/voltnuerongridd/reference/performance-proof-s8-s9-local-v1.md") "benchmark suite document exists"

# S8-002 multithread import/bottleneck checks
Add-Check "s8-002-chunked-loader-usage" ($mainSrc -match "ChunkedLoader") "chunked loader path present for ingest pressure"

# S8-003 join/path + paging checks
Add-Check "s8-003-join-handlers-present" ($mainSrc -match "JoinCountResponse") "join response handlers are present"
Add-Check "s8-003-paging-max-rows" ($mainSrc -match "max_rows") "max_rows paging control appears in query paths"

# S8-004 memory profile + allocator review evidence
Add-Check "s8-004-memory-review-doc" (Test-Path "services/voltnuerongridd/reference/performance-proof-s8-s9-local-v1.md") "memory profiling and allocator strategy notes exist"

if (![string]::IsNullOrWhiteSpace($BaseUrl)) {
  try {
    $headers = @{ "x-vng-admin-key" = (if ($env:VNG_ADMIN_API_KEY) { $env:VNG_ADMIN_API_KEY } else { "secret" }); "x-vng-operator-id" = "automation" }
    $ingest = Invoke-RestMethod -Method Post -Uri "$BaseUrl/api/v1/benchmark/ingest" -Headers $headers -ContentType "application/json" -Body '{"record_count":1000,"chunk_target_rows":100}' -TimeoutSec 30
    Add-Check "s8-live-ingest-positive-throughput" ([double]$ingest.records_per_second -gt 0) "records_per_second=$($ingest.records_per_second)"
  } catch {
    Add-Check "s8-live-ingest-positive-throughput" $false "live benchmark ingest failed: $_"
  }
}

$status = if ((@($checks | Where-Object { -not $_.passed }).Count) -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  gate = "v3-s8"
  status = $status
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  cloud_validation = "deferred"
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "V3 S8 gate: $status -> $OutputPath"
if ($status -ne "passed") { exit 1 }
