param(
  [string]$OutputPath = "tests/kpi/results/ws2/ws2-index-constraint-smoke.json"
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
$storeSrc = Get-Content -Path "crates/voltnuerongrid-store/src/index.rs" -Raw
$constraintSrc = Get-Content -Path "crates/voltnuerongrid-store/src/constraints.rs" -Raw
$mainSrc = Get-Content -Path "services/voltnuerongridd/src/main.rs" -Raw

# 1. IndexManager struct exists
$c1 = if ($storeSrc -match "pub struct IndexManager") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "index_manager_struct_exists"; status = $c1 }

# 2. BTreeIndex struct exists
$c2 = if ($storeSrc -match "pub struct BTreeIndex") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "btree_index_struct_exists"; status = $c2 }

# 3. IndexDescriptor struct exists
$c3 = if ($storeSrc -match "pub struct IndexDescriptor") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "index_descriptor_struct_exists"; status = $c3 }

# 4. create_index method exists
$c4 = if ($storeSrc -match "pub fn create_index") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "create_index_method_exists"; status = $c4 }

# 5. drop_index method exists
$c5 = if ($storeSrc -match "pub fn drop_index") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "drop_index_method_exists"; status = $c5 }

# 6. lookup method exists
$c6 = if ($storeSrc -match "pub fn lookup") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "lookup_method_exists"; status = $c6 }

# 7. range_scan method exists
$c7 = if ($storeSrc -match "pub fn range_scan") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "range_scan_method_exists"; status = $c7 }

# 8. UniqueViolation error variant exists
$c8 = if ($storeSrc -match "UniqueViolation") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "unique_violation_error_exists"; status = $c8 }

# 9. ConstraintManager struct exists
$c9 = if ($constraintSrc -match "pub struct ConstraintManager") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "constraint_manager_struct_exists"; status = $c9 }

# 10. ConstraintKind enum with PrimaryKey
$c10 = if ($constraintSrc -match "PrimaryKey") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "constraint_kind_primary_key"; status = $c10 }

# 11. ConstraintKind enum with Unique
$c11 = if ($constraintSrc -match "Unique,") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "constraint_kind_unique"; status = $c11 }

# 12. ConstraintKind enum with NotNull
$c12 = if ($constraintSrc -match "NotNull") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "constraint_kind_not_null"; status = $c12 }

# 13. validate method exists
$c13 = if ($constraintSrc -match "pub fn validate") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "constraint_validate_method_exists"; status = $c13 }

# 14. record_committed_value method exists
$c14 = if ($constraintSrc -match "pub fn record_committed_value") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "record_committed_value_method_exists"; status = $c14 }

# 15. Main.rs has /api/v1/store/indexes route
$c15 = if ($mainSrc -match "/api/v1/store/indexes") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "store_indexes_route_exists"; status = $c15 }

# 16. Main.rs has /api/v1/store/constraints route
$c16 = if ($mainSrc -match "/api/v1/store/constraints") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "store_constraints_route_exists"; status = $c16 }

# 17. Store handlers route through shared runtime RBAC and preserve operator-managed writes
$c17 = if ($mainSrc -match "fn require_store_runtime_principal" -and $mainSrc -match "async fn store_list_indexes[\s\S]*?require_store_runtime_principal" -and $mainSrc -match "async fn store_create_index[\s\S]*?require_operator_privilege") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "store_handlers_require_runtime_rbac"; status = $c17 }

# 18. Tenant store grants exist in the default RBAC matrix
$c18 = if ($mainSrc -match "tenants/\{tenant\}/store/indexes" -and $mainSrc -match "tenants/\{tenant\}/store/constraints/validate") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "tenant_store_grants_exist"; status = $c18 }

# 19. ws2_index unit test exists
$c19 = if ($mainSrc -match "ws2_index_create_lookup_drop_lifecycle") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ws2_index_unit_test_exists"; status = $c19 }

# 20. ws2_constraint unit test exists
$c20 = if ($mainSrc -match "ws2_constraint_pk_not_null_via_appstate") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "ws2_constraint_unit_test_exists"; status = $c20 }

# 21. tenant-scoped WS2 metadata tests exist
$c21 = if ($mainSrc -match "store_list_indexes_filters_to_tenant_namespace" -and $mainSrc -match "store_index_lookup_denies_cross_tenant_index_lookup") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "tenant_store_tests_exist"; status = $c21 }

# 22. constraint validation enforces tenant-scoped reads
$c22 = if ($mainSrc -match "store_validate_constraint_accepts_tenant_scoped_table" -and $mainSrc -match "ensure_store_table_access") { "passed" } else { "failed" }
$checks += [ordered]@{ check = "tenant_constraint_scope_enforced"; status = $c22 }

foreach ($c in $checks) {
  if ($c.status -ne "passed") { $status = "failed" }
}

$finished = Get-Date
$artifact = [ordered]@{
  smoke = "ws2-index-constraint-scaffold"
  status = $status
  started_at_utc = $start.ToUniversalTime().ToString("o")
  finished_at_utc = $finished.ToUniversalTime().ToString("o")
  duration_ms = [int](($finished - $start).TotalMilliseconds)
  total_checks = $checks.Count
  passed_checks = ($checks | Where-Object { $_.status -eq "passed" }).Count
  checks = $checks
}

$artifact | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath
Write-Host "WS2 index/constraint smoke: $OutputPath ($status) - $($checks.Count) checks"
if ($status -ne "passed") { exit 1 }
