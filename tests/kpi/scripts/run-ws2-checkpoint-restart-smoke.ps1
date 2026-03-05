param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$OutputPath = "tests/kpi/results/ws2/ws2-checkpoint-restart-smoke.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

$oldErrorPref = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$testOutput = & cargo test -p voltnuerongrid-store preserves_continuity_after_checkpoint_and_restore -- --nocapture 2>&1
$exitCode = $LASTEXITCODE
$ErrorActionPreference = $oldErrorPref
$status = if ($exitCode -eq 0) { "passed" } else { "failed" }

$excerpt = @($testOutput | ForEach-Object { "$_" } | Select-Object -First 30) -join "`n"
$result = @{
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  status = $status
  command = "cargo test -p voltnuerongrid-store preserves_continuity_after_checkpoint_and_restore -- --nocapture"
  output_excerpt = $excerpt
}

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}
$result | ConvertTo-Json -Depth 10 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS2 checkpoint/restart smoke result: $OutputPath ($status)"

if ($status -eq "failed") { exit 1 }
exit 0
