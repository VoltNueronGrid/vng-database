param(
  [string]$AstSourcePath = "crates/voltnuerongrid-sql/src/ast.rs",
  [string]$OutputPath = "tests/kpi/results/ws1/ansi-conformance-smoke.json"
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

if (!(Test-Path -Path $AstSourcePath)) {
  throw "AST source file not found at $AstSourcePath"
}

function Invoke-CargoTestCapture {
  param([string[]]$Arguments)
  $tempFile = [System.IO.Path]::GetTempFileName()
  try {
    $commandText = "cargo " + (($Arguments | ForEach-Object {
      if ($_ -match "\s") { '"' + $_ + '"' } else { $_ }
    }) -join " ")
    $process = Start-Process -FilePath "cmd.exe" -ArgumentList "/c", "$commandText > `"$tempFile`" 2>&1" -Wait -PassThru -NoNewWindow
    $text = if (Test-Path -Path $tempFile) { Get-Content -Path $tempFile -Raw } else { "" }
    $ok = ($text -match "test result: ok\." -and $text -notmatch "test result: FAILED" -and $text -notmatch "(?m)^error:")
    return [pscustomobject]@{ Ok = $ok; Text = $text; ExitCode = $process.ExitCode }
  } finally {
    if (Test-Path -Path $tempFile) { Remove-Item -Path $tempFile -Force -ErrorAction SilentlyContinue }
  }
}

$astContent = Get-Content -Raw -Path $AstSourcePath

# ── Static source checks ─────────────────────────────────────────────────────
$checks = [ordered]@{
  ast_source_present                     = ($null -ne $astContent -and $astContent.Length -gt 0)
  parse_one_entrypoint_present           = ($astContent -match 'pub fn parse_one')
  statement_enum_present                 = ($astContent -match 'pub enum Statement')
  select_variant_present                 = ($astContent -match 'Select\s*\(')
  insert_variant_present                 = ($astContent -match 'Insert\s*\(')
  update_variant_present                 = ($astContent -match 'Update\s*\(')
  delete_variant_present                 = ($astContent -match 'Delete\s*\(')
  create_table_variant_present           = ($astContent -match 'CreateTable\s*\(')
  begin_commit_rollback_present          = ($astContent -match 'Begin' -and $astContent -match 'Commit' -and $astContent -match 'Rollback')
  select_statement_has_where_clause      = ($astContent -match 'where_clause')
  select_statement_has_group_by          = ($astContent -match 'group_by')
  select_statement_has_having            = ($astContent -match 'having')
  select_statement_has_order_by          = ($astContent -match 'order_by')
  select_statement_has_limit             = ($astContent -match 'limit')
  insert_supports_column_list            = ($astContent -match 'columns.*Vec<String>')
  insert_supports_multi_row_values       = ($astContent -match 'values.*Vec<Vec<String>>')
  update_has_assignments                 = ($astContent -match 'assignments')
  ansi_conformance_test_module_present   = ($astContent -match 'mod ansi_conformance')
  conformance_tests_select_star          = ($astContent -match 'ansi_select_distinct_parses_as_select')
  conformance_tests_insert_multi_row     = ($astContent -match 'ansi_insert_multi_row_values')
  conformance_tests_update_multi_assign  = ($astContent -match 'ansi_update_multiple_assignments')
  conformance_tests_create_table_types   = ($astContent -match 'ansi_create_table_various_types')
  conformance_tests_unsupported_ddl      = ($astContent -match 'ansi_unsupported_ddl_falls_to_unknown')
  conformance_tests_transaction_control  = ($astContent -match 'ansi_transaction_control_statements')
}

# ── Cargo test run ────────────────────────────────────────────────────────────
$conformanceRun = Invoke-CargoTestCapture -Arguments @(
  "test", "-p", "voltnuerongrid-sql", "ansi_conformance", "--", "--test-threads=1"
)
$checks.ansi_conformance_cargo_tests_pass = $conformanceRun.Ok

$allAstRun = Invoke-CargoTestCapture -Arguments @(
  "test", "-p", "voltnuerongrid-sql", "--", "--test-threads=1"
)
$checks.all_sql_crate_tests_pass = $allAstRun.Ok

# Extract test count from cargo output
$testCountMatch = [regex]::Match($allAstRun.Text, 'test result: ok\. (\d+) passed')
$testCount = if ($testCountMatch.Success) { [int]$testCountMatch.Groups[1].Value } else { 0 }
$conformanceCountMatch = [regex]::Match($conformanceRun.Text, 'test result: ok\. (\d+) passed')
$conformanceCount = if ($conformanceCountMatch.Success) { [int]$conformanceCountMatch.Groups[1].Value } else { 0 }

$failedChecks = @($checks.GetEnumerator() | Where-Object { $_.Value -eq $false } | ForEach-Object { $_.Key })
$status = if ($failedChecks.Count -eq 0) { "passed" } else { "failed" }
$timestamp = (Get-Date -Format "yyyy-MM-ddTHH:mm:ssZ")

$artifact = [ordered]@{
  schema          = "vng-kpi-gate/1.0"
  gate            = "ws1-ansi-conformance-smoke"
  timestamp       = $timestamp
  status          = $status
  summary         = "ANSI SQL conformance harness: $conformanceCount conformance tests, $testCount total sql-crate tests"
  test_counts     = [ordered]@{
    ansi_conformance_tests = $conformanceCount
    all_sql_crate_tests    = $testCount
  }
  checks          = $checks
  failed_checks   = $failedChecks
  cargo_output    = [ordered]@{
    ansi_conformance = ($conformanceRun.Text -replace "`r`n", "`n") | Out-String
    all_sql_tests    = ($allAstRun.Text | Select-String "test result:" | Out-String)
  }
}

$artifact | ConvertTo-Json -Depth 6 | Set-Content -Path $OutputPath -Encoding UTF8
Write-Host "[$status] ANSI conformance smoke -> $OutputPath (conformance: $conformanceCount, total: $testCount)"
if ($failedChecks.Count -gt 0) {
  Write-Host "  FAILED checks: $($failedChecks -join ', ')"
  exit 1
}
