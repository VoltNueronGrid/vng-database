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

Status: In progress
Completion: 75%

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

Execution status:
- OLAP/DataFusion path: the listed built-in and compatibility functions are queryable through normal SQL execution where DataFusion supports them.
- OLTP path: full scalar compatibility execution parity is not complete yet (legacy OLTP executor still has reduced function coverage).

Planned function families:
- Null-handling and conditional: `COALESCE`, `NULLIF`, `IFNULL`, `NVL`, `NVL2`, `IFF`, `DECODE`, `ZEROIFNULL`, `NULLIFZERO`, `GREATEST`, `LEAST`
- String: `LOWER`, `UPPER`, `TRIM`, `LTRIM`, `RTRIM`, `SUBSTR`, `SUBSTRING`, `INSTR`, `POSITION`, `CONCAT`, `CONCAT_WS`, `REPLACE`, `SPLIT_PART`, `LPAD`, `RPAD`, `REVERSE`, `REGEXP_MATCH`, `REGEXP_REPLACE`, `REGEXP_SUBSTR`, `REGEXP_INSTR`, `REGEXP_COUNT`
- Date/time: `NOW`, `CURRENT_TIMESTAMP`, `CURRENT_DATE`, `CURRENT_TIME`, `DATE_TRUNC`, `DATE_PART`, `DATEDIFF`, `DATEADD`, `TO_DATE`, `TO_CHAR`, `TO_TIMESTAMP`, `TO_TIMESTAMP_NTZ`, `TO_TIMESTAMP_TZ`
- Math: `ABS`, `CEIL`, `CEILING`, `FLOOR`, `ROUND`, `POWER`, `SQRT`, `MOD`, `SIGN`, `EXP`, `LN`, `LOG`
- Aggregates: `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, `STRING_AGG`, `LISTAGG`, `ARRAY_AGG`, `JSON_AGG`, `ANY_VALUE`, `APPROX_COUNT_DISTINCT`
- JSON / semi-structured: `JSON_EXTRACT`, `JSON_OBJECT`, `JSON_ARRAY`, `JSONB_EXTRACT_PATH`, `OBJECT_CONSTRUCT`, `OBJECT_AGG`, `ARRAY_CONSTRUCT`, `MAP_CONSTRUCT`
- Type conversion: `CAST`, `TRY_CAST`, `TO_NUMBER`, `TO_VARCHAR`, `TO_VARIANT`, `TRY_TO_NUMBER`, `TRY_TO_DATE`

## Enhancement-3: Pivot operator

Status: Planned
Completion: 0%

Scope:
- Add a `PIVOT` operator that behaves like spreadsheet-style pivoting over tables.
- Do not code this yet.
- Review the design first, then implement once the query shape and output contract are agreed.

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

Notes:
- This item remains intentionally out of code implementation for now.
- Coding starts only after design approval.