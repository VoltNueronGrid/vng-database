#![forbid(unsafe_code)]

//! In-memory DDL object lifecycle catalog (REQ-02).
//!
//! Tracks CREATE / DROP / ALTER operations for tables, views,
//! materialized views, and functions so the runtime can expose
//! object-lifecycle state without a persistent schema store.
//!
//! Objects are keyed by their fully-qualified name `"database.schema.object"`.
//! Unqualified names default to `"default"` database and `"public"` schema.

use std::collections::HashMap;

// ── Public types ─────────────────────────────────────────────────────────

/// One registered DDL object (table, view, materialized view, function).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdlCatalogEntry {
    /// Lower-cased base (unqualified) object name.
    pub object_name: String,
    /// Database the object belongs to (lower-cased, defaults to `"default"`).
    pub database_name: String,
    /// Schema the object belongs to (lower-cased, defaults to `"public"`).
    pub schema_name: String,
    /// `"table"`, `"view"`, `"materialized_view"`, `"function"`, `"trigger"`, or `"event"`.
    pub object_kind: String,
    /// The full DDL statement that created the object.
    pub original_statement: String,
    /// Epoch-millisecond timestamp of the CREATE statement.
    pub created_at_unix_ms: u128,
    /// Epoch-millisecond timestamp of the last ALTER, if any.
    pub last_altered_at_unix_ms: Option<u128>,
    /// Running count of ALTER statements applied to this object.
    pub alteration_count: u32,
    /// Set to `true` after a DROP; the entry is kept for history.
    pub dropped: bool,
}

/// Catalog operation result returned by mutating methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogResult {
    Created,
    Replaced,
    AlterApplied,
    Dropped,
    /// Object did not exist when a DROP or ALTER was attempted.
    NotFound,
    /// Object already existed but was `dropped == true`.
    AlreadyDropped,
    /// Attempted to CREATE a table that already exists (and is active).
    AlreadyExists,
}

/// Build the fully-qualified catalog key: `"db.schema.name"` (all lower-cased).
fn qualified_key(db: &str, schema: &str, name: &str) -> String {
    format!(
        "{}.{}.{}",
        db.trim().to_ascii_lowercase(),
        schema.trim().to_ascii_lowercase(),
        name.trim().to_ascii_lowercase()
    )
}

/// Lightweight in-memory DDL catalog keyed by `"database.schema.object"`.
#[derive(Default)]
pub struct DdlCatalog {
    entries: HashMap<String, DdlCatalogEntry>,
}

impl DdlCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a CREATE statement.
    ///
    /// If `replace_ok` is true (CREATE OR REPLACE semantics), an existing active
    /// entry is silently replaced. Otherwise, an active entry triggers
    /// `AlreadyExists`. Previously dropped entries are always reused.
    pub fn record_create(
        &mut self,
        kind: &str,
        db: &str,
        schema: &str,
        name: &str,
        statement: &str,
        now_ms: u128,
        replace_ok: bool,
    ) -> CatalogResult {
        let key = qualified_key(db, schema, name);
        if let Some(existing) = self.entries.get(&key) {
            if !existing.dropped && !replace_ok {
                return CatalogResult::AlreadyExists;
            }
        }
        let existed = self.entries.contains_key(&key);
        self.entries.insert(
            key,
            DdlCatalogEntry {
                object_name: name.trim().to_ascii_lowercase(),
                database_name: db.trim().to_ascii_lowercase(),
                schema_name: schema.trim().to_ascii_lowercase(),
                object_kind: kind.to_string(),
                original_statement: statement.to_string(),
                created_at_unix_ms: now_ms,
                last_altered_at_unix_ms: None,
                alteration_count: 0,
                dropped: false,
            },
        );
        if existed {
            CatalogResult::Replaced
        } else {
            CatalogResult::Created
        }
    }

    /// Mark an object as dropped (entry is retained for audit history).
    pub fn record_drop(&mut self, db: &str, schema: &str, name: &str) -> CatalogResult {
        let key = qualified_key(db, schema, name);
        match self.entries.get_mut(&key) {
            None => CatalogResult::NotFound,
            Some(e) if e.dropped => CatalogResult::AlreadyDropped,
            Some(e) => {
                e.dropped = true;
                CatalogResult::Dropped
            }
        }
    }

    /// Record an ALTER against an existing active object.
    ///
    /// M-2: Applies the schema change to `original_statement` so that
    /// `extract_column_names_from_ddl()` always returns the current column list.
    /// Supports:
    /// - `ALTER TABLE t ADD [COLUMN] col type` → appends column to DDL
    /// - `ALTER TABLE t DROP [COLUMN] col`     → removes column from DDL
    pub fn record_alter(
        &mut self,
        db: &str,
        schema: &str,
        name: &str,
        statement: &str,
        now_ms: u128,
    ) -> CatalogResult {
        let key = qualified_key(db, schema, name);
        match self.entries.get_mut(&key) {
            None => CatalogResult::NotFound,
            Some(e) if e.dropped => CatalogResult::AlreadyDropped,
            Some(e) => {
                // Apply schema change to original_statement (M-2).
                let lower = statement.trim().to_ascii_lowercase();
                if lower.contains(" add ") {
                    if let Some((_tbl, col_name, col_def)) = parse_alter_add_column(statement) {
                        e.original_statement = apply_add_column_to_ddl(&e.original_statement, &col_name, &col_def);
                    }
                } else if lower.contains(" drop ") {
                    if let Some((_tbl, col_name)) = parse_alter_drop_column(statement) {
                        e.original_statement = remove_column_from_ddl(&e.original_statement, &col_name);
                    }
                }
                e.last_altered_at_unix_ms = Some(now_ms);
                e.alteration_count += 1;
                CatalogResult::AlterApplied
            }
        }
    }

    /// All entries that have **not** been dropped.
    pub fn active_entries(&self) -> Vec<&DdlCatalogEntry> {
        self.entries.values().filter(|e| !e.dropped).collect()
    }

    /// All entries regardless of drop state.
    pub fn all_entries(&self) -> Vec<&DdlCatalogEntry> {
        self.entries.values().collect()
    }

    /// Lookup a single entry by name.
    ///
    /// Resolution order:
    /// 1. `"db.schema.obj"` (2 dots) → exact key lookup.
    /// 2. `"schema.obj"` (1 dot) → resolved as `"default.schema.obj"`.
    /// 3. Unqualified `"obj"` (0 dots) → resolved as `"default.public.obj"`.
    pub fn get(&self, name: &str) -> Option<&DdlCatalogEntry> {
        let lower = name.trim().to_ascii_lowercase();
        if let Some(entry) = self.entries.get(&lower) {
            return Some(entry);
        }
        let dot_count = lower.chars().filter(|&c| c == '.').count();
        let resolved = match dot_count {
            0 => format!("default.public.{}", lower),
            1 => format!("default.{}", lower),
            _ => return None,
        };
        self.entries.get(&resolved)
    }

    pub fn total_count(&self) -> usize {
        self.entries.len()
    }

    pub fn active_count(&self) -> usize {
        self.entries.values().filter(|e| !e.dropped).count()
    }

    /// Return all active (non-dropped) function entries registered via `CREATE FUNCTION`.
    /// Used by the UDF execution engine to resolve catalog-defined functions at query time.
    pub fn list_active_functions(&self) -> Vec<&DdlCatalogEntry> {
        self.entries
            .values()
            .filter(|e| !e.dropped && e.object_kind == "function")
            .collect()
    }

    /// Look up an active function entry by its unqualified name (case-insensitive).
    /// Returns `None` if the function is not registered or has been dropped.
    pub fn get_active_function(&self, name: &str) -> Option<&DdlCatalogEntry> {
        let lower = name.trim().to_ascii_lowercase();
        self.entries
            .values()
            .find(|e| !e.dropped && e.object_kind == "function" && e.object_name == lower)
    }
}

// ── Helper: extract DDL object name + kind from a SQL string ─────────────

/// Classification of a single parsed DDL token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdlObjectInfo {
    /// `"create"`, `"drop"`, or `"alter"`.
    pub operation: &'static str,
    /// Object kind string for [`DdlCatalog`] methods.
    pub object_kind: &'static str,
    /// Base (unqualified) object name (lower-cased).
    pub object_name: String,
    /// Database extracted from the DDL name token, defaults to `"default"`.
    pub database_name: String,
    /// Schema extracted from the DDL name token, defaults to `"public"`.
    pub schema_name: String,
    /// True for `CREATE OR REPLACE` statements.
    pub replace_ok: bool,
}

/// Parse a potentially-qualified DDL name token into `(database, schema, base_name)`.
///
/// - `"db.schema.table"` → `("db", "schema", "table")`
/// - `"schema.table"`    → `("default", "schema", "table")`
/// - `"table"`           → `("default", "public", "table")`
fn parse_qualifiers(raw: &str) -> (String, String, String) {
    let base = raw.split('(').next().unwrap_or(raw);
    let base = base
        .trim_matches(|c: char| c == ')' || c == ';' || c == ',')
        .to_ascii_lowercase();
    let parts: Vec<&str> = base.splitn(3, '.').collect();
    match parts.as_slice() {
        [db, schema, name] => (db.to_string(), schema.to_string(), name.to_string()),
        [schema, name] => ("default".to_string(), schema.to_string(), name.to_string()),
        [name] => (
            "default".to_string(),
            "public".to_string(),
            name.to_string(),
        ),
        _ => (
            "default".to_string(),
            "public".to_string(),
            base.to_string(),
        ),
    }
}

/// Try to extract DDL metadata from a SQL statement.
///
/// Returns `None` for non-DDL statements or statements with ambiguous syntax.
pub fn parse_ddl_info(sql: &str) -> Option<DdlObjectInfo> {
    let lower = sql.trim().to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }

    macro_rules! info {
        ($op:expr, $kind:expr, $name:expr, $replace:expr) => {{
            let (database_name, schema_name, object_name) = parse_qualifiers($name);
            Some(DdlObjectInfo {
                operation: $op,
                object_kind: $kind,
                object_name,
                database_name,
                schema_name,
                replace_ok: $replace,
            })
        }};
    }

    match words.as_slice() {
        ["create", "table", name, ..] => info!("create", "table", name, false),
        ["create", "view", name, ..] => info!("create", "view", name, false),
        ["create", "materialized", "view", name, ..] => {
            info!("create", "materialized_view", name, false)
        }
        ["create", "function", name, ..] => info!("create", "function", name, false),
        ["create", "trigger", name, ..] => info!("create", "trigger", name, false),
        ["create", "event", name, ..] => info!("create", "event", name, false),
        ["create", "or", "replace", "function", name, ..] => {
            info!("create", "function", name, true)
        }
        ["create", "or", "replace", "view", name, ..] => info!("create", "view", name, true),
        ["create", "or", "replace", "table", name, ..] => info!("create", "table", name, true),
        // CREATE INDEX idx_name ON table_name (col)
        ["create", "index", name, ..] => info!("create", "index", name, false),
        // CREATE UNIQUE INDEX idx_name ON table_name (col)
        ["create", "unique", "index", name, ..] => info!("create", "index", name, false),
        ["drop", "index", "if", "exists", name, ..] | ["drop", "index", name, ..] => {
            info!("drop", "index", name, false)
        }
        ["drop", "table", "if", "exists", name, ..] | ["drop", "table", name, ..] => {
            info!("drop", "table", name, false)
        }
        ["drop", "view", "if", "exists", name, ..] | ["drop", "view", name, ..] => {
            info!("drop", "view", name, false)
        }
        ["drop", "function", "if", "exists", name, ..] | ["drop", "function", name, ..] => {
            info!("drop", "function", name, false)
        }
        ["drop", "trigger", "if", "exists", name, ..] | ["drop", "trigger", name, ..] => {
            info!("drop", "trigger", name, false)
        }
        ["drop", "event", "if", "exists", name, ..] | ["drop", "event", name, ..] => {
            info!("drop", "event", name, false)
        }
        ["alter", "table", name, ..] => info!("alter", "table", name, false),
        _ => None,
    }
}

// ── ALTER TABLE DDL helpers ───────────────────────────────────────────────

/// Parse an `ALTER TABLE t ADD COLUMN col TYPE [constraints]` or
/// `ALTER TABLE t ADD col TYPE` statement and return `(table_name, col_name, col_def)`.
/// Returns `None` for unsupported forms.
pub fn parse_alter_add_column(sql: &str) -> Option<(String, String, String)> {
    let lower = sql.trim().to_ascii_lowercase();
    // Normalize: "alter table T add column C type" or "alter table T add C type"
    let rest = lower.strip_prefix("alter table ")?;
    // rest = "t add column c type ..." or "t add c type ..."
    let (table_part, after_table) = {
        let mut it = rest.splitn(2, " add ");
        let t = it.next()?.trim().to_string();
        let after = it.next()?.trim().to_string();
        (t, after)
    };
    // after_table = "column c type ..." or "c type ..."
    let col_rest = after_table
        .strip_prefix("column ")
        .unwrap_or(&after_table);
    // col_rest = "c type [default ...]"
    let mut parts = col_rest.splitn(2, ' ');
    let col_name = parts.next()?.trim().to_string();
    let col_def = parts.next().unwrap_or("text").trim().to_string();
    if col_name.is_empty() {
        return None;
    }
    // Also extract from original casing for the definition.
    let original_lower = sql.trim().to_ascii_lowercase();
    let col_def_original = {
        // Find the column name in the original SQL (case-insensitive).
        let pos = original_lower.find(&col_name)?;
        sql[pos + col_name.len()..].trim().to_string()
    };
    Some((table_part, col_name, if col_def_original.is_empty() { col_def } else { col_def_original }))
}

/// Parse an `ALTER TABLE t DROP COLUMN col` or `ALTER TABLE t DROP col` statement.
/// Returns `(table_name, col_name)`.
pub fn parse_alter_drop_column(sql: &str) -> Option<(String, String)> {
    let lower = sql.trim().to_ascii_lowercase();
    let rest = lower.strip_prefix("alter table ")?;
    let (table_part, after_table) = {
        let mut it = rest.splitn(2, " drop ");
        let t = it.next()?.trim().to_string();
        let after = it.next()?.trim().to_string();
        (t, after)
    };
    let col_name = after_table
        .strip_prefix("column ")
        .unwrap_or(&after_table)
        .trim()
        .trim_end_matches(';')
        .to_string();
    if col_name.is_empty() {
        return None;
    }
    Some((table_part, col_name))
}

/// Apply an ADD COLUMN alter to an existing CREATE TABLE DDL string.
/// Inserts `col_name col_def` before the closing `)`.
pub fn apply_add_column_to_ddl(original_ddl: &str, col_name: &str, col_def: &str) -> String {
    // Find the last `)` to insert before.
    let close = original_ddl.rfind(')');
    match close {
        None => {
            // Degenerate — just append.
            format!("{original_ddl}, {col_name} {col_def}")
        }
        Some(pos) => {
            let before = original_ddl[..pos].trim_end_matches([' ', '\t', '\n', '\r', ',']);
            let after = &original_ddl[pos..];
            format!("{before}, {col_name} {col_def}{after}")
        }
    }
}

/// Apply a DROP COLUMN alter to an existing CREATE TABLE DDL string.
/// Removes the column definition from the column list.  Column matching is
/// case-insensitive.  If the column is not found, the original DDL is returned unchanged.
pub fn remove_column_from_ddl(original_ddl: &str, col_name: &str) -> String {
    // Find the `(` that opens the column list.
    let open = match original_ddl.find('(') {
        Some(i) => i,
        None => return original_ddl.to_string(),
    };
    let close = match original_ddl.rfind(')') {
        Some(i) => i,
        None => return original_ddl.to_string(),
    };
    let prefix = &original_ddl[..=open]; // "CREATE TABLE foo ("
    let suffix = &original_ddl[close..]; // ")"
    let col_list = &original_ddl[open + 1..close];

    // Split by comma, filter out the target column.
    let col_lower = col_name.to_ascii_lowercase();
    let remaining: Vec<&str> = col_list
        .split(',')
        .filter(|def| {
            let trimmed = def.trim().to_ascii_lowercase();
            // Match if the first word of this column def equals col_name.
            let first_word = trimmed.split_whitespace().next().unwrap_or("");
            first_word != col_lower.as_str()
        })
        .collect();
    format!("{}{}{}", prefix, remaining.join(","), suffix)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_table_parsed_and_registered() {
        let sql = "CREATE TABLE orders (id INT, amount FLOAT)";
        let info = parse_ddl_info(sql).expect("should parse");
        assert_eq!(info.operation, "create");
        assert_eq!(info.object_kind, "table");
        assert_eq!(info.object_name, "orders");

        let mut cat = DdlCatalog::new();
        cat.record_create(&info.object_kind, &info.database_name, &info.schema_name, &info.object_name, sql, 1_000, false);
        assert_eq!(cat.active_count(), 1);
        assert_eq!(cat.get("orders").unwrap().object_kind, "table");
    }

    #[test]
    fn drop_table_marks_entry_dropped() {
        let mut cat = DdlCatalog::new();
        cat.record_create("table", "default", "public", "products", "CREATE TABLE products (id INT)", 1_000, false);
        let result = cat.record_drop("default", "public", "products");
        assert_eq!(result, CatalogResult::Dropped);
        assert_eq!(cat.active_count(), 0);
        assert_eq!(cat.total_count(), 1); // still kept for history
    }

    #[test]
    fn alter_table_increments_alteration_count() {
        let mut cat = DdlCatalog::new();
        cat.record_create("table", "default", "public", "users", "CREATE TABLE users (id INT)", 1_000, false);
        cat.record_alter("default", "public", "users", "ALTER TABLE users ADD COLUMN email TEXT", 2_000);
        cat.record_alter("default", "public", "users", "ALTER TABLE users ADD COLUMN age INT", 3_000);
        let entry = cat.get("users").unwrap();
        assert_eq!(entry.alteration_count, 2);
        assert_eq!(entry.last_altered_at_unix_ms, Some(3_000));
    }

    #[test]
    fn create_view_and_function_parsed() {
        let view = parse_ddl_info("CREATE VIEW order_summary AS SELECT COUNT(*) FROM orders").unwrap();
        assert_eq!(view.object_kind, "view");
        assert_eq!(view.object_name, "order_summary");

        let func = parse_ddl_info("CREATE FUNCTION compute_tax(x FLOAT) RETURNS FLOAT AS $$...$$").unwrap();
        assert_eq!(func.object_kind, "function");
        assert_eq!(func.object_name, "compute_tax");
    }

    #[test]
    fn create_trigger_and_event_parsed() {
        let trigger = parse_ddl_info(
            "CREATE TRIGGER users_updated BEFORE UPDATE ON users FOR EACH ROW EXECUTE FUNCTION stamp()",
        )
        .unwrap();
        assert_eq!(trigger.object_kind, "trigger");
        assert_eq!(trigger.object_name, "users_updated");

        let event = parse_ddl_info(
            "CREATE EVENT refresh_cache ON SCHEDULE EVERY 1 HOUR DO CALL refresh()",
        )
        .unwrap();
        assert_eq!(event.object_kind, "event");
        assert_eq!(event.object_name, "refresh_cache");
    }

    #[test]
    fn drop_not_found_returns_correct_result() {
        let mut cat = DdlCatalog::new();
        assert_eq!(cat.record_drop("default", "public", "nonexistent"), CatalogResult::NotFound);
    }

    #[test]
    fn active_entries_excludes_dropped() {
        let mut cat = DdlCatalog::new();
        cat.record_create("table", "default", "public", "t1", "CREATE TABLE t1 (id INT)", 1_000, false);
        cat.record_create("table", "default", "public", "t2", "CREATE TABLE t2 (id INT)", 2_000, false);
        cat.record_drop("default", "public", "t1");
        let active = cat.active_entries();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].object_name, "t2");
    }

    #[test]
    fn create_duplicate_returns_already_exists() {
        let mut cat = DdlCatalog::new();
        let r1 = cat.record_create("table", "default", "public", "orders", "CREATE TABLE orders (id INT)", 1_000, false);
        assert_eq!(r1, CatalogResult::Created);
        let r2 = cat.record_create("table", "default", "public", "orders", "CREATE TABLE orders (id INT, name TEXT)", 2_000, false);
        assert_eq!(r2, CatalogResult::AlreadyExists);
        // Original entry should be preserved
        assert_eq!(cat.active_count(), 1);
        assert_eq!(cat.get("orders").unwrap().created_at_unix_ms, 1_000);
    }

    #[test]
    fn create_or_replace_replaces_existing() {
        let mut cat = DdlCatalog::new();
        cat.record_create("table", "default", "public", "orders", "CREATE TABLE orders (id INT)", 1_000, false);
        let r2 = cat.record_create("table", "default", "public", "orders", "CREATE OR REPLACE TABLE orders (id INT, name TEXT)", 2_000, true);
        assert_eq!(r2, CatalogResult::Replaced);
        assert_eq!(cat.active_count(), 1);
        assert_eq!(cat.get("orders").unwrap().created_at_unix_ms, 2_000);
    }

    #[test]
    fn create_after_drop_reuses_slot() {
        let mut cat = DdlCatalog::new();
        cat.record_create("table", "default", "public", "orders", "CREATE TABLE orders (id INT)", 1_000, false);
        cat.record_drop("default", "public", "orders");
        let r = cat.record_create("table", "default", "public", "orders", "CREATE TABLE orders (id INT)", 3_000, false);
        assert_eq!(r, CatalogResult::Replaced);
        assert_eq!(cat.active_count(), 1);
    }

    #[test]
    fn parse_ddl_strips_schema_qualifiers() {
        let info = parse_ddl_info("CREATE TABLE public.orders (id INT)").unwrap();
        assert_eq!(info.object_name, "orders");
        assert_eq!(info.database_name, "default");
        assert_eq!(info.schema_name, "public");

        let info2 = parse_ddl_info("CREATE TABLE db.public.orders (id INT)").unwrap();
        assert_eq!(info2.object_name, "orders");
        assert_eq!(info2.database_name, "db");
        assert_eq!(info2.schema_name, "public");

        let info3 = parse_ddl_info("DROP TABLE IF EXISTS public.orders").unwrap();
        assert_eq!(info3.object_name, "orders");
        assert_eq!(info3.database_name, "default");
        assert_eq!(info3.schema_name, "public");
    }

    #[test]
    fn parse_ddl_create_or_replace_sets_replace_ok() {
        let info = parse_ddl_info("CREATE TABLE t (id INT)").unwrap();
        assert!(!info.replace_ok);

        let info2 = parse_ddl_info("CREATE OR REPLACE TABLE t (id INT)").unwrap();
        assert!(info2.replace_ok);
    }
}
