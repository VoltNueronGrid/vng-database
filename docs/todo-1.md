# todo-1

## Enhancement-1: Show `information_schema` in every created database

Status: In progress
Completion: 35%

What is being implemented:
- Make `information_schema.schemata` aware of the database catalog so every created database exposes the standard system schemas (`public`, `pg_catalog`, `information_schema`).
- Keep the existing global compatibility rows so older tooling does not lose visibility.
- Add regression coverage for newly created databases that have no user tables yet.

Current behavior summary:
- The virtual catalog already exists.
- The visibility gap is that system schemas were only being synthesized from objects already present in the DDL catalog.
- Newly created empty databases could therefore appear to have no `information_schema` rows yet.

## Enhancement-2: Cross-dialect built-in function catalog

Status: In progress
Completion: 20%

Goal:
- Build a comprehensive compatibility catalog for Oracle, PostgreSQL, MySQL, and Snowflake built-ins.
- Cover both OLTP and OLAP query paths as far as the current engine supports them.
- Make the supported function surface discoverable and gradually executable by family.

Implemented so far:
- Expanded the built-in function registry with common cross-dialect aliases and vendor names.
- Added parser/tokenizer recognition for the most common null-handling, string, and date-time compatibility functions.

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

Notes:
- This item is intentionally kept out of the code changes for now.
- The desired behavior is to group and rotate data into pivoted columns with predictable aggregation semantics.