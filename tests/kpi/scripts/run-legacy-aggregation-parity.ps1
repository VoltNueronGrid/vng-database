param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$ManifestPath = "tests/parity/legacy/required-aggregations.txt",
  [string]$OutputPath = "tests/kpi/results/parity/legacy-aggregation-parity.json"
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

$required = @()
foreach ($line in Get-Content -Path $ManifestPath) {
  $item = $line.Trim()
  if ($item -and -not $item.StartsWith("#")) {
    $required += $item.ToUpperInvariant()
  }
}

$missing = @($required | Where-Object { $_ -notin $supported })
$extra = @($supported | Where-Object { $_ -notin $required })

$status = if ($missing.Count -eq 0) { "passed" } else { "failed" }
$result = @{
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  status = $status
  manifest_path = $ManifestPath
  required_count = $required.Count
  supported_count = $supported.Count
  missing = $missing
  extra_supported = $extra
}

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$result | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "Legacy aggregation parity result: $OutputPath ($status)"

if ($status -eq "failed") {
  exit 1
}
exit 0
