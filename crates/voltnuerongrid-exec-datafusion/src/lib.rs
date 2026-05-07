//! Phase 1.7 — correct OLTP SELECT executor.
//!
//! Replaces the legacy [`row_key.contains(prefix)`] scan in
//! `services/voltnuerongridd/src/main.rs::execute_oltp_select` with an AST-driven
//! evaluator. Critical correctness fix: `WHERE id = 5` now returns exactly the
//! row with id `5`, not rows 15, 25, 50, 51 etc.
//!
//! # Coverage today
//!
//! - Equality / inequality: `=`, `!=` / `<>`
//! - Range: `<`, `<=`, `>`, `>=`, `BETWEEN ... AND ...`
//! - Set membership: `IN (...)` (literal list only)
//! - Null tests: `IS NULL`, `IS NOT NULL`
//! - Boolean composition: `AND`, `OR`, `NOT`
//! - Pattern matching: `LIKE`, `NOT LIKE` (with `%` and `_` wildcards)
//! - Column projection (only the listed columns are returned)
//! - `ORDER BY` (any single column, ASC / DESC)
//! - `LIMIT` / `OFFSET`
//! - Bare aggregates without `GROUP BY`: `COUNT(*)`, `COUNT(col)`,
//!   `SUM(col)`, `AVG(col)`, `MIN(col)`, `MAX(col)`
//!
//! # Deferred
//!
//! - JOINs: complex; covered by widening the legacy executor or by adopting
//!   DataFusion once the workspace MSRV permits.
//! - `GROUP BY`, `HAVING`, window functions, subqueries: same.
//!
//! # Naming
//!
//! The crate is named `voltnuerongrid-exec-datafusion` even though it does not
//! depend on DataFusion *yet*. When the workspace MSRV reaches Rust 2024
//! (DataFusion's transitive deps require it), DataFusion will be added here
//! alongside the existing path; both can coexist behind a feature flag.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use sqlparser::ast as sa;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

use voltnuerongrid_sql::{parse_one, SelectStatement, Statement};
use voltnuerongrid_store::mvcc::{PagedRowStore, RowData};

pub const CRATE_NAME: &str = "voltnuerongrid-exec-datafusion";

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// A single result row. Keeps the original key for the existing wire format
/// and carries either the full row or a projected subset.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultRow {
    pub key: String,
    pub data: RowData,
}

/// One column value of a single-row aggregate result.
#[derive(Debug, Clone, PartialEq)]
pub enum AggregateCell {
    Int(i64),
    Float(f64),
    Text(String),
    Null,
}

/// What an aggregate query (no GROUP BY) returns: one row, named by the agg expr.
#[derive(Debug, Clone, PartialEq)]
pub struct AggregateResult {
    pub columns: Vec<String>,
    pub values: Vec<AggregateCell>,
}

/// Either ordinary rows or a one-row aggregate result.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectOutput {
    Rows(Vec<ResultRow>),
    Aggregate(AggregateResult),
}

#[derive(Debug, Clone)]
pub enum ExecError {
    /// The statement is not yet supported by this executor — caller may
    /// fall back to a legacy path.
    Unsupported(String),
    /// The statement is a SELECT but parsing the WHERE clause failed.
    BadPredicate(String),
    /// The statement isn't a SELECT.
    NotASelect,
    /// Column referenced in projection / WHERE / ORDER BY does not exist on the row.
    /// Surfaced as `null` per SQL semantics; only returned when strict mode is requested.
    UnknownColumn(String),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(s) => write!(f, "unsupported SELECT feature: {s}"),
            Self::BadPredicate(s) => write!(f, "bad predicate: {s}"),
            Self::NotASelect => f.write_str("statement is not a SELECT"),
            Self::UnknownColumn(c) => write!(f, "unknown column: {c}"),
        }
    }
}

impl std::error::Error for ExecError {}

// ─────────────────────────────────────────────────────────────────────────────
// Public entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Execute a SELECT statement against the given row store, returning correct
/// row-level results. The caller has typically already snapshotted the store;
/// pass `snapshot_xid = rs.current_xid()` if you don't need a specific snapshot.
pub fn execute_select(
    sql: &str,
    rs: &PagedRowStore,
    max_rows: usize,
) -> Result<SelectOutput, ExecError> {
    let parsed = parse_one(sql).map_err(|e| ExecError::BadPredicate(e))?;
    let select = match parsed {
        Statement::Select(s) => s,
        _ => return Err(ExecError::NotASelect),
    };
    execute_parsed_select(&select, sql, rs, max_rows)
}

/// Execute an already-parsed SelectStatement. Useful when the caller has
/// already produced the AST (so we don't re-parse).
pub fn execute_parsed_select(
    sel: &SelectStatement,
    raw_sql: &str,
    rs: &PagedRowStore,
    max_rows: usize,
) -> Result<SelectOutput, ExecError> {
    // Reject features we don't yet support — caller should fall back.
    if sel.has_group_by || sel.has_having {
        return Err(ExecError::Unsupported("GROUP BY / HAVING".into()));
    }
    if sel.join.is_some() {
        return Err(ExecError::Unsupported("JOIN".into()));
    }
    if sel.has_subquery {
        return Err(ExecError::Unsupported("subquery".into()));
    }

    // Re-parse the WHERE clause structurally to evaluate it correctly.
    // The SelectStatement carries WHERE as a string for backwards compat;
    // here we lift it to an Expr.
    let where_expr = if let Some(w) = &sel.where_clause {
        Some(parse_predicate(w)?)
    } else {
        None
    };

    // Snapshot once.
    let snapshot_xid = rs.current_xid();
    let table_prefix = sel.table.as_deref();

    // Apply table-name filter on the row key (keys are typically "<table>:<id>").
    let key_filter: Box<dyn Fn(&str) -> bool> = if let Some(t) = table_prefix {
        let tp = format!("{t}:");
        Box::new(move |k: &str| k == t || k.starts_with(&tp))
    } else {
        Box::new(|_: &str| true)
    };

    // Walk all visible rows, applying the predicate.
    let mut matched: Vec<(String, RowData)> = Vec::new();
    for (k, d) in rs.scan_at_snapshot(snapshot_xid) {
        if !key_filter(k) {
            continue;
        }
        let mut env = RowEnv {
            row_data: d,
            row_key: k,
        };
        let pass = match &where_expr {
            Some(e) => match eval_predicate(e, &mut env) {
                Ok(b) => b,
                Err(_) => false,
            },
            None => true,
        };
        if pass {
            matched.push((k.to_string(), d.clone()));
        }
    }

    // Aggregate fast-path: SELECT COUNT(*) / SUM(col) / ... FROM ... [WHERE ...]
    // (no GROUP BY since we rejected that above).
    if let Some(agg) = try_extract_aggregates(&sel.columns, raw_sql) {
        let mut values: Vec<AggregateCell> = Vec::new();
        let mut col_names: Vec<String> = Vec::new();
        for agg_call in &agg {
            col_names.push(agg_call.alias.clone().unwrap_or_else(|| agg_call.display.clone()));
            values.push(eval_aggregate(agg_call, &matched));
        }
        return Ok(SelectOutput::Aggregate(AggregateResult {
            columns: col_names,
            values,
        }));
    }

    // ORDER BY (single-column for now).
    if !sel.order_by.is_empty() {
        let ob = &sel.order_by[0];
        let key = ob.column.clone();
        let descending = ob.descending;
        matched.sort_by(|a, b| {
            let av = a.1.get(&key).map(String::as_str).unwrap_or("");
            let bv = b.1.get(&key).map(String::as_str).unwrap_or("");
            // Try numeric comparison first; fall back to lexicographic.
            let cmp = match (av.parse::<f64>(), bv.parse::<f64>()) {
                (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                _ => av.cmp(bv),
            };
            if descending { cmp.reverse() } else { cmp }
        });
    }

    // OFFSET / LIMIT.
    let offset = sel.offset.unwrap_or(0) as usize;
    let limit = sel.limit.map(|l| l as usize).unwrap_or(max_rows).min(max_rows);
    let projected: Vec<ResultRow> = matched
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|(k, d)| ResultRow {
            key: k,
            data: project_columns(&sel.columns, &d),
        })
        .collect();

    Ok(SelectOutput::Rows(projected))
}

// ─────────────────────────────────────────────────────────────────────────────
// Predicate evaluation
// ─────────────────────────────────────────────────────────────────────────────

struct RowEnv<'a> {
    row_data: &'a RowData,
    row_key: &'a str,
}

fn parse_predicate(s: &str) -> Result<sa::Expr, ExecError> {
    // Wrap the predicate in a synthetic SELECT so sqlparser will accept it.
    let synthetic = format!("SELECT 1 FROM __t WHERE {}", s);
    let dialect = GenericDialect {};
    let mut stmts = Parser::parse_sql(&dialect, &synthetic)
        .map_err(|e| ExecError::BadPredicate(e.to_string()))?;
    if stmts.is_empty() {
        return Err(ExecError::BadPredicate("empty parse".into()));
    }
    if let sa::Statement::Query(q) = stmts.remove(0) {
        if let sa::SetExpr::Select(sel) = *q.body {
            if let Some(w) = sel.selection {
                return Ok(w);
            }
        }
    }
    Err(ExecError::BadPredicate("no WHERE in synthetic parse".into()))
}

fn eval_predicate(expr: &sa::Expr, env: &mut RowEnv) -> Result<bool, ExecError> {
    use sa::Expr::*;
    match expr {
        BinaryOp { left, op, right } => match op {
            sa::BinaryOperator::And => {
                Ok(eval_predicate(left, env)? && eval_predicate(right, env)?)
            }
            sa::BinaryOperator::Or => {
                Ok(eval_predicate(left, env)? || eval_predicate(right, env)?)
            }
            // Comparison
            sa::BinaryOperator::Eq
            | sa::BinaryOperator::NotEq
            | sa::BinaryOperator::Lt
            | sa::BinaryOperator::LtEq
            | sa::BinaryOperator::Gt
            | sa::BinaryOperator::GtEq => {
                let l = eval_value(left, env);
                let r = eval_value(right, env);
                Ok(compare_values(&l, op, &r))
            }
            _ => Err(ExecError::Unsupported(format!("binary op {:?}", op))),
        },
        UnaryOp { op, expr: inner } if matches!(op, sa::UnaryOperator::Not) => {
            Ok(!eval_predicate(inner, env)?)
        }
        IsNull(inner) => {
            let v = eval_value(inner, env);
            Ok(matches!(v, Val::Null))
        }
        IsNotNull(inner) => {
            let v = eval_value(inner, env);
            Ok(!matches!(v, Val::Null))
        }
        Between { negated, expr: inner, low, high } => {
            let v = eval_value(inner, env);
            let lo = eval_value(low, env);
            let hi = eval_value(high, env);
            // BETWEEN means lo <= v <= hi.
            let above = compare_values(&v, &sa::BinaryOperator::GtEq, &lo);
            let below = compare_values(&v, &sa::BinaryOperator::LtEq, &hi);
            let r = above && below;
            Ok(if *negated { !r } else { r })
        }
        InList { expr: inner, list, negated } => {
            let v = eval_value(inner, env);
            let any_eq = list.iter().any(|item| {
                let iv = eval_value(item, env);
                compare_values(&v, &sa::BinaryOperator::Eq, &iv)
            });
            Ok(if *negated { !any_eq } else { any_eq })
        }
        Like { negated, expr: inner, pattern, .. } => {
            let v = eval_value(inner, env);
            let p = eval_value(pattern, env);
            let m = like_match(&v, &p);
            Ok(if *negated { !m } else { m })
        }
        Nested(e) => eval_predicate(e, env),
        // A bare value expression in WHERE evaluates to truthy/falsy.
        _ => {
            let v = eval_value(expr, env);
            Ok(value_truthy(&v))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Value evaluation
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Val {
    Int(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    Null,
}

fn eval_value(expr: &sa::Expr, env: &mut RowEnv) -> Val {
    use sa::Expr::*;
    match expr {
        Identifier(ident) => col_value(env, ident.value.as_str()),
        CompoundIdentifier(parts) => {
            // table.col — drop the table prefix; row_data is single-table.
            let last = parts.last().map(|p| p.value.as_str()).unwrap_or("");
            col_value(env, last)
        }
        Value(v) => match v {
            sa::Value::Number(n, _) => {
                if let Ok(i) = n.parse::<i64>() {
                    Val::Int(i)
                } else if let Ok(f) = n.parse::<f64>() {
                    Val::Float(f)
                } else {
                    Val::Text(n.clone())
                }
            }
            sa::Value::SingleQuotedString(s)
            | sa::Value::DoubleQuotedString(s)
            | sa::Value::EscapedStringLiteral(s)
            | sa::Value::NationalStringLiteral(s)
            | sa::Value::HexStringLiteral(s) => Val::Text(s.clone()),
            sa::Value::Boolean(b) => Val::Bool(*b),
            sa::Value::Null => Val::Null,
            _ => Val::Null,
        },
        Nested(e) => eval_value(e, env),
        UnaryOp { op: sa::UnaryOperator::Minus, expr: inner } => {
            match eval_value(inner, env) {
                Val::Int(i) => Val::Int(-i),
                Val::Float(f) => Val::Float(-f),
                _ => Val::Null,
            }
        }
        UnaryOp { op: sa::UnaryOperator::Plus, expr: inner } => eval_value(inner, env),
        _ => Val::Null,
    }
}

fn col_value(env: &RowEnv, name: &str) -> Val {
    // Special-case: the row's storage key may not be in row_data, but the
    // user might ask for it as `_key`.
    if name == "_key" {
        return Val::Text(env.row_key.to_string());
    }
    match env.row_data.get(name) {
        None => Val::Null,
        Some(s) => parse_str_to_value(s),
    }
}

fn parse_str_to_value(s: &str) -> Val {
    if s.is_empty() {
        return Val::Text(String::new());
    }
    if let Ok(i) = s.parse::<i64>() {
        return Val::Int(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        return Val::Float(f);
    }
    if s.eq_ignore_ascii_case("true") {
        return Val::Bool(true);
    }
    if s.eq_ignore_ascii_case("false") {
        return Val::Bool(false);
    }
    Val::Text(s.to_string())
}

fn value_truthy(v: &Val) -> bool {
    match v {
        Val::Bool(b) => *b,
        Val::Int(i) => *i != 0,
        Val::Float(f) => *f != 0.0,
        Val::Text(s) => !s.is_empty(),
        Val::Null => false,
    }
}

fn compare_values(a: &Val, op: &sa::BinaryOperator, b: &Val) -> bool {
    use sa::BinaryOperator as Op;
    // SQL NULL comparison is unknown — treat as false.
    if matches!(a, Val::Null) || matches!(b, Val::Null) {
        return false;
    }
    let ord = match (a, b) {
        (Val::Int(x), Val::Int(y)) => x.cmp(y),
        (Val::Float(x), Val::Float(y)) => {
            x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Val::Int(x), Val::Float(y)) => {
            (*x as f64).partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Val::Float(x), Val::Int(y)) => {
            x.partial_cmp(&(*y as f64)).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Val::Text(x), Val::Text(y)) => x.cmp(y),
        (Val::Bool(x), Val::Bool(y)) => x.cmp(y),
        // Mixed types: try string coercion.
        (a, b) => value_to_string(a).cmp(&value_to_string(b)),
    };
    use std::cmp::Ordering::*;
    match op {
        Op::Eq => ord == Equal,
        Op::NotEq => ord != Equal,
        Op::Lt => ord == Less,
        Op::LtEq => ord != Greater,
        Op::Gt => ord == Greater,
        Op::GtEq => ord != Less,
        _ => false,
    }
}

fn value_to_string(v: &Val) -> String {
    match v {
        Val::Int(i) => i.to_string(),
        Val::Float(f) => f.to_string(),
        Val::Text(s) => s.clone(),
        Val::Bool(b) => b.to_string(),
        Val::Null => String::new(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LIKE pattern matching
// ─────────────────────────────────────────────────────────────────────────────

fn like_match(value: &Val, pattern: &Val) -> bool {
    let s = match value {
        Val::Text(t) => t.clone(),
        v => value_to_string(v),
    };
    let p = match pattern {
        Val::Text(t) => t.clone(),
        v => value_to_string(v),
    };
    glob_like(&s, &p)
}

/// Implement SQL LIKE: `%` matches any number of chars; `_` matches one char.
/// No regex escapes — anything not `%` or `_` is a literal.
fn glob_like(text: &str, pattern: &str) -> bool {
    let t: Vec<char> = text.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    glob_like_chars(&t, 0, &p, 0)
}

fn glob_like_chars(t: &[char], ti: usize, p: &[char], pi: usize) -> bool {
    if pi == p.len() {
        return ti == t.len();
    }
    match p[pi] {
        '%' => {
            // Match zero or more.
            for skip in 0..=t.len().saturating_sub(ti) {
                if glob_like_chars(t, ti + skip, p, pi + 1) {
                    return true;
                }
            }
            false
        }
        '_' => {
            if ti < t.len() {
                glob_like_chars(t, ti + 1, p, pi + 1)
            } else {
                false
            }
        }
        c => {
            if ti < t.len() && t[ti] == c {
                glob_like_chars(t, ti + 1, p, pi + 1)
            } else {
                false
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Projection
// ─────────────────────────────────────────────────────────────────────────────

fn project_columns(columns: &[String], full_row: &RowData) -> RowData {
    // SELECT * → return everything.
    if columns.is_empty() || columns.iter().any(|c| c.trim() == "*") {
        return full_row.clone();
    }
    let mut out: RowData = HashMap::new();
    for col in columns {
        let trimmed = col.trim();
        // Strip alias: "expr AS name" → keep the name as the projection key.
        let (key, source) = if let Some(idx) = trimmed.to_ascii_uppercase().find(" AS ") {
            let alias = trimmed[idx + 4..].trim();
            let source = trimmed[..idx].trim();
            (alias, source)
        } else {
            (trimmed, trimmed)
        };
        // If the source is a simple column ref, look it up.
        if let Some(v) = full_row.get(source) {
            out.insert(key.to_string(), v.clone());
        } else if source.contains('.') {
            // table.col — strip prefix.
            let last = source.rsplit('.').next().unwrap_or(source);
            if let Some(v) = full_row.get(last) {
                out.insert(key.to_string(), v.clone());
            }
        }
        // Else: column not present in row; SQL semantics is NULL — represent
        // by absence rather than inserting empty string (callers that need
        // explicit NULL can detect missing keys).
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Aggregate fast-path
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
struct AggCall {
    func: AggFunc,
    arg: AggArg,
    /// As entered in SQL ("COUNT(*)", "SUM(price)").
    display: String,
    alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum AggFunc { Count, Sum, Avg, Min, Max }

#[derive(Debug, Clone, PartialEq)]
enum AggArg { Star, Column(String) }

/// If every projected column is an aggregate call, return them. Else None.
fn try_extract_aggregates(projection: &[String], _raw_sql: &str) -> Option<Vec<AggCall>> {
    if projection.is_empty() {
        return None;
    }
    let mut out: Vec<AggCall> = Vec::new();
    for col in projection {
        let trimmed = col.trim();
        let (expr_str, alias) = match trimmed.to_ascii_uppercase().find(" AS ") {
            Some(i) => (trimmed[..i].trim(), Some(trimmed[i + 4..].trim().to_string())),
            None => (trimmed, None),
        };
        let upper = expr_str.to_ascii_uppercase();
        let (func, arg) = if upper.starts_with("COUNT(") {
            let inner = &expr_str[6..expr_str.len() - 1].trim();
            if *inner == "*" {
                (AggFunc::Count, AggArg::Star)
            } else {
                (AggFunc::Count, AggArg::Column(inner.to_string()))
            }
        } else if upper.starts_with("SUM(") {
            (AggFunc::Sum, AggArg::Column(expr_str[4..expr_str.len() - 1].trim().to_string()))
        } else if upper.starts_with("AVG(") {
            (AggFunc::Avg, AggArg::Column(expr_str[4..expr_str.len() - 1].trim().to_string()))
        } else if upper.starts_with("MIN(") {
            (AggFunc::Min, AggArg::Column(expr_str[4..expr_str.len() - 1].trim().to_string()))
        } else if upper.starts_with("MAX(") {
            (AggFunc::Max, AggArg::Column(expr_str[4..expr_str.len() - 1].trim().to_string()))
        } else {
            // Not an aggregate — can't take this fast path.
            return None;
        };
        out.push(AggCall {
            func,
            arg,
            display: expr_str.to_string(),
            alias,
        });
    }
    Some(out)
}

fn eval_aggregate(call: &AggCall, rows: &[(String, RowData)]) -> AggregateCell {
    match (&call.func, &call.arg) {
        (AggFunc::Count, AggArg::Star) => AggregateCell::Int(rows.len() as i64),
        (AggFunc::Count, AggArg::Column(c)) => {
            let n = rows.iter().filter(|(_, d)| d.contains_key(c)).count() as i64;
            AggregateCell::Int(n)
        }
        (AggFunc::Sum, AggArg::Column(c)) => {
            let mut sum = 0.0f64;
            let mut any_int = true;
            let mut int_sum = 0i64;
            for (_, d) in rows {
                if let Some(v) = d.get(c) {
                    if let Ok(i) = v.parse::<i64>() {
                        int_sum = int_sum.saturating_add(i);
                        sum += i as f64;
                    } else if let Ok(f) = v.parse::<f64>() {
                        any_int = false;
                        sum += f;
                    }
                }
            }
            if any_int { AggregateCell::Int(int_sum) } else { AggregateCell::Float(sum) }
        }
        (AggFunc::Avg, AggArg::Column(c)) => {
            let mut sum = 0.0f64;
            let mut n = 0u64;
            for (_, d) in rows {
                if let Some(v) = d.get(c) {
                    if let Ok(f) = v.parse::<f64>() {
                        sum += f;
                        n += 1;
                    }
                }
            }
            if n == 0 { AggregateCell::Null } else { AggregateCell::Float(sum / (n as f64)) }
        }
        (AggFunc::Min, AggArg::Column(c)) => agg_minmax(rows, c, true),
        (AggFunc::Max, AggArg::Column(c)) => agg_minmax(rows, c, false),
        _ => AggregateCell::Null,
    }
}

fn agg_minmax(rows: &[(String, RowData)], col: &str, want_min: bool) -> AggregateCell {
    let mut best_num: Option<f64> = None;
    let mut best_text: Option<String> = None;
    for (_, d) in rows {
        let s = match d.get(col) { Some(s) => s, None => continue };
        if let Ok(f) = s.parse::<f64>() {
            best_num = Some(match best_num {
                None => f,
                Some(b) => if (want_min && f < b) || (!want_min && f > b) { f } else { b },
            });
        } else {
            best_text = Some(match best_text {
                None => s.clone(),
                Some(b) => if (want_min && *s < b) || (!want_min && *s > b) { s.clone() } else { b },
            });
        }
    }
    match (best_num, best_text) {
        (Some(n), _) => {
            if n.fract() == 0.0 { AggregateCell::Int(n as i64) } else { AggregateCell::Float(n) }
        }
        (None, Some(s)) => AggregateCell::Text(s),
        (None, None) => AggregateCell::Null,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// Phase 3: DataFusion executor for JOIN / GROUP BY / window / subquery
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(feature = "datafusion")]
pub mod datafusion;

// ─────────────────────────────────────────────────────────────────────────────
// Table-name extraction (used by service dispatch for DataFusion routing)
// ─────────────────────────────────────────────────────────────────────────────

/// Return every base table name referenced in `sql` (FROM clause + all JOINs,
/// including nested joins and UNION branches). Aliases are stripped; subquery
/// aliases are not counted as table names. Duplicates are removed and the list
/// is sorted for determinism.
///
/// Used by the service to decide which per-table row slices to pass to
/// [`datafusion::execute_select_from_rows`] for multi-table queries.
pub fn collect_query_table_names(sql: &str) -> Vec<String> {
    use sqlparser::ast as sa;
    use sqlparser::dialect::GenericDialect;
    use sqlparser::parser::Parser;

    let Ok(stmts) = Parser::parse_sql(&GenericDialect {}, sql) else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for stmt in &stmts {
        if let sa::Statement::Query(q) = stmt {
            collect_from_set_expr(&q.body, &mut names);
        }
    }
    names.sort();
    names.dedup();
    names
}

fn collect_from_set_expr(expr: &sqlparser::ast::SetExpr, out: &mut Vec<String>) {
    use sqlparser::ast as sa;
    match expr {
        sa::SetExpr::Select(sel) => {
            for twj in &sel.from {
                collect_table_factor(&twj.relation, out);
                for join in &twj.joins {
                    collect_table_factor(&join.relation, out);
                }
            }
        }
        sa::SetExpr::Query(q) => collect_from_set_expr(&q.body, out),
        sa::SetExpr::SetOperation { left, right, .. } => {
            collect_from_set_expr(left, out);
            collect_from_set_expr(right, out);
        }
        _ => {}
    }
}

fn collect_table_factor(factor: &sqlparser::ast::TableFactor, out: &mut Vec<String>) {
    use sqlparser::ast as sa;
    match factor {
        sa::TableFactor::Table { name, .. } => {
            if let Some(ident) = name.0.last() {
                out.push(ident.value.clone());
            }
        }
        sa::TableFactor::Derived { subquery, .. } => {
            collect_from_set_expr(&subquery.body, out);
        }
        sa::TableFactor::NestedJoin { table_with_joins, .. } => {
            collect_table_factor(&table_with_joins.relation, out);
            for join in &table_with_joins.joins {
                collect_table_factor(&join.relation, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_row(items: &[(&str, &str)]) -> RowData {
        let mut r = RowData::new();
        for (k, v) in items { r.insert(k.to_string(), v.to_string()); }
        r
    }

    /// Build a fresh PagedRowStore with a fixed set of rows for testing.
    fn build_rs(rows: Vec<(&str, RowData)>) -> PagedRowStore {
        let mut rs = PagedRowStore::new(64);
        let xid = rs.begin_xid();
        for (k, d) in rows {
            rs.insert(xid, k, d);
        }
        rs
    }

    fn unwrap_rows(out: SelectOutput) -> Vec<ResultRow> {
        match out {
            SelectOutput::Rows(rs) => rs,
            other => panic!("expected Rows, got {:?}", other),
        }
    }

    fn unwrap_agg(out: SelectOutput) -> AggregateResult {
        match out {
            SelectOutput::Aggregate(a) => a,
            other => panic!("expected Aggregate, got {:?}", other),
        }
    }

    // ── REGRESSION: WHERE id = 5 must return only row 5, not 15/25/50/51 ────

    #[test]
    fn where_eq_does_not_match_substrings() {
        let rs = build_rs(vec![
            ("t:5",  make_row(&[("id", "5"),  ("name", "Alice")])),
            ("t:15", make_row(&[("id", "15"), ("name", "Bob")])),
            ("t:25", make_row(&[("id", "25"), ("name", "Carol")])),
            ("t:50", make_row(&[("id", "50"), ("name", "Dan")])),
            ("t:51", make_row(&[("id", "51"), ("name", "Eve")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM t WHERE id = 5", &rs, 100).unwrap());
        assert_eq!(rows.len(), 1, "WHERE id = 5 must match exactly one row, got {:?}", rows);
        assert_eq!(rows[0].data.get("name").map(String::as_str), Some("Alice"));
    }

    #[test]
    fn where_eq_string_value() {
        let rs = build_rs(vec![
            ("u:1", make_row(&[("name", "alice")])),
            ("u:2", make_row(&[("name", "bob")])),
            ("u:3", make_row(&[("name", "carol")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM u WHERE name = 'bob'", &rs, 100).unwrap());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].data.get("name").map(String::as_str), Some("bob"));
    }

    #[test]
    fn where_neq() {
        let rs = build_rs(vec![
            ("u:1", make_row(&[("status", "active")])),
            ("u:2", make_row(&[("status", "inactive")])),
            ("u:3", make_row(&[("status", "active")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM u WHERE status <> 'active'", &rs, 100).unwrap());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].data.get("status").map(String::as_str), Some("inactive"));
    }

    #[test]
    fn where_lt_gt_range() {
        let rs = build_rs(vec![
            ("p:1", make_row(&[("price", "10")])),
            ("p:2", make_row(&[("price", "20")])),
            ("p:3", make_row(&[("price", "30")])),
            ("p:4", make_row(&[("price", "40")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM p WHERE price > 15 AND price <= 30", &rs, 100).unwrap());
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn where_between() {
        let rs = build_rs(vec![
            ("p:1", make_row(&[("v", "1")])),
            ("p:2", make_row(&[("v", "5")])),
            ("p:3", make_row(&[("v", "10")])),
            ("p:4", make_row(&[("v", "15")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM p WHERE v BETWEEN 5 AND 10", &rs, 100).unwrap());
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn where_in_list() {
        let rs = build_rs(vec![
            ("c:1", make_row(&[("country", "US")])),
            ("c:2", make_row(&[("country", "UK")])),
            ("c:3", make_row(&[("country", "FR")])),
            ("c:4", make_row(&[("country", "DE")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM c WHERE country IN ('US', 'FR')", &rs, 100).unwrap());
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn where_is_null() {
        let rs = build_rs(vec![
            ("u:1", make_row(&[("name", "Alice"), ("email", "a@x.com")])),
            ("u:2", make_row(&[("name", "Bob")])), // no email
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM u WHERE email IS NULL", &rs, 100).unwrap());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].data.get("name").map(String::as_str), Some("Bob"));
    }

    #[test]
    fn where_like_percent() {
        let rs = build_rs(vec![
            ("u:1", make_row(&[("name", "Alice")])),
            ("u:2", make_row(&[("name", "Aaron")])),
            ("u:3", make_row(&[("name", "Bob")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM u WHERE name LIKE 'A%'", &rs, 100).unwrap());
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn where_like_underscore() {
        let rs = build_rs(vec![
            ("u:1", make_row(&[("code", "AB1")])),
            ("u:2", make_row(&[("code", "AB12")])),
            ("u:3", make_row(&[("code", "CD1")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM u WHERE code LIKE 'A_1'", &rs, 100).unwrap());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].data.get("code").map(String::as_str), Some("AB1"));
    }

    #[test]
    fn where_and_or() {
        let rs = build_rs(vec![
            ("o:1", make_row(&[("status", "paid"),    ("amount", "100")])),
            ("o:2", make_row(&[("status", "paid"),    ("amount", "500")])),
            ("o:3", make_row(&[("status", "pending"), ("amount", "200")])),
            ("o:4", make_row(&[("status", "void"),    ("amount", "50")])),
        ]);
        let rows = unwrap_rows(execute_select(
            "SELECT * FROM o WHERE status = 'paid' AND amount > 200", &rs, 100).unwrap());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].data.get("amount").map(String::as_str), Some("500"));

        let rows = unwrap_rows(execute_select(
            "SELECT * FROM o WHERE status = 'pending' OR status = 'void'", &rs, 100).unwrap());
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn projection_specific_columns() {
        let rs = build_rs(vec![
            ("u:1", make_row(&[("name", "Alice"), ("age", "30"), ("email", "a@x.com")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT name, age FROM u", &rs, 100).unwrap());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].data.len(), 2);
        assert!(rows[0].data.contains_key("name"));
        assert!(rows[0].data.contains_key("age"));
        assert!(!rows[0].data.contains_key("email"));
    }

    #[test]
    fn projection_star_returns_all_columns() {
        let rs = build_rs(vec![
            ("u:1", make_row(&[("name", "Alice"), ("age", "30")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM u", &rs, 100).unwrap());
        assert_eq!(rows[0].data.len(), 2);
    }

    #[test]
    fn order_by_ascending_numeric() {
        let rs = build_rs(vec![
            ("p:1", make_row(&[("price", "30")])),
            ("p:2", make_row(&[("price", "10")])),
            ("p:3", make_row(&[("price", "20")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM p ORDER BY price ASC", &rs, 100).unwrap());
        let prices: Vec<&str> = rows.iter().map(|r| r.data.get("price").unwrap().as_str()).collect();
        assert_eq!(prices, vec!["10", "20", "30"]);
    }

    #[test]
    fn order_by_descending() {
        let rs = build_rs(vec![
            ("p:1", make_row(&[("price", "10")])),
            ("p:2", make_row(&[("price", "30")])),
            ("p:3", make_row(&[("price", "20")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM p ORDER BY price DESC", &rs, 100).unwrap());
        let prices: Vec<&str> = rows.iter().map(|r| r.data.get("price").unwrap().as_str()).collect();
        assert_eq!(prices, vec!["30", "20", "10"]);
    }

    #[test]
    fn limit_and_offset() {
        let rs = build_rs(vec![
            ("p:1", make_row(&[("v", "1")])),
            ("p:2", make_row(&[("v", "2")])),
            ("p:3", make_row(&[("v", "3")])),
            ("p:4", make_row(&[("v", "4")])),
            ("p:5", make_row(&[("v", "5")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM p ORDER BY v ASC LIMIT 2 OFFSET 1", &rs, 100).unwrap());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].data.get("v").map(String::as_str), Some("2"));
        assert_eq!(rows[1].data.get("v").map(String::as_str), Some("3"));
    }

    #[test]
    fn max_rows_caps_result() {
        let rs = build_rs(vec![
            ("p:1", make_row(&[("v", "1")])),
            ("p:2", make_row(&[("v", "2")])),
            ("p:3", make_row(&[("v", "3")])),
        ]);
        let rows = unwrap_rows(execute_select("SELECT * FROM p", &rs, 2).unwrap());
        assert_eq!(rows.len(), 2);
    }

    // ── Aggregates ──────────────────────────────────────────────────────────

    #[test]
    fn count_star() {
        let rs = build_rs(vec![
            ("p:1", make_row(&[("v", "1")])),
            ("p:2", make_row(&[("v", "2")])),
            ("p:3", make_row(&[("v", "3")])),
        ]);
        let agg = unwrap_agg(execute_select("SELECT COUNT(*) FROM p", &rs, 100).unwrap());
        assert_eq!(agg.values, vec![AggregateCell::Int(3)]);
    }

    #[test]
    fn count_with_where() {
        let rs = build_rs(vec![
            ("o:1", make_row(&[("status", "paid")])),
            ("o:2", make_row(&[("status", "paid")])),
            ("o:3", make_row(&[("status", "void")])),
        ]);
        let agg = unwrap_agg(execute_select(
            "SELECT COUNT(*) FROM o WHERE status = 'paid'", &rs, 100).unwrap());
        assert_eq!(agg.values, vec![AggregateCell::Int(2)]);
    }

    #[test]
    fn sum_int_column() {
        let rs = build_rs(vec![
            ("p:1", make_row(&[("price", "10")])),
            ("p:2", make_row(&[("price", "20")])),
            ("p:3", make_row(&[("price", "30")])),
        ]);
        let agg = unwrap_agg(execute_select("SELECT SUM(price) FROM p", &rs, 100).unwrap());
        assert_eq!(agg.values, vec![AggregateCell::Int(60)]);
    }

    #[test]
    fn avg_float() {
        let rs = build_rs(vec![
            ("p:1", make_row(&[("v", "10")])),
            ("p:2", make_row(&[("v", "20")])),
        ]);
        let agg = unwrap_agg(execute_select("SELECT AVG(v) FROM p", &rs, 100).unwrap());
        match &agg.values[0] {
            AggregateCell::Float(f) => assert!((*f - 15.0).abs() < 0.01),
            other => panic!("expected float, got {:?}", other),
        }
    }

    #[test]
    fn min_max() {
        let rs = build_rs(vec![
            ("p:1", make_row(&[("v", "30")])),
            ("p:2", make_row(&[("v", "10")])),
            ("p:3", make_row(&[("v", "20")])),
        ]);
        let agg_min = unwrap_agg(execute_select("SELECT MIN(v) FROM p", &rs, 100).unwrap());
        assert_eq!(agg_min.values, vec![AggregateCell::Int(10)]);
        let agg_max = unwrap_agg(execute_select("SELECT MAX(v) FROM p", &rs, 100).unwrap());
        assert_eq!(agg_max.values, vec![AggregateCell::Int(30)]);
    }

    // ── Unsupported features: must error so caller falls back ───────────────

    #[test]
    fn group_by_returns_unsupported() {
        let rs = build_rs(vec![
            ("o:1", make_row(&[("region", "us"), ("amt", "10")])),
        ]);
        let result = execute_select("SELECT region, COUNT(*) FROM o GROUP BY region", &rs, 100);
        assert!(matches!(result, Err(ExecError::Unsupported(_))));
    }

    #[test]
    fn join_returns_unsupported() {
        let rs = build_rs(vec![
            ("o:1", make_row(&[("v", "1")])),
        ]);
        let result = execute_select("SELECT * FROM o JOIN c ON o.cid = c.id", &rs, 100);
        assert!(matches!(result, Err(ExecError::Unsupported(_))));
    }

    #[test]
    fn subquery_returns_unsupported() {
        let rs = build_rs(vec![
            ("o:1", make_row(&[("v", "1")])),
        ]);
        let result = execute_select(
            "SELECT * FROM o WHERE v IN (SELECT id FROM c)", &rs, 100);
        assert!(matches!(result, Err(ExecError::Unsupported(_))));
    }

    // ── glob_like unit tests ────────────────────────────────────────────────

    #[test]
    fn glob_like_basic() {
        assert!(glob_like("hello", "hello"));
        assert!(!glob_like("hello", "world"));
        assert!(glob_like("hello", "h%"));
        assert!(glob_like("hello", "%o"));
        assert!(glob_like("hello", "%ll%"));
        assert!(glob_like("hello", "h_llo"));
        assert!(!glob_like("hello", "h_llox"));
        assert!(glob_like("", "%"));
        assert!(glob_like("anything", "%"));
        assert!(!glob_like("", "_"));
    }

    // ── collect_query_table_names ────────────────────────────────────────────

    #[test]
    fn table_names_single_table() {
        let names = collect_query_table_names("SELECT id, name FROM users WHERE id = 1");
        assert_eq!(names, vec!["users"]);
    }

    #[test]
    fn table_names_inner_join() {
        let names = collect_query_table_names(
            "SELECT o.id, c.name FROM orders o JOIN customers c ON o.cid = c.id"
        );
        assert_eq!(names, vec!["customers", "orders"]);
    }

    #[test]
    fn table_names_triple_join() {
        let names = collect_query_table_names(
            "SELECT o.id, c.name, p.title \
             FROM orders o \
             JOIN customers c ON o.cid = c.id \
             JOIN products p ON o.pid = p.id"
        );
        assert_eq!(names, vec!["customers", "orders", "products"]);
    }

    #[test]
    fn table_names_left_join() {
        let names = collect_query_table_names(
            "SELECT u.name, r.role FROM users u LEFT JOIN roles r ON u.role_id = r.id"
        );
        assert_eq!(names, vec!["roles", "users"]);
    }

    #[test]
    fn table_names_union() {
        let names = collect_query_table_names(
            "SELECT id FROM active_users UNION ALL SELECT id FROM archived_users"
        );
        assert_eq!(names, vec!["active_users", "archived_users"]);
    }

    #[test]
    fn table_names_subquery_not_counted() {
        // The derived table alias `sub` should NOT appear; `orders` should.
        let names = collect_query_table_names(
            "SELECT * FROM orders o WHERE o.id IN (SELECT id FROM orders WHERE amount > 100)"
        );
        // `orders` appears twice in AST but dedup keeps one.
        assert_eq!(names, vec!["orders"]);
    }

    #[test]
    fn table_names_empty_on_bad_sql() {
        let names = collect_query_table_names("this is not sql !@#");
        assert_eq!(names, Vec::<String>::new());
    }
}
