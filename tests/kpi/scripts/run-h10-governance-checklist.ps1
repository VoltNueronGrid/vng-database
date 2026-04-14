param(
  [string]$OutputPath = "tests/kpi/results/h10/h10-governance-checklist.json"
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
  param([string]$Name, [bool]$Ok, [string]$Detail)
  $script:checks += [ordered]@{
    check = $Name
    ok = $Ok
    detail = $Detail
  }
}

function Resolve-FirstExistingPath {
  param([string[]]$Candidates)
  $valid = @($Candidates | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
  $found = $valid | Where-Object { Test-Path $_ } | Select-Object -First 1
  if ([string]::IsNullOrWhiteSpace($found)) {
    return $valid[0]
  }
  return $found
}

Ensure-OutputDir -PathValue $OutputPath

$start = Get-Date
$checks = @()

$charterPath = Resolve-FirstExistingPath -Candidates @(
  "reference/h10-arb-charter.md",
  "services/voltnuerongridd/reference/h10-arb-charter.md"
)
$policyPath = Resolve-FirstExistingPath -Candidates @(
  "reference/h10-deprecation-policy.md",
  "services/voltnuerongridd/reference/h10-deprecation-policy.md"
)
$registryPath = Resolve-FirstExistingPath -Candidates @(
  "reference/h10-deprecation-registry.md",
  "services/voltnuerongridd/reference/h10-deprecation-registry.md"
)
$checklistPath = Resolve-FirstExistingPath -Candidates @(
  "reference/h10-governance-checklist.md",
  "services/voltnuerongridd/reference/h10-governance-checklist.md"
)

Add-Check -Name "arb_charter_exists" -Ok (Test-Path $charterPath) -Detail $charterPath
Add-Check -Name "deprecation_policy_exists" -Ok (Test-Path $policyPath) -Detail $policyPath
Add-Check -Name "deprecation_registry_exists" -Ok (Test-Path $registryPath) -Detail $registryPath
Add-Check -Name "governance_checklist_exists" -Ok (Test-Path $checklistPath) -Detail $checklistPath

$registryHasEntry = $false
if (Test-Path $registryPath) {
  $registryContent = Get-Content -Raw -Path $registryPath
  $registryHasEntry = ($registryContent -match "\| D-001 \|")
}
Add-Check -Name "deprecation_registry_seeded" -Ok $registryHasEntry -Detail "Contains initial D-001 entry"

$status = if (($checks | Where-Object { -not $_.ok }).Count -eq 0) { "passed" } else { "failed" }
$finished = Get-Date

$artifact = [ordered]@{
  smoke = "h10-governance-checklist"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
if ($status -ne "passed") {
  Write-Error "H-10 governance checklist smoke failed."
  exit 1
}

Write-Host "H-10 governance checklist smoke passed. Artifact: $OutputPath"
