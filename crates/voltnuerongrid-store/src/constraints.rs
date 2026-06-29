#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};

/// The type of constraint enforced on a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    PrimaryKey,
    Unique,
    NotNull,
    ForeignKey,
    /// Q-4: column-level `CHECK (<expr>)` constraint. The expression text is
    /// carried in [`ConstraintDescriptor::check_expr`].
    Check,
}

/// Describes a single column-level constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintDescriptor {
    pub name: String,
    pub table: String,
    pub column: String,
    pub kind: ConstraintKind,
    /// For `ForeignKey` constraints: the referenced parent table.
    pub ref_table: Option<String>,
    /// For `ForeignKey` constraints: the referenced column in the parent table.
    pub ref_column: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintViolation {
    PrimaryKeyDuplicate {
        constraint: String,
        value: String,
    },
    UniqueDuplicate {
        constraint: String,
        value: String,
    },
    NotNullViolation {
        constraint: String,
        column: String,
    },
    ForeignKeyViolation {
        constraint: String,
        value: String,
        ref_table: String,
        ref_column: String,
    },
    /// Q-4: a `CHECK (<expr>)` predicate evaluated to false for the given value.
    CheckViolation {
        constraint: String,
        expr: String,
        value: String,
    },
    ConstraintAlreadyExists(String),
    ConstraintNotFound(String),
}

impl std::fmt::Display for ConstraintViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PrimaryKeyDuplicate { constraint, value } => {
                write!(f, "primary key '{constraint}' duplicate for value '{value}'")
            }
            Self::UniqueDuplicate { constraint, value } => {
                write!(f, "unique constraint '{constraint}' violation for value '{value}'")
            }
            Self::NotNullViolation { constraint, column } => {
                write!(f, "not-null constraint '{constraint}' on column '{column}'")
            }
            Self::ForeignKeyViolation { constraint, value, ref_table, ref_column } => {
                write!(f, "foreign key '{constraint}' violation: value '{value}' not found in '{ref_table}.{ref_column}'")
            }
            Self::CheckViolation { constraint, expr, value } => {
                write!(f, "check constraint '{constraint}' violation: value '{value}' fails predicate '{expr}'")
            }
            Self::ConstraintAlreadyExists(name) => {
                write!(f, "constraint '{name}' already exists")
            }
            Self::ConstraintNotFound(name) => write!(f, "constraint '{name}' not found"),
        }
    }
}

/// Manages table constraints and validates mutations against them.
#[derive(Debug, Default)]
pub struct ConstraintManager {
    constraints: HashMap<String, ConstraintDescriptor>,
    /// Tracks seen values for PK / UNIQUE constraints: constraint_name → set of values
    unique_sets: HashMap<String, HashSet<String>>,
    /// Q-4: CHECK predicate text keyed by constraint name.
    check_exprs: HashMap<String, String>,
}

impl ConstraintManager {
    pub fn new() -> Self {
        Self {
            constraints: HashMap::new(),
            unique_sets: HashMap::new(),
            check_exprs: HashMap::new(),
        }
    }

    pub fn add_constraint(
        &mut self,
        descriptor: ConstraintDescriptor,
    ) -> Result<(), ConstraintViolation> {
        if self.constraints.contains_key(&descriptor.name) {
            return Err(ConstraintViolation::ConstraintAlreadyExists(
                descriptor.name.clone(),
            ));
        }
        let name = descriptor.name.clone();
        if descriptor.kind == ConstraintKind::PrimaryKey
            || descriptor.kind == ConstraintKind::Unique
        {
            self.unique_sets.insert(name.clone(), HashSet::new());
        }
        self.constraints.insert(name, descriptor);
        Ok(())
    }

    /// Q-4: Register a `CHECK (<expr>)` constraint. The predicate is stored
    /// alongside the descriptor and evaluated against each mutated value.
    pub fn add_check_constraint(
        &mut self,
        name: &str,
        table: &str,
        column: &str,
        expr: &str,
    ) -> Result<(), ConstraintViolation> {
        if self.constraints.contains_key(name) {
            return Err(ConstraintViolation::ConstraintAlreadyExists(name.to_string()));
        }
        self.constraints.insert(
            name.to_string(),
            ConstraintDescriptor {
                name: name.to_string(),
                table: table.to_string(),
                column: column.to_string(),
                kind: ConstraintKind::Check,
                ref_table: None,
                ref_column: None,
            },
        );
        self.check_exprs.insert(name.to_string(), expr.to_string());
        Ok(())
    }

    pub fn drop_constraint(
        &mut self,
        name: &str,
    ) -> Result<ConstraintDescriptor, ConstraintViolation> {
        self.unique_sets.remove(name);
        self.check_exprs.remove(name);
        self.constraints
            .remove(name)
            .ok_or_else(|| ConstraintViolation::ConstraintNotFound(name.to_string()))
    }

    /// Validate a proposed column value against all constraints for the given table+column.
    /// `value` is `None` when the column is absent (NULL).
    pub fn validate(
        &self,
        table: &str,
        column: &str,
        value: Option<&str>,
    ) -> Result<(), ConstraintViolation> {
        for constraint in self.constraints.values() {
            if constraint.table != table || constraint.column != column {
                continue;
            }
            match constraint.kind {
                ConstraintKind::NotNull => {
                    if value.is_none() {
                        return Err(ConstraintViolation::NotNullViolation {
                            constraint: constraint.name.clone(),
                            column: column.to_string(),
                        });
                    }
                }
                ConstraintKind::PrimaryKey => {
                    if let Some(val) = value {
                        if let Some(seen) = self.unique_sets.get(&constraint.name) {
                            if seen.contains(val) {
                                return Err(ConstraintViolation::PrimaryKeyDuplicate {
                                    constraint: constraint.name.clone(),
                                    value: val.to_string(),
                                });
                            }
                        }
                    } else {
                        return Err(ConstraintViolation::NotNullViolation {
                            constraint: constraint.name.clone(),
                            column: column.to_string(),
                        });
                    }
                }
                ConstraintKind::Unique => {
                    if let Some(val) = value {
                        if let Some(seen) = self.unique_sets.get(&constraint.name) {
                            if seen.contains(val) {
                                return Err(ConstraintViolation::UniqueDuplicate {
                                    constraint: constraint.name.clone(),
                                    value: val.to_string(),
                                });
                            }
                        }
                    }
                }
                ConstraintKind::ForeignKey => {
                    // Q-4: FK validation — the value must exist in the parent
                    // table's PK/UNIQUE committed-value set. A NULL FK value is
                    // permitted (matches SQL semantics: NULL never violates FK).
                    if let Some(val) = value {
                        let (Some(ref_table), Some(ref_column)) =
                            (&constraint.ref_table, &constraint.ref_column)
                        else {
                            continue;
                        };
                        // Locate the parent PK/UNIQUE constraint covering the
                        // referenced (table, column) and check its value set.
                        let parent_known = self.constraints.values().any(|c| {
                            (c.kind == ConstraintKind::PrimaryKey
                                || c.kind == ConstraintKind::Unique)
                                && &c.table == ref_table
                                && &c.column == ref_column
                        });
                        let exists = self
                            .constraints
                            .values()
                            .filter(|c| {
                                (c.kind == ConstraintKind::PrimaryKey
                                    || c.kind == ConstraintKind::Unique)
                                    && &c.table == ref_table
                                    && &c.column == ref_column
                            })
                            .any(|c| {
                                self.unique_sets
                                    .get(&c.name)
                                    .map(|set| set.contains(val))
                                    .unwrap_or(false)
                            });
                        // Only enforce when the parent key is tracked; without a
                        // registered parent PK/UNIQUE we cannot prove existence
                        // and must not reject (avoids false positives).
                        if parent_known && !exists {
                            return Err(ConstraintViolation::ForeignKeyViolation {
                                constraint: constraint.name.clone(),
                                value: val.to_string(),
                                ref_table: ref_table.clone(),
                                ref_column: ref_column.clone(),
                            });
                        }
                    }
                }
                ConstraintKind::Check => {
                    // Q-4: evaluate the CHECK predicate against the value. A NULL
                    // value makes the predicate UNKNOWN, which SQL treats as
                    // satisfied (CHECK only fails on a definitively false result).
                    if let Some(val) = value {
                        if let Some(expr) = self.check_exprs.get(&constraint.name) {
                            if !eval_check_predicate(expr, &constraint.column, val) {
                                return Err(ConstraintViolation::CheckViolation {
                                    constraint: constraint.name.clone(),
                                    expr: expr.clone(),
                                    value: val.to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Record a value as committed for uniqueness tracking.
    /// Must be called AFTER `validate` succeeds for PK/UNIQUE columns.
    pub fn record_committed_value(&mut self, table: &str, column: &str, value: &str) {
        for constraint in self.constraints.values() {
            if constraint.table != table || constraint.column != column {
                continue;
            }
            if constraint.kind == ConstraintKind::PrimaryKey
                || constraint.kind == ConstraintKind::Unique
            {
                if let Some(seen) = self.unique_sets.get_mut(&constraint.name) {
                    seen.insert(value.to_string());
                }
            }
        }
    }

    /// Remove a previously committed value (e.g. on row delete).
    pub fn remove_committed_value(&mut self, table: &str, column: &str, value: &str) {
        for constraint in self.constraints.values() {
            if constraint.table != table || constraint.column != column {
                continue;
            }
            if constraint.kind == ConstraintKind::PrimaryKey
                || constraint.kind == ConstraintKind::Unique
            {
                if let Some(seen) = self.unique_sets.get_mut(&constraint.name) {
                    seen.remove(value);
                }
            }
        }
    }

    pub fn list_constraints(&self) -> Vec<&ConstraintDescriptor> {
        self.constraints.values().collect()
    }

    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    /// Q-4: Return the columns of `table` that carry a NOT NULL (or PRIMARY KEY)
    /// constraint, so callers can reject an INSERT that omits a required column.
    pub fn not_null_columns(&self, table: &str) -> Vec<String> {
        self.constraints
            .values()
            .filter(|d| {
                d.table == table
                    && (d.kind == ConstraintKind::NotNull || d.kind == ConstraintKind::PrimaryKey)
            })
            .map(|d| d.column.clone())
            .collect()
    }

    /// Returns all FK constraints for `(table, column)` with their referenced table+column.
    pub fn list_fk_refs(&self, table: &str, column: &str) -> Vec<(String, String, String)> {
        self.constraints
            .values()
            .filter(|d| d.kind == ConstraintKind::ForeignKey && d.table == table && d.column == column)
            .filter_map(|d| {
                let ref_t = d.ref_table.as_ref()?;
                let ref_c = d.ref_column.as_ref()?;
                Some((d.name.clone(), ref_t.clone(), ref_c.clone()))
            })
            .collect()
    }
}

// ─── Q-4: CHECK predicate evaluation ──────────────────────────────────────────

/// Evaluate a single-column `CHECK` predicate against `value`.
///
/// Supported predicate forms (case-insensitive, `column` is the constrained
/// column name):
///
/// * `col OP literal` where `OP ∈ {=, <>, !=, >, >=, <, <=}` — numeric compare
///   when both sides parse as `f64`, otherwise string compare.
/// * `col IN (a, b, c)` / `col NOT IN (...)` — membership test (string).
/// * `col IS NOT NULL` — always true here (a present value is non-null).
/// * `LENGTH(col) OP n` — string-length comparison.
///
/// Unknown / unparseable predicates default to **true** (fail-open) so an
/// exotic expression never blocks a write it was not designed to guard.
pub fn eval_check_predicate(expr: &str, column: &str, value: &str) -> bool {
    let e = expr.trim();
    let lower = e.to_ascii_lowercase();
    let col_lower = column.to_ascii_lowercase();

    // IS NOT NULL — a present value satisfies it.
    if lower.contains("is not null") {
        return !value.is_empty();
    }
    if lower.ends_with("is null") {
        return value.is_empty();
    }

    // IN / NOT IN list membership.
    if let Some(pos) = lower.find(" in ") {
        let is_not = lower[..pos].trim_end().ends_with("not");
        if let Some(open) = e.find('(') {
            if let Some(close) = e.rfind(')') {
                if close > open {
                    let items: Vec<String> = e[open + 1..close]
                        .split(',')
                        .map(|s| s.trim().trim_matches(|c| c == '\'' || c == '"').to_string())
                        .collect();
                    let contained = items.iter().any(|it| it == value);
                    return if is_not { !contained } else { contained };
                }
            }
        }
    }

    // LENGTH(col) OP n
    if lower.contains("length(") {
        if let Some((op, rhs)) = split_comparison(&lower) {
            if let Ok(n) = rhs.trim().parse::<f64>() {
                let len = value.chars().count() as f64;
                return compare_numeric(len, &op, n);
            }
        }
    }

    // col OP literal
    if let Some((op, rhs)) = split_comparison(e) {
        // Confirm the left side references the constrained column.
        let lhs = e[..e.find(&op).unwrap_or(0)].trim().to_ascii_lowercase();
        if !lhs.is_empty() && lhs != col_lower && !lhs.contains(&col_lower) {
            // Predicate does not reference this column — cannot evaluate, fail-open.
            return true;
        }
        let rhs_clean = rhs.trim().trim_matches(|c| c == '\'' || c == '"');
        // Numeric comparison when both sides are numbers.
        if let (Ok(lv), Ok(rv)) = (value.parse::<f64>(), rhs_clean.parse::<f64>()) {
            return compare_numeric(lv, &op, rv);
        }
        // String comparison.
        return compare_string(value, &op, rhs_clean);
    }

    // Unrecognised predicate — fail open.
    true
}

/// Split a comparison expression into `(operator, right_hand_side)`.
/// Recognises (longest-first) `>=`, `<=`, `<>`, `!=`, `=`, `>`, `<`.
fn split_comparison(expr: &str) -> Option<(String, String)> {
    for op in [">=", "<=", "<>", "!=", "=", ">", "<"] {
        if let Some(pos) = expr.find(op) {
            let rhs = expr[pos + op.len()..].trim().to_string();
            if !rhs.is_empty() {
                return Some((op.to_string(), rhs));
            }
        }
    }
    None
}

fn compare_numeric(lhs: f64, op: &str, rhs: f64) -> bool {
    match op {
        "=" => (lhs - rhs).abs() < f64::EPSILON,
        "<>" | "!=" => (lhs - rhs).abs() >= f64::EPSILON,
        ">" => lhs > rhs,
        ">=" => lhs >= rhs,
        "<" => lhs < rhs,
        "<=" => lhs <= rhs,
        _ => true,
    }
}

fn compare_string(lhs: &str, op: &str, rhs: &str) -> bool {
    match op {
        "=" => lhs == rhs,
        "<>" | "!=" => lhs != rhs,
        ">" => lhs > rhs,
        ">=" => lhs >= rhs,
        "<" => lhs < rhs,
        "<=" => lhs <= rhs,
        _ => true,
    }
}

// ─── Q-4: Constraint DDL parsing ──────────────────────────────────────────────

/// A constraint extracted from a `CREATE TABLE` or `ALTER TABLE ADD CONSTRAINT`
/// statement, ready to register with [`ConstraintManager`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedConstraint {
    pub name: String,
    pub table: String,
    pub column: String,
    pub kind: ConstraintKind,
    pub ref_table: Option<String>,
    pub ref_column: Option<String>,
    pub check_expr: Option<String>,
}

/// Split a parenthesised body on top-level commas, respecting nested `()`.
fn split_top_level_commas(body: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for ch in body.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => {
                parts.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_string());
    }
    parts
}

/// Extract the `(...)` argument of a keyword like `REFERENCES parent(col)` or
/// `CHECK (expr)`. Returns the inner text without the surrounding parentheses.
fn extract_paren_arg(s: &str, after_lower_kw: &str) -> Option<String> {
    let lower = s.to_ascii_lowercase();
    let kw_pos = lower.find(after_lower_kw)?;
    let rest = &s[kw_pos + after_lower_kw.len()..];
    let open = rest.find('(')?;
    let mut depth = 0i32;
    let mut end = None;
    for (i, ch) in rest[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    Some(rest[open + 1..end].trim().to_string())
}

/// Parse `CREATE TABLE [db.]<name> ( ... )` and extract all column-level and
/// table-level constraints. Constraint names are auto-generated when the DDL
/// does not name them: `{table}_{column}_{kind}`.
pub fn parse_create_table_constraints(sql: &str) -> Vec<ParsedConstraint> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("create table") {
        return Vec::new();
    }
    // Body between the first '(' and the matching last ')'.
    let Some(open) = trimmed.find('(') else { return Vec::new() };
    let Some(close) = trimmed.rfind(')') else { return Vec::new() };
    if close <= open {
        return Vec::new();
    }
    // Table name: tokens between "create table" and '('.
    let header = trimmed[..open].to_ascii_lowercase();
    let header = header
        .trim_start_matches("create table")
        .trim()
        .trim_start_matches("if not exists")
        .trim();
    let raw_table = header.split_whitespace().next().unwrap_or("");
    let table = raw_table.rsplit('.').next().unwrap_or(raw_table).to_string();
    if table.is_empty() {
        return Vec::new();
    }

    let body = &trimmed[open + 1..close];
    let mut out = Vec::new();
    for seg in split_top_level_commas(body) {
        let seg_lower = seg.to_ascii_lowercase();
        let first = seg.split_whitespace().next().unwrap_or("");
        let first_lower = first.to_ascii_lowercase();
        // Table-level constraint forms.
        if first_lower == "primary"
            || first_lower == "foreign"
            || first_lower == "unique"
            || first_lower == "check"
            || first_lower == "constraint"
        {
            parse_table_level_constraint(&table, &seg, &mut out);
            continue;
        }
        // Column-level: first token is the column name.
        let column = first.trim_matches(|c| c == '"' || c == '`').to_string();
        if column.is_empty() {
            continue;
        }
        if seg_lower.contains(" not null") || seg_lower.contains("\tnot null") {
            out.push(ParsedConstraint {
                name: format!("{table}_{column}_notnull"),
                table: table.clone(),
                column: column.clone(),
                kind: ConstraintKind::NotNull,
                ref_table: None,
                ref_column: None,
                check_expr: None,
            });
        }
        if seg_lower.contains("primary key") {
            out.push(ParsedConstraint {
                name: format!("{table}_{column}_pk"),
                table: table.clone(),
                column: column.clone(),
                kind: ConstraintKind::PrimaryKey,
                ref_table: None,
                ref_column: None,
                check_expr: None,
            });
        } else if seg_lower.contains(" unique") {
            out.push(ParsedConstraint {
                name: format!("{table}_{column}_unique"),
                table: table.clone(),
                column: column.clone(),
                kind: ConstraintKind::Unique,
                ref_table: None,
                ref_column: None,
                check_expr: None,
            });
        }
        if seg_lower.contains("references") {
            if let Some((rt, rc)) = parse_references(&seg) {
                out.push(ParsedConstraint {
                    name: format!("{table}_{column}_fk"),
                    table: table.clone(),
                    column: column.clone(),
                    kind: ConstraintKind::ForeignKey,
                    ref_table: Some(rt),
                    ref_column: Some(rc),
                    check_expr: None,
                });
            }
        }
        if seg_lower.contains("check") {
            if let Some(expr) = extract_paren_arg(&seg, "check") {
                out.push(ParsedConstraint {
                    name: format!("{table}_{column}_check"),
                    table: table.clone(),
                    column: column.clone(),
                    kind: ConstraintKind::Check,
                    ref_table: None,
                    ref_column: None,
                    check_expr: Some(expr),
                });
            }
        }
    }
    out
}

/// Parse `REFERENCES parent(col)` → `(parent_table, parent_column)`.
fn parse_references(seg: &str) -> Option<(String, String)> {
    let lower = seg.to_ascii_lowercase();
    let pos = lower.find("references")?;
    let rest = seg[pos + "references".len()..].trim();
    // rest = "parent(col) ..." or "parent (col)"
    let open = rest.find('(')?;
    let ref_table = rest[..open].trim().rsplit('.').next().unwrap_or("").trim().to_string();
    let close = rest.find(')')?;
    let ref_col = rest[open + 1..close].trim().to_string();
    if ref_table.is_empty() || ref_col.is_empty() {
        return None;
    }
    Some((ref_table, ref_col))
}

/// Parse a table-level constraint segment (PRIMARY KEY/UNIQUE/FOREIGN KEY/CHECK,
/// optionally prefixed by `CONSTRAINT <name>`).
fn parse_table_level_constraint(table: &str, seg: &str, out: &mut Vec<ParsedConstraint>) {
    let mut s = seg.trim().to_string();
    let lower = s.to_ascii_lowercase();
    // Optional CONSTRAINT <name> prefix.
    let mut name: Option<String> = None;
    if lower.starts_with("constraint ") {
        let after = s["constraint ".len()..].trim_start();
        let nm = after.split_whitespace().next().unwrap_or("").to_string();
        if !nm.is_empty() {
            name = Some(nm.clone());
            // Strip "CONSTRAINT <name>" so the remaining text starts with the kind.
            let idx = s.to_ascii_lowercase().find(&nm.to_ascii_lowercase()).unwrap_or(0) + nm.len();
            s = s[idx..].trim().to_string();
        }
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("primary key") {
        if let Some(col) = extract_paren_arg(&s, "key") {
            let column = col.split(',').next().unwrap_or("").trim().to_string();
            out.push(ParsedConstraint {
                name: name.unwrap_or_else(|| format!("{table}_{column}_pk")),
                table: table.to_string(),
                column,
                kind: ConstraintKind::PrimaryKey,
                ref_table: None,
                ref_column: None,
                check_expr: None,
            });
        }
    } else if lower.starts_with("unique") {
        if let Some(col) = extract_paren_arg(&s, "unique") {
            let column = col.split(',').next().unwrap_or("").trim().to_string();
            out.push(ParsedConstraint {
                name: name.unwrap_or_else(|| format!("{table}_{column}_unique")),
                table: table.to_string(),
                column,
                kind: ConstraintKind::Unique,
                ref_table: None,
                ref_column: None,
                check_expr: None,
            });
        }
    } else if lower.starts_with("foreign key") {
        if let Some(col) = extract_paren_arg(&s, "key") {
            let column = col.split(',').next().unwrap_or("").trim().to_string();
            if let Some((rt, rc)) = parse_references(&s) {
                out.push(ParsedConstraint {
                    name: name.unwrap_or_else(|| format!("{table}_{column}_fk")),
                    table: table.to_string(),
                    column,
                    kind: ConstraintKind::ForeignKey,
                    ref_table: Some(rt),
                    ref_column: Some(rc),
                    check_expr: None,
                });
            }
        }
    } else if lower.starts_with("check") {
        if let Some(expr) = extract_paren_arg(&s, "check") {
            // Best-effort: the constrained column is the first identifier in expr.
            let column = expr
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .find(|t| !t.is_empty())
                .unwrap_or("")
                .to_string();
            out.push(ParsedConstraint {
                name: name.unwrap_or_else(|| format!("{table}_{column}_check")),
                table: table.to_string(),
                column,
                kind: ConstraintKind::Check,
                ref_table: None,
                ref_column: None,
                check_expr: Some(expr),
            });
        }
    }
}

/// Parse `ALTER TABLE <table> ADD [CONSTRAINT <name>] {UNIQUE(col) | CHECK(expr)
/// | PRIMARY KEY(col) | FOREIGN KEY(col) REFERENCES parent(pcol)}`.
pub fn parse_alter_add_constraint(sql: &str) -> Option<ParsedConstraint> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("alter table") {
        return None;
    }
    let after = trimmed["alter table".len()..].trim();
    let table_tok = after.split_whitespace().next()?;
    let table = table_tok.rsplit('.').next().unwrap_or(table_tok).to_string();
    // Find " add " and parse the constraint definition after it.
    let add_pos = lower.find(" add ")?;
    let def = trimmed[add_pos + 5..].trim();
    let mut results = Vec::new();
    // Re-use the table-level constraint parser (handles optional CONSTRAINT name).
    parse_table_level_constraint(&table, def, &mut results);
    results.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Q-4: CHECK predicate evaluator ─────────────────────────────────────
    #[test]
    fn check_predicate_numeric_comparisons() {
        assert!(eval_check_predicate("age >= 0", "age", "5"));
        assert!(!eval_check_predicate("age >= 0", "age", "-1"));
        assert!(eval_check_predicate("qty > 0", "qty", "1"));
        assert!(!eval_check_predicate("qty > 0", "qty", "0"));
        assert!(eval_check_predicate("price <= 100", "price", "99.5"));
        assert!(!eval_check_predicate("price <= 100", "price", "100.1"));
    }

    #[test]
    fn check_predicate_in_list() {
        assert!(eval_check_predicate("status IN ('new','done')", "status", "new"));
        assert!(!eval_check_predicate("status IN ('new','done')", "status", "x"));
        assert!(eval_check_predicate("status NOT IN ('bad')", "status", "ok"));
        assert!(!eval_check_predicate("status NOT IN ('bad')", "status", "bad"));
    }

    #[test]
    fn check_predicate_length_and_null() {
        assert!(eval_check_predicate("LENGTH(code) >= 3", "code", "abcd"));
        assert!(!eval_check_predicate("LENGTH(code) >= 3", "code", "ab"));
        assert!(eval_check_predicate("name IS NOT NULL", "name", "alice"));
        assert!(!eval_check_predicate("name IS NOT NULL", "name", ""));
    }

    // ── Q-4: CHECK constraint via manager ──────────────────────────────────
    #[test]
    fn check_constraint_rejects_failing_value() {
        let mut mgr = ConstraintManager::new();
        mgr.add_check_constraint("ck_age", "people", "age", "age >= 0").unwrap();
        assert!(mgr.validate("people", "age", Some("10")).is_ok());
        let err = mgr.validate("people", "age", Some("-5")).unwrap_err();
        assert!(matches!(err, ConstraintViolation::CheckViolation { .. }));
    }

    // ── Q-4: FK validation against parent unique set ───────────────────────
    #[test]
    fn foreign_key_requires_parent_value() {
        let mut mgr = ConstraintManager::new();
        // Parent PK on customers.id.
        mgr.add_constraint(ConstraintDescriptor {
            name: "pk_customers".to_string(),
            table: "customers".to_string(),
            column: "id".to_string(),
            kind: ConstraintKind::PrimaryKey,
            ref_table: None,
            ref_column: None,
        })
        .unwrap();
        // FK on orders.customer_id → customers.id.
        mgr.add_constraint(ConstraintDescriptor {
            name: "fk_orders_customer".to_string(),
            table: "orders".to_string(),
            column: "customer_id".to_string(),
            kind: ConstraintKind::ForeignKey,
            ref_table: Some("customers".to_string()),
            ref_column: Some("id".to_string()),
        })
        .unwrap();
        // No parent value yet → FK violation.
        let err = mgr.validate("orders", "customer_id", Some("42")).unwrap_err();
        assert!(matches!(err, ConstraintViolation::ForeignKeyViolation { .. }));
        // Insert parent value → FK now satisfied.
        mgr.record_committed_value("customers", "id", "42");
        assert!(mgr.validate("orders", "customer_id", Some("42")).is_ok());
        // NULL FK value is always allowed.
        assert!(mgr.validate("orders", "customer_id", None).is_ok());
    }

    // ── Q-4: CREATE TABLE constraint parsing ───────────────────────────────
    #[test]
    fn parse_create_table_column_constraints() {
        let cons = parse_create_table_constraints(
            "CREATE TABLE people (id INT PRIMARY KEY, email TEXT UNIQUE, name TEXT NOT NULL, age INT CHECK (age >= 0))",
        );
        assert!(cons.iter().any(|c| c.kind == ConstraintKind::PrimaryKey && c.column == "id"));
        assert!(cons.iter().any(|c| c.kind == ConstraintKind::Unique && c.column == "email"));
        assert!(cons.iter().any(|c| c.kind == ConstraintKind::NotNull && c.column == "name"));
        let check = cons.iter().find(|c| c.kind == ConstraintKind::Check).expect("check");
        assert_eq!(check.column, "age");
        assert_eq!(check.check_expr.as_deref(), Some("age >= 0"));
    }

    #[test]
    fn parse_create_table_inline_references() {
        let cons = parse_create_table_constraints(
            "CREATE TABLE orders (id INT PRIMARY KEY, customer_id INT REFERENCES customers(id))",
        );
        let fk = cons.iter().find(|c| c.kind == ConstraintKind::ForeignKey).expect("fk");
        assert_eq!(fk.column, "customer_id");
        assert_eq!(fk.ref_table.as_deref(), Some("customers"));
        assert_eq!(fk.ref_column.as_deref(), Some("id"));
    }

    #[test]
    fn parse_table_level_foreign_key() {
        let cons = parse_create_table_constraints(
            "CREATE TABLE orders (id INT, customer_id INT, CONSTRAINT fk1 FOREIGN KEY (customer_id) REFERENCES customers(id))",
        );
        let fk = cons.iter().find(|c| c.kind == ConstraintKind::ForeignKey).expect("fk");
        assert_eq!(fk.name, "fk1");
        assert_eq!(fk.column, "customer_id");
        assert_eq!(fk.ref_table.as_deref(), Some("customers"));
    }

    #[test]
    fn parse_alter_add_constraint_forms() {
        let u = parse_alter_add_constraint("ALTER TABLE t ADD CONSTRAINT uq UNIQUE (email)").expect("unique");
        assert_eq!(u.kind, ConstraintKind::Unique);
        assert_eq!(u.name, "uq");
        assert_eq!(u.column, "email");

        let c = parse_alter_add_constraint("ALTER TABLE t ADD CHECK (age >= 18)").expect("check");
        assert_eq!(c.kind, ConstraintKind::Check);
        assert_eq!(c.check_expr.as_deref(), Some("age >= 18"));

        let fk = parse_alter_add_constraint(
            "ALTER TABLE orders ADD CONSTRAINT fk2 FOREIGN KEY (customer_id) REFERENCES customers(id)",
        )
        .expect("fk");
        assert_eq!(fk.kind, ConstraintKind::ForeignKey);
        assert_eq!(fk.ref_table.as_deref(), Some("customers"));
    }

    fn pk_descriptor(name: &str) -> ConstraintDescriptor {
        ConstraintDescriptor {
            name: name.to_string(),
            table: "users".to_string(),
            column: "id".to_string(),
            kind: ConstraintKind::PrimaryKey,
            ref_table: None,
            ref_column: None,
        }
    }

    fn unique_descriptor(name: &str) -> ConstraintDescriptor {
        ConstraintDescriptor {
            name: name.to_string(),
            table: "users".to_string(),
            column: "email".to_string(),
            kind: ConstraintKind::Unique,
            ref_table: None,
            ref_column: None,
        }
    }

    fn not_null_descriptor(name: &str) -> ConstraintDescriptor {
        ConstraintDescriptor {
            name: name.to_string(),
            table: "users".to_string(),
            column: "name".to_string(),
            kind: ConstraintKind::NotNull,
            ref_table: None,
            ref_column: None,
        }
    }

    #[test]
    fn primary_key_rejects_duplicate() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(pk_descriptor("pk_users")).unwrap();
        mgr.validate("users", "id", Some("1")).unwrap();
        mgr.record_committed_value("users", "id", "1");
        let err = mgr.validate("users", "id", Some("1")).unwrap_err();
        assert_eq!(
            err,
            ConstraintViolation::PrimaryKeyDuplicate {
                constraint: "pk_users".to_string(),
                value: "1".to_string()
            }
        );
    }

    #[test]
    fn primary_key_rejects_null() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(pk_descriptor("pk_users")).unwrap();
        let err = mgr.validate("users", "id", None).unwrap_err();
        assert!(matches!(err, ConstraintViolation::NotNullViolation { .. }));
    }

    #[test]
    fn unique_rejects_duplicate_but_allows_null() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(unique_descriptor("uq_email")).unwrap();
        mgr.validate("users", "email", Some("a@b.com")).unwrap();
        mgr.record_committed_value("users", "email", "a@b.com");

        let err = mgr.validate("users", "email", Some("a@b.com")).unwrap_err();
        assert!(matches!(err, ConstraintViolation::UniqueDuplicate { .. }));

        // NULL is allowed for UNIQUE
        mgr.validate("users", "email", None).unwrap();
    }

    #[test]
    fn not_null_rejects_absent_value() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(not_null_descriptor("nn_name")).unwrap();
        mgr.validate("users", "name", Some("Alice")).unwrap();
        let err = mgr.validate("users", "name", None).unwrap_err();
        assert!(matches!(err, ConstraintViolation::NotNullViolation { .. }));
    }

    #[test]
    fn remove_committed_value_allows_reuse() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(pk_descriptor("pk_users")).unwrap();
        mgr.validate("users", "id", Some("42")).unwrap();
        mgr.record_committed_value("users", "id", "42");

        mgr.remove_committed_value("users", "id", "42");
        // Now the value should be accepted again
        mgr.validate("users", "id", Some("42")).unwrap();
    }

    #[test]
    fn constraint_lifecycle_add_and_drop() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(pk_descriptor("pk_users")).unwrap();
        mgr.add_constraint(not_null_descriptor("nn_name")).unwrap();
        assert_eq!(mgr.constraint_count(), 2);

        let dropped = mgr.drop_constraint("pk_users").unwrap();
        assert_eq!(dropped.kind, ConstraintKind::PrimaryKey);
        assert_eq!(mgr.constraint_count(), 1);
    }

    #[test]
    fn duplicate_constraint_name_rejected() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(pk_descriptor("pk_users")).unwrap();
        let err = mgr.add_constraint(pk_descriptor("pk_users")).unwrap_err();
        assert!(matches!(err, ConstraintViolation::ConstraintAlreadyExists(_)));
    }
}
