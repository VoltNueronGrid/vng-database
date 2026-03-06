param(
  [string]$TargetsPath = "tests/kpi/config/targets.yaml",
  [string]$ScenarioPath = "tests/kpi/scenarios/htap-mixed-throughput.yaml",
  [string]$OutputPath = "tests/kpi/results/ws3/ws3-htap-target-contract-smoke.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

function Get-YamlScalarValue {
  param(
    [string[]]$Lines,
    [string]$Section,
    [string]$SubSection,
    [string]$Key,
    [string]$DefaultValue
  )

  $inSection = $false
  $inSubSection = [string]::IsNullOrWhiteSpace($SubSection)
  foreach ($line in $Lines) {
    if (!$inSection) {
      if ($line -match "^\s*${Section}:\s*$") { $inSection = $true }
      continue
    }
    if ($line -match "^[A-Za-z0-9_]+:\s*$") { break }

    if (-not [string]::IsNullOrWhiteSpace($SubSection)) {
      if (-not $inSubSection -and $line -match "^\s{2}${SubSection}:\s*$") {
        $inSubSection = $true
        continue
      }
      if ($inSubSection -and $line -match "^\s{2}[A-Za-z0-9_]+:\s*$" -and $line -notmatch "^\s{2}${SubSection}:\s*$") { break }
      if (-not $inSubSection) { continue }
      if ($line -match "^\s{4}${Key}:\s*(.+)\s*$") { return $Matches[1].Trim() }
    } else {
      if ($line -match "^\s{2}${Key}:\s*(.+)\s*$") { return $Matches[1].Trim() }
    }
  }
  return $DefaultValue
}

Ensure-OutputDir -PathValue $OutputPath
if (!(Test-Path -Path $TargetsPath)) { throw "Targets file missing: $TargetsPath" }
if (!(Test-Path -Path $ScenarioPath)) { throw "Scenario file missing: $ScenarioPath" }

$targetsLines = Get-Content -Path $TargetsPath
$scenarioLines = Get-Content -Path $ScenarioPath

$targetReadQps = [double](Get-YamlScalarValue -Lines $targetsLines -Section "kpis" -SubSection "htap_mixed_throughput" -Key "read_qps_min" -DefaultValue "0")
$targetWriteTps = [double](Get-YamlScalarValue -Lines $targetsLines -Section "kpis" -SubSection "htap_mixed_throughput" -Key "write_tps_min" -DefaultValue "0")

$scenarioReadQps = 0.0
$scenarioWriteTps = 0.0
foreach ($line in $scenarioLines) {
  if ($line -match "^\s*target:\s*([0-9]+(?:\.[0-9]+)?)\s*$") {
    if ($scenarioReadQps -eq 0.0) { $scenarioReadQps = [double]$Matches[1] }
    elseif ($scenarioWriteTps -eq 0.0) { $scenarioWriteTps = [double]$Matches[1] }
  }
}

$checks = [ordered]@{
  targets_file_present = $true
  scenario_file_present = $true
  target_read_qps_positive = ($targetReadQps -gt 0)
  target_write_tps_positive = ($targetWriteTps -gt 0)
  scenario_read_qps_matches_target = ($scenarioReadQps -eq $targetReadQps)
  scenario_write_tps_matches_target = ($scenarioWriteTps -eq $targetWriteTps)
}

$status = if ((@($checks.Values | Where-Object { $_ -eq $false }).Count) -eq 0) { "passed" } else { "failed" }
$artifact = [ordered]@{
  smoke = "ws3-htap-target-contract"
  status = $status
  timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
  targets_path = $TargetsPath
  scenario_path = $ScenarioPath
  targets = [ordered]@{
    read_qps_min = $targetReadQps
    write_tps_min = $targetWriteTps
  }
  scenario_assertions = [ordered]@{
    read_qps_target = $scenarioReadQps
    write_tps_target = $scenarioWriteTps
  }
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 10 | Set-Content -Path $OutputPath
Write-Host "WS3 HTAP target contract smoke result: $OutputPath ($status)"
if ($status -ne "passed") { exit 1 }
