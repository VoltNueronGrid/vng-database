param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$ParityRoot = "tests/parity/legacy",
  [string]$OutputPath = "tests/kpi/results/parity/legacy-aggregation-gap-report.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

$supported = @(
  "SUM",
  "COUNT",
  "MIN",
  "MAX",
  "AVG",
  "COUNT_DISTINCT",
  "MEDIAN",
  "STDDEV",
  "VARIANCE",
  "PERCENTILE"
)
$p2Stubbed = @(
  "APPROX_COUNT_DISTINCT",
  "TOP_N",
  "BOTTOM_N"
)

function Read-Bucket {
  param([string]$Path)
  $values = @()
  foreach ($line in Get-Content -Path $Path) {
    $item = $line.Trim()
    if ($item -and -not $item.StartsWith("#")) {
      $values += $item.ToUpperInvariant()
    }
  }
  return $values
}

$buckets = @(
  @{ id = "P0"; file = Join-Path $ParityRoot "p0-required-aggregations.txt" },
  @{ id = "P1"; file = Join-Path $ParityRoot "p1-required-aggregations.txt" },
  @{ id = "P2"; file = Join-Path $ParityRoot "p2-required-aggregations.txt" }
)

$bucketReports = @()
$globalMissing = @()

foreach ($bucket in $buckets) {
  $required = Read-Bucket -Path $bucket.file
  $missing = @($required | Where-Object { $_ -notin $supported -and $_ -notin $p2Stubbed })
  $stubbed = @($required | Where-Object { $_ -in $p2Stubbed })
  $present = @($required | Where-Object { $_ -in $supported })
  $globalMissing += $missing
  $bucketReports += @{
    bucket = $bucket.id
    required_count = $required.Count
    present_count = $present.Count
    stubbed_count = $stubbed.Count
    stubbed = $stubbed
    missing_count = $missing.Count
    missing = $missing
  }
}

$result = @{
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  supported_count = $supported.Count
  status = if ($globalMissing.Count -eq 0) { "passed" } else { "gaps_present" }
  buckets = $bucketReports
  global_missing = @($globalMissing | Sort-Object -Unique)
  p2_stubbed = $p2Stubbed
}

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$result | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "Legacy aggregation gap report: $OutputPath ($($result.status))"

exit 0
