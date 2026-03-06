param(
  [string]$RepoRoot = "D:/by/polap-db",
  [int]$Iterations = 3,
  [int]$P95MaxMs = 6000,
  [string]$OutputPath = "tests/kpi/results/ws6/ws6-reconcile-latency-envelope-smoke.json"
)

$ErrorActionPreference = "Stop"
Set-Location $RepoRoot

$outputDir = Split-Path -Parent $OutputPath
if ($outputDir -and !(Test-Path $outputDir)) {
  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$runs = @()
$overallPassed = $true

for ($i = 1; $i -le $Iterations; $i++) {
  $started = Get-Date
  $global:LASTEXITCODE = 0
  $output = & cargo test -p voltnuerongridd ws12_reconcile_marks_critical_resolved -- --nocapture 2>&1
  $exitCode = $LASTEXITCODE
  $finished = Get-Date
  $durationMs = [int](($finished - $started).TotalMilliseconds)
  $passed = ($? -and $exitCode -eq 0)
  if (-not $passed) { $overallPassed = $false }
  $runs += [ordered]@{
    iteration = $i
    status = if ($passed) { "passed" } else { "failed" }
    duration_ms = $durationMs
    output_excerpt = (($output | Select-Object -First 6) -join "`n")
  }
}

$durations = @($runs | ForEach-Object { [int]$_.duration_ms } | Sort-Object)
$p95Index = [Math]::Min($durations.Count - 1, [Math]::Floor(($durations.Count - 1) * 0.95))
$p95 = if ($durations.Count -gt 0) { $durations[$p95Index] } else { 0 }
$envelopePass = ($p95 -le $P95MaxMs)
if (-not $envelopePass) { $overallPassed = $false }

$status = if ($overallPassed) { "passed" } else { "failed" }
$result = [ordered]@{
  smoke = "ws6-reconcile-latency-envelope"
  status = $status
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  command = "cargo test -p voltnuerongridd ws12_reconcile_marks_critical_resolved -- --nocapture"
  iterations = $Iterations
  latency_envelope = [ordered]@{
    p95_ms = $p95
    p95_max_ms = $P95MaxMs
    pass = $envelopePass
  }
  run_results = $runs
}

$result | ConvertTo-Json -Depth 12 | Out-File -FilePath $OutputPath -Encoding utf8
Write-Host "WS6 reconcile latency envelope smoke result: $OutputPath ($status)"
if ($status -eq "failed") { exit 1 }
exit 0
