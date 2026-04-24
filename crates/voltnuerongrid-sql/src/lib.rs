#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-sql";

use std::collections::HashMap;

pub mod ast;
pub mod legacy_aggregations;
pub mod planner;
pub mod tokenizer;

pub use ast::{
    parse_one, ColumnDef, CreateTableStatement, DeleteStatement, InsertStatement,
    JoinClause, OrderByClause, SelectStatement, Statement, UpdateStatement,
};
pub use legacy_aggregations::eval_legacy_numeric_aggregation;
pub use planner::{plan, CostEstimate, PlanNode, QueryPlan, RoutingHint};
pub use tokenizer::{keyword_count, semantic_tokens, tokenize, Token};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlStatementKind {
    Select,
    Insert,
    Update,
    Delete,
    CreateTable,
    CreateView,
    CreateMaterializedView,
    CreateFunction,
    AlterTable,
    DropTable,
    Begin,
    Commit,
    Rollback,
    // REQ-23: savepoint support
    Savepoint,
    ReleaseSavepoint,
    RollbackToSavepoint,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlStatement {
    pub raw: String,
    pub kind: SqlStatementKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisResult {
    pub kind: SqlStatementKind,
    pub requires_transaction: bool,
    pub touches_catalog: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionLanguage {
    Builtin,
    Rust,
    JavaScript,
    Python,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredFunction {
    pub name: String,
    pub language: FunctionLanguage,
    pub deterministic: bool,
    pub description: String,
}

#[derive(Debug, Default)]
pub struct FunctionRegistry {
    functions: HashMap<String, RegisteredFunction>,
}

impl FunctionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, function: RegisteredFunction) -> bool {
        let key = normalize_ident(&function.name);
        self.functions.insert(key, function).is_none()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.functions.contains_key(&normalize_ident(name))
    }

    pub fn get(&self, name: &str) -> Option<&RegisteredFunction> {
        self.functions.get(&normalize_ident(name))
    }

    pub fn list(&self) -> Vec<&RegisteredFunction> {
        let mut values: Vec<&RegisteredFunction> = self.functions.values().collect();
        values.sort_by(|a, b| a.name.cmp(&b.name));
        values
    }

    // S7-004: Seed the standard built-in function catalog (parity closure pass).
    pub fn with_builtins() -> Self {
        let mut r = Self::new();
        for f in builtin_function_catalog() {
            r.register(f);
        }
        r
    }

    pub fn register_missing_builtins(&mut self) {
        for f in builtin_function_catalog() {
            if !self.contains(&f.name) {
                self.register(f);
            }
        }
    }
}

/// S7-004: Returns the canonical set of seeded built-in function signatures.
///
/// Each entry documents the function name, language (always `Builtin`),
/// determinism flag, and a short description.  The execution engine consults
/// this catalog when resolving function calls during planning.
pub fn builtin_function_catalog() -> Vec<RegisteredFunction> {
    macro_rules! b {
        ($name:expr, $det:expr, $desc:expr) => {
            RegisteredFunction {
                name: $name.to_string(),
                language: FunctionLanguage::Builtin,
                deterministic: $det,
                description: $desc.to_string(),
            }
        };
    }

    vec![
        // ── Aggregates ────────────────────────────────────────────────────────
        b!("COUNT",       true,  "Returns the number of rows or non-NULL values"),
        b!("SUM",         true,  "Returns the sum of a numeric expression"),
        b!("AVG",         true,  "Returns the arithmetic mean of a numeric expression"),
        b!("MIN",         true,  "Returns the minimum value"),
        b!("MAX",         true,  "Returns the maximum value"),
        b!("STRING_AGG",  false, "Concatenates string values with a delimiter"),
        b!("ARRAY_AGG",   false, "Aggregates values into an array"),
        b!("JSON_AGG",    false, "Aggregates values into a JSON array"),
        // ── Conditional / null-handling ───────────────────────────────────────
        b!("COALESCE",    true,  "Returns the first non-NULL argument"),
        b!("NULLIF",      true,  "Returns NULL if two arguments are equal"),
        b!("GREATEST",    true,  "Returns the largest of the supplied values"),
        b!("LEAST",       true,  "Returns the smallest of the supplied values"),
        // ── Date / time ───────────────────────────────────────────────────────
        b!("DATE_TRUNC",          true,  "Truncates a timestamp to the specified precision"),
        b!("DATE_PART",           true,  "Extracts a subfield from a date/time value"),
        b!("NOW",                 false, "Returns the current date and time"),
        b!("CURRENT_TIMESTAMP",   false, "Returns the current date and time"),
        b!("CURRENT_DATE",        false, "Returns the current date"),
        b!("CURRENT_TIME",        false, "Returns the current time"),
        // ── String ────────────────────────────────────────────────────────────
        b!("LOWER",    true, "Converts a string to lower case"),
        b!("UPPER",    true, "Converts a string to upper case"),
        b!("TRIM",     true, "Removes leading and trailing whitespace (or specified characters)"),
        b!("LTRIM",    true, "Removes leading whitespace or specified characters"),
        b!("RTRIM",    true, "Removes trailing whitespace or specified characters"),
        b!("SUBSTR",   true, "Extracts a substring"),
        b!("SUBSTRING",true, "Extracts a substring (SQL-standard alias for SUBSTR)"),
        b!("LENGTH",   true, "Returns the length of a string"),
        b!("CONCAT",   true, "Concatenates strings"),
        b!("REPLACE",  true, "Replaces occurrences of a substring"),
        b!("SPLIT_PART", true, "Splits a string on a delimiter and returns the nth part"),
        b!("LPAD",     true, "Pads a string on the left to a given length"),
        b!("RPAD",     true, "Pads a string on the right to a given length"),
        b!("REVERSE",  true, "Reverses a string"),
        b!("REGEXP_MATCH",   true, "Matches a string against a regular expression"),
        b!("REGEXP_REPLACE", true, "Replaces matches of a regular expression"),
        // ── Type conversion ───────────────────────────────────────────────────
        b!("CAST",     true, "Converts a value to the specified data type"),
        b!("TRY_CAST", true, "Converts a value; returns NULL instead of raising on failure"),
        // ── Math ─────────────────────────────────────────────────────────────
        b!("ABS",    true, "Absolute value"),
        b!("CEIL",   true, "Ceiling (smallest integer >= argument)"),
        b!("FLOOR",  true, "Floor (largest integer <= argument)"),
        b!("ROUND",  true, "Rounds to the nearest integer or given number of decimals"),
        b!("POWER",  true, "Raises a value to the given power"),
        b!("SQRT",   true, "Square root"),
        b!("MOD",    true, "Remainder of division"),
        b!("SIGN",   true, "Sign of a number (-1, 0, or 1)"),
        // ── JSON helpers ──────────────────────────────────────────────────────
        b!("JSON_EXTRACT",    true, "Extracts a value from a JSON document by path"),
        b!("JSON_OBJECT",     true, "Constructs a JSON object from key/value pairs"),
        b!("JSON_ARRAY",      true, "Constructs a JSON array"),
        b!("JSONB_EXTRACT_PATH", true, "Extracts a sub-object from a JSONB value"),
        // ── Window helpers ────────────────────────────────────────────────────
        b!("ROW_NUMBER",   true, "Assigns a sequential integer to each row in the partition"),
        b!("RANK",         true, "Assigns a rank with gaps for ties"),
        b!("DENSE_RANK",   true, "Assigns a rank without gaps for ties"),
        b!("LAG",          true, "Returns a value from a prior row in the partition"),
        b!("LEAD",         true, "Returns a value from a subsequent row in the partition"),
    ]
}

#[derive(Debug, Default)]
pub struct SqlAnalyzer;

impl SqlAnalyzer {
    pub fn classify_statement(sql: &str) -> SqlStatementKind {
        let normalized = normalize_sql_for_match(sql);
        let tokens: Vec<&str> = normalized.split_whitespace().collect();
        if tokens.is_empty() {
            return SqlStatementKind::Unknown;
        }

        match (tokens.first().copied(), tokens.get(1).copied(), tokens.get(2).copied()) {
            (Some("SELECT"), _, _) => SqlStatementKind::Select,
            (Some("INSERT"), _, _) => SqlStatementKind::Insert,
            (Some("UPDATE"), _, _) => SqlStatementKind::Update,
            (Some("DELETE"), _, _) => SqlStatementKind::Delete,
            (Some("CREATE"), Some("TABLE"), _) => SqlStatementKind::CreateTable,
            (Some("CREATE"), Some("VIEW"), _) => SqlStatementKind::CreateView,
            (Some("CREATE"), Some("MATERIALIZED"), Some("VIEW")) => {
                SqlStatementKind::CreateMaterializedView
            }
            (Some("CREATE"), Some("FUNCTION"), _) => SqlStatementKind::CreateFunction,
            (Some("ALTER"), Some("TABLE"), _) => SqlStatementKind::AlterTable,
            (Some("DROP"), Some("TABLE"), _) => SqlStatementKind::DropTable,
            (Some("BEGIN"), _, _) => SqlStatementKind::Begin,
            (Some("COMMIT"), _, _) => SqlStatementKind::Commit,
            // ROLLBACK TO [SAVEPOINT] — must match before bare ROLLBACK
            (Some("ROLLBACK"), Some("TO"), _) => SqlStatementKind::RollbackToSavepoint,
            (Some("ROLLBACK"), _, _) => SqlStatementKind::Rollback,
            (Some("SAVEPOINT"), _, _) => SqlStatementKind::Savepoint,
            (Some("RELEASE"), Some("SAVEPOINT"), _) => SqlStatementKind::ReleaseSavepoint,
            _ => SqlStatementKind::Unknown,
        }
    }

    pub fn parse_batch(sql_batch: &str) -> Vec<SqlStatement> {
        sql_batch
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| SqlStatement {
                raw: s.to_string(),
                kind: Self::classify_statement(s),
            })
            .collect()
    }

    pub fn analyze_statement(sql: &str) -> AnalysisResult {
        let kind = Self::classify_statement(sql);
        AnalysisResult {
            requires_transaction: matches!(
                kind,
                SqlStatementKind::Insert
                    | SqlStatementKind::Update
                    | SqlStatementKind::Delete
                    | SqlStatementKind::CreateTable
                    | SqlStatementKind::CreateView
                    | SqlStatementKind::CreateMaterializedView
                    | SqlStatementKind::CreateFunction
                    | SqlStatementKind::AlterTable
                    | SqlStatementKind::DropTable
            ),
            touches_catalog: matches!(
                kind,
                SqlStatementKind::CreateTable
                    | SqlStatementKind::CreateView
                    | SqlStatementKind::CreateMaterializedView
                    | SqlStatementKind::CreateFunction
                    | SqlStatementKind::AlterTable
                    | SqlStatementKind::DropTable
            ),
            kind,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportedLocale {
    EnUs,
    FrFr,
    EsEs,
}

impl SupportedLocale {
    pub fn parse(value: &str) -> Self {
        let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
        match normalized.as_str() {
            "fr-fr" => Self::FrFr,
            "es-es" => Self::EsEs,
            _ => Self::EnUs,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::EnUs => "en-US",
            Self::FrFr => "fr-FR",
            Self::EsEs => "es-ES",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalizedMessage {
    pub locale: SupportedLocale,
    pub key: &'static str,
    pub message: &'static str,
}

#[derive(Debug, Default)]
pub struct I18nCatalog;

impl I18nCatalog {
    pub fn message(locale: SupportedLocale, key: &'static str) -> LocalizedMessage {
        let message = match (locale, key) {
            (SupportedLocale::FrFr, "unauthorized") => "Demande non autorisee",
            (SupportedLocale::EsEs, "unauthorized") => "Solicitud no autorizada",
            (SupportedLocale::FrFr, "missing_or_invalid_admin_key") => {
                "Cle administrateur absente ou invalide"
            }
            (SupportedLocale::EsEs, "missing_or_invalid_admin_key") => {
                "Clave de administrador ausente o invalida"
            }
            (SupportedLocale::FrFr, "health_ok") => "Sante OK",
            (SupportedLocale::EsEs, "health_ok") => "Salud OK",
            _ => match key {
                "unauthorized" => "Unauthorized request",
                "missing_or_invalid_admin_key" => "Missing or invalid admin key",
                "health_ok" => "Health OK",
                _ => "Message key not found",
            },
        };
        LocalizedMessage {
            locale,
            key,
            message,
        }
    }
}

fn normalize_ident(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_sql_for_match(sql: &str) -> String {
    let stripped = sql.trim_start_matches('\u{feff}').trim();
    let mut result = String::with_capacity(stripped.len());
    for line in stripped.lines() {
        let maybe_comment_start = line.find("--");
        let content = match maybe_comment_start {
            Some(pos) => &line[..pos],
            None => line,
        };
        if !content.trim().is_empty() {
            result.push_str(content);
            result.push(' ');
        }
    }
    result.trim().to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_aggregations::{
        is_legacy_aggregation_supported, is_p2_stub_supported, run_p2_stub,
        P2_STUB_AGGREGATIONS, SUPPORTED_LEGACY_AGGREGATIONS,
    };

    #[test]
    fn classifies_core_statements() {
        assert_eq!(
            SqlAnalyzer::classify_statement("select * from t"),
            SqlStatementKind::Select
        );
        assert_eq!(
            SqlAnalyzer::classify_statement(" CREATE TABLE t(id int)"),
            SqlStatementKind::CreateTable
        );
        assert_eq!(
            SqlAnalyzer::classify_statement("-- cmt\ncreate function f()"),
            SqlStatementKind::CreateFunction
        );
    }

    #[test]
    fn parses_batch_in_order() {
        let parsed = SqlAnalyzer::parse_batch(
            "BEGIN; INSERT INTO t VALUES (1); UPDATE t SET v=2; COMMIT;  ;",
        );
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].kind, SqlStatementKind::Begin);
        assert_eq!(parsed[1].kind, SqlStatementKind::Insert);
        assert_eq!(parsed[3].kind, SqlStatementKind::Commit);
    }

    #[test]
    fn analyzes_catalog_touch_and_transaction() {
        let ddl = SqlAnalyzer::analyze_statement("create materialized view mv as select 1");
        assert!(ddl.requires_transaction);
        assert!(ddl.touches_catalog);

        let query = SqlAnalyzer::analyze_statement("select 1");
        assert!(!query.touches_catalog);
        assert!(!query.requires_transaction);
    }

    #[test]
    fn function_registry_registers_and_reads() {
        let mut registry = FunctionRegistry::new();
        let created = registry.register(RegisteredFunction {
            name: "sum_fast".to_string(),
            language: FunctionLanguage::Builtin,
            deterministic: true,
            description: "Optimized aggregate".to_string(),
        });
        assert!(created);
        assert!(registry.contains("SUM_FAST"));
        let f = registry.get("sum_fast").expect("function should exist");
        assert_eq!(f.language, FunctionLanguage::Builtin);
    }

    #[test]
    fn function_registry_supports_polyglot_udf_contract() {
        let mut registry = FunctionRegistry::new();
        assert!(registry.register(RegisteredFunction {
            name: "risk_score_rs".to_string(),
            language: FunctionLanguage::Rust,
            deterministic: true,
            description: "Rust UDF".to_string(),
        }));
        assert!(registry.register(RegisteredFunction {
            name: "risk_score_js".to_string(),
            language: FunctionLanguage::JavaScript,
            deterministic: false,
            description: "JavaScript UDF".to_string(),
        }));
        assert!(registry.register(RegisteredFunction {
            name: "risk_score_py".to_string(),
            language: FunctionLanguage::Python,
            deterministic: false,
            description: "Python UDF".to_string(),
        }));

        assert_eq!(
            registry.get("risk_score_rs").map(|f| f.language),
            Some(FunctionLanguage::Rust)
        );
        assert_eq!(
            registry.get("risk_score_js").map(|f| f.language),
            Some(FunctionLanguage::JavaScript)
        );
        assert_eq!(
            registry.get("risk_score_py").map(|f| f.language),
            Some(FunctionLanguage::Python)
        );
        assert_eq!(registry.list().len(), 3);
    }

    #[test]
    fn legacy_aggregation_parity_manifest_alignment() {
        let required = include_str!("../../../tests/parity/legacy/required-aggregations.txt");
        let mut missing = Vec::new();
        for line in required.lines() {
            let value = line.trim();
            if value.is_empty() || value.starts_with('#') {
                continue;
            }
            if !is_legacy_aggregation_supported(value) {
                missing.push(value.to_string());
            }
        }
        assert!(
            missing.is_empty(),
            "missing legacy aggregation support for: {:?}; supported={:?}",
            missing,
            SUPPORTED_LEGACY_AGGREGATIONS
        );
    }

    #[test]
    fn p2_stub_hooks_cover_expected_aggregations() {
        for agg in P2_STUB_AGGREGATIONS {
            assert!(is_p2_stub_supported(agg));
            let result = run_p2_stub(agg);
            assert!(result.accepted, "stub should accept {agg}");
            assert_eq!(result.mode, "stub");
        }

        let unknown = run_p2_stub("UNKNOWN_P2");
        assert!(!unknown.accepted);
        assert_eq!(unknown.mode, "unsupported");
    }

    #[test]
    fn ws11_supports_locale_parsing_and_messages() {
        let locale = SupportedLocale::parse("fr_FR");
        assert_eq!(locale, SupportedLocale::FrFr);
        let msg = I18nCatalog::message(locale, "unauthorized");
        assert_eq!(msg.message, "Demande non autorisee");
    }

    #[test]
    fn ws11_locale_fallback_defaults_to_en_us() {
        let locale = SupportedLocale::parse("de-DE");
        assert_eq!(locale, SupportedLocale::EnUs);
    }

    #[test]
    fn ws11_unknown_message_key_uses_safe_fallback() {
        let msg = I18nCatalog::message(SupportedLocale::FrFr, "missing_key_for_test");
        assert_eq!(msg.message, "Message key not found");
    }
}
