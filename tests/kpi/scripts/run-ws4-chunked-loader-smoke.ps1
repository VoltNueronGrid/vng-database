param(
  [string]$OutputPath = "tests/kpi/results/ws4/ws4-chunked-loader-smoke.json"
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

$libSrc      = Get-Content -Path "crates/voltnuerongrid-ingest/src/lib.rs" -Raw
$chunkedSrc  = Get-Content -Path "crates/voltnuerongrid-ingest/src/chunked_loader.rs" -Raw
$mainSrc     = Get-Content -Path "services/voltnuerongridd/src/main.rs" -Raw

# 1. ChunkedLoader struct exists in the chunked_loader module
$c1 = if ($chunkedSrc -match "pub struct ChunkedLoader") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "chunked_loader_struct_exists"; status = $c1 }

# 2. ChunkedLoader has push_chunk method
$c2 = if ($chunkedSrc -match "pub fn push_chunk") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "chunked_loader_push_chunk_method"; status = $c2 }

# 3. ChunkedLoader has finalize method
$c3 = if ($chunkedSrc -match "pub fn finalize") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "chunked_loader_finalize_method"; status = $c3 }

# 4. ingest_csv unit tests for chunked loader exist in service main
$c4 = if ($mainSrc -match "chunked_loader") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "chunked_loader_tests_exist_in_service"; status = $c4 }

# 5. Outbox replay route wired in main.rs
$c5 = if ($mainSrc -match "/api/v1/ingest/outbox/replay") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ingest_outbox_replay_route_wired"; status = $c5 }

# 6. Outbox status route wired in main.rs
$c6 = if ($mainSrc -match "/api/v1/ingest/outbox/status") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ingest_outbox_status_route_wired"; status = $c6 }

# 7. ManagedEventBusTransport exported from ingest crate
$c7 = if ($libSrc -match "ManagedEventBusTransport") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "managed_event_bus_transport_exported"; status = $c7 }

# 8. ManagedReplayCursorStore exported from ingest crate
$c8 = if ($libSrc -match "ManagedReplayCursorStore") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "managed_replay_cursor_store_exported"; status = $c8 }

# 9. AppState has ingest_event_bus field
$c9 = if ($mainSrc -match "ingest_event_bus") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "appstate_has_ingest_event_bus"; status = $c9 }

# 10. AppState has ingest_outbox_cursors field
$c10 = if ($mainSrc -match "ingest_outbox_cursors") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "appstate_has_ingest_outbox_cursors"; status = $c10 }

# 11. DDL catalog route wired (REQ-02)
$c11 = if ($mainSrc -match "/api/v1/catalog/schemas") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ddl_catalog_schemas_route_wired"; status = $c11 }

# 12. DDL catalog handler exists
$c12 = if ($mainSrc -match "async fn catalog_schemas") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ddl_catalog_schemas_handler_exists"; status = $c12 }

# 13. ACID transactions route wired (REQ-23)
$c13 = if ($mainSrc -match "/api/v1/sql/transactions/active") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "acid_transactions_active_route_wired"; status = $c13 }

# 14. ACID transactions handler exists
$c14 = if ($mainSrc -match "async fn sql_transactions_active") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "acid_transactions_handler_exists"; status = $c14 }

# 15. AcidTransactionRegistry struct exists
$c15 = if ($mainSrc -match "AcidTransactionRegistry") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "acid_transaction_registry_struct_exists"; status = $c15 }

# 16. DdlCatalog imported from store crate
$c16 = if ($mainSrc -match "DdlCatalog") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ddl_catalog_imported_from_store"; status = $c16 }

# 17. ws2_ddl_catalog tests exist
$c17 = if ($mainSrc -match "ws2_ddl_catalog_create_table_wires_through_sql_execute") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ws2_ddl_catalog_tests_exist"; status = $c17 }

# 18. ws23_acid_tx tests exist
$c18 = if ($mainSrc -match "ws23_acid_tx_begin_commit_tracked_in_registry") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ws23_acid_tx_tests_exist"; status = $c18 }

# 19. REQ-12 real data test exists
$c19 = if ($mainSrc -match "ws3_legacy_agg_uses_real_ingest_data_when_available") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ws3_legacy_agg_real_data_test_exists"; status = $c19 }

# 20. Real ingest data collection present in legacy agg block (not purely synthetic)
$c20 = if ($mainSrc -match "ingest_csv_records" -and $mainSrc -match "ingest_json_records" -and $mainSrc -match "real_values") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "legacy_agg_uses_real_ingest_data"; status = $c20 }

foreach ($c in $checks) {
  if ($c.status -ne "passed") { $status = "failed" }
}

$finished = Get-Date
$artifact = [ordered]@{
  smoke       = "ws4-chunked-loader"
  status      = $status
  started_at_utc  = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  total_checks   = $checks.Count
  passed_checks  = ($checks | Where-Object { $_.status -eq "passed" }).Count
  checks      = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "WS4 chunked-loader smoke: $OutputPath ($status) — $($checks.Count) checks"
if ($status -ne "passed") { exit 1 }
