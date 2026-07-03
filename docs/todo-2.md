# todo-2

## Enhancement-1

Status: Completed
Completion: 100%

Delivered:
- `information_schema.schemata` includes system schemas for each created database.
- Admin schema tree includes `public`, `pg_catalog`, and `information_schema` for empty and populated databases.
- Unit/integration coverage in service tests:
  - `enhancement1_schema_tree_exposes_system_schemas_for_empty_database`
  - `helpers::information_schema::tests::synth_is_schemata_includes_each_created_database`

## Enhancement-2

Status: Completed (catalog/discovery scope)
Completion: 100%

Delivered:
- Expanded cross-dialect built-in function catalog metadata.
- Runtime endpoint: `GET /api/v1/sql/functions`.
- Studio client method: `listSqlFunctions()`.
- MCP method: `tools/functions`.
- IDE extension method: `listSqlFunctions()` via shared API contract.
- Unit/integration coverage:
  - `enhancement2_sql_functions_returns_vendor_aliases`
  - `test_capabilities_default`
  - `mcp_008_operator_functions_tool_proxies_runtime_catalog`

Notes:
- This completion reflects catalog/discovery and cross-surface execution paths.
- Full OLTP scalar function execution parity is tracked separately.
