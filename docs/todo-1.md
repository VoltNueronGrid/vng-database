# todo-1

## Enhancement-1: Show `information_schema` in every created database

Status: Completed
Completion: 100%

What is being implemented:
- Make `information_schema.schemata` aware of the database catalog so every created database exposes the standard system schemas (`public`, `pg_catalog`, `information_schema`).
- Keep the existing global compatibility rows so older tooling does not lose visibility.
- Add regression coverage for newly created databases that have no user tables yet.

Implemented:
- `information_schema.schemata` now includes system schemas for every database in `database_catalog`, including empty databases.
- Admin schema tree (`/api/v1/admin/schema/tree`) now emits `public`, `pg_catalog`, and `information_schema` for every created database.
- Tests added:
	- `enhancement1_schema_tree_exposes_system_schemas_for_empty_database`
	- `helpers::information_schema::tests::synth_is_schemata_includes_each_created_database`

Current behavior summary:
- The virtual catalog already exists.
- The visibility gap is that system schemas were only being synthesized from objects already present in the DDL catalog.
- Newly created empty databases could therefore appear to have no `information_schema` rows yet.

## Enhancement-2: Cross-dialect built-in function catalog

Status: Completed
Completion: 100%

Goal:
- Build a comprehensive compatibility catalog for Oracle, PostgreSQL, MySQL, and Snowflake built-ins.
- Cover both OLTP and OLAP query paths as far as the current engine supports them.
- Make the supported function surface discoverable and gradually executable by family.

Implemented so far:
- Expanded the built-in function registry with common cross-dialect aliases and vendor names.
- Added parser/tokenizer recognition for the most common null-handling, string, and date-time compatibility functions.
- Added runtime endpoint: `GET /api/v1/sql/functions`.
- Added Studio client support: `listSqlFunctions()`.
- Added IDE contract entry: `sql_functions` in `common-api-contract.json`.
- Added IDE client helper: `listSqlFunctions()` in VSCode/Cursor extension client.
- Added MCP capability and route: `tools/functions` (operator scope).
- Added tests:
	- `enhancement2_sql_functions_returns_vendor_aliases`
	- MCP capability test updated for new `functions` tool.
	- MCP integration test: `mcp_008_operator_functions_tool_proxies_runtime_catalog`

Execution status:
- OLAP/DataFusion path: the listed built-in and compatibility functions are queryable through normal SQL execution where DataFusion supports them.
- OLTP path: cross-dialect scalar aliases are normalized and routed to the compatible execution path, including cost-router guards to avoid accidental OLTP demotion.
- Cross-surface execution:
	- Runtime endpoint available: `/api/v1/sql/functions`
	- Studio client callable: `listSqlFunctions()`
	- MCP callable: `tools/functions`
	- IDE extension callable: `listSqlFunctions()` via shared contract

Additional parity tests:
- `voltnuerongrid_exec::tests::routes_cross_dialect_scalar_alias_to_olap`
- `voltnuerongrid_exec::tests::q1_small_table_cross_dialect_function_stays_olap`

Planned function families:
- Null-handling and conditional: `COALESCE`, `NULLIF`, `IFNULL`, `NVL`, `NVL2`, `IFF`, `DECODE`, `ZEROIFNULL`, `NULLIFZERO`, `GREATEST`, `LEAST`
- String: `LOWER`, `UPPER`, `TRIM`, `LTRIM`, `RTRIM`, `SUBSTR`, `SUBSTRING`, `INSTR`, `POSITION`, `CONCAT`, `CONCAT_WS`, `REPLACE`, `SPLIT_PART`, `LPAD`, `RPAD`, `REVERSE`, `REGEXP_MATCH`, `REGEXP_REPLACE`, `REGEXP_SUBSTR`, `REGEXP_INSTR`, `REGEXP_COUNT`
- Date/time: `NOW`, `CURRENT_TIMESTAMP`, `CURRENT_DATE`, `CURRENT_TIME`, `DATE_TRUNC`, `DATE_PART`, `DATEDIFF`, `DATEADD`, `TO_DATE`, `TO_CHAR`, `TO_TIMESTAMP`, `TO_TIMESTAMP_NTZ`, `TO_TIMESTAMP_TZ`
- Math: `ABS`, `CEIL`, `CEILING`, `FLOOR`, `ROUND`, `POWER`, `SQRT`, `MOD`, `SIGN`, `EXP`, `LN`, `LOG`
- Aggregates: `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, `STRING_AGG`, `LISTAGG`, `ARRAY_AGG`, `JSON_AGG`, `ANY_VALUE`, `APPROX_COUNT_DISTINCT`
- JSON / semi-structured: `JSON_EXTRACT`, `JSON_OBJECT`, `JSON_ARRAY`, `JSONB_EXTRACT_PATH`, `OBJECT_CONSTRUCT`, `OBJECT_AGG`, `ARRAY_CONSTRUCT`, `MAP_CONSTRUCT`
- Type conversion: `CAST`, `TRY_CAST`, `TO_NUMBER`, `TO_VARCHAR`, `TO_VARIANT`, `TRY_TO_NUMBER`, `TRY_TO_DATE`

## Enhancement-3: Pivot operator

Status: Completed (plugin-backed first phase)
Completion: 100%

Scope:
- Add a `PIVOT` operator that behaves like spreadsheet-style pivoting over tables.
- Implement as a pluggable function plugin that can be enabled or disabled.
- Provide runtime-level lifecycle controls and tests.

Detailed design proposal:

1. Feature goal
- Transform row-oriented grouped data into dynamic or explicit cross-tab columns.
- Provide Excel-like pivot behavior for SQL consumers in Studio, MCP, and IDE clients.

2. Input contract
- Source relation: any table/subquery that produces columns used in pivoting.
- Required parameters:
	- `pivot_column`: column whose distinct values become output columns.
	- `value_expression`: measure expression to aggregate.
	- `aggregate_fn`: one of `SUM`, `COUNT`, `AVG`, `MIN`, `MAX`, `ANY_VALUE`.
- Optional parameters:
	- `group_by_columns`: columns that remain as row keys.
	- `pivot_values`: explicit list of values (static pivot) or omitted (auto-discovery).
	- `value_alias_prefix`: naming prefix for generated columns.
	- `null_fill`: fill value for missing combinations (default `NULL`).
	- `order_by`: final output ordering.

3. Output contract
- Result columns:
	- All `group_by_columns` first (in input order).
	- Generated pivot columns next (explicit list order or deterministic sorted discovery order).
- Row cardinality:
	- One row per distinct `group_by_columns` tuple.
- Cell values:
	- `aggregate_fn(value_expression)` for each `(group tuple, pivot value)` bucket.
	- `null_fill` when no source rows match bucket.

4. SQL syntax options
- ANSI-like style (preferred):
	- `SELECT ... FROM (...) PIVOT (SUM(amount) FOR month IN ('JAN', 'FEB', 'MAR'))`
- Function style (portable fallback):
	- `SELECT * FROM PIVOT_TABLE(source_query, group_cols, pivot_col, value_expr, agg_fn, pivot_values, null_fill)`

5. Examples

Example A: static month pivot

Input rows (sales):

| region | month | amount |
|--------|-------|--------|
| East   | JAN   | 120    |
| East   | FEB   | 100    |
| West   | JAN   | 80     |
| West   | MAR   | 90     |

Query:
`SELECT * FROM sales PIVOT (SUM(amount) FOR month IN ('JAN','FEB','MAR')) ORDER BY region;`

Output:

| region | JAN | FEB | MAR |
|--------|-----|-----|-----|
| East   | 120 | 100 | NULL |
| West   | 80  | NULL| 90  |

Example B: count pivot with null fill

Input rows (tickets):

| owner | status  |
|-------|---------|
| A     | OPEN    |
| A     | CLOSED  |
| A     | OPEN    |
| B     | OPEN    |

Query:
`SELECT * FROM tickets PIVOT (COUNT(*) FOR status IN ('OPEN','CLOSED')) WITH NULL FILL 0 ORDER BY owner;`

Output:

| owner | OPEN | CLOSED |
|-------|------|--------|
| A     | 2    | 1      |
| B     | 1    | 0      |

Example C: dynamic pivot discovery

Input rows (inventory):

| warehouse | category | qty |
|-----------|----------|-----|
| W1        | CPU      | 12  |
| W1        | RAM      | 20  |
| W2        | CPU      | 7   |
| W2        | SSD      | 15  |

Query:
`SELECT * FROM inventory PIVOT (SUM(qty) FOR category) ORDER BY warehouse;`

Output columns discovered in deterministic sorted order: `CPU`, `RAM`, `SSD`.

| warehouse | CPU | RAM | SSD |
|-----------|-----|-----|-----|
| W1        | 12  | 20  | NULL|
| W2        | 7   | NULL| 15  |

5.1 Request/response-style examples (for Studio/MCP/IDE planning)

Example request shape (logical, not final API contract):

```json
{
	"source_sql": "SELECT region, month, amount FROM sales",
	"group_by": ["region"],
	"pivot_column": "month",
	"aggregate": {"fn": "SUM", "value": "amount"},
	"pivot_values": ["JAN", "FEB", "MAR"],
	"null_fill": null,
	"order_by": ["region ASC"]
}
```

Example response metadata + rows:

```json
{
	"status": "ok",
	"columns": [
		{"name": "region", "type": "TEXT"},
		{"name": "JAN", "type": "NUMERIC"},
		{"name": "FEB", "type": "NUMERIC"},
		{"name": "MAR", "type": "NUMERIC"}
	],
	"rows": [
		["East", 120, 100, null],
		["West", 80, null, 90]
	],
	"row_count": 2,
	"pivot_metadata": {
		"group_by": ["region"],
		"pivot_column": "month",
		"pivot_values": ["JAN", "FEB", "MAR"],
		"aggregate": "SUM(amount)"
	}
}
```

6. Validation rules
- Reject non-aggregate `value_expression` in pivot measure slot.
- Reject unsupported aggregate functions with clear error.
- Reject ambiguous pivot value typing across mixed domains unless explicit cast is provided.
- Enforce generated column-name safety and deterministic collision handling.

7. Non-functional expectations
- Stable output column order across runs.
- Bounded memory use via chunked grouping for large result sets.
- Explain plan should include a dedicated `Pivot` logical node with estimated row/column expansion.

8. Open review decisions before coding
- Whether to support multiple measures in one pivot statement.
- Whether dynamic pivot is enabled by default or behind feature flag.
- Exact identifier quoting strategy for generated columns from non-alphanumeric pivot values.

Implemented runtime plugin phase:
- Added plugin lifecycle endpoints:
	- `POST /api/v1/plugins/enable`
	- `POST /api/v1/plugins/disable`
- Added pivot function plugin ID: `function.pivot`.
- Installing/enabling the pivot plugin auto-registers UDF `pivot_table`.
- Disabling/uninstalling the pivot plugin unregisters `pivot_table`.

Pivot plugin tests:
- `plug4_plugin_disable_and_enable_toggles_active_listing`
- `plug4_pivot_plugin_enable_disable_manages_udf_lifecycle`