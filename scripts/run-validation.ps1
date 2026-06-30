param(
    [string]$BaseUrl = "http://127.0.0.1:8080"
)

$ErrorActionPreference = "Continue"
$results = @{
    ws5_passed = 0
    ws5_failed = 0
    ws6_passed = 0
    ws6_failed = 0
}

Write-Host "================== WS5 AUTH+RBAC VALIDATION =================="
Write-Host "Running operator auth tests..."

$ws5_tests = @(
    "operator_auth_allows_request_when_admin_key_not_configured",
    "operator_auth_denies_request_when_admin_key_required_but_not_provided",
    "operator_auth_denies_request_when_admin_key_invalid",
    "operator_auth_enforces_role_matrix",
    "ws5_rbac_matrix_operator_user_admin_scoped"
)

foreach ($test in $ws5_tests) {
    Write-Host "- Running: $test"
    $output = & cargo test -p voltnuerongridd $test -- --nocapture 2>&1
    if ($output -match "test result: ok") {
        Write-Host "  ✓ PASSED"
        $results.ws5_passed++
    } else {
        Write-Host "  ✗ FAILED"
        $results.ws5_failed++
    }
}

Write-Host ""
Write-Host "================== WS6 FAILOVER VALIDATION =================="
Write-Host "Running failover contract tests..."

$ws6_tests = @(
    "ws6_failover_contract_status_route",
    "ws6_failover_contract_simulate_route",
    "ws6_failover_contract_critical_signals",
    "ws6_failover_contract_rto_rpo_targets",
    "ws6_failover_contract_handoff_report"
)

foreach ($test in $ws6_tests) {
    Write-Host "- Running: $test"
    $output = & cargo test -p voltnuerongridd $test -- --nocapture 2>&1
    if ($output -match "test result: ok") {
        Write-Host "  ✓ PASSED"
        $results.ws6_passed++
    } else {
        Write-Host "  ✗ FAILED"
        $results.ws6_failed++
    }
}

Write-Host ""
Write-Host "================== VALIDATION SUMMARY =================="
Write-Host "WS5 Auth:    $($results.ws5_passed) passed, $($results.ws5_failed) failed"
Write-Host "WS6 Failover: $($results.ws6_passed) passed, $($results.ws6_failed) failed"
Write-Host "Total:       $($results.ws5_passed + $results.ws6_passed) passed, $($results.ws5_failed + $results.ws6_failed) failed"

if ($results.ws5_failed -eq 0 -and $results.ws6_failed -eq 0) {
    Write-Host ""
    Write-Host "✅ ALL VALIDATIONS PASSED!"
    exit 0
} else {
    Write-Host ""
    Write-Host "❌ SOME VALIDATIONS FAILED"
    exit 1
}
