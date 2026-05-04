//! VoltNueronGrid metadata layer.
//!
//! This crate owns the **database** as a first-class concept:
//! - A unique-named, case-insensitive `Database` with creation timestamp,
//!   owner, and per-database state.
//! - A [`DatabaseCatalog`] that enforces uniqueness, supports CRUD, and is
//!   serialisable for durable persistence.
//! - The per-database **metadata schema** view (`metadata.tables`,
//!   `metadata.columns`, `metadata.schemas`, `metadata.routines`,
//!   `metadata.users`, `metadata.roles`, `metadata.settings`) is described
//!   here as data; the executor pulls live rows via a thin trait.
//!
//! See `gaps-may26-1.md` §3.2 and §3.5 for the rationale; see
//! `remaining.md` Phase 1.3 for the rollout plan.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const CRATE_NAME: &str = "voltnuerongrid-meta";

// ─────────────────────────────────────────────────────────────────────────────
// Database identity
// ─────────────────────────────────────────────────────────────────────────────

/// A normalised database name. Case-insensitive equality, lowercase canonical
/// form, and validated to a small allowed character set so that database
/// names can also be used as filesystem-safe directory names (RocksDB column
/// families, future page-store directories).
///
/// Allowed characters: `[a-z0-9_]`, length 1..=63, must not start with a digit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DatabaseName(String);

impl DatabaseName {
    pub fn parse(input: &str) -> Result<Self, DatabaseNameError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(DatabaseNameError::Empty);
        }
        if trimmed.len() > 63 {
            return Err(DatabaseNameError::TooLong { len: trimmed.len() });
        }
        let lower = trimmed.to_ascii_lowercase();
        let mut chars = lower.chars();
        let first = chars.next().expect("non-empty checked above");
        if !first.is_ascii_alphabetic() && first != '_' {
            return Err(DatabaseNameError::InvalidStart { ch: first });
        }
        for ch in lower.chars() {
            if !(ch.is_ascii_alphanumeric() || ch == '_') {
                return Err(DatabaseNameError::InvalidChar { ch });
            }
        }
        // Reserve a small set of names that conflict with system roles or
        // would create confusing UX.
        if matches!(
            lower.as_str(),
            "metadata" | "information_schema" | "pg_catalog" | "vng_system"
        ) {
            return Err(DatabaseNameError::Reserved { name: lower });
        }
        Ok(Self(lower))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DatabaseName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatabaseNameError {
    Empty,
    TooLong { len: usize },
    InvalidStart { ch: char },
    InvalidChar { ch: char },
    Reserved { name: String },
}

impl std::fmt::Display for DatabaseNameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("database name must not be empty"),
            Self::TooLong { len } => write!(f, "database name is {len} chars (max 63)"),
            Self::InvalidStart { ch } => {
                write!(f, "database name must start with a letter or underscore, got {ch:?}")
            }
            Self::InvalidChar { ch } => write!(
                f,
                "database name may only contain letters, digits, and underscore; found {ch:?}"
            ),
            Self::Reserved { name } => write!(f, "database name {name:?} is reserved"),
        }
    }
}

impl std::error::Error for DatabaseNameError {}

// ─────────────────────────────────────────────────────────────────────────────
// Database record + catalog
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Database {
    pub name: DatabaseName,
    /// Epoch milliseconds at creation.
    pub created_at_ms: u128,
    /// Optional owner (operator id / user id). The catalog itself is not
    /// authoritative for ownership — that lives in the auth crate — but
    /// recording it here lets `DROP DATABASE` enforce ownership at the SQL
    /// boundary without a cross-crate join.
    pub owner: Option<String>,
    /// Free-form description — surfaced in `metadata.databases`.
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatabaseCatalogError {
    AlreadyExists { name: String },
    NotFound { name: String },
    InvalidName(DatabaseNameError),
}

impl std::fmt::Display for DatabaseCatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyExists { name } => write!(f, "database {name:?} already exists"),
            Self::NotFound { name } => write!(f, "database {name:?} not found"),
            Self::InvalidName(e) => write!(f, "invalid database name: {e}"),
        }
    }
}

impl std::error::Error for DatabaseCatalogError {}

impl From<DatabaseNameError> for DatabaseCatalogError {
    fn from(value: DatabaseNameError) -> Self {
        Self::InvalidName(value)
    }
}

/// In-memory catalog of databases. Persistent layering belongs in
/// `voltnuerongrid-store` once Phase 2 (RocksDB) lands. For now this struct
/// is `serde`-friendly so the service can snapshot/restore via JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatabaseCatalog {
    /// Keyed by the canonical (lowercase) database name. `BTreeMap` so
    /// listing is deterministically alphabetical.
    databases: BTreeMap<String, Database>,
}

impl DatabaseCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new database. Returns `AlreadyExists` if the name (after
    /// normalisation) is already taken.
    pub fn create(
        &mut self,
        name: &str,
        now_ms: u128,
        owner: Option<&str>,
        description: Option<&str>,
    ) -> Result<&Database, DatabaseCatalogError> {
        let parsed = DatabaseName::parse(name)?;
        if self.databases.contains_key(parsed.as_str()) {
            return Err(DatabaseCatalogError::AlreadyExists {
                name: parsed.as_str().to_string(),
            });
        }
        let key = parsed.as_str().to_string();
        let db = Database {
            name: parsed,
            created_at_ms: now_ms,
            owner: owner.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            description: description.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        };
        self.databases.insert(key.clone(), db);
        Ok(self.databases.get(&key).expect("just inserted"))
    }

    /// Drop a database. Idempotent only when `if_exists` is true.
    pub fn drop_database(
        &mut self,
        name: &str,
        if_exists: bool,
    ) -> Result<Option<Database>, DatabaseCatalogError> {
        let parsed = DatabaseName::parse(name)?;
        match self.databases.remove(parsed.as_str()) {
            Some(db) => Ok(Some(db)),
            None if if_exists => Ok(None),
            None => Err(DatabaseCatalogError::NotFound {
                name: parsed.as_str().to_string(),
            }),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Database> {
        DatabaseName::parse(name)
            .ok()
            .and_then(|p| self.databases.get(p.as_str()))
    }

    pub fn exists(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn list(&self) -> Vec<&Database> {
        self.databases.values().collect()
    }

    pub fn len(&self) -> usize {
        self.databases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.databases.is_empty()
    }

    /// Serialise to JSON for persistent snapshot. Pair with [`Self::restore`].
    pub fn snapshot_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn restore(input: &str) -> Result<Self, String> {
        serde_json::from_str(input).map_err(|e| e.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Metadata-schema layout
// ─────────────────────────────────────────────────────────────────────────────

/// The set of system tables exposed inside every database under the
/// `metadata` schema. This is the contract for the Studio to display and
/// for SQL queries like `SELECT * FROM metadata.tables` to reach.
///
/// Phase 1.4 will wire each table to a live source (DDL catalog, role
/// matrix, runtime-config snapshot). For now this enum is consumed by the
/// service to publish the *shape* via HTTP so the UI can render the panels
/// even before live data wires up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataTable {
    Databases,
    Schemas,
    Tables,
    Columns,
    Indexes,
    Views,
    Routines,
    Triggers,
    Users,
    Roles,
    Grants,
    Settings,
}

impl MetadataTable {
    pub const ALL: &'static [MetadataTable] = &[
        Self::Databases,
        Self::Schemas,
        Self::Tables,
        Self::Columns,
        Self::Indexes,
        Self::Views,
        Self::Routines,
        Self::Triggers,
        Self::Users,
        Self::Roles,
        Self::Grants,
        Self::Settings,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Self::Databases => "databases",
            Self::Schemas => "schemas",
            Self::Tables => "tables",
            Self::Columns => "columns",
            Self::Indexes => "indexes",
            Self::Views => "views",
            Self::Routines => "routines",
            Self::Triggers => "triggers",
            Self::Users => "users",
            Self::Roles => "roles",
            Self::Grants => "grants",
            Self::Settings => "settings",
        }
    }

    pub fn columns(&self) -> &'static [&'static str] {
        match self {
            Self::Databases => &["name", "owner", "created_at_ms", "description"],
            Self::Schemas => &["database_name", "schema_name"],
            Self::Tables => &[
                "database_name",
                "schema_name",
                "table_name",
                "kind",
                "created_at_ms",
            ],
            Self::Columns => &[
                "database_name",
                "schema_name",
                "table_name",
                "column_name",
                "ordinal_position",
                "data_type",
                "is_nullable",
            ],
            Self::Indexes => &[
                "database_name",
                "schema_name",
                "table_name",
                "index_name",
                "column_name",
                "is_unique",
            ],
            Self::Views => &[
                "database_name",
                "schema_name",
                "view_name",
                "definition",
            ],
            Self::Routines => &[
                "database_name",
                "schema_name",
                "routine_name",
                "language",
                "kind",
            ],
            Self::Triggers => &[
                "database_name",
                "schema_name",
                "trigger_name",
                "table_name",
                "event",
            ],
            Self::Users => &["database_name", "username", "created_at_ms"],
            Self::Roles => &["database_name", "role_name"],
            Self::Grants => &["database_name", "role_name", "object", "privilege"],
            Self::Settings => &["database_name", "key", "value", "scope"],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataTableSpec {
    pub name: &'static str,
    pub columns: Vec<&'static str>,
}

/// Returns the static schema layout for the metadata schema, suitable for
/// JSON-encoding into an HTTP response.
pub fn metadata_schema_layout() -> Vec<MetadataTableSpec> {
    MetadataTable::ALL
        .iter()
        .map(|t| MetadataTableSpec {
            name: t.name(),
            columns: t.columns().to_vec(),
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> u128 {
        1_700_000_000_000
    }

    // ── DatabaseName ────────────────────────────────────────────────────────

    #[test]
    fn database_name_lowercases() {
        let n = DatabaseName::parse("MyDb").unwrap();
        assert_eq!(n.as_str(), "mydb");
    }

    #[test]
    fn database_name_trims_whitespace() {
        let n = DatabaseName::parse("  hr  ").unwrap();
        assert_eq!(n.as_str(), "hr");
    }

    #[test]
    fn database_name_allows_underscore_start() {
        DatabaseName::parse("_internal").unwrap();
    }

    #[test]
    fn database_name_rejects_digit_start() {
        let err = DatabaseName::parse("9lives").unwrap_err();
        assert!(matches!(err, DatabaseNameError::InvalidStart { .. }));
    }

    #[test]
    fn database_name_rejects_special_chars() {
        let err = DatabaseName::parse("hr-team").unwrap_err();
        assert!(matches!(err, DatabaseNameError::InvalidChar { ch: '-' }));
    }

    #[test]
    fn database_name_rejects_too_long() {
        let s = "a".repeat(64);
        let err = DatabaseName::parse(&s).unwrap_err();
        assert!(matches!(err, DatabaseNameError::TooLong { len: 64 }));
    }

    #[test]
    fn database_name_rejects_reserved() {
        for reserved in &["metadata", "information_schema", "pg_catalog", "vng_system"] {
            let err = DatabaseName::parse(reserved).unwrap_err();
            assert!(matches!(err, DatabaseNameError::Reserved { .. }), "{reserved}");
        }
    }

    // ── DatabaseCatalog ─────────────────────────────────────────────────────

    #[test]
    fn create_then_get() {
        let mut cat = DatabaseCatalog::new();
        let db = cat.create("hr", now(), Some("admin"), Some("HR data")).unwrap();
        assert_eq!(db.name.as_str(), "hr");
        assert_eq!(db.owner.as_deref(), Some("admin"));
        assert_eq!(cat.get("HR").map(|d| d.name.as_str()), Some("hr"));
    }

    #[test]
    fn create_duplicate_after_normalisation_fails() {
        let mut cat = DatabaseCatalog::new();
        cat.create("hr", now(), None, None).unwrap();
        let err = cat.create("HR", now(), None, None).unwrap_err();
        assert!(matches!(err, DatabaseCatalogError::AlreadyExists { .. }));
        assert_eq!(cat.len(), 1);
    }

    #[test]
    fn drop_existing_database_returns_record() {
        let mut cat = DatabaseCatalog::new();
        cat.create("sales", now(), None, None).unwrap();
        let dropped = cat.drop_database("sales", false).unwrap();
        assert!(dropped.is_some());
        assert!(!cat.exists("sales"));
    }

    #[test]
    fn drop_missing_without_if_exists_errors() {
        let mut cat = DatabaseCatalog::new();
        let err = cat.drop_database("nope", false).unwrap_err();
        assert!(matches!(err, DatabaseCatalogError::NotFound { .. }));
    }

    #[test]
    fn drop_missing_with_if_exists_is_idempotent() {
        let mut cat = DatabaseCatalog::new();
        let result = cat.drop_database("nope", true).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn list_returns_alphabetical_order() {
        let mut cat = DatabaseCatalog::new();
        cat.create("zeta", now(), None, None).unwrap();
        cat.create("alpha", now(), None, None).unwrap();
        cat.create("mu", now(), None, None).unwrap();
        let names: Vec<&str> = cat.list().iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "mu", "zeta"]);
    }

    #[test]
    fn snapshot_round_trips() {
        let mut cat = DatabaseCatalog::new();
        cat.create("hr", now(), Some("admin"), None).unwrap();
        cat.create("sales", now() + 1, None, Some("revenue")).unwrap();

        let snapshot = cat.snapshot_json().unwrap();
        let restored = DatabaseCatalog::restore(&snapshot).unwrap();

        assert_eq!(restored.len(), 2);
        assert_eq!(restored.get("hr").unwrap().owner.as_deref(), Some("admin"));
        assert_eq!(
            restored.get("sales").unwrap().description.as_deref(),
            Some("revenue")
        );
    }

    // ── Metadata schema layout ──────────────────────────────────────────────

    #[test]
    fn metadata_schema_layout_has_all_tables() {
        let layout = metadata_schema_layout();
        assert_eq!(layout.len(), MetadataTable::ALL.len());
        for t in &layout {
            assert!(!t.columns.is_empty(), "{} has zero columns", t.name);
        }
    }

    #[test]
    fn metadata_table_names_are_unique() {
        let mut names: Vec<&str> = MetadataTable::ALL.iter().map(|t| t.name()).collect();
        names.sort();
        let original_len = names.len();
        names.dedup();
        assert_eq!(names.len(), original_len, "duplicate metadata table name");
    }
}
