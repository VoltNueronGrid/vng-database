#![forbid(unsafe_code)]

//! In-memory DDL object lifecycle catalog (REQ-02).
//!
//! Tracks CREATE / DROP / ALTER operations for tables, views,
//! materialized views, and functions so the runtime can expose
//! object-lifecycle state without a persistent schema store.

use std::collections::HashMap;

// ── Public types ─────────────────────────────────────────────────────────

/// One registered DDL object (table, view, materialized view, function).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdlCatalogEntry {
    /// Lower-cased canonical name (used as the HashMap key).
    pub object_name: String,
    /// `"table"`, `"view"`, `"materialized_view"`, or `"function"`.
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
}

/// Lightweight in-memory DDL catalog keyed by lower-cased object name.
#[derive(Default)]
pub struct DdlCatalog {
    entries: HashMap<String, DdlCatalogEntry>,
}

impl DdlCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a CREATE statement. If the name already exists and is active,
    /// the old entry is replaced (CREATE OR REPLACE semantics); if the entry
    /// was previously dropped the slot is reused.
    pub fn record_create(
        &mut self,
        kind: &str,
        name: &str,
        statement: &str,
        now_ms: u128,
    ) -> CatalogResult {
        let key = name.trim().to_ascii_lowercase();
        let existed = self.entries.contains_key(&key);
        self.entries.insert(
            key,
            DdlCatalogEntry {
                object_name: name.trim().to_string(),
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
    pub fn record_drop(&mut self, name: &str) -> CatalogResult {
        let key = name.trim().to_ascii_lowercase();
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
    pub fn record_alter(
        &mut self,
        name: &str,
        _statement: &str,
        now_ms: u128,
    ) -> CatalogResult {
        let key = name.trim().to_ascii_lowercase();
        match self.entries.get_mut(&key) {
            None => CatalogResult::NotFound,
            Some(e) if e.dropped => CatalogResult::AlreadyDropped,
            Some(e) => {
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

    /// Lookup a single entry by name (any state).
    pub fn get(&self, name: &str) -> Option<&DdlCatalogEntry> {
        self.entries.get(&name.trim().to_ascii_lowercase())
    }

    pub fn total_count(&self) -> usize {
        self.entries.len()
    }

    pub fn active_count(&self) -> usize {
        self.entries.values().filter(|e| !e.dropped).count()
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
    /// Extracted object name (lowercased).
    pub object_name: String,
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

    let clean = |s: &str| -> String {
        // Split at '(' to strip function argument lists (e.g. "compute_tax(x" → "compute_tax")
        let base = s.split('(').next().unwrap_or(s);
        base.trim_matches(|c: char| c == ')' || c == ';' || c == ',')
            .to_string()
    };

    match words.as_slice() {
        ["create", "table", name, ..] => Some(DdlObjectInfo {
            operation: "create",
            object_kind: "table",
            object_name: clean(name),
        }),
        ["create", "view", name, ..] => Some(DdlObjectInfo {
            operation: "create",
            object_kind: "view",
            object_name: clean(name),
        }),
        ["create", "materialized", "view", name, ..] => Some(DdlObjectInfo {
            operation: "create",
            object_kind: "materialized_view",
            object_name: clean(name),
        }),
        ["create", "function", name, ..] | ["create", "or", "replace", "function", name, ..] => {
            Some(DdlObjectInfo {
                operation: "create",
                object_kind: "function",
                object_name: clean(name),
            })
        }
        ["create", "or", "replace", "view", name, ..] => Some(DdlObjectInfo {
            operation: "create",
            object_kind: "view",
            object_name: clean(name),
        }),
        ["create", "or", "replace", "table", name, ..] => Some(DdlObjectInfo {
            operation: "create",
            object_kind: "table",
            object_name: clean(name),
        }),
        ["drop", "table", "if", "exists", name, ..] | ["drop", "table", name, ..] => {
            Some(DdlObjectInfo {
                operation: "drop",
                object_kind: "table",
                object_name: clean(name),
            })
        }
        ["drop", "view", "if", "exists", name, ..] | ["drop", "view", name, ..] => {
            Some(DdlObjectInfo {
                operation: "drop",
                object_kind: "view",
                object_name: clean(name),
            })
        }
        ["drop", "function", "if", "exists", name, ..] | ["drop", "function", name, ..] => {
            Some(DdlObjectInfo {
                operation: "drop",
                object_kind: "function",
                object_name: clean(name),
            })
        }
        ["alter", "table", name, ..] => Some(DdlObjectInfo {
            operation: "alter",
            object_kind: "table",
            object_name: clean(name),
        }),
        _ => None,
    }
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
        cat.record_create(&info.object_kind, &info.object_name, sql, 1_000);
        assert_eq!(cat.active_count(), 1);
        assert_eq!(cat.get("orders").unwrap().object_kind, "table");
    }

    #[test]
    fn drop_table_marks_entry_dropped() {
        let mut cat = DdlCatalog::new();
        cat.record_create("table", "products", "CREATE TABLE products (id INT)", 1_000);
        let result = cat.record_drop("products");
        assert_eq!(result, CatalogResult::Dropped);
        assert_eq!(cat.active_count(), 0);
        assert_eq!(cat.total_count(), 1); // still kept for history
    }

    #[test]
    fn alter_table_increments_alteration_count() {
        let mut cat = DdlCatalog::new();
        cat.record_create("table", "users", "CREATE TABLE users (id INT)", 1_000);
        cat.record_alter("users", "ALTER TABLE users ADD COLUMN email TEXT", 2_000);
        cat.record_alter("users", "ALTER TABLE users ADD COLUMN age INT", 3_000);
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
    fn drop_not_found_returns_correct_result() {
        let mut cat = DdlCatalog::new();
        assert_eq!(cat.record_drop("nonexistent"), CatalogResult::NotFound);
    }

    #[test]
    fn active_entries_excludes_dropped() {
        let mut cat = DdlCatalog::new();
        cat.record_create("table", "t1", "CREATE TABLE t1 (id INT)", 1_000);
        cat.record_create("table", "t2", "CREATE TABLE t2 (id INT)", 2_000);
        cat.record_drop("t1");
        let active = cat.active_entries();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].object_name, "t2");
    }
}
