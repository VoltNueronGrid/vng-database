#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-sql";

use std::collections::HashMap;

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
            (Some("ROLLBACK"), _, _) => SqlStatementKind::Rollback,
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
}
