param(
  [string]$RepoRoot = "D:/by/polap-db",
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-dr-failover-smoke.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$command = "cargo test -p voltnuerongridd ws12_dr_hook_executes_failover_when_not_dry_run -- --nocapture"
$global:LASTEXITCODE = 0
$testOutput = & cargo test -p voltnuerongridd ws12_dr_hook_executes_failover_when_not_dry_run -- --nocapture 2>&1
$exitCode = $LASTEXITCODE
$status = if ($? -and $exitCode -eq 0) { "passed" } else { "failed" }

$result = [ordered]@{
  smoke = "ws6-dr-failover-path"
  status = $status
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  command = $command
  output_excerpt = (($testOutput | Select-Object -First 20) -join "`n")
}

$result | ConvertTo-Json -Depth 8 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS6 DR failover smoke result: $OutputPath ($status)"
if ($status -eq "failed") { exit 1 }
exit 0
