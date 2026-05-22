//! L-2: Stored-procedure registry.
//!
//! # Overview
//!
//! Provides a lightweight, in-process stored-procedure catalog that maps
//! procedure names to parameterised SQL bodies.  Procedures are registered
//! either at boot (built-ins) or at runtime via the `CREATE PROCEDURE` DDL
//! statement handled by [`ProcedureRegistry::register_from_ddl`].
//!
//! # Call resolution
//!
//! When the SQL executor encounters a `CALL name(arg1, arg2, …)` statement it
//! delegates to [`ProcedureRegistry::resolve_call`] which:
//! 1. Looks up the procedure by name (case-insensitive).
//! 2. Checks the supplied argument count against the registered arity.
//! 3. Substitutes positional placeholders (`$1`, `$2`, …) in the body.
//! 4. Returns the expanded SQL so it can be executed through the normal path.
//!
//! # DDL syntax
//!
//! ```sql
//! CREATE PROCEDURE upsert_order(order_id, status)
//! AS $$
//!   INSERT INTO orders VALUES ($1, $2);
//!   UPDATE audit_log SET last_updated = NOW() WHERE ref_id = $1;
//! $$;
//! ```
//!
//! Dollar-quoted bodies may span multiple lines. Everything between the outer
//! `$$` markers is stored verbatim and used as the template.
//!
//! # Built-in procedures
//!
//! The registry is pre-populated at boot via [`ProcedureRegistry::register_builtins`].
//! Built-ins are read-only markers — they cannot be dropped via DDL.

#![forbid(unsafe_code)]

use std::collections::HashMap;

// ── Data types ────────────────────────────────────────────────────────────────

/// A single registered stored procedure.
#[derive(Debug, Clone)]
pub struct StoredProcedure {
    /// Lower-case canonical procedure name.
    pub name: String,
    /// Named parameter list (used for documentation; substitution uses `$N`
    /// positional syntax in the body).
    pub params: Vec<String>,
    /// SQL body template.  Positional placeholders `$1`, `$2`, … are replaced
    /// by the caller-supplied arguments at call time.
    pub body: String,
    /// `true` for built-in procedures that cannot be dropped via DDL.
    pub builtin: bool,
}

impl StoredProcedure {
    /// Expand the body template by substituting positional arguments.
    ///
    /// `$1` → `args[0]`, `$2` → `args[1]`, …
    fn expand(&self, args: &[String]) -> String {
        let mut out = self.body.clone();
        for (i, arg) in args.iter().enumerate() {
            let placeholder = format!("${}", i + 1);
            out = out.replace(&placeholder, arg);
        }
        out
    }
}

// ── Registry ──────────────────────────────────────────────────────────────────

/// In-process stored-procedure catalog.
///
/// Shared via `Arc<Mutex<ProcedureRegistry>>` on [`crate::AppState`].
#[derive(Debug, Default)]
pub struct ProcedureRegistry {
    procedures: HashMap<String, StoredProcedure>,
}

impl ProcedureRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Populate built-in procedures.
    ///
    /// Built-ins are *markers* only — the actual logic for complex built-ins
    /// (e.g. `insert_rows`) is still handled by the dedicated shim in
    /// `try_handle_call_insert_rows_demo`.  Registering them here makes them
    /// visible in `SHOW PROCEDURES` queries and prevents user DDL from shadowing
    /// them.
    pub fn register_builtins(&mut self) {
        // insert_rows(table_name, num_records)
        // Actual execution is delegated to the shim in main.rs.
        self.procedures.insert(
            "insert_rows".to_string(),
            StoredProcedure {
                name: "insert_rows".to_string(),
                params: vec!["table_name".to_string(), "num_records".to_string()],
                body: "-- built-in: handled by insert_rows shim".to_string(),
                builtin: true,
            },
        );
    }

    /// Register (or replace) a user-defined stored procedure.
    ///
    /// Returns `Err` if `proc.builtin == true` was attempted to be replaced
    /// from user DDL.
    pub fn register(&mut self, proc: StoredProcedure) -> Result<(), String> {
        if let Some(existing) = self.procedures.get(&proc.name) {
            if existing.builtin {
                return Err(format!(
                    "cannot redefine built-in procedure '{}'",
                    proc.name
                ));
            }
        }
        self.procedures.insert(proc.name.clone(), proc);
        Ok(())
    }

    /// Drop a user-defined procedure by name.
    ///
    /// Returns `Err` if the procedure does not exist or is a built-in.
    pub fn drop_procedure(&mut self, name: &str) -> Result<(), String> {
        let key = name.to_ascii_lowercase();
        match self.procedures.get(&key) {
            None => Err(format!("procedure '{}' does not exist", name)),
            Some(p) if p.builtin => Err(format!(
                "cannot drop built-in procedure '{}'",
                name
            )),
            _ => {
                self.procedures.remove(&key);
                Ok(())
            }
        }
    }

    /// List all registered procedures (name + param count + builtin flag).
    /// Used by the `SHOW PROCEDURES` introspection path and unit tests.
    #[allow(dead_code)]
    pub fn list(&self) -> Vec<(&str, usize, bool)> {
        let mut out: Vec<_> = self
            .procedures
            .values()
            .map(|p| (p.name.as_str(), p.params.len(), p.builtin))
            .collect();
        out.sort_by_key(|(name, _, _)| *name);
        out
    }

    /// Resolve a `CALL name(arg1, arg2, …)` statement.
    ///
    /// Returns:
    /// - `Ok(Some(sql))` — procedure found and body expanded.
    /// - `Ok(None)` — statement is not a CALL (caller should handle normally).
    /// - `Err(msg)` — CALL but unknown procedure or wrong arity.
    pub fn resolve_call(&self, sql: &str) -> Result<Option<String>, String> {
        let trimmed = sql.trim();
        let upper = trimmed.to_ascii_uppercase();
        if !upper.starts_with("CALL ") {
            return Ok(None);
        }
        let after_call = trimmed[5..].trim();
        let (name, args) = parse_call_args(after_call)
            .ok_or_else(|| format!("malformed CALL statement: '{}'", sql))?;

        let key = name.to_ascii_lowercase();
        let proc = self
            .procedures
            .get(&key)
            .ok_or_else(|| format!("unknown procedure '{name}'"))?;

        if proc.builtin {
            // Signal to the caller that this is a built-in — return a sentinel
            // so the existing shim can handle it.
            return Ok(None);
        }

        if args.len() != proc.params.len() {
            return Err(format!(
                "procedure '{}' expects {} argument(s), got {}",
                proc.name,
                proc.params.len(),
                args.len()
            ));
        }

        Ok(Some(proc.expand(&args)))
    }

    /// Parse and register a procedure from `CREATE PROCEDURE … AS $$ … $$` DDL.
    ///
    /// Returns `Ok(proc_name)` on success or `Err(message)` on parse failure.
    pub fn register_from_ddl(&mut self, sql: &str) -> Result<String, String> {
        let trimmed = sql.trim();
        let upper = trimmed.to_ascii_uppercase();

        // Syntax: CREATE [OR REPLACE] PROCEDURE name(p1, p2, …) AS $$ body $$
        let after_create = if upper.starts_with("CREATE OR REPLACE PROCEDURE ") {
            &trimmed[28..]
        } else if upper.starts_with("CREATE PROCEDURE ") {
            &trimmed[17..]
        } else {
            return Err("not a CREATE PROCEDURE statement".to_string());
        };

        // Extract name and parameter list.
        let open_paren = after_create
            .find('(')
            .ok_or("missing '(' in procedure signature")?;
        let name = after_create[..open_paren].trim().to_ascii_lowercase();
        if name.is_empty() {
            return Err("procedure name must not be empty".into());
        }

        let close_paren = after_create
            .find(')')
            .ok_or("missing ')' in procedure signature")?;
        let params_str = after_create[open_paren + 1..close_paren].trim();
        let params: Vec<String> = if params_str.is_empty() {
            vec![]
        } else {
            params_str
                .split(',')
                .map(|p| p.trim().to_ascii_lowercase())
                .filter(|p| !p.is_empty())
                .collect()
        };

        // Extract body between $$ markers.
        let rest = after_create[close_paren + 1..].trim();
        let as_upper = rest.to_ascii_uppercase();
        let rest_after_as = if as_upper.starts_with("AS") {
            rest[2..].trim()
        } else {
            return Err("expected 'AS' after procedure signature".into());
        };

        let body = extract_dollar_quoted_body(rest_after_as)
            .ok_or("expected dollar-quoted body $$ … $$ after AS")?;

        let proc = StoredProcedure {
            name: name.clone(),
            params,
            body,
            builtin: false,
        };
        self.register(proc)?;
        Ok(name)
    }

    /// Return true if the SQL starts with CREATE [OR REPLACE] PROCEDURE.
    pub fn is_create_procedure(sql: &str) -> bool {
        let u = sql.trim().to_ascii_uppercase();
        u.starts_with("CREATE OR REPLACE PROCEDURE ") || u.starts_with("CREATE PROCEDURE ")
    }

    /// Return true if the SQL starts with DROP PROCEDURE.
    pub fn is_drop_procedure(sql: &str) -> bool {
        sql.trim().to_ascii_uppercase().starts_with("DROP PROCEDURE ")
    }

    /// Return true if the SQL is a CALL statement.
    pub fn is_call(sql: &str) -> bool {
        sql.trim().to_ascii_uppercase().starts_with("CALL ")
    }
}

// ── Parsing helpers ───────────────────────────────────────────────────────────

/// Parse `name(arg1, arg2, …)` into `(name, args)`.
///
/// Arguments are trimmed and unquoted (single- or double-quoted strings have
/// their outer quotes removed).
fn parse_call_args(s: &str) -> Option<(String, Vec<String>)> {
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    if close <= open {
        return None;
    }
    let name = s[..open].trim().to_string();
    let args_str = s[open + 1..close].trim();
    let args = if args_str.is_empty() {
        vec![]
    } else {
        split_args(args_str)
    };
    Some((name, args))
}

/// Split a comma-separated argument list respecting quoted strings.
fn split_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut buf = String::new();
    let mut in_single = false;
    let mut in_double = false;
    for ch in s.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                buf.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                buf.push(ch);
            }
            ',' if !in_single && !in_double => {
                args.push(trim_arg(&buf));
                buf.clear();
            }
            _ => buf.push(ch),
        }
    }
    if !buf.trim().is_empty() {
        args.push(trim_arg(&buf));
    }
    args
}

/// Trim surrounding whitespace from an argument, preserving quotes.
///
/// SQL string literals (`'value'`), identifiers (`"name"`), and numeric
/// literals are all passed through verbatim after whitespace trimming so that
/// the substituted body remains valid SQL.  Callers that need the raw unquoted
/// value (e.g. for table-name lookup) should strip quotes themselves.
fn trim_arg(s: &str) -> String {
    s.trim().to_string()
}

/// Extract the content of `$$ … $$` dollar-quoting.
fn extract_dollar_quoted_body(s: &str) -> Option<String> {
    let s = s.trim();
    if !s.starts_with("$$") {
        return None;
    }
    let inner = &s[2..];
    let end = inner.find("$$")?;
    Some(inner[..end].trim().to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn reg_with_builtins() -> ProcedureRegistry {
        let mut r = ProcedureRegistry::new();
        r.register_builtins();
        r
    }

    #[test]
    fn test_register_and_call_udf() {
        let mut reg = ProcedureRegistry::new();
        // Body uses $1 as a bare placeholder; the arg is the SQL literal 'alice'
        // (with quotes) so the expanded SQL is: SELECT $1 AS greeting
        // → SELECT 'alice' AS greeting
        reg.register(StoredProcedure {
            name: "greet".to_string(),
            params: vec!["name".to_string()],
            body: "SELECT $1 AS greeting".to_string(),
            builtin: false,
        }).unwrap();

        let expanded = reg.resolve_call("CALL greet('alice')").unwrap().unwrap();
        // The SQL literal 'alice' (with quotes) is preserved verbatim.
        assert!(expanded.contains("'alice'"), "arg should be substituted: {expanded}");
    }

    #[test]
    fn test_non_call_returns_none() {
        let reg = ProcedureRegistry::new();
        assert!(reg.resolve_call("SELECT 1").unwrap().is_none());
    }

    #[test]
    fn test_unknown_proc_returns_err() {
        let reg = ProcedureRegistry::new();
        assert!(reg.resolve_call("CALL no_such_proc()").is_err());
    }

    #[test]
    fn test_arity_mismatch_returns_err() {
        let mut reg = ProcedureRegistry::new();
        reg.register(StoredProcedure {
            name: "two_args".to_string(),
            params: vec!["a".to_string(), "b".to_string()],
            body: "SELECT $1, $2".to_string(),
            builtin: false,
        }).unwrap();
        assert!(reg.resolve_call("CALL two_args('only_one')").is_err());
    }

    #[test]
    fn test_create_procedure_ddl() {
        let mut reg = ProcedureRegistry::new();
        let ddl = "CREATE PROCEDURE log_event(event_type, payload) AS $$
            INSERT INTO event_log VALUES ($1, $2);
        $$";
        let name = reg.register_from_ddl(ddl).unwrap();
        assert_eq!(name, "log_event");

        let expanded = reg.resolve_call("CALL log_event('click', 'btn-submit')").unwrap().unwrap();
        assert!(expanded.contains("'click'"), "first arg substituted: {expanded}");
        assert!(expanded.contains("'btn-submit'"), "second arg substituted: {expanded}");
    }

    #[test]
    fn test_cannot_redefine_builtin() {
        let mut reg = reg_with_builtins();
        let result = reg.register(StoredProcedure {
            name: "insert_rows".to_string(),
            params: vec![],
            body: "SELECT 1".to_string(),
            builtin: false,
        });
        assert!(result.is_err(), "should not allow redefining a built-in");
    }

    #[test]
    fn test_drop_user_proc() {
        let mut reg = ProcedureRegistry::new();
        reg.register(StoredProcedure {
            name: "tmp".to_string(),
            params: vec![],
            body: "SELECT 1".to_string(),
            builtin: false,
        }).unwrap();
        assert!(reg.drop_procedure("tmp").is_ok());
        assert!(reg.resolve_call("CALL tmp()").is_err());
    }

    #[test]
    fn test_drop_builtin_rejected() {
        let mut reg = reg_with_builtins();
        assert!(reg.drop_procedure("insert_rows").is_err());
    }

    #[test]
    fn test_builtin_call_passes_through_to_shim() {
        // Built-in CALL should return Ok(None) so the shim handles it.
        let reg = reg_with_builtins();
        let result = reg.resolve_call("CALL insert_rows('my_table', 10)").unwrap();
        assert!(result.is_none(), "built-in must pass through to shim (Ok(None))");
    }

    #[test]
    fn test_is_create_procedure() {
        assert!(ProcedureRegistry::is_create_procedure("CREATE PROCEDURE foo() AS $$ SELECT 1 $$"));
        assert!(ProcedureRegistry::is_create_procedure("CREATE OR REPLACE PROCEDURE foo() AS $$ SELECT 1 $$"));
        assert!(!ProcedureRegistry::is_create_procedure("SELECT 1"));
    }

    #[test]
    fn test_list() {
        let mut reg = ProcedureRegistry::new();
        reg.register_builtins();
        let lst = reg.list();
        assert!(lst.iter().any(|(n, _, _)| *n == "insert_rows"));
    }
}
