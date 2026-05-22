//! UDF runtime scaffold and catalog contract.
use voltnuerongrid_sql::{SqlAnalyzer, SqlStatementKind};
use crate::{
    UdfExecutionResult, UdfExecutionPlanStep, UdfFunctionCatalogEntry,
    UdfInvocationPlan, UdfLanguageGuardPolicy,
};


pub(crate) fn execute_udf_runtime_scaffold(sql_batch: &str) -> Result<Vec<UdfExecutionResult>, String> {
    enforce_udf_guardrails(sql_batch)?;
    let mut results = Vec::new();
    for statement in SqlAnalyzer::parse_batch(sql_batch) {
        let normalized = statement.raw.to_ascii_lowercase();
        if normalized.contains("udf_rust(") {
            let input = extract_udf_input(&statement.raw).unwrap_or_else(|| "sample".to_string());
            results.push(UdfExecutionResult {
                language: "rust",
                function: "udf_rust",
                output: input.to_ascii_uppercase(),
                input,
            });
        }
        if normalized.contains("udf_js(") {
            let input = extract_udf_input(&statement.raw).unwrap_or_else(|| "sample".to_string());
            let output: String = input.chars().rev().collect();
            results.push(UdfExecutionResult {
                language: "javascript",
                function: "udf_js",
                output,
                input,
            });
        }
        if normalized.contains("udf_python(") {
            let input = extract_udf_input(&statement.raw).unwrap_or_else(|| "sample".to_string());
            results.push(UdfExecutionResult {
                language: "python",
                function: "udf_python",
                output: input.len().to_string(),
                input,
            });
        }
    }
    Ok(results)
}


pub(crate) fn udf_function_catalog_contract() -> Vec<UdfFunctionCatalogEntry> {
    vec![
        UdfFunctionCatalogEntry {
            name: "udf_rust",
            language: "rust",
            deterministic: true,
            status: "enabled",
        },
        UdfFunctionCatalogEntry {
            name: "udf_js",
            language: "javascript",
            deterministic: false,
            status: "enabled",
        },
        UdfFunctionCatalogEntry {
            name: "udf_python",
            language: "python",
            deterministic: false,
            status: "enabled",
        },
    ]
}


pub(crate) fn udf_guard_policy_contract() -> Vec<UdfLanguageGuardPolicy> {
    vec![
        UdfLanguageGuardPolicy {
            language: "rust",
            blocked_tokens: vec!["unsafe", "std::process", "process::"],
            max_input_bytes: 256,
        },
        UdfLanguageGuardPolicy {
            language: "javascript",
            blocked_tokens: vec!["eval(", "function(", "child_process"],
            max_input_bytes: 256,
        },
        UdfLanguageGuardPolicy {
            language: "python",
            blocked_tokens: vec!["import os", "subprocess", "exec("],
            max_input_bytes: 256,
        },
    ]
}


pub(crate) fn build_udf_execution_plan(sql_batch: &str) -> Vec<UdfExecutionPlanStep> {
    let mut plan = Vec::new();
    for statement in SqlAnalyzer::parse_batch(sql_batch) {
        let mut invocations = Vec::new();
        let normalized = statement.raw.to_ascii_lowercase();
        if normalized.contains("udf_rust(") {
            invocations.push(UdfInvocationPlan {
                function: "udf_rust",
                language: "rust",
                guard_policy: "rust_default",
            });
        }
        if normalized.contains("udf_js(") {
            invocations.push(UdfInvocationPlan {
                function: "udf_js",
                language: "javascript",
                guard_policy: "javascript_default",
            });
        }
        if normalized.contains("udf_python(") {
            invocations.push(UdfInvocationPlan {
                function: "udf_python",
                language: "python",
                guard_policy: "python_default",
            });
        }
        let analysis = SqlAnalyzer::analyze_statement(&statement.raw);
        let route_path = if analysis.kind == SqlStatementKind::Select {
            "olap"
        } else {
            "oltp"
        };
        plan.push(UdfExecutionPlanStep {
            statement: statement.raw,
            route_path: route_path.to_string(),
            udf_invocations: invocations,
        });
    }
    plan
}


pub(crate) fn enforce_udf_guardrails(sql_batch: &str) -> Result<(), String> {
    let lowered = sql_batch.to_ascii_lowercase();
    let has_rust_udf = lowered.contains("udf_rust(");
    let has_js_udf = lowered.contains("udf_js(");
    let has_python_udf = lowered.contains("udf_python(");

    if has_rust_udf && ["unsafe", "std::process", "process::"].iter().any(|t| lowered.contains(t)) {
        return Err("udf_guardrail_blocked_rust_payload".to_string());
    }
    if has_js_udf && ["eval(", "function(", "child_process"].iter().any(|t| lowered.contains(t)) {
        return Err("udf_guardrail_blocked_javascript_payload".to_string());
    }
    if has_python_udf && ["import os", "subprocess", "exec("].iter().any(|t| lowered.contains(t)) {
        return Err("udf_guardrail_blocked_python_payload".to_string());
    }
    Ok(())
}


pub(crate) fn extract_udf_input(statement: &str) -> Option<String> {
    let first = statement.find('\'')?;
    let remaining = &statement[first + 1..];
    let end = remaining.find('\'')?;
    Some(remaining[..end].to_string())
}

// ── ISSUE-05: Catalog UDF execution ──────────────────────────────────────────

/// Describes a user-defined function resolved from the DDL catalog.
#[derive(Debug, Clone)]
pub(crate) struct CatalogUdfEntry {
    /// Unqualified function name (lower-cased).
    pub(crate) name: String,
    /// The SQL body extracted from the `CREATE FUNCTION` DDL, if any.
    pub(crate) sql_body: Option<String>,
    /// The full original DDL statement.
    pub(crate) ddl: String,
}

/// Extract the SQL function body from a `CREATE FUNCTION … AS $$ … $$` DDL statement.
///
/// Returns `None` if no dollar-quoted body is found.
pub(crate) fn extract_sql_function_body(ddl: &str) -> Option<String> {
    // Dollar-quoted body delimiters: $$ … $$ or $BODY$ … $BODY$, etc.
    // We look for the simplest common form: `AS $$ body $$`
    let lower = ddl.to_ascii_lowercase();
    if let Some(start) = lower.find("$$") {
        let body_start = start + 2;
        if let Some(end_off) = lower[body_start..].find("$$") {
            let body = ddl[body_start..body_start + end_off].trim().to_string();
            if !body.is_empty() {
                return Some(body);
            }
        }
    }
    None
}

/// Look up all catalog-registered functions whose names appear in `sql_batch`.
///
/// Returns pairs of `(function_name, sql_body_if_available)` for each match found.
/// The caller can use `sql_body` to inline or evaluate the function.
///
/// Accepts a snapshot of the DDL catalog entries (call site should lock + clone).
pub(crate) fn resolve_catalog_udfs(
    sql_batch: &str,
    catalog_functions: &[CatalogUdfEntry],
) -> Vec<(String, Option<String>)> {
    let lower = sql_batch.to_ascii_lowercase();
    catalog_functions
        .iter()
        .filter(|f| {
            // Check for `name(` pattern — the function must be called with parentheses.
            let call_pat = format!("{}(", f.name);
            lower.contains(&call_pat)
        })
        .map(|f| (f.name.clone(), f.sql_body.clone()))
        .collect()
}

/// Evaluate a user-defined SQL function call within `sql` by inlining its body.
///
/// For simple scalar SQL functions whose body is a `SELECT expr` expression, this
/// replaces the `function_name(arg)` call with a subquery `(SELECT expr)` so the
/// surrounding query planner can evaluate it.
///
/// Returns `None` if the body cannot be safely inlined (complex body, no body, etc.).
///
/// This is a best-effort text-level rewrite; a proper AST-level expansion would live
/// in the query planner (future work). For now it handles the common case of
/// single-expression SQL UDFs.
pub(crate) fn try_inline_catalog_udf(
    sql: &str,
    fn_name: &str,
    sql_body: &str,
) -> Option<String> {
    // Only inline bodies that are a simple SELECT expression (single-statement bodies).
    let body_lower = sql_body.trim().to_ascii_lowercase();
    if !body_lower.starts_with("select ") && !body_lower.starts_with("return ") {
        return None; // multi-statement or procedural body — skip
    }
    // Build the subquery replacement: `(SELECT body)`
    let subquery = if body_lower.starts_with("select ") {
        format!("({})", sql_body.trim())
    } else {
        // `RETURN expr` → `(SELECT expr)`
        let expr = sql_body.trim().trim_start_matches("RETURN").trim_start_matches("return").trim();
        format!("(SELECT {})", expr)
    };

    // Replace the first call to `fn_name(...)` in sql with the subquery.
    // We look for `fn_name(` and find the matching `)`.
    let lower = sql.to_ascii_lowercase();
    let call_pat = format!("{}(", fn_name.to_ascii_lowercase());
    let pos = lower.find(&call_pat)?;
    // Find the matching closing paren by tracking depth.
    let after_open = pos + call_pat.len();
    let mut depth = 1usize;
    let mut close_pos = None;
    for (i, ch) in sql[after_open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close_pos = Some(after_open + i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = close_pos?;
    Some(format!("{}{}{}", &sql[..pos], subquery, &sql[end..]))
}

