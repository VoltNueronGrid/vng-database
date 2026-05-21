//! Virtual `information_schema` and `pg_catalog` table synthesizer.
//!
//! Intercepts queries targeting standard introspection views and returns
//! synthesized metadata rows derived from the live `DdlCatalog`.  This
//! enables DBeaver, TablePlus, psql, and SQLAlchemy to enumerate tables,
//! columns, and schemas without a real PostgreSQL backend.
//!
//! ## Supported virtual tables
//! - `information_schema.schemata`
//! - `information_schema.tables`
//! - `information_schema.columns`
//! - `information_schema.settings` / `information_schema.parameters` (M-9)
//! - `pg_catalog.pg_namespace`
//! - `pg_catalog.pg_class`
//! - `pg_catalog.pg_attribute`
//! - `pg_catalog.pg_type` (5 fixed system rows)
//! - Combined `pg_class JOIN pg_namespace` (DBeaver `\dt` query)

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use serde_json::{json, Value};
use voltnuerongrid_store::ddl_catalog::DdlCatalogEntry;

// ─── Detection helpers ────────────────────────────────────────────────────────

/// Returns `true` when the SQL batch references any virtual catalog schema.
pub(crate) fn is_virtual_catalog_query(sql_batch: &str) -> bool {
    let lower = sql_batch.to_ascii_lowercase();
    lower.contains("information_schema.") || lower.contains("pg_catalog.")
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum VirtualTable {
    IsColumns,
    IsTables,
    IsSchemata,
    /// M-9: `information_schema.settings` / `information_schema.parameters`
    /// — exposes runtime server configuration as SQL-queryable rows.
    IsSettings,
    PgAttribute,
    PgClass,
    PgNamespace,
    PgType,
    /// DBeaver `\dt` pattern: `pg_class JOIN pg_namespace` in same query.
    PgClassNamespaceJoin,
    Unknown,
}

/// Detect which virtual table the batch targets. Order matters — check more
/// specific patterns before broader ones.
pub(crate) fn detect_virtual_table(sql_batch: &str) -> VirtualTable {
    let lower = sql_batch.to_ascii_lowercase();
    // Check multi-table join first.
    if lower.contains("pg_catalog.pg_class") && lower.contains("pg_catalog.pg_namespace") {
        return VirtualTable::PgClassNamespaceJoin;
    }
    if lower.contains("information_schema.columns") {
        return VirtualTable::IsColumns;
    }
    if lower.contains("information_schema.tables") {
        return VirtualTable::IsTables;
    }
    if lower.contains("information_schema.schemata") {
        return VirtualTable::IsSchemata;
    }
    if lower.contains("information_schema.settings") || lower.contains("information_schema.parameters") {
        return VirtualTable::IsSettings;
    }
    if lower.contains("pg_catalog.pg_attribute") {
        return VirtualTable::PgAttribute;
    }
    if lower.contains("pg_catalog.pg_class") {
        return VirtualTable::PgClass;
    }
    if lower.contains("pg_catalog.pg_namespace") {
        return VirtualTable::PgNamespace;
    }
    if lower.contains("pg_catalog.pg_type") {
        return VirtualTable::PgType;
    }
    VirtualTable::Unknown
}

// ─── OID helpers ─────────────────────────────────────────────────────────────

/// Deterministic OID for any string — stable within a process, avoids
/// collision with real PostgreSQL system OIDs (which are < 16384).
pub(crate) fn oid_for(name: &str) -> u32 {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    let hash = h.finish();
    // Keep upper 16 bits + lower 16 bits XORed together, then add 16384.
    let folded = ((hash >> 32) as u32) ^ ((hash & 0xFFFF_FFFF) as u32);
    16384u32.wrapping_add(folded & 0x0FFF_FFFF)
}

// ─── Type mapping ─────────────────────────────────────────────────────────────

/// Map a DDL type token (e.g. `"INT"`, `"VARCHAR(255)"`) to a canonical
/// PostgreSQL type name for `information_schema.columns.data_type`.
fn map_sql_type_to_pg(ddl_type: &str) -> &'static str {
    // Strip length/precision qualifier: VARCHAR(255) → VARCHAR
    let bare = ddl_type.split('(').next().unwrap_or(ddl_type).trim();
    match bare.to_ascii_uppercase().as_str() {
        "INT" | "INTEGER" | "INT4" | "SMALLINT" | "INT2" => "integer",
        "BIGINT" | "INT8" => "bigint",
        "TEXT" | "CLOB" => "text",
        "VARCHAR" | "CHARACTER VARYING" | "NVARCHAR" => "character varying",
        "CHAR" | "CHARACTER" | "NCHAR" => "character",
        "FLOAT" | "REAL" | "FLOAT4" => "real",
        "DOUBLE" | "DOUBLE PRECISION" | "FLOAT8" => "double precision",
        "NUMERIC" | "DECIMAL" => "numeric",
        "BOOL" | "BOOLEAN" => "boolean",
        "TIMESTAMP" | "TIMESTAMP WITHOUT TIME ZONE" => "timestamp without time zone",
        "TIMESTAMPTZ" | "TIMESTAMP WITH TIME ZONE" => "timestamp with time zone",
        "DATE" => "date",
        "TIME" => "time without time zone",
        "UUID" => "uuid",
        "BYTEA" | "BLOB" | "BINARY" => "bytea",
        "JSON" => "json",
        "JSONB" => "jsonb",
        _ => "text", // safe default
    }
}

/// Map a DDL type token to a PostgreSQL system type OID (`atttypid`).
fn map_sql_type_to_atttypid(ddl_type: &str) -> u32 {
    let bare = ddl_type.split('(').next().unwrap_or(ddl_type).trim();
    match bare.to_ascii_uppercase().as_str() {
        "INT" | "INTEGER" | "INT4" | "SMALLINT" | "INT2" => 23,   // int4
        "BIGINT" | "INT8" => 20,                                   // int8
        "TEXT" | "CLOB" => 25,                                     // text
        "VARCHAR" | "CHARACTER VARYING" | "NVARCHAR" => 1043,      // varchar
        "CHAR" | "CHARACTER" | "NCHAR" => 1042,                    // bpchar
        "FLOAT" | "REAL" | "FLOAT4" => 700,                       // float4
        "DOUBLE" | "DOUBLE PRECISION" | "FLOAT8" => 701,           // float8
        "NUMERIC" | "DECIMAL" => 1700,                             // numeric
        "BOOL" | "BOOLEAN" => 16,                                  // bool
        "TIMESTAMP" | "TIMESTAMP WITHOUT TIME ZONE" => 1114,       // timestamp
        "TIMESTAMPTZ" | "TIMESTAMP WITH TIME ZONE" => 1184,        // timestamptz
        "DATE" => 1082,                                            // date
        "TIME" => 1083,                                            // time
        "UUID" => 2950,                                            // uuid
        "BYTEA" | "BLOB" | "BINARY" => 17,                         // bytea
        "JSON" => 114,                                             // json
        "JSONB" => 3802,                                           // jsonb
        _ => 25, // text as safe default
    }
}

// ─── DDL column detail extraction ────────────────────────────────────────────

/// Extract `(column_name, sql_type_token)` pairs from a `CREATE TABLE` DDL
/// statement.  Returns an empty Vec for non-table DDL (views, functions).
pub(crate) fn extract_ddl_column_details(ddl: &str) -> Vec<(String, String)> {
    // Find the opening parenthesis — everything after is the column list.
    let paren_start = match ddl.find('(') {
        Some(p) => p + 1,
        None => return Vec::new(),
    };
    // Find the closing parenthesis — strip constraints at the end.
    let paren_end = match ddl.rfind(')') {
        Some(p) => p,
        None => return Vec::new(),
    };
    if paren_end <= paren_start {
        return Vec::new();
    }
    let body = &ddl[paren_start..paren_end];

    let mut results = Vec::new();
    // Split on commas that are not inside nested parentheses.
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in body.chars() {
        match ch {
            '(' => { depth += 1; current.push(ch); }
            ')' => { depth = depth.saturating_sub(1); current.push(ch); }
            ',' if depth == 0 => {
                process_column_clause(current.trim(), &mut results);
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        process_column_clause(current.trim(), &mut results);
    }
    results
}

fn process_column_clause(clause: &str, out: &mut Vec<(String, String)>) {
    let upper = clause.trim().to_ascii_uppercase();
    // Skip constraint declarations: PRIMARY KEY, UNIQUE, CHECK, FOREIGN KEY, etc.
    for kw in &["PRIMARY KEY", "UNIQUE", "CHECK", "FOREIGN KEY", "CONSTRAINT", "INDEX"] {
        if upper.starts_with(kw) { return; }
    }
    // Split on whitespace: first token = column name, second = type.
    let mut tokens = clause.split_ascii_whitespace();
    let col_name = match tokens.next() {
        Some(n) => n.trim_matches('"').trim_matches('`').to_ascii_lowercase(),
        None => return,
    };
    if col_name.is_empty() { return; }
    let col_type = tokens.next().unwrap_or("text").to_string();
    out.push((col_name, col_type));
}

// ─── Virtual table synthesizers ──────────────────────────────────────────────

fn col_schema(columns: &[&str]) -> Vec<Value> {
    columns.iter().map(|c| json!({"name": c, "data_type": "text"})).collect()
}

/// Distinct (database, schema) pairs plus always-present built-in schemas.
fn synth_is_schemata(entries: &[DdlCatalogEntry]) -> (Vec<Value>, Vec<Value>) {
    let columns = col_schema(&[
        "catalog_name", "schema_name", "schema_owner",
        "default_character_set_catalog", "default_character_set_schema", "default_character_set_name",
    ]);
    let mut seen = std::collections::HashSet::new();
    let mut rows: Vec<Value> = Vec::new();

    // Built-in schemas always present.
    for schema in &["public", "pg_catalog", "information_schema"] {
        if seen.insert(format!("voltdb.{schema}")) {
            rows.push(json!({
                "catalog_name": "voltdb",
                "schema_name": schema,
                "schema_owner": "voltdb",
                "default_character_set_catalog": null,
                "default_character_set_schema": null,
                "default_character_set_name": "utf8",
            }));
        }
    }

    for e in entries {
        let key = format!("{}.{}", e.database_name, e.schema_name);
        if seen.insert(key) {
            rows.push(json!({
                "catalog_name": e.database_name,
                "schema_name": e.schema_name,
                "schema_owner": "voltdb",
                "default_character_set_catalog": null,
                "default_character_set_schema": null,
                "default_character_set_name": "utf8",
            }));
        }
    }
    (columns, rows)
}

fn synth_is_tables(entries: &[DdlCatalogEntry]) -> (Vec<Value>, Vec<Value>) {
    let columns = col_schema(&[
        "table_catalog", "table_schema", "table_name",
        "table_type", "is_insertable_into", "is_typed",
    ]);
    let rows = entries.iter()
        .filter(|e| matches!(e.object_kind.as_str(), "table" | "view" | "materialized_view"))
        .map(|e| {
            let (table_type, insertable) = match e.object_kind.as_str() {
                "table" => ("BASE TABLE", "YES"),
                _ => ("VIEW", "NO"),
            };
            json!({
                "table_catalog": e.database_name,
                "table_schema": e.schema_name,
                "table_name": e.object_name,
                "table_type": table_type,
                "is_insertable_into": insertable,
                "is_typed": "NO",
            })
        })
        .collect();
    (columns, rows)
}

fn synth_is_columns(entries: &[DdlCatalogEntry]) -> (Vec<Value>, Vec<Value>) {
    let columns = col_schema(&[
        "table_catalog", "table_schema", "table_name",
        "column_name", "ordinal_position", "column_default",
        "is_nullable", "data_type",
        "character_maximum_length", "numeric_precision",
    ]);
    let mut rows: Vec<Value> = Vec::new();
    for e in entries.iter().filter(|e| e.object_kind == "table") {
        for (idx, (col_name, col_type)) in extract_ddl_column_details(&e.original_statement).iter().enumerate() {
            rows.push(json!({
                "table_catalog": e.database_name,
                "table_schema": e.schema_name,
                "table_name": e.object_name,
                "column_name": col_name,
                "ordinal_position": (idx + 1).to_string(),
                "column_default": null,
                "is_nullable": "YES",
                "data_type": map_sql_type_to_pg(col_type),
                "character_maximum_length": null,
                "numeric_precision": null,
            }));
        }
    }
    (columns, rows)
}

fn synth_pg_namespace(entries: &[DdlCatalogEntry]) -> (Vec<Value>, Vec<Value>) {
    let columns = col_schema(&["oid", "nspname", "nspowner", "nspacl"]);
    let mut seen = std::collections::HashSet::new();
    let mut rows: Vec<Value> = Vec::new();

    for schema in &["public", "pg_catalog", "information_schema"] {
        if seen.insert(*schema) {
            rows.push(json!({
                "oid": oid_for(schema).to_string(),
                "nspname": schema,
                "nspowner": "10",
                "nspacl": null,
            }));
        }
    }
    for e in entries {
        if seen.insert(e.schema_name.as_str()) {
            rows.push(json!({
                "oid": oid_for(&e.schema_name).to_string(),
                "nspname": e.schema_name,
                "nspowner": "10",
                "nspacl": null,
            }));
        }
    }
    (columns, rows)
}

fn synth_pg_class(entries: &[DdlCatalogEntry]) -> (Vec<Value>, Vec<Value>) {
    let columns = col_schema(&[
        "oid", "relname", "relnamespace", "relkind",
        "relnatts", "reltablespace", "relrowsecurity", "relacl",
    ]);
    let rows = entries.iter()
        .filter(|e| matches!(e.object_kind.as_str(), "table" | "view" | "materialized_view"))
        .map(|e| {
            let relkind = match e.object_kind.as_str() {
                "table" => "r",
                "view" => "v",
                "materialized_view" => "m",
                _ => "r",
            };
            let nat = extract_ddl_column_details(&e.original_statement).len();
            let fq = format!("{}.{}.{}", e.database_name, e.schema_name, e.object_name);
            json!({
                "oid": oid_for(&fq).to_string(),
                "relname": e.object_name,
                "relnamespace": oid_for(&e.schema_name).to_string(),
                "relkind": relkind,
                "relnatts": nat.to_string(),
                "reltablespace": "0",
                "relrowsecurity": "false",
                "relacl": null,
            })
        })
        .collect();
    (columns, rows)
}

/// DBeaver `\dt` and Navigator: denormalized join of pg_class + pg_namespace.
fn synth_pg_class_namespace_join(entries: &[DdlCatalogEntry]) -> (Vec<Value>, Vec<Value>) {
    let columns = col_schema(&[
        "oid", "relname", "nspname", "relkind",
        "relnatts", "reltablespace", "relrowsecurity",
    ]);
    let rows = entries.iter()
        .filter(|e| matches!(e.object_kind.as_str(), "table" | "view" | "materialized_view"))
        .map(|e| {
            let relkind = match e.object_kind.as_str() {
                "table" => "r",
                "view" => "v",
                "materialized_view" => "m",
                _ => "r",
            };
            let nat = extract_ddl_column_details(&e.original_statement).len();
            let fq = format!("{}.{}.{}", e.database_name, e.schema_name, e.object_name);
            json!({
                "oid": oid_for(&fq).to_string(),
                "relname": e.object_name,
                "nspname": e.schema_name,
                "relkind": relkind,
                "relnatts": nat.to_string(),
                "reltablespace": "0",
                "relrowsecurity": "false",
            })
        })
        .collect();
    (columns, rows)
}

fn synth_pg_attribute(entries: &[DdlCatalogEntry]) -> (Vec<Value>, Vec<Value>) {
    let columns = col_schema(&[
        "attrelid", "attname", "atttypid", "attnum",
        "attnotnull", "attisdropped",
    ]);
    let mut rows: Vec<Value> = Vec::new();
    for e in entries.iter().filter(|e| e.object_kind == "table") {
        let fq = format!("{}.{}.{}", e.database_name, e.schema_name, e.object_name);
        let rel_oid = oid_for(&fq).to_string();
        for (idx, (col_name, col_type)) in extract_ddl_column_details(&e.original_statement).iter().enumerate() {
            rows.push(json!({
                "attrelid": rel_oid,
                "attname": col_name,
                "atttypid": map_sql_type_to_atttypid(col_type).to_string(),
                "attnum": (idx + 1).to_string(),
                "attnotnull": "false",
                "attisdropped": "false",
            }));
        }
    }
    (columns, rows)
}

/// Five fixed system type rows — always present, catalog-independent.
fn synth_pg_type() -> (Vec<Value>, Vec<Value>) {
    let columns = col_schema(&["oid", "typname", "typnamespace", "typlen"]);
    let ns_oid = oid_for("pg_catalog").to_string();
    let rows = vec![
        json!({"oid": "25",   "typname": "text",      "typnamespace": ns_oid, "typlen": "-1"}),
        json!({"oid": "23",   "typname": "int4",      "typnamespace": ns_oid, "typlen": "4"}),
        json!({"oid": "701",  "typname": "float8",    "typnamespace": ns_oid, "typlen": "8"}),
        json!({"oid": "16",   "typname": "bool",      "typnamespace": ns_oid, "typlen": "1"}),
        json!({"oid": "1114", "typname": "timestamp", "typnamespace": ns_oid, "typlen": "8"}),
    ];
    (columns, rows)
}

/// M-9: Synthesize `information_schema.settings` — one row per runtime config
/// parameter, keyed by parameter name.  Columns mirror the PostgreSQL
/// `pg_settings` view: `name`, `setting`, `unit`, `category`, `short_desc`.
fn synth_is_settings(state: &crate::AppState) -> (Vec<Value>, Vec<Value>) {
    let columns = col_schema(&["name", "setting", "unit", "category", "short_desc"]);
    let cfg = &state.runtime_config;

    let storage_engine_str = match cfg.storage.engine {
        voltnuerongrid_config::StorageEngine::Rocksdb => "rocksdb",
        voltnuerongrid_config::StorageEngine::Vng => "vng",
    };
    let sql_engine_str = match cfg.sql.engine {
        voltnuerongrid_config::SqlEngine::Datafusion => "datafusion",
        voltnuerongrid_config::SqlEngine::Vng => "vng",
    };

    let rows = vec![
        json!({
            "name": "storage_engine",
            "setting": storage_engine_str,
            "unit": null,
            "category": "Storage",
            "short_desc": "Durable storage substrate (rocksdb | vng)",
        }),
        json!({
            "name": "data_dir",
            "setting": cfg.storage.data_dir,
            "unit": null,
            "category": "Storage",
            "short_desc": "Filesystem path for the storage engine data directory",
        }),
        json!({
            "name": "wal_fsync_on_commit",
            "setting": if cfg.storage.wal_fsync_on_commit { "on" } else { "off" },
            "unit": null,
            "category": "Storage",
            "short_desc": "Whether to fsync the WAL on every commit",
        }),
        json!({
            "name": "max_background_jobs",
            "setting": cfg.storage.max_background_jobs.to_string(),
            "unit": null,
            "category": "Storage",
            "short_desc": "Number of background flush/compaction threads (RocksDB)",
        }),
        json!({
            "name": "sql_engine",
            "setting": sql_engine_str,
            "unit": null,
            "category": "SQL",
            "short_desc": "SQL parser and execution engine (datafusion | vng)",
        }),
        json!({
            "name": "htap_olap_threshold_rows",
            "setting": cfg.sql.htap_olap_threshold_rows.to_string(),
            "unit": "rows",
            "category": "SQL",
            "short_desc": "Row count above which queries are routed to the OLAP path",
        }),
        json!({
            "name": "max_result_rows",
            "setting": cfg.sql.max_result_rows.to_string(),
            "unit": "rows",
            "category": "SQL",
            "short_desc": "Maximum rows a single SELECT may return",
        }),
    ];
    (columns, rows)
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Synthesize `(columns, rows)` for the detected virtual catalog table.
///
/// Locks `ddl_catalog`, clones active entries, then builds the response —
/// no lock held during JSON construction.
pub(crate) fn synthesize_virtual_catalog_response(
    sql_batch: &str,
    state: &crate::AppState,
) -> (Vec<Value>, Vec<Value>) {
    let entries: Vec<DdlCatalogEntry> = {
        let catalog = state.ddl_catalog.lock().expect("ddl_catalog lock");
        catalog.active_entries().into_iter().cloned().collect()
    };

    match detect_virtual_table(sql_batch) {
        VirtualTable::IsSchemata => synth_is_schemata(&entries),
        VirtualTable::IsTables => synth_is_tables(&entries),
        VirtualTable::IsSettings => synth_is_settings(state),
        VirtualTable::IsColumns => synth_is_columns(&entries),
        VirtualTable::PgNamespace => synth_pg_namespace(&entries),
        VirtualTable::PgClassNamespaceJoin => synth_pg_class_namespace_join(&entries),
        VirtualTable::PgClass => synth_pg_class(&entries),
        VirtualTable::PgAttribute => synth_pg_attribute(&entries),
        VirtualTable::PgType => synth_pg_type(),
        VirtualTable::Unknown => {
            // Fallback: treat as information_schema.tables
            synth_is_tables(&entries)
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use voltnuerongrid_store::ddl_catalog::DdlCatalogEntry;

    fn make_table_entry(db: &str, schema: &str, name: &str, ddl: &str) -> DdlCatalogEntry {
        DdlCatalogEntry {
            object_name: name.to_string(),
            database_name: db.to_string(),
            schema_name: schema.to_string(),
            object_kind: "table".to_string(),
            original_statement: ddl.to_string(),
            created_at_unix_ms: 0,
            last_altered_at_unix_ms: None,
            alteration_count: 0,
            dropped: false,
        }
    }

    #[test]
    fn is_virtual_catalog_query_true_for_information_schema() {
        assert!(is_virtual_catalog_query("SELECT * FROM information_schema.tables"));
    }

    #[test]
    fn is_virtual_catalog_query_true_for_pg_catalog() {
        assert!(is_virtual_catalog_query("SELECT * FROM pg_catalog.pg_class"));
    }

    #[test]
    fn is_virtual_catalog_query_false_for_dml() {
        assert!(!is_virtual_catalog_query("INSERT INTO orders VALUES (1, 'x')"));
    }

    #[test]
    fn detect_virtual_table_columns_before_tables() {
        assert_eq!(
            detect_virtual_table("SELECT * FROM information_schema.columns"),
            VirtualTable::IsColumns
        );
    }

    #[test]
    fn detect_virtual_table_pg_class_namespace_join() {
        let q = "SELECT c.oid, c.relname FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace";
        assert_eq!(detect_virtual_table(q), VirtualTable::PgClassNamespaceJoin);
    }

    #[test]
    fn synth_is_tables_empty_catalog() {
        let (cols, rows) = synth_is_tables(&[]);
        assert!(!cols.is_empty(), "column schema should always be present");
        assert!(rows.is_empty(), "no tables in empty catalog");
    }

    #[test]
    fn synth_is_tables_one_table_entry() {
        let entry = make_table_entry("default", "public", "orders", "CREATE TABLE orders (id INT, name TEXT)");
        let (_, rows) = synth_is_tables(&[entry]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["table_name"], "orders");
        assert_eq!(rows[0]["table_type"], "BASE TABLE");
        assert_eq!(rows[0]["is_insertable_into"], "YES");
    }

    #[test]
    fn synth_is_columns_with_create_table() {
        let entry = make_table_entry("default", "public", "orders", "CREATE TABLE orders (id INT, name TEXT, price FLOAT)");
        let (_, rows) = synth_is_columns(&[entry]);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["column_name"], "id");
        assert_eq!(rows[0]["data_type"], "integer");
        assert_eq!(rows[0]["ordinal_position"], "1");
        assert_eq!(rows[1]["column_name"], "name");
        assert_eq!(rows[1]["data_type"], "text");
        assert_eq!(rows[2]["column_name"], "price");
    }

    #[test]
    fn synth_pg_type_always_five_rows() {
        let (_, rows) = synth_pg_type();
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn synth_pg_class_oid_stable() {
        let entry = make_table_entry("default", "public", "orders", "CREATE TABLE orders (id INT)");
        let (_, rows1) = synth_pg_class(&[entry.clone()]);
        let (_, rows2) = synth_pg_class(&[entry]);
        assert_eq!(rows1[0]["oid"], rows2[0]["oid"]);
    }

    #[test]
    fn extract_ddl_column_details_basic() {
        let details = extract_ddl_column_details("CREATE TABLE orders (id INT, name TEXT, price FLOAT)");
        assert_eq!(details.len(), 3);
        assert_eq!(details[0].0, "id");
        assert_eq!(details[1].0, "name");
        assert_eq!(details[2].0, "price");
    }

    #[test]
    fn extract_ddl_column_details_with_primary_key_constraint() {
        let details = extract_ddl_column_details(
            "CREATE TABLE orders (id INT, name TEXT, PRIMARY KEY (id))"
        );
        assert_eq!(details.len(), 2);
    }

    // M-9: information_schema.settings detection
    #[test]
    fn detect_virtual_table_settings() {
        assert_eq!(
            detect_virtual_table("SELECT * FROM information_schema.settings"),
            VirtualTable::IsSettings
        );
    }

    #[test]
    fn detect_virtual_table_parameters_alias() {
        assert_eq!(
            detect_virtual_table("SELECT name, setting FROM information_schema.parameters"),
            VirtualTable::IsSettings
        );
    }

    #[test]
    fn detect_virtual_table_settings_not_confused_with_schemata() {
        // "settings" substring is distinct — must not match schemata
        assert_eq!(
            detect_virtual_table("SELECT * FROM information_schema.schemata"),
            VirtualTable::IsSchemata
        );
    }

    #[test]
    fn synth_is_schemata_always_includes_builtin_schemas() {
        let (_, rows) = synth_is_schemata(&[]);
        let schema_names: Vec<&str> = rows.iter()
            .map(|r| r["schema_name"].as_str().unwrap_or(""))
            .collect();
        assert!(schema_names.contains(&"public"));
        assert!(schema_names.contains(&"pg_catalog"));
        assert!(schema_names.contains(&"information_schema"));
    }
}
