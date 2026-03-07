param(
  [string]$OutputPath = "tests/kpi/results/ws4/ws4-ingest-parser-smoke.json"
)

$ErrorActionPreference = "Stop"

function Ensure-OutputDir {
  param([string]$PathValue)
  $parent = Split-Path -Parent $PathValue
  if (![string]::IsNullOrWhiteSpace($parent) -and !(Test-Path -Path $parent)) {
    New-Item -Path $parent -ItemType Directory -Force | Out-Null
  }
}

Ensure-OutputDir -PathValue $OutputPath

$start = Get-Date
$checks = @()
$status = "passed"

# --- Contract checks against Rust source ---
$csvSrc = Get-Content -Path "crates/voltnuerongrid-ingest/src/csv.rs" -Raw
$jsonSrc = Get-Content -Path "crates/voltnuerongrid-ingest/src/json.rs" -Raw
$libSrc = Get-Content -Path "crates/voltnuerongrid-ingest/src/lib.rs" -Raw
$mainSrc = Get-Content -Path "services/voltnuerongridd/src/main.rs" -Raw

# 1. CsvConnector struct exists
$c1 = if ($csvSrc -match "pub struct CsvConnector") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "csv_connector_struct_exists"; status = $c1 }

# 2. CsvConnector implements IngestionConnector
$c2 = if ($csvSrc -match "impl IngestionConnector for CsvConnector") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "csv_connector_implements_trait"; status = $c2 }

# 3. load_csv method exists
$c3 = if ($csvSrc -match "pub fn load_csv") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "csv_load_method_exists"; status = $c3 }

# 4. JsonConnector struct exists
$c4 = if ($jsonSrc -match "pub struct JsonConnector") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "json_connector_struct_exists"; status = $c4 }

# 5. JsonConnector implements IngestionConnector
$c5 = if ($jsonSrc -match "impl IngestionConnector for JsonConnector") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "json_connector_implements_trait"; status = $c5 }

# 6. load_ndjson method exists
$c6 = if ($jsonSrc -match "pub fn load_ndjson") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "json_load_method_exists"; status = $c6 }

# 7. lib.rs exports csv module
$c7 = if ($libSrc -match "pub mod csv") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "lib_exports_csv_module"; status = $c7 }

# 8. lib.rs exports json module
$c8 = if ($libSrc -match "pub mod json") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "lib_exports_json_module"; status = $c8 }

# 9. IngestFormat::Csv variant exists
$c9 = if ($libSrc -match "Csv,") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ingest_format_csv_variant"; status = $c9 }

# 10. IngestFormat::Json variant exists
$c10 = if ($libSrc -match "Json,") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ingest_format_json_variant"; status = $c10 }

# 11. Main.rs has /api/v1/ingest/csv route
$c11 = if ($mainSrc -match "/api/v1/ingest/csv") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ingest_csv_route_exists"; status = $c11 }

# 12. Main.rs has /api/v1/ingest/json route
$c12 = if ($mainSrc -match "/api/v1/ingest/json") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ingest_json_route_exists"; status = $c12 }

# 13. Main.rs has /api/v1/ingest/status route
$c13 = if ($mainSrc -match "/api/v1/ingest/status") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ingest_status_route_exists"; status = $c13 }

# 14. ws4_csv_ingest unit test exists
$c14 = if ($mainSrc -match "ws4_csv_ingest_via_appstate") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ws4_csv_ingest_test_exists"; status = $c14 }

# 15. ws4_json_ingest unit test exists
$c15 = if ($mainSrc -match "ws4_json_ingest_via_appstate") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ws4_json_ingest_test_exists"; status = $c15 }

# 16. ws4_ingest_status unit test exists
$c16 = if ($mainSrc -match "ws4_ingest_status_counts_loaded_records") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ws4_ingest_status_test_exists"; status = $c16 }

foreach ($c in $checks) {
  if ($c.status -ne "passed") { $status = "failed" }
}

$finished = Get-Date
$artifact = [ordered]@{
  smoke = "ws4-ingest-parser-scaffold"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  total_checks = $checks.Count
  passed_checks = ($checks | Where-Object { $_.status -eq "passed" }).Count
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "WS4 ingest parser smoke: $OutputPath ($status) — $($checks.Count) checks"
if ($status -ne "passed") { exit 1 }
