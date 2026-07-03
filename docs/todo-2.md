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

Status: Completed
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
  - `routes_cross_dialect_scalar_alias_to_olap`
  - `q1_small_table_cross_dialect_function_stays_olap`

Notes:
- Cross-dialect scalar aliases are rewritten/routed to compatible execution paths.
- Execution remains accessible through Runtime, Studio, MCP, and IDE clients.

## Enhancement-3

Status: Completed (plugin-backed first phase)
Completion: 100%

Delivered:
- Pivot function plugin implemented with plugin ID `function.pivot`.
- Pivot function exposed as pluggable UDF `pivot_table`.
- Plugin lifecycle controls added:
  - `POST /api/v1/plugins/enable`
  - `POST /api/v1/plugins/disable`
- Enabling/installing plugin registers `pivot_table`; disabling/uninstalling removes it.
- Unit/integration coverage:
  - `plug4_plugin_disable_and_enable_toggles_active_listing`
  - `plug4_pivot_plugin_enable_disable_manages_udf_lifecycle`
