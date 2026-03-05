param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$OutputPath = "tests/kpi/results/ws2/store-durability-smoke.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

$oldErrorPref = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$testOutput = & cargo test -p voltnuerongrid-store -- --nocapture 2>&1
$exitCode = $LASTEXITCODE
$ErrorActionPreference = $oldErrorPref
$status = if ($exitCode -eq 0) { "passed" } else { "failed" }
$summaryLines = @($testOutput | ForEach-Object { "$_" } | Select-Object -First 20)
$summaryText = ($summaryLines -join "`n")
if ($summaryText.Length -gt 4000) {
  $summaryText = $summaryText.Substring(0, 4000)
}

$result = @{
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  status = $status
  crate = "voltnuerongrid-store"
  command = "cargo test -p voltnuerongrid-store -- --nocapture"
  output_excerpt = $summaryText
}

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}
$result | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "Store durability smoke result: $OutputPath ($status)"

if ($status -eq "failed") {
  exit 1
}
exit 0
