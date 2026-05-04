//! VoltNueronGrid runtime configuration.
//!
//! Centralises selection of pluggable backends:
//!
//! - **Storage engine:** RocksDB (default, supported) or VNG-native
//!   (placeholder, not implemented yet — emits a clear error at startup).
//! - **SQL engine:** DataFusion + sqlparser-rs (default, supported) or
//!   VNG-native (placeholder, not implemented yet).
//!
//! See `gaps-may26-1.md` for the rationale: we adopt mature OSS for the first
//! production-grade release, then implement our own engines as drop-in
//! replacements without changing the public API.
//!
//! # Loading order
//!
//! 1. Defaults (everything sane and supported).
//! 2. Config file (`VNG_CONFIG_PATH`, default `./vng.config.json`) if present.
//! 3. Environment variables (override per field).
//!
//! Every field is also queryable via the metadata schema (Phase 1) so the
//! Studio UI's settings panel can read and write them at runtime.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub const CRATE_NAME: &str = "voltnuerongrid-config";

// ─────────────────────────────────────────────────────────────────────────────
// Backend selectors
// ─────────────────────────────────────────────────────────────────────────────

/// Choice of durable storage substrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageEngine {
    /// RocksDB (column-family per database). Default. **Supported.**
    Rocksdb,
    /// VNG-native page-based heap + WAL. Placeholder — emits a startup error.
    Vng,
}

impl Default for StorageEngine {
    fn default() -> Self { StorageEngine::Rocksdb }
}

impl StorageEngine {
    /// Returns `Ok(())` if this engine is implemented, otherwise a friendly
    /// error message the caller should surface to the operator.
    pub fn require_supported(&self) -> Result<(), String> {
        match self {
            StorageEngine::Rocksdb => Ok(()),
            StorageEngine::Vng => Err(
                "Storage engine 'vng' is reserved for future releases. \
                 Set VNG_STORAGE_ENGINE=rocksdb (or omit it) to use the \
                 supported default. Tracking: gaps-may26-1.md §3.1."
                    .to_string(),
            ),
        }
    }

    pub fn from_env_str(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "rocksdb" | "rocks" => Ok(StorageEngine::Rocksdb),
            "vng" | "voltnuerongrid" | "native" => Ok(StorageEngine::Vng),
            other => Err(format!(
                "VNG_STORAGE_ENGINE={other:?} is not recognised. \
                 Valid values: rocksdb, vng."
            )),
        }
    }
}

/// Choice of SQL parser + execution engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlEngine {
    /// `sqlparser-rs` for parsing + DataFusion for execution. Default. **Supported.**
    Datafusion,
    /// VNG-native parser + executor. Placeholder.
    Vng,
}

impl Default for SqlEngine {
    fn default() -> Self { SqlEngine::Datafusion }
}

impl SqlEngine {
    pub fn require_supported(&self) -> Result<(), String> {
        match self {
            SqlEngine::Datafusion => Ok(()),
            SqlEngine::Vng => Err(
                "SQL engine 'vng' is reserved for future releases. \
                 Set VNG_SQL_ENGINE=datafusion (or omit it) to use the \
                 supported default. Tracking: gaps-may26-1.md §3.3 §3.4 §3.11."
                    .to_string(),
            ),
        }
    }

    pub fn from_env_str(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "datafusion" | "df" | "sqlparser" => Ok(SqlEngine::Datafusion),
            "vng" | "voltnuerongrid" | "native" => Ok(SqlEngine::Vng),
            other => Err(format!(
                "VNG_SQL_ENGINE={other:?} is not recognised. \
                 Valid values: datafusion, vng."
            )),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Top-level runtime config
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageConfig {
    pub engine: StorageEngine,
    /// Filesystem path where the storage engine lives (one subdirectory per database).
    pub data_dir: String,
    /// Number of background flush threads (RocksDB).
    pub max_background_jobs: u32,
    /// Whether to fsync the WAL on every commit. Defaults to true (durable).
    pub wal_fsync_on_commit: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            engine: StorageEngine::default(),
            data_dir: "./data".to_string(),
            max_background_jobs: 4,
            wal_fsync_on_commit: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SqlConfig {
    pub engine: SqlEngine,
    /// Default OLAP target row count before a query gets pushed off the OLTP path.
    pub htap_olap_threshold_rows: usize,
    /// Maximum result rows a single SELECT may return (server-side cap).
    pub max_result_rows: usize,
}

impl Default for SqlConfig {
    fn default() -> Self {
        Self {
            engine: SqlEngine::default(),
            htap_olap_threshold_rows: 100_000,
            max_result_rows: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub sql: SqlConfig,
}

impl RuntimeConfig {
    /// Build the effective runtime config: defaults → file → env.
    /// Each step is best-effort; missing file is fine, malformed file errors out.
    pub fn from_env_and_file(
        env: &dyn EnvProvider,
        file_contents: Option<&str>,
    ) -> Result<Self, String> {
        let mut cfg = if let Some(contents) = file_contents {
            serde_json::from_str::<RuntimeConfig>(contents)
                .map_err(|e| format!("malformed VNG config file: {e}"))?
        } else {
            RuntimeConfig::default()
        };

        if let Some(v) = env.get("VNG_STORAGE_ENGINE") {
            cfg.storage.engine = StorageEngine::from_env_str(&v)?;
        }
        if let Some(v) = env.get("VNG_DATA_DIR") {
            cfg.storage.data_dir = v.trim().to_string();
        }
        if let Some(v) = env.get("VNG_STORAGE_BACKGROUND_JOBS") {
            cfg.storage.max_background_jobs = v.trim().parse().map_err(|e| {
                format!("VNG_STORAGE_BACKGROUND_JOBS not a u32: {e}")
            })?;
        }
        if let Some(v) = env.get("VNG_WAL_FSYNC_ON_COMMIT") {
            cfg.storage.wal_fsync_on_commit = matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }
        if let Some(v) = env.get("VNG_SQL_ENGINE") {
            cfg.sql.engine = SqlEngine::from_env_str(&v)?;
        }
        if let Some(v) = env.get("VNG_HTAP_OLAP_THRESHOLD_ROWS") {
            cfg.sql.htap_olap_threshold_rows = v.trim().parse().map_err(|e| {
                format!("VNG_HTAP_OLAP_THRESHOLD_ROWS not a usize: {e}")
            })?;
        }
        if let Some(v) = env.get("VNG_MAX_RESULT_ROWS") {
            cfg.sql.max_result_rows = v.trim().parse().map_err(|e| {
                format!("VNG_MAX_RESULT_ROWS not a usize: {e}")
            })?;
        }

        Ok(cfg)
    }

    /// Validate that all selected backends are supported. Returns the first
    /// problem so the caller can fail-fast.
    pub fn validate(&self) -> Result<(), String> {
        self.storage.engine.require_supported()?;
        self.sql.engine.require_supported()?;
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// EnvProvider abstraction (so unit tests don't touch process env)
// ─────────────────────────────────────────────────────────────────────────────

pub trait EnvProvider {
    fn get(&self, key: &str) -> Option<String>;
}

/// Reads from real `std::env`. Use in production.
pub struct ProcessEnv;

impl EnvProvider for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// In-memory env for tests.
#[derive(Default, Debug, Clone)]
pub struct MemEnv(pub std::collections::HashMap<String, String>);

impl EnvProvider for MemEnv {
    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> MemEnv {
        MemEnv(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
    }

    #[test]
    fn defaults_are_all_supported() {
        let cfg = RuntimeConfig::default();
        assert_eq!(cfg.storage.engine, StorageEngine::Rocksdb);
        assert_eq!(cfg.sql.engine, SqlEngine::Datafusion);
        cfg.validate().expect("defaults must validate");
    }

    #[test]
    fn env_overrides_apply() {
        let e = env(&[("VNG_STORAGE_ENGINE", "rocksdb"), ("VNG_DATA_DIR", "/var/lib/vng")]);
        let cfg = RuntimeConfig::from_env_and_file(&e, None).expect("ok");
        assert_eq!(cfg.storage.data_dir, "/var/lib/vng");
    }

    #[test]
    fn vng_storage_engine_is_rejected_at_validate() {
        let e = env(&[("VNG_STORAGE_ENGINE", "vng")]);
        let cfg = RuntimeConfig::from_env_and_file(&e, None).expect("parse ok");
        let err = cfg.validate().expect_err("should reject vng engine");
        assert!(err.contains("not supported") || err.contains("future releases"),
                "got: {err}");
    }

    #[test]
    fn vng_sql_engine_is_rejected_at_validate() {
        let e = env(&[("VNG_SQL_ENGINE", "vng")]);
        let cfg = RuntimeConfig::from_env_and_file(&e, None).expect("parse ok");
        cfg.validate().expect_err("should reject vng sql engine");
    }

    #[test]
    fn unknown_storage_value_errors_at_parse() {
        let e = env(&[("VNG_STORAGE_ENGINE", "tigerbeetle")]);
        let err = RuntimeConfig::from_env_and_file(&e, None).expect_err("should fail");
        assert!(err.contains("tigerbeetle"));
    }

    #[test]
    fn json_file_then_env_overrides() {
        let json = r#"{
            "storage": { "engine": "rocksdb", "data_dir": "/from-file", "max_background_jobs": 8, "wal_fsync_on_commit": true },
            "sql": { "engine": "datafusion", "htap_olap_threshold_rows": 1000, "max_result_rows": 50000 }
        }"#;
        let e = env(&[("VNG_DATA_DIR", "/from-env")]);
        let cfg = RuntimeConfig::from_env_and_file(&e, Some(json)).expect("ok");
        assert_eq!(cfg.storage.data_dir, "/from-env");
        assert_eq!(cfg.storage.max_background_jobs, 8); // came from file, env didn't touch it
        assert_eq!(cfg.sql.htap_olap_threshold_rows, 1000);
    }

    #[test]
    fn malformed_json_errors() {
        let e = MemEnv(HashMap::new());
        let err = RuntimeConfig::from_env_and_file(&e, Some("{not json}"))
            .expect_err("malformed JSON must error");
        assert!(err.contains("malformed VNG config file"));
    }
}
