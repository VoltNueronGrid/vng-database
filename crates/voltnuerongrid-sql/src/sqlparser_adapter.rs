//! Phase 1.6 — `sqlparser-rs` adapter (sqlparser v0.51).
//!
//! Replaces the substring-based heuristics in [`crate::ast::parse_one`] with a
//! real, structural parse. For any SELECT / transaction-control statement that
//! parses cleanly, this adapter's output is used and the legacy path is skipped.
//! Everything else (INSERT / UPDATE / DELETE / DDL) falls back to the legacy
//! parser until Phase 1.7 widens this adapter.
//!
//! See `gaps-may26-1.md` §3.3 §3.4.

#![cfg(feature = "sqlparser-adapter")]

use sqlparser::ast as sa;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

use crate::ast::{JoinClause, OrderByClause, SelectStatement, Statement};

/// Parse `sql` using `sqlparser-rs`. Returns `Some(stmt)` on success, `None`
/// for statements not yet adapted (caller should fall back to the legacy parser).
pub fn parse_with_sqlparser(sql: &str) -> Option<Statement> {
    let dialect = GenericDialect {};
    let parsed = Parser::parse_sql(&dialect, sql).ok()?;
    let first = parsed.into_iter().next()?;

    match first {
        sa::Statement::Query(query) => Some(adapt_query(*query, sql)),
        sa::Statement::StartTransaction { .. } => Some(Statement::Begin),
        sa::Statement::Commit { .. } => Some(Statement::Commit),
        sa::Statement::Rollback { .. } => Some(Statement::Rollback),
        // INSERT / UPDATE / DELETE / DDL — defer to legacy parser until Phase 1.7.
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Query → SelectStatement
// ─────────────────────────────────────────────────────────────────────────────

fn adapt_query(query: sa::Query, raw_sql: &str) -> Statement {
    let mut out = SelectStatement::default();

    // Drill through UNION / INTERSECT / EXCEPT to find the leaf SELECT.
    let leaf = leaf_select(&query.body, &mut out);

    if let Some(select) = leaf {
        // Projection columns.
        out.columns = select.projection.iter().map(projection_str).collect();
        out.has_column_alias = select
            .projection
            .iter()
            .any(|p| matches!(p, sa::SelectItem::ExprWithAlias { .. }));

        // DISTINCT.
        out.is_distinct = select.distinct.is_some();

        // FROM — first table + first JOIN.
        if let Some(twj) = select.from.first() {
            out.table = table_name(&twj.relation);
            out.has_table_alias =
                matches!(&twj.relation, sa::TableFactor::Table { alias: Some(_), .. });
            if let Some(j) = twj.joins.first() {
                out.join = Some(adapt_join(j));
            }
        }

        // WHERE.
        if let Some(w) = &select.selection {
            out.where_clause = Some(w.to_string());
            walk_predicate_flags(w, &mut out);
        }

        // Projection — walk for predicate flags (CASE / CAST / COALESCE /
        // string/math/date functions may appear in the SELECT list, not only WHERE).
        for item in &select.projection {
            match item {
                sa::SelectItem::UnnamedExpr(e)
                | sa::SelectItem::ExprWithAlias { expr: e, .. } => {
                    walk_predicate_flags(e, &mut out);
                }
                _ => {}
            }
        }

        // GROUP BY.
        match &select.group_by {
            sa::GroupByExpr::All(_) => {
                out.has_group_by = true;
            }
            sa::GroupByExpr::Expressions(exprs, _) if !exprs.is_empty() => {
                out.has_group_by = true;
                out.group_by = exprs.iter().map(|e| e.to_string()).collect();
            }
            _ => {}
        }

        // HAVING.
        if let Some(h) = &select.having {
            out.has_having = true;
            out.having = Some(h.to_string());
        }

        // Aggregate + window detection (walk projection + HAVING).
        let mut agg = AggState::default();
        for item in &select.projection {
            match item {
                sa::SelectItem::UnnamedExpr(e)
                | sa::SelectItem::ExprWithAlias { expr: e, .. } => walk_agg(e, &mut agg),
                _ => {}
            }
        }
        if let Some(h) = &select.having {
            walk_agg(h, &mut agg);
        }
        out.has_agg_fn = agg.any_agg;
        out.has_aggregate_distinct = agg.distinct;
        out.has_window_fn = agg.window;

        // Subquery detection.
        out.has_subquery = has_subquery_in_select(select);
    }

    // ORDER BY (lives on Query, not Select).
    if let Some(ob) = &query.order_by {
        if !ob.exprs.is_empty() {
            out.has_order_by = true;
            out.order_by = ob
                .exprs
                .iter()
                .map(|o| OrderByClause {
                    column: o.expr.to_string(),
                    descending: o.asc.map(|a| !a).unwrap_or(false),
                })
                .collect();
        }
    }

    // LIMIT / OFFSET.
    if let Some(lim) = &query.limit {
        out.limit = expr_u64(lim);
    }
    if let Some(off) = &query.offset {
        out.offset = expr_u64(&off.value);
    }

    // Backfill extended flags using the legacy substring heuristics. The
    // structural adapter above handles the *correctness-critical* flags
    // (GROUP BY, ORDER BY, aggregate detection, LIKE/BETWEEN/IN etc.) without
    // false positives from string literals and comments. The extended flags
    // below (join kinds, advanced ORDER BY attributes, window specifics, etc.)
    // are still populated by raw-text scan because they are routing hints, not
    // execution correctness gates, and the false-positive risk is acceptable
    // until Phase 1.7 widens the structural coverage.
    //
    // Flags already set structurally above are guarded so the substring scan
    // can only *set* them, not unset them.
    backfill_extended_flags_from_raw(raw_sql, &mut out);

    Statement::Select(out)
}

/// Walk a `SetExpr` tree, recording UNION/UNION ALL along the way and
/// returning the leaf `Select` (first branch of a set operation).
fn leaf_select<'a>(body: &'a sa::SetExpr, out: &mut SelectStatement) -> Option<&'a sa::Select> {
    match body {
        sa::SetExpr::Select(s) => Some(s),
        sa::SetExpr::SetOperation {
            op,
            set_quantifier,
            left,
            ..
        } => {
            if matches!(op, sa::SetOperator::Union) {
                out.has_union = true;
                if matches!(set_quantifier, sa::SetQuantifier::All) {
                    out.has_union_all = true;
                }
            }
            leaf_select(left, out)
        }
        sa::SetExpr::Query(q) => leaf_select(&q.body, out),
        _ => None,
    }
}

fn projection_str(p: &sa::SelectItem) -> String {
    match p {
        sa::SelectItem::Wildcard(_) => "*".to_string(),
        sa::SelectItem::QualifiedWildcard(n, _) => format!("{n}.*"),
        sa::SelectItem::UnnamedExpr(e) => e.to_string(),
        sa::SelectItem::ExprWithAlias { expr, alias } => format!("{expr} AS {alias}"),
    }
}

fn table_name(rel: &sa::TableFactor) -> Option<String> {
    match rel {
        sa::TableFactor::Table { name, .. } => Some(name.to_string()),
        _ => None,
    }
}

fn adapt_join(j: &sa::Join) -> JoinClause {
    let join_table = table_name(&j.relation).unwrap_or_default();
    let on_condition = match &j.join_operator {
        sa::JoinOperator::Inner(c)
        | sa::JoinOperator::LeftOuter(c)
        | sa::JoinOperator::RightOuter(c)
        | sa::JoinOperator::FullOuter(c) => join_constraint_str(c),
        _ => None,
    };
    JoinClause { join_table, on_condition }
}

fn join_constraint_str(c: &sa::JoinConstraint) -> Option<String> {
    match c {
        sa::JoinConstraint::On(e) => Some(e.to_string()),
        sa::JoinConstraint::Using(cols) => {
            let s: Vec<String> = cols.iter().map(|i| i.to_string()).collect();
            Some(format!("USING ({})", s.join(", ")))
        }
        _ => None,
    }
}

fn expr_u64(e: &sa::Expr) -> Option<u64> {
    if let sa::Expr::Value(sa::Value::Number(n, _)) = e {
        n.parse().ok()
    } else {
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Predicate flag walker — sets has_between / has_like / has_in_list / …
// ─────────────────────────────────────────────────────────────────────────────

fn backfill_extended_flags_from_raw(sql: &str, out: &mut SelectStatement) {
    // Backfill extended routing-hint flags using raw-text heuristics.
    // These are OR'd in: structural analysis already handled the correctness-
    // critical flags (GROUP BY, ORDER BY, aggregate, predicate types).
    // This handles join kinds, advanced ORDER BY, window details, etc.
    // Delete this function in Phase 1.7 once the adapter covers all flags.
    let up = sql.to_ascii_uppercase();

    if up.contains("INNER JOIN") { out.has_inner_join = true; }
    if up.contains("LEFT JOIN") || up.contains("LEFT OUTER JOIN") { out.has_left_join = true; }
    if up.contains("RIGHT JOIN") || up.contains("RIGHT OUTER JOIN") { out.has_right_join = true; }
    if up.contains("FULL JOIN") || up.contains("FULL OUTER JOIN") { out.has_full_outer_join = true; }
    if up.contains("CROSS JOIN") { out.has_cross_join = true; }
    if up.contains("LEFT SEMI JOIN") { out.has_left_semi_join = true; }
    if up.contains("RIGHT SEMI JOIN") { out.has_right_semi_join = true; }
    if up.contains("LEFT ANTI JOIN") { out.has_left_anti_join = true; }
    if up.contains("RIGHT ANTI JOIN") { out.has_right_anti_join = true; }
    if up.contains("CROSS APPLY") || up.contains("OUTER APPLY") { out.has_apply = true; }
    if up.contains("CROSS APPLY") { out.has_cross_apply = true; }
    if up.contains("OUTER APPLY") { out.has_outer_apply = true; }
    if up.contains(" EXCEPT ") || up.contains("EXCEPT ALL") { out.has_except = true; }
    if up.contains(" INTERSECT ") || up.contains("INTERSECT ALL") { out.has_intersect = true; }
    if up.contains("CUBE") && up.contains("GROUP BY") { out.has_group_by_cube = true; }
    if up.contains("ROLLUP") && up.contains("GROUP BY") { out.has_group_by_rollup = true; }
    if up.contains("GROUPING SETS") { out.has_grouping_sets = true; }
    if out.has_having && out.has_group_by { out.has_having_with_group_by = true; }
    if out.has_having && !out.has_group_by { out.has_having_without_group_by = true; }
    if up.contains("FOR UPDATE") || up.contains("FOR SHARE") { out.has_for_update = true; }
    if up.contains("FETCH FIRST") || up.contains("FETCH NEXT") { out.has_fetch = true; }
    if out.limit.is_some() && out.offset.is_some() { out.has_limit_offset_pagination = true; }
    if out.offset.is_some() && out.limit.is_none() { out.has_offset_only_pagination = true; }
    if !out.order_by.is_empty() {
        if out.order_by.iter().any(|o| !o.descending) { out.has_order_by_asc_direction = true; }
        if out.order_by.iter().any(|o| o.descending) { out.has_order_by_desc_direction = true; }
        if out.order_by.len() > 1 { out.has_order_by_multi_column = true; }
        if up.contains("NULLS FIRST") || up.contains("NULLS LAST") { out.has_nulls_ordering = true; }
        if up.contains("COLLATE") { out.has_order_by_collation = true; }
        let ob_part = up.find("ORDER BY").map(|p| &up[p..]).unwrap_or("");
        // Positional: check the structural order_by list — sqlparser renders
        // positional references as plain numbers in the column string.
        let is_positional = out.order_by.iter().any(|o| o.column.trim().parse::<u64>().is_ok());
        if is_positional { out.has_order_by_positional = true; }
        let nt = up.replace(' ', "");
        if nt.contains("RAND()") || nt.contains("RANDOM()") { out.has_order_by_random = true; }
        if nt.contains("RANDOM(") && !nt.contains("RANDOM()") { out.has_order_by_random_seeded = true; }
        if up.contains("ORDER BY") && up.contains("CASE") { out.has_order_by_case_expression = true; }
        // ORDER BY with expression (function call, arithmetic, CASE, etc.).
        // Detect by checking if any order_by column string isn't a simple identifier/number.
        let has_ob_expr = out.order_by.iter().any(|o| {
            let c = o.column.trim();
            let is_simple = c.chars().all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '.');
            let is_number = c.parse::<f64>().is_ok();
            !is_simple && !is_number
        });
        if has_ob_expr { out.has_order_by_expression = true; }

        // ORDER BY with a function call (has parentheses).
        let has_ob_fn = out.order_by.iter().any(|o| o.column.contains('('));
        if has_ob_fn { out.has_order_by_function_expression = true; }

        // ORDER BY RAND() (MySQL-style alias for random) — but NOT RANDOM().
        let rand_col = out.order_by.iter().any(|o| {
            let c = o.column.trim().to_ascii_uppercase();
            c == "RAND()" || c.starts_with("RAND()")
        });
        if rand_col { out.has_order_by_rand_alias = true; }
    }
    if up.contains("PARTITION BY") { out.has_window_partition = true; }
    if out.has_window_fn { out.has_window_agg = true; }
    if up.contains("->") || up.contains("->>") { out.has_json_op = true; }
    if up.contains("MATCH (") || up.contains("MATCH(") || up.contains("TSVECTOR") {
        out.has_full_text_search = true;
    }
    if up.contains(" REGEXP ") || up.contains(" RLIKE ") { out.has_regexp = true; }
    if up.contains("DISTINCT ON") { out.has_select_distinct_on = true; }
    if up.contains("INTERVAL") { out.has_interval = true; }

    // NOT IN / NOT EXISTS / IN subquery.
    if up.contains("NOT IN") { out.has_not_in = true; }
    if up.contains("NOT EXISTS") { out.has_not_exists = true; }
    if up.contains("IN (SELECT") || up.contains("IN(SELECT") { out.has_in_subquery = true; }

    // IS NULL / IS NOT NULL (the structural walker sets has_null_literal; legacy tests use has_is_null).
    if up.contains("IS NULL") || up.contains("IS NOT NULL") { out.has_is_null = true; }

    // LATERAL join/subquery.
    if up.contains("LATERAL") { out.has_lateral = true; }

    // NATURAL JOIN.
    if up.contains("NATURAL JOIN") { out.has_natural_join = true; }

    // USING join.
    if up.contains("USING (") || up.contains("USING(") { out.has_using_join = true; }

    // Named window (WINDOW <name> AS ...).
    if up.contains("WINDOW ") && up.contains(" AS (") { out.has_named_window = true; }

    // Window frame (ROWS BETWEEN / RANGE BETWEEN / RANGE UNBOUNDED).
    if up.contains("ROWS BETWEEN") || up.contains("RANGE BETWEEN") || up.contains("RANGE UNBOUNDED")
        || up.contains("ROWS UNBOUNDED")
    {
        out.has_window_frame = true;
    }

    // Window ORDER (only inside OVER clause, not outer query ORDER BY).
    // Set only if there's an OVER clause with ORDER BY but WITHOUT PARTITION BY.
    // If PARTITION BY is present the window is classified as has_window_partition.
    if (up.contains("OVER (") || up.contains("OVER(") || up.contains("WINDOW ")) && up.contains("ORDER BY") {
        if !up.contains("PARTITION BY") {
            out.has_window_order = true;
        }
    }

    // PIVOT / UNPIVOT.
    if up.contains("PIVOT") || up.contains("UNPIVOT") { out.has_pivot = true; }

    // QUALIFY clause (Snowflake/BigQuery extension).
    if up.contains("QUALIFY") { out.has_qualify = true; }

    // RECURSIVE CTE.
    if up.contains("WITH RECURSIVE") { out.has_recursive_cte = true; }

    // WITH (CTE).
    if up.trim_start().starts_with("WITH ") || up.trim_start().starts_with("WITH\n") {
        out.has_with_cte = true;
    }

    // TRIM function (separate from generic string fn).
    if up.contains("TRIM(") || up.contains("LTRIM(") || up.contains("RTRIM(") { out.has_trim = true; }

    // VALUES clause.
    if up.contains(" VALUES ") || up.contains("(VALUES") || up.contains("VALUES(") {
        out.has_values = true;
    }

    // JSON EXTRACT form: json_col -> 'key'.
    if up.contains("EXTRACT(") { out.has_json_op = true; }
}

fn walk_predicate_flags(expr: &sa::Expr, out: &mut SelectStatement) {
    use sa::Expr::*;
    match expr {
        Between { negated, low, high, expr: e } => {
            out.has_between = true;
            if *negated { out.has_not = true; }
            walk_predicate_flags(e, out);
            walk_predicate_flags(low, out);
            walk_predicate_flags(high, out);
        }
        Like { negated, expr: e, pattern, .. } | ILike { negated, expr: e, pattern, .. } => {
            out.has_like = true;
            if *negated { out.has_not = true; }
            walk_predicate_flags(e, out);
            walk_predicate_flags(pattern, out);
        }
        InList { expr: e, list, negated } => {
            out.has_in_list = true;
            if *negated { out.has_not = true; }
            walk_predicate_flags(e, out);
            for i in list { walk_predicate_flags(i, out); }
        }
        InSubquery { expr: e, negated, .. } => {
            out.has_in_list = true;
            out.has_subquery = true;
            if *negated { out.has_not = true; }
            walk_predicate_flags(e, out);
        }
        IsNull(e) | IsNotNull(e) | IsDistinctFrom(e, _) | IsNotDistinctFrom(e, _) => {
            out.has_null_literal = true;
            walk_predicate_flags(e, out);
        }
        UnaryOp { op: sa::UnaryOperator::Not, expr: e } => {
            out.has_not = true;
            walk_predicate_flags(e, out);
        }
        BinaryOp { left, right, op } => {
            if matches!(op, sa::BinaryOperator::StringConcat) {
                out.has_concat = true;
            }
            walk_predicate_flags(left, out);
            walk_predicate_flags(right, out);
        }
        Case { conditions, results, else_result, .. } => {
            out.has_case = true;
            for c in conditions { walk_predicate_flags(c, out); }
            for r in results { walk_predicate_flags(r, out); }
            if let Some(e) = else_result { walk_predicate_flags(e, out); }
        }
        Exists { negated, .. } => {
            out.has_exists = true;
            out.has_subquery = true;
            if *negated { out.has_not = true; }
        }
        // Cast: all cast kinds share one variant in v0.51
        Cast { expr: e, .. } => {
            out.has_cast = true;
            walk_predicate_flags(e, out);
        }
        Function(f) => {
            let name = f.name.to_string().to_ascii_uppercase();
            check_fn_kind(&name, out);
            walk_fn_args_predicate(f, out);
        }
        Subquery(_) => { out.has_subquery = true; }
        _ => {}
    }
}

fn check_fn_kind(name: &str, out: &mut SelectStatement) {
    match name {
        "LENGTH" | "UPPER" | "LOWER" | "SUBSTR" | "SUBSTRING" | "TRIM"
        | "REPLACE" | "LTRIM" | "RTRIM" | "POSITION" => out.has_string_fn = true,
        "NOW" | "DATE_TRUNC" | "EXTRACT" | "CURRENT_DATE" | "CURRENT_TIMESTAMP"
        | "DATE" | "TIME" => out.has_date_fn = true,
        "ABS" | "ROUND" | "CEIL" | "CEILING" | "FLOOR" | "POWER" | "SQRT"
        | "MOD" | "EXP" | "LN" | "LOG" => out.has_math_fn = true,
        "CONCAT" => out.has_concat = true,
        "COALESCE" => out.has_coalesce = true,
        "NULLIF" => out.has_nullif = true,
        _ => {}
    }
}

fn walk_fn_args_predicate(f: &sa::Function, out: &mut SelectStatement) {
    if let sa::FunctionArguments::List(list) = &f.args {
        for arg in &list.args {
            if let sa::FunctionArg::Unnamed(sa::FunctionArgExpr::Expr(e)) = arg {
                walk_predicate_flags(e, out);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Aggregate + window walker
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct AggState {
    any_agg: bool,
    distinct: bool,
    window: bool,
}

fn walk_agg(expr: &sa::Expr, s: &mut AggState) {
    use sa::Expr::*;
    match expr {
        Function(f) => {
            let name = f.name.to_string().to_ascii_uppercase();
            if matches!(name.as_str(), "COUNT" | "SUM" | "AVG" | "MIN" | "MAX") {
                s.any_agg = true;
                // DISTINCT is on FunctionArgumentList.duplicate_treatment
                if let sa::FunctionArguments::List(list) = &f.args {
                    if matches!(
                        list.duplicate_treatment,
                        Some(sa::DuplicateTreatment::Distinct)
                    ) {
                        s.distinct = true;
                    }
                }
            }
            if f.over.is_some() {
                s.window = true;
            }
            if let sa::FunctionArguments::List(list) = &f.args {
                for arg in &list.args {
                    if let sa::FunctionArg::Unnamed(sa::FunctionArgExpr::Expr(e)) = arg {
                        walk_agg(e, s);
                    }
                }
            }
        }
        BinaryOp { left, right, .. } => { walk_agg(left, s); walk_agg(right, s); }
        UnaryOp { expr: e, .. } | IsNull(e) | IsNotNull(e) | Cast { expr: e, .. } => {
            walk_agg(e, s);
        }
        Case { conditions, results, else_result, .. } => {
            for c in conditions { walk_agg(c, s); }
            for r in results { walk_agg(r, s); }
            if let Some(e) = else_result { walk_agg(e, s); }
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Subquery detector
// ─────────────────────────────────────────────────────────────────────────────

fn has_subquery_in_select(sel: &sa::Select) -> bool {
    // Check projection.
    for item in &sel.projection {
        match item {
            sa::SelectItem::UnnamedExpr(e) | sa::SelectItem::ExprWithAlias { expr: e, .. } => {
                if expr_has_subquery(e) { return true; }
            }
            _ => {}
        }
    }
    // Check WHERE.
    if let Some(w) = &sel.selection {
        if expr_has_subquery(w) { return true; }
    }
    // Check derived tables in FROM.
    for twj in &sel.from {
        if matches!(&twj.relation, sa::TableFactor::Derived { .. }) {
            return true;
        }
    }
    false
}

fn expr_has_subquery(expr: &sa::Expr) -> bool {
    use sa::Expr::*;
    match expr {
        Subquery(_) | Exists { .. } | InSubquery { .. } => true,
        BinaryOp { left, right, .. } => expr_has_subquery(left) || expr_has_subquery(right),
        UnaryOp { expr: e, .. } | IsNull(e) | IsNotNull(e) | Cast { expr: e, .. } => {
            expr_has_subquery(e)
        }
        Between { expr: e, low, high, .. } => {
            expr_has_subquery(e) || expr_has_subquery(low) || expr_has_subquery(high)
        }
        InList { expr: e, list, .. } => {
            expr_has_subquery(e) || list.iter().any(expr_has_subquery)
        }
        Case { conditions, results, else_result, .. } => {
            conditions.iter().any(expr_has_subquery)
                || results.iter().any(expr_has_subquery)
                || else_result.as_deref().map(expr_has_subquery).unwrap_or(false)
        }
        Function(f) => {
            if let sa::FunctionArguments::List(list) = &f.args {
                list.args.iter().any(|a| {
                    if let sa::FunctionArg::Unnamed(sa::FunctionArgExpr::Expr(e)) = a {
                        expr_has_subquery(e)
                    } else {
                        false
                    }
                })
            } else {
                false
            }
        }
        _ => false,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_select(sql: &str) -> SelectStatement {
        match parse_with_sqlparser(sql) {
            Some(Statement::Select(s)) => s,
            other => panic!("expected SELECT statement, got: {:?}", other),
        }
    }

    // ── Regression: false-positive flags from substring matching ────────────

    #[test]
    fn group_by_in_string_literal_does_not_set_flag() {
        let stmt = parse_select("SELECT 'GROUP BY' FROM t");
        assert!(!stmt.has_group_by, "literal 'GROUP BY' must not set has_group_by");
    }

    #[test]
    fn group_by_in_comment_does_not_set_flag() {
        let stmt = parse_select("SELECT a /* GROUP BY ignored */ FROM t");
        assert!(!stmt.has_group_by);
    }

    #[test]
    fn aggregate_in_string_literal_does_not_set_flag() {
        let stmt = parse_select("SELECT 'COUNT(*) is informative' FROM t");
        assert!(!stmt.has_agg_fn);
    }

    #[test]
    fn order_by_in_string_does_not_set_flag() {
        let stmt = parse_select("SELECT 'ORDER BY id' AS note FROM t");
        assert!(!stmt.has_order_by);
    }

    // ── True positives ──────────────────────────────────────────────────────

    #[test]
    fn real_group_by_sets_flag() {
        let stmt = parse_select("SELECT region, COUNT(*) FROM sales GROUP BY region");
        assert!(stmt.has_group_by);
        assert_eq!(stmt.group_by, vec!["region".to_string()]);
        assert!(stmt.has_agg_fn);
    }

    #[test]
    fn count_star_without_group_by() {
        let stmt = parse_select("SELECT COUNT(*) FROM sales");
        assert!(stmt.has_agg_fn);
        assert!(!stmt.has_group_by);
    }

    #[test]
    fn count_distinct_sets_flag() {
        let stmt = parse_select("SELECT COUNT(DISTINCT region) FROM sales");
        assert!(stmt.has_aggregate_distinct);
    }

    #[test]
    fn order_by_descending() {
        let stmt = parse_select("SELECT * FROM t ORDER BY id DESC");
        assert!(stmt.has_order_by);
        assert_eq!(stmt.order_by.len(), 1);
        assert_eq!(stmt.order_by[0].column, "id");
        assert!(stmt.order_by[0].descending);
    }

    #[test]
    fn having_sets_flag() {
        let stmt = parse_select(
            "SELECT region, COUNT(*) FROM sales GROUP BY region HAVING COUNT(*) > 10",
        );
        assert!(stmt.has_having);
        assert!(stmt.has_group_by);
    }

    #[test]
    fn limit_and_offset() {
        let stmt = parse_select("SELECT * FROM t LIMIT 10 OFFSET 20");
        assert_eq!(stmt.limit, Some(10));
        assert_eq!(stmt.offset, Some(20));
    }

    #[test]
    fn select_distinct() {
        let stmt = parse_select("SELECT DISTINCT region FROM sales");
        assert!(stmt.is_distinct);
    }

    #[test]
    fn between_predicate() {
        let stmt = parse_select("SELECT * FROM t WHERE x BETWEEN 1 AND 10");
        assert!(stmt.has_between);
    }

    #[test]
    fn like_predicate() {
        let stmt = parse_select("SELECT * FROM t WHERE name LIKE 'A%'");
        assert!(stmt.has_like);
    }

    #[test]
    fn in_list_predicate() {
        let stmt = parse_select("SELECT * FROM t WHERE id IN (1, 2, 3)");
        assert!(stmt.has_in_list);
    }

    #[test]
    fn is_null_predicate() {
        let stmt = parse_select("SELECT * FROM t WHERE x IS NULL");
        assert!(stmt.has_null_literal);
    }

    #[test]
    fn not_predicate() {
        let stmt = parse_select("SELECT * FROM t WHERE NOT (x > 10)");
        assert!(stmt.has_not);
    }

    #[test]
    fn case_expression() {
        let stmt = parse_select("SELECT CASE WHEN x > 0 THEN 'p' ELSE 'n' END FROM t");
        assert!(stmt.has_case);
    }

    #[test]
    fn coalesce_function() {
        let stmt = parse_select("SELECT COALESCE(a, b) FROM t");
        assert!(stmt.has_coalesce);
    }

    #[test]
    fn cast_expression() {
        let stmt = parse_select("SELECT CAST(x AS INTEGER) FROM t");
        assert!(stmt.has_cast);
    }

    #[test]
    fn nullif_function() {
        let stmt = parse_select("SELECT NULLIF(x, 0) FROM t");
        assert!(stmt.has_nullif);
    }

    #[test]
    fn string_function() {
        let stmt = parse_select("SELECT LENGTH(name) FROM t");
        assert!(stmt.has_string_fn);
    }

    #[test]
    fn math_function() {
        let stmt = parse_select("SELECT ABS(x) FROM t");
        assert!(stmt.has_math_fn);
    }

    #[test]
    fn concat_pipe() {
        let stmt = parse_select("SELECT a || b FROM t");
        assert!(stmt.has_concat);
    }

    #[test]
    fn exists_subquery() {
        let stmt = parse_select(
            "SELECT * FROM t WHERE EXISTS (SELECT 1 FROM u WHERE u.tid = t.id)",
        );
        assert!(stmt.has_exists);
        assert!(stmt.has_subquery);
    }

    #[test]
    fn scalar_subquery_in_list() {
        let stmt = parse_select("SELECT id, (SELECT COUNT(*) FROM u) AS n FROM t");
        assert!(stmt.has_subquery);
    }

    #[test]
    fn inner_join_extracted() {
        let stmt = parse_select(
            "SELECT * FROM orders o JOIN customers c ON c.id = o.customer_id",
        );
        let join = stmt.join.unwrap();
        assert!(!join.join_table.is_empty());
        assert!(join.on_condition.is_some());
    }

    #[test]
    fn union_set_op() {
        let stmt = parse_select("SELECT a FROM t UNION SELECT a FROM u");
        assert!(stmt.has_union);
        assert!(!stmt.has_union_all);
    }

    #[test]
    fn union_all_set_op() {
        let stmt = parse_select("SELECT a FROM t UNION ALL SELECT a FROM u");
        assert!(stmt.has_union);
        assert!(stmt.has_union_all);
    }

    #[test]
    fn transaction_control() {
        assert!(matches!(parse_with_sqlparser("BEGIN"), Some(Statement::Begin)));
        assert!(matches!(parse_with_sqlparser("COMMIT"), Some(Statement::Commit)));
        assert!(matches!(parse_with_sqlparser("ROLLBACK"), Some(Statement::Rollback)));
    }

    #[test]
    fn insert_returns_none_for_legacy_fallback() {
        assert!(parse_with_sqlparser("INSERT INTO t (a) VALUES (1)").is_none());
    }

    #[test]
    fn malformed_returns_none() {
        assert!(parse_with_sqlparser("not sql @#$%").is_none());
        assert!(parse_with_sqlparser("").is_none());
    }
}
