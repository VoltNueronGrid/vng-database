//! SQL logical planner and cost estimator.
//!
//! Converts a parsed [`voltnuerongrid_sql::Statement`] AST into a [`LogicalPlan`]
//! tree, then produces a [`CostEstimate`] to drive HTAP path selection.
//!
//! Advances sprint backlog item S3-WS1-05 (planner/optimizer + cost model).

use voltnuerongrid_sql::{
    DeleteStatement, InsertStatement, JoinClause as SqlJoinClause, SelectStatement, Statement,
    UpdateStatement,
};

use crate::QueryPath;

// ─── Logical plan nodes ───────────────────────────────────────────────────────

/// A node in the logical query plan tree.
#[derive(Debug, Clone, PartialEq)]
pub enum LogicalPlan {
    /// Full or filtered table scan.
    Scan {
        table: String,
        /// Raw WHERE predicate text when present, drives selectivity.
        filter: Option<String>,
    },
    /// Column projection on top of an inner plan.
    Project {
        input: Box<LogicalPlan>,
        columns: Vec<String>,
    },
    /// Predicate filter node (split from Scan for clarity).
    Filter {
        input: Box<LogicalPlan>,
        predicate: String,
    },
    /// GROUP BY aggregation, optionally with HAVING.
    Aggregate {
        input: Box<LogicalPlan>,
        group_by: Vec<String>,
        having: Option<String>,
    },
    /// ORDER BY sort.
    Sort {
        input: Box<LogicalPlan>,
        /// `(column_name, is_descending)` pairs.
        order_by: Vec<(String, bool)>,
    },
    /// LIMIT operator.
    Limit {
        input: Box<LogicalPlan>,
        count: u64,
    },
    /// INSERT DML.
    Insert {
        table: String,
        columns: Vec<String>,
        /// Number of value rows to insert.
        value_count: usize,
    },
    /// UPDATE DML.
    Update {
        table: String,
        /// `(column, new_value_literal)` pairs.
        assignments: Vec<(String, String)>,
        filter: Option<String>,
    },
    /// DELETE DML.
    Delete {
        table: String,
        filter: Option<String>,
    },
    /// CREATE TABLE DDL.
    CreateTable {
        table: String,
        column_count: usize,
    },
    /// Transaction control statements.
    Begin,
    Commit,
    Rollback,
    /// JOIN of two tables (from S3-WS1-04 JoinClause support).
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        join_table: String,
        condition: Option<String>,
    },
    /// UNION / set-operation combining two result sets (from S3-WS1-04 has_union support).
    Union {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
    },
    /// IN-list predicate filter (from S3-WS1-07 has_in_list support).
    InList {
        input: Box<LogicalPlan>,
    },
    /// BETWEEN ... AND range predicate filter (from S3-WS1-08 has_between support).
    Between {
        input: Box<LogicalPlan>,
    },
    /// LIKE / ILIKE string pattern filter (from S3-WS1-09 has_like support).
    Like {
        input: Box<LogicalPlan>,
    },
    /// NOT keyword predicate filter (from S3-WS1-10 has_not support).
    Not {
        input: Box<LogicalPlan>,
    },
    /// CASE WHEN analytical expression (from S3-WS1-11 has_case support).
    Case {
        input: Box<LogicalPlan>,
    },
    /// COALESCE() null-coalescing expression (from S3-WS1-12 has_coalesce support).
    Coalesce {
        input: Box<LogicalPlan>,
    },
    /// CAST() / :: type-cast expression (from S3-WS1-13 has_cast support).
    Cast {
        input: Box<LogicalPlan>,
    },
    /// NULLIF() null-substitution expression (from S3-WS1-14 has_nullif support).
    Nullif {
        input: Box<LogicalPlan>,
    },
    /// String function expression (LENGTH/UPPER/LOWER/SUBSTR) (from S3-WS1-15 has_string_fn support).
    StringFn {
        input: Box<LogicalPlan>,
    },
    /// Date/time function expression (NOW/DATE_TRUNC/EXTRACT) (from S3-WS1-16 has_date_fn support).
    DateFn {
        input: Box<LogicalPlan>,
    },
    /// String concatenation expression (CONCAT/||) (from S3-WS1-17 has_concat support).
    Concat {
        input: Box<LogicalPlan>,
    },
    /// Math function expression (ABS/ROUND/CEIL/FLOOR) (from S3-WS1-18 has_math_fn support).
    MathFn {
        input: Box<LogicalPlan>,
    },
    /// EXISTS subquery predicate (from S3-WS1-19 has_exists support).
    Exists {
        input: Box<LogicalPlan>,
    },
    /// ANY/ALL quantifier expression (from S3-WS1-20 has_any_all support).
    AnyAll {
        input: Box<LogicalPlan>,
    },
    /// NOT IN predicate (anti-semi-join pattern) (from S3-WS1-21 has_not_in support).
    NotIn {
        input: Box<LogicalPlan>,
    },
    /// TRIM / LTRIM / RTRIM string function applied to result set (S3-WS1-22 has_trim support).
    Trim {
        input: Box<LogicalPlan>,
    },
    /// INTERVAL date arithmetic expression (S3-WS1-23 has_interval support).
    Interval {
        input: Box<LogicalPlan>,
    },
    /// IN (SELECT ...) subquery predicate (anti-join / semi-join pattern) (S3-WS1-24 has_in_subquery support).
    InSubquery {
        input: Box<LogicalPlan>,
    },
    /// IS NULL / IS NOT NULL predicate node (S3-WS1-25 has_is_null support).
    IsNull {
        input: Box<LogicalPlan>,
    },
    /// REGEXP / RLIKE / SIMILAR TO pattern-match node (S3-WS1-26 has_regexp support).
    Regexp {
        input: Box<LogicalPlan>,
    },
    /// JSON operator node (`->` / `->>` / `JSON_EXTRACT`) (S3-WS1-27 has_json_op support).
    JsonOp {
        input: Box<LogicalPlan>,
    },
    /// Window function applied to a result set (from S3-WS1-04 has_window_fn support).
    WindowFn {
        input: Box<LogicalPlan>,
        /// The window function expression indicator (scaffold: always "OVER").
        window_func: String,
    },
    /// Deduplication of result rows via SELECT DISTINCT (S3-WS1-04 is_distinct support).
    Distinct {
        input: Box<LogicalPlan>,
    },
    /// Pagination skip-N rows (S3-WS1-06 offset support).
    Offset {
        input: Box<LogicalPlan>,
        offset: u64,
    },
    /// Post-aggregate HAVING filter (S3-WS1-06 has_group_by support).
    Having {
        input: Box<LogicalPlan>,
        condition: String,
    },
    /// Combined Sort+Limit optimisation for ORDER BY … LIMIT queries (S3-WS1-05).
    TopN {
        input: Box<LogicalPlan>,
        count: u64,
        order_by: String,
    },
    /// Correlated or scalar subquery wrapper (S3-WS1-04 has_subquery support).
    Subquery {
        input: Box<LogicalPlan>,
    },
    /// Unrecognised or unparseable statement.
    Unknown(String),
    /// Window aggregate function node (COUNT/SUM/AVG/ROW_NUMBER OVER ...) (S3-WS1-28 has_window_agg support).
    WindowAgg {
        input: Box<LogicalPlan>,
    },
    /// LATERAL join or LATERAL subquery (S3-WS1-29 has_lateral support).
    Lateral {
        input: Box<LogicalPlan>,
    },
    /// PIVOT or UNPIVOT clause for cross-tabulation (S3-WS1-30 has_pivot support).
    Pivot {
        input: Box<LogicalPlan>,
    },
    /// FETCH NEXT/FIRST pagination clause (S3-WS1-31 has_fetch support).
    Fetch {
        input: Box<LogicalPlan>,
    },
    /// VALUES clause used as a row source / VALUES CTE (S3-WS1-32 has_values support).
    Values {
        input: Box<LogicalPlan>,
    },
    /// CROSS JOIN expression between two relations (S3-WS1-33 has_cross_join support).
    CrossJoin {
        input: Box<LogicalPlan>,
    },
    /// Full-text search predicate (MATCH/AGAINST or @@) (S3-WS1-34 has_full_text_search support).
    FullTextSearch {
        input: Box<LogicalPlan>,
    },
    /// GROUPING SETS aggregate grouping strategy (S3-WS1-35 has_grouping_sets support).
    GroupingSets {
        input: Box<LogicalPlan>,
    },
    /// NATURAL JOIN clause semantics wrapper (S3-WS1-36 has_natural_join support).
    NaturalJoin {
        input: Box<LogicalPlan>,
    },
    /// JOIN ... USING (...) clause semantics wrapper (S3-WS1-37 has_using_join support).
    UsingJoin {
        input: Box<LogicalPlan>,
    },
    /// EXCEPT set operation semantics wrapper (S3-WS1-38 has_except support).
    Except {
        input: Box<LogicalPlan>,
    },
    /// INTERSECT set operation semantics wrapper (S3-WS1-39 has_intersect support).
    Intersect {
        input: Box<LogicalPlan>,
    },
    /// QUALIFY clause semantics wrapper (S3-WS1-40 has_qualify support).
    Qualify {
        input: Box<LogicalPlan>,
    },
    /// WITH ... AS (...) CTE semantics wrapper (S3-WS1-41 has_with_cte support).
    WithCte {
        input: Box<LogicalPlan>,
    },
    /// WITH RECURSIVE ... AS (...) CTE semantics wrapper (S3-WS1-42 has_recursive_cte support).
    RecursiveCte {
        input: Box<LogicalPlan>,
    },
    /// NOT EXISTS subquery semantics wrapper (S3-WS1-43 has_not_exists support).
    NotExists {
        input: Box<LogicalPlan>,
    },
}

impl LogicalPlan {
    /// The primary table touched by this plan node (for routing hints).
    pub fn primary_table(&self) -> Option<&str> {
        match self {
            LogicalPlan::Scan { table, .. }
            | LogicalPlan::Insert { table, .. }
            | LogicalPlan::Update { table, .. }
            | LogicalPlan::Delete { table, .. }
            | LogicalPlan::CreateTable { table, .. } => Some(table.as_str()),
            LogicalPlan::Project { input, .. }
            | LogicalPlan::Filter { input, .. }
            | LogicalPlan::Aggregate { input, .. }
            | LogicalPlan::Sort { input, .. }
            | LogicalPlan::Limit { input, .. }
            | LogicalPlan::TopN { input, .. } => input.primary_table(),
            LogicalPlan::Join { left, .. } => left.primary_table(),
            LogicalPlan::Union { left, .. } => left.primary_table(),
            LogicalPlan::InList { input } => input.primary_table(),
            LogicalPlan::Between { input } => input.primary_table(),
            LogicalPlan::Like { input } => input.primary_table(),
            LogicalPlan::Not { input } => input.primary_table(),
            LogicalPlan::Case { input } => input.primary_table(),
            LogicalPlan::Coalesce { input } => input.primary_table(),
            LogicalPlan::Cast { input } => input.primary_table(),
            LogicalPlan::Nullif { input } => input.primary_table(),
            LogicalPlan::StringFn { input } => input.primary_table(),
            LogicalPlan::DateFn { input } => input.primary_table(),
            LogicalPlan::Concat { input } => input.primary_table(),
            LogicalPlan::MathFn { input } => input.primary_table(),
            LogicalPlan::Exists { input } => input.primary_table(),
            LogicalPlan::AnyAll { input } => input.primary_table(),
            LogicalPlan::NotIn { input } => input.primary_table(),
            LogicalPlan::Trim { input } => input.primary_table(),
            LogicalPlan::Interval { input } => input.primary_table(),
            LogicalPlan::InSubquery { input } => input.primary_table(),
            LogicalPlan::IsNull { input } => input.primary_table(),
            LogicalPlan::Regexp { input } => input.primary_table(),
            LogicalPlan::JsonOp { input } => input.primary_table(),
            LogicalPlan::WindowFn { input, .. } => input.primary_table(),
            LogicalPlan::WindowAgg { input } => input.primary_table(),
            LogicalPlan::Lateral { input } => input.primary_table(),
            LogicalPlan::Pivot { input } => input.primary_table(),
            LogicalPlan::Fetch { input } => input.primary_table(),
            LogicalPlan::Values { input } => input.primary_table(),
            LogicalPlan::CrossJoin { input } => input.primary_table(),
            LogicalPlan::FullTextSearch { input } => input.primary_table(),
            LogicalPlan::GroupingSets { input } => input.primary_table(),
            LogicalPlan::NaturalJoin { input } => input.primary_table(),
            LogicalPlan::UsingJoin { input } => input.primary_table(),
            LogicalPlan::Except { input } => input.primary_table(),
            LogicalPlan::Intersect { input } => input.primary_table(),
            LogicalPlan::Qualify { input } => input.primary_table(),
            LogicalPlan::WithCte { input } => input.primary_table(),
            LogicalPlan::RecursiveCte { input } => input.primary_table(),
            LogicalPlan::NotExists { input } => input.primary_table(),
            LogicalPlan::Distinct { input } => input.primary_table(),
            LogicalPlan::Offset { input, .. } => input.primary_table(),
            LogicalPlan::Having { input, .. } => input.primary_table(),
            LogicalPlan::Subquery { input } => input.primary_table(),
            _ => None,
        }
    }

    /// True when the plan is a read-only access pattern.
    pub fn is_read_only(&self) -> bool {
        !matches!(
            self,
            LogicalPlan::Insert { .. }
                | LogicalPlan::Update { .. }
                | LogicalPlan::Delete { .. }
                | LogicalPlan::CreateTable { .. }
                | LogicalPlan::Commit
        )
    }

    /// True when the plan contains aggregation (OLAP hint).
    pub fn has_aggregation(&self) -> bool {
        match self {
            LogicalPlan::Aggregate { .. } => true,
            LogicalPlan::Project { input, .. }
            | LogicalPlan::Filter { input, .. }
            | LogicalPlan::Sort { input, .. }
            | LogicalPlan::Limit { input, .. }
            | LogicalPlan::TopN { input, .. } => input.has_aggregation(),
            LogicalPlan::Join { left, right, .. } => {
                left.has_aggregation() || right.has_aggregation()
            }
            LogicalPlan::Union { left, right } => {
                left.has_aggregation() || right.has_aggregation()
            }
            LogicalPlan::InList { input } => input.has_aggregation(),
            LogicalPlan::Between { input } => input.has_aggregation(),
            LogicalPlan::Like { input } => input.has_aggregation(),
            LogicalPlan::Not { input } => input.has_aggregation(),
            LogicalPlan::Case { input } => input.has_aggregation(),
            LogicalPlan::Coalesce { input } => input.has_aggregation(),
            LogicalPlan::Cast { input } => input.has_aggregation(),
            LogicalPlan::Nullif { input } => input.has_aggregation(),
            LogicalPlan::StringFn { input } => input.has_aggregation(),
            LogicalPlan::DateFn { input } => input.has_aggregation(),
            LogicalPlan::Concat { input } => input.has_aggregation(),
            LogicalPlan::MathFn { input } => input.has_aggregation(),
            LogicalPlan::Exists { input } => input.has_aggregation(),
            LogicalPlan::AnyAll { input } => input.has_aggregation(),
            LogicalPlan::NotIn { input } => input.has_aggregation(),
            LogicalPlan::Trim { input } => input.has_aggregation(),
            LogicalPlan::Interval { input } => input.has_aggregation(),
            LogicalPlan::InSubquery { input } => input.has_aggregation(),
            LogicalPlan::IsNull { input } => input.has_aggregation(),
            LogicalPlan::Regexp { input } => input.has_aggregation(),
            LogicalPlan::JsonOp { input } => input.has_aggregation(),
            LogicalPlan::WindowFn { input, .. } => input.has_aggregation(),
            LogicalPlan::WindowAgg { .. } => true,
            LogicalPlan::Lateral { input } => input.has_aggregation(),
            LogicalPlan::Pivot { input } => input.has_aggregation(),
            LogicalPlan::Fetch { input } => input.has_aggregation(),
            LogicalPlan::Values { input } => input.has_aggregation(),
            LogicalPlan::CrossJoin { input } => input.has_aggregation(),
            LogicalPlan::FullTextSearch { input } => input.has_aggregation(),
            LogicalPlan::GroupingSets { input } => input.has_aggregation(),
            LogicalPlan::NaturalJoin { input } => input.has_aggregation(),
            LogicalPlan::UsingJoin { input } => input.has_aggregation(),
            LogicalPlan::Except { input } => input.has_aggregation(),
            LogicalPlan::Intersect { input } => input.has_aggregation(),
            LogicalPlan::Qualify { input } => input.has_aggregation(),
            LogicalPlan::WithCte { input } => input.has_aggregation(),
            LogicalPlan::RecursiveCte { input } => input.has_aggregation(),
            LogicalPlan::NotExists { input } => input.has_aggregation(),
            LogicalPlan::Distinct { input } => input.has_aggregation(),
            LogicalPlan::Offset { input, .. } => input.has_aggregation(),
            LogicalPlan::Having { input, .. } => input.has_aggregation(),
            LogicalPlan::Subquery { input } => input.has_aggregation(),
            _ => false,
        }
    }
}

// ─── Cost estimate ─────────────────────────────────────────────────────────────

/// A simple cost estimate produced by [`QueryPlanner::estimate_cost`].
#[derive(Debug, Clone, PartialEq)]
pub struct CostEstimate {
    /// Approximate output row count.
    pub estimated_rows: u64,
    /// Relative cost score: `0.0` = trivial, higher = more expensive.
    pub relative_cost: f64,
    /// Recommended HTAP execution path based on cost model.
    pub recommended_path: QueryPath,
}

// ─── Query planner ─────────────────────────────────────────────────────────────

/// Stateless planner — call [`QueryPlanner::plan`] then [`QueryPlanner::estimate_cost`].
pub struct QueryPlanner;

impl QueryPlanner {
    /// Convert an AST [`Statement`] into a [`LogicalPlan`].
    pub fn plan(stmt: &Statement) -> LogicalPlan {
        match stmt {
            Statement::Select(sel) => Self::plan_select(sel),
            Statement::Insert(ins) => Self::plan_insert(ins),
            Statement::Update(upd) => Self::plan_update(upd),
            Statement::Delete(del) => Self::plan_delete(del),
            Statement::CreateTable(ct) => LogicalPlan::CreateTable {
                table: ct.table.clone(),
                column_count: ct.columns.len(),
            },
            Statement::Begin => LogicalPlan::Begin,
            Statement::Commit => LogicalPlan::Commit,
            Statement::Rollback => LogicalPlan::Rollback,
            Statement::Unknown(s) => LogicalPlan::Unknown(s.clone()),
        }
    }

    /// Estimate the cost of executing a [`LogicalPlan`].
    pub fn estimate_cost(plan: &LogicalPlan) -> CostEstimate {
        match plan {
            LogicalPlan::Scan { filter, .. } => {
                let (rows, cost, path) = if filter.is_some() {
                    (100, 1.0, QueryPath::Oltp)
                } else {
                    (10_000, 10.0, QueryPath::Olap)
                };
                CostEstimate {
                    estimated_rows: rows,
                    relative_cost: cost,
                    recommended_path: path,
                }
            }
            LogicalPlan::Filter { input, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows / 10).max(1),
                    relative_cost: inner.relative_cost * 0.5,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Aggregate { input, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows / 20).max(1),
                    relative_cost: inner.relative_cost * 5.0,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Sort { input, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost * 1.5,
                    recommended_path: inner.recommended_path,
                }
            }
            LogicalPlan::Limit { input, count } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows.min(*count),
                    relative_cost: inner.relative_cost * 0.1,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::TopN { input, count, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows.min(*count),
                    relative_cost: inner.relative_cost * 1.3,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Project { input, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost * 0.9,
                    recommended_path: inner.recommended_path,
                }
            }
            LogicalPlan::Insert { value_count, .. } => CostEstimate {
                estimated_rows: *value_count as u64,
                relative_cost: 0.5,
                recommended_path: QueryPath::Oltp,
            },
            LogicalPlan::Update { filter, .. } => CostEstimate {
                estimated_rows: if filter.is_some() { 1 } else { 100 },
                relative_cost: if filter.is_some() { 0.5 } else { 5.0 },
                recommended_path: QueryPath::Oltp,
            },
            LogicalPlan::Delete { filter, .. } => CostEstimate {
                estimated_rows: if filter.is_some() { 1 } else { 100 },
                relative_cost: if filter.is_some() { 0.5 } else { 5.0 },
                recommended_path: QueryPath::Oltp,
            },
            LogicalPlan::CreateTable { .. } => CostEstimate {
                estimated_rows: 0,
                relative_cost: 0.1,
                recommended_path: QueryPath::Oltp,
            },
            LogicalPlan::Begin | LogicalPlan::Commit | LogicalPlan::Rollback => CostEstimate {
                estimated_rows: 0,
                relative_cost: 0.05,
                recommended_path: QueryPath::Oltp,
            },
            LogicalPlan::Join { left, right, .. } => {
                let lc = Self::estimate_cost(left);
                let rc = Self::estimate_cost(right);
                CostEstimate {
                    estimated_rows: lc.estimated_rows.saturating_add(rc.estimated_rows),
                    relative_cost: lc.relative_cost + rc.relative_cost + 3.0,
                    recommended_path: QueryPath::Hybrid,
                }
            }
            LogicalPlan::Union { left, right } => {
                let lc = Self::estimate_cost(left);
                let rc = Self::estimate_cost(right);
                CostEstimate {
                    estimated_rows: lc.estimated_rows.saturating_add(rc.estimated_rows),
                    relative_cost: lc.relative_cost + rc.relative_cost + 2.0,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::InList { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.8) as u64,
                    relative_cost: inner.relative_cost + 0.5,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Between { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.75) as u64,
                    relative_cost: inner.relative_cost + 0.4,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Like { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.7) as u64,
                    relative_cost: inner.relative_cost + 1.2,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Not { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.85) as u64,
                    relative_cost: inner.relative_cost + 0.6,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Case { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.9) as u64,
                    relative_cost: inner.relative_cost + 1.5,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Coalesce { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.3,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Cast { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.2,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Nullif { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.15,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::StringFn { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.1,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::DateFn { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.12,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Concat { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.08,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::MathFn { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.09,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Exists { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.5) as u64,
                    relative_cost: inner.relative_cost + 1.2,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::AnyAll { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.8) as u64,
                    relative_cost: inner.relative_cost + 0.6,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::NotIn { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.7) as u64,
                    relative_cost: inner.relative_cost + 0.4,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Trim { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.05,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Interval { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.9) as u64,
                    relative_cost: inner.relative_cost + 0.3,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::InSubquery { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.6) as u64,
                    relative_cost: inner.relative_cost + 0.8,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::IsNull { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.1,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Regexp { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.7) as u64,
                    relative_cost: inner.relative_cost + 0.5,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::JsonOp { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.4,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::WindowAgg { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 1.5,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Lateral { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.7) as u64,
                    relative_cost: inner.relative_cost + 0.7,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Pivot { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.9) as u64,
                    relative_cost: inner.relative_cost + 0.8,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Fetch { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.05,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Values { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.02,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::CrossJoin { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows * inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.30,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::FullTextSearch { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.3) as u64,
                    relative_cost: inner.relative_cost + 0.60,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::GroupingSets { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 1.5) as u64,
                    relative_cost: inner.relative_cost + 0.70,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::NaturalJoin { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.35,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::UsingJoin { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.25,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Except { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.8) as u64,
                    relative_cost: inner.relative_cost + 0.45,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Intersect { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.7) as u64,
                    relative_cost: inner.relative_cost + 0.50,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Qualify { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.6) as u64,
                    relative_cost: inner.relative_cost + 0.20,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::WithCte { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.15,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::RecursiveCte { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 3.00,
                    recommended_path: QueryPath::Hybrid,
                }
            }
            LogicalPlan::NotExists { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: (inner.estimated_rows as f64 * 0.8) as u64,
                    relative_cost: inner.relative_cost + 2.00,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::WindowFn { input, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 2.5,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Distinct { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows / 2,
                    relative_cost: inner.relative_cost + 0.3,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Offset { input, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 0.1,
                    recommended_path: QueryPath::Oltp,
                }
            }
            LogicalPlan::Having { input, .. } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows / 2,
                    relative_cost: inner.relative_cost + 1.0,
                    recommended_path: QueryPath::Olap,
                }
            }
            LogicalPlan::Subquery { input } => {
                let inner = Self::estimate_cost(input);
                CostEstimate {
                    estimated_rows: inner.estimated_rows,
                    relative_cost: inner.relative_cost + 2.0,
                    recommended_path: QueryPath::Hybrid,
                }
            }
            LogicalPlan::Unknown(_) => CostEstimate {
                estimated_rows: 0,
                relative_cost: 0.0,
                recommended_path: QueryPath::Unknown,
            },
        }
    }

    // ── Internal builders ────────────────────────────────────────────────────

    fn plan_select(sel: &SelectStatement) -> LogicalPlan {
        // Base scan
        let scan = LogicalPlan::Scan {
            table: sel.table.clone().unwrap_or_else(|| "<unknown>".to_string()),
            filter: None,
        };

        // JOIN (S3-WS1-05)
        let after_join = if let Some(SqlJoinClause { join_table, on_condition }) = &sel.join {
            LogicalPlan::Join {
                left: Box::new(scan),
                right: Box::new(LogicalPlan::Scan {
                    table: join_table.clone(),
                    filter: on_condition.clone(),
                }),
                join_table: join_table.clone(),
                condition: on_condition.clone(),
            }
        } else {
            scan
        };

        // Filter
        let after_filter = if let Some(pred) = &sel.where_clause {
            LogicalPlan::Filter {
                input: Box::new(after_join),
                predicate: pred.clone(),
            }
        } else {
            after_join
        };

        // Aggregate (GROUP BY)
        let after_agg = if !sel.group_by.is_empty() {
            LogicalPlan::Aggregate {
                input: Box::new(after_filter),
                group_by: sel.group_by.clone(),
                having: sel.having.clone(),
            }
        } else {
            after_filter
        };

        // HAVING (post-aggregate filter, S3-WS1-06 has_group_by support)
        let after_having = if let Some(cond) = &sel.having {
            LogicalPlan::Having {
                input: Box::new(after_agg),
                condition: cond.clone(),
            }
        } else {
            after_agg
        };

        // Sort+Limit → TopN optimisation (S3-WS1-05): combine when both present.
        let after_limit = if !sel.order_by.is_empty() && sel.limit.is_some() {
            LogicalPlan::TopN {
                input: Box::new(after_having),
                count: sel.limit.unwrap(),
                order_by: sel.order_by.first().map(|o| o.column.clone()).unwrap_or_default(),
            }
        } else {
            // Sort (ORDER BY only)
            let after_sort = if !sel.order_by.is_empty() {
                LogicalPlan::Sort {
                    input: Box::new(after_having),
                    order_by: sel
                        .order_by
                        .iter()
                        .map(|o| (o.column.clone(), o.descending))
                        .collect(),
                }
            } else {
                after_having
            };
            // Limit (no ORDER BY)
            if let Some(n) = sel.limit {
                LogicalPlan::Limit {
                    input: Box::new(after_sort),
                    count: n,
                }
            } else {
                after_sort
            }
        };

        // Offset (S3-WS1-06 OFFSET support)
        let after_offset = if let Some(off) = sel.offset {
            if off > 0 {
                LogicalPlan::Offset {
                    input: Box::new(after_limit),
                    offset: off,
                }
            } else {
                after_limit
            }
        } else {
            after_limit
        };

        // Project (only when not SELECT *)
        let after_project = if sel.columns != vec!["*".to_string()] && !sel.columns.is_empty() {
            LogicalPlan::Project {
                input: Box::new(after_offset),
                columns: sel.columns.clone(),
            }
        } else {
            after_offset
        };

        // UNION (S3-WS1-04 has_union detection): wrap in Union node with synthetic rhs
        let after_union = if sel.has_union {
            let rhs_table = sel.table.clone().unwrap_or_else(|| "<union_rhs>".to_string());
            LogicalPlan::Union {
                left: Box::new(after_project),
                right: Box::new(LogicalPlan::Scan {
                    table: rhs_table,
                    filter: None,
                }),
            }
        } else {
            after_project
        };

        // Window function (S3-WS1-04 has_window_fn detection): wrap outermost node.
        let after_window = if sel.has_window_fn {
            LogicalPlan::WindowFn {
                input: Box::new(after_union),
                window_func: "OVER".to_string(),
            }
        } else {
            after_union
        };

        // SELECT DISTINCT deduplication (S3-WS1-04 is_distinct detection): wrap outermost.
        let after_distinct = if sel.is_distinct {
            LogicalPlan::Distinct {
                input: Box::new(after_window),
            }
        } else {
            after_window
        };

        // Subquery wrapper (S3-WS1-04 has_subquery detection): outermost node.
        let after_subquery = if sel.has_subquery {
            LogicalPlan::Subquery {
                input: Box::new(after_distinct),
            }
        } else {
            after_distinct
        };

        // InList wrapper (S3-WS1-07 has_in_list detection): outermost node.
        let after_in_list = if sel.has_in_list {
            LogicalPlan::InList {
                input: Box::new(after_subquery),
            }
        } else {
            after_subquery
        };

        // Between wrapper (S3-WS1-08 has_between detection): outermost node.
        let after_between = if sel.has_between {
            LogicalPlan::Between {
                input: Box::new(after_in_list),
            }
        } else {
            after_in_list
        };

        // Like wrapper (S3-WS1-09 has_like detection): outermost node.
        let after_like = if sel.has_like {
            LogicalPlan::Like {
                input: Box::new(after_between),
            }
        } else {
            after_between
        };

        // Not wrapper (S3-WS1-10 has_not detection): outermost node.
        let after_not = if sel.has_not {
            LogicalPlan::Not {
                input: Box::new(after_like),
            }
        } else {
            after_like
        };

        // Case wrapper (S3-WS1-11 has_case detection): outermost node.
        let after_case = if sel.has_case {
            LogicalPlan::Case {
                input: Box::new(after_not),
            }
        } else {
            after_not
        };

        // Coalesce wrapper (S3-WS1-12 has_coalesce detection): outermost node.
        let after_coalesce = if sel.has_coalesce {
            LogicalPlan::Coalesce {
                input: Box::new(after_case),
            }
        } else {
            after_case
        };

        // Cast wrapper (S3-WS1-13 has_cast detection): outermost node.
        let after_cast = if sel.has_cast {
            LogicalPlan::Cast {
                input: Box::new(after_coalesce),
            }
        } else {
            after_coalesce
        };

        // Nullif wrapper (S3-WS1-14 has_nullif detection): outermost node.
        let after_nullif = if sel.has_nullif {
            LogicalPlan::Nullif {
                input: Box::new(after_cast),
            }
        } else {
            after_cast
        };

        // StringFn wrapper (S3-WS1-15 has_string_fn detection): outermost node.
        let after_string_fn = if sel.has_string_fn {
            LogicalPlan::StringFn {
                input: Box::new(after_nullif),
            }
        } else {
            after_nullif
        };

        // DateFn wrapper (S3-WS1-16 has_date_fn detection): outermost node.
        let after_date_fn = if sel.has_date_fn {
            LogicalPlan::DateFn {
                input: Box::new(after_string_fn),
            }
        } else {
            after_string_fn
        };

        // Concat wrapper (S3-WS1-17 has_concat detection): outermost node.
        let after_concat = if sel.has_concat {
            LogicalPlan::Concat {
                input: Box::new(after_date_fn),
            }
        } else {
            after_date_fn
        };

        // MathFn wrapper (S3-WS1-18 has_math_fn detection).
        let after_math_fn = if sel.has_math_fn {
            LogicalPlan::MathFn {
                input: Box::new(after_concat),
            }
        } else {
            after_concat
        };

        // Exists wrapper (S3-WS1-19 has_exists detection).
        let after_exists = if sel.has_exists {
            LogicalPlan::Exists {
                input: Box::new(after_math_fn),
            }
        } else {
            after_math_fn
        };

        // AnyAll wrapper (S3-WS1-20 has_any_all detection).
        let after_any_all = if sel.has_any_all {
            LogicalPlan::AnyAll {
                input: Box::new(after_exists),
            }
        } else {
            after_exists
        };

        // NotIn wrapper (S3-WS1-21 has_not_in detection).
        let after_not_in = if sel.has_not_in {
            LogicalPlan::NotIn {
                input: Box::new(after_any_all),
            }
        } else {
            after_any_all
        };

        // Trim wrapper (S3-WS1-22 has_trim detection).
        let after_trim = if sel.has_trim {
            LogicalPlan::Trim {
                input: Box::new(after_not_in),
            }
        } else {
            after_not_in
        };

        // Interval wrapper (S3-WS1-23 has_interval detection).
        let after_interval = if sel.has_interval {
            LogicalPlan::Interval {
                input: Box::new(after_trim),
            }
        } else {
            after_trim
        };

        // InSubquery wrapper (S3-WS1-24 has_in_subquery detection).
        let after_in_subquery = if sel.has_in_subquery {
            LogicalPlan::InSubquery {
                input: Box::new(after_interval),
            }
        } else {
            after_interval
        };

        // IsNull wrapper (S3-WS1-25 has_is_null detection).
        let after_is_null = if sel.has_is_null {
            LogicalPlan::IsNull {
                input: Box::new(after_in_subquery),
            }
        } else {
            after_in_subquery
        };

        // Regexp wrapper (S3-WS1-26 has_regexp detection).
        let after_regexp = if sel.has_regexp {
            LogicalPlan::Regexp {
                input: Box::new(after_is_null),
            }
        } else {
            after_is_null
        };

        // JsonOp wrapper (S3-WS1-27 has_json_op detection).
        let after_json_op = if sel.has_json_op {
            LogicalPlan::JsonOp {
                input: Box::new(after_regexp),
            }
        } else {
            after_regexp
        };

        // WindowAgg wrapper (S3-WS1-28 has_window_agg detection).
        let after_window_agg = if sel.has_window_agg {
            LogicalPlan::WindowAgg {
                input: Box::new(after_json_op),
            }
        } else {
            after_json_op
        };

        // Lateral wrapper (S3-WS1-29 has_lateral detection).
        let after_lateral = if sel.has_lateral {
            LogicalPlan::Lateral {
                input: Box::new(after_window_agg),
            }
        } else {
            after_window_agg
        };

        // Pivot wrapper (S3-WS1-30 has_pivot detection).
        let after_pivot = if sel.has_pivot {
            LogicalPlan::Pivot {
                input: Box::new(after_lateral),
            }
        } else {
            after_lateral
        };

        // Fetch wrapper (S3-WS1-31 has_fetch detection): outermost node.
        let after_fetch = if sel.has_fetch {
            LogicalPlan::Fetch {
                input: Box::new(after_pivot),
            }
        } else {
            after_pivot
        };

        // Values wrapper (S3-WS1-32 has_values detection).
        let after_values = if sel.has_values {
            LogicalPlan::Values {
                input: Box::new(after_fetch),
            }
        } else {
            after_fetch
        };

        // CrossJoin wrapper (S3-WS1-33 has_cross_join detection).
        let after_cross_join = if sel.has_cross_join {
            LogicalPlan::CrossJoin {
                input: Box::new(after_values),
            }
        } else {
            after_values
        };

        // FullTextSearch wrapper (S3-WS1-34 has_full_text_search detection).
        let after_full_text_search = if sel.has_full_text_search {
            LogicalPlan::FullTextSearch {
                input: Box::new(after_cross_join),
            }
        } else {
            after_cross_join
        };

        // GroupingSets wrapper (S3-WS1-35 has_grouping_sets detection).
        let after_grouping_sets = if sel.has_grouping_sets {
            LogicalPlan::GroupingSets {
                input: Box::new(after_full_text_search),
            }
        } else {
            after_full_text_search
        };

        // NaturalJoin wrapper (S3-WS1-36 has_natural_join detection).
        let after_natural_join = if sel.has_natural_join {
            LogicalPlan::NaturalJoin {
                input: Box::new(after_grouping_sets),
            }
        } else {
            after_grouping_sets
        };

        // UsingJoin wrapper (S3-WS1-37 has_using_join detection).
        let after_using_join = if sel.has_using_join {
            LogicalPlan::UsingJoin {
                input: Box::new(after_natural_join),
            }
        } else {
            after_natural_join
        };

        // Except wrapper (S3-WS1-38 has_except detection).
        let after_except = if sel.has_except {
            LogicalPlan::Except {
                input: Box::new(after_using_join),
            }
        } else {
            after_using_join
        };

        // Intersect wrapper (S3-WS1-39 has_intersect detection).
        let after_intersect = if sel.has_intersect {
            LogicalPlan::Intersect {
                input: Box::new(after_except),
            }
        } else {
            after_except
        };

        // Qualify wrapper (S3-WS1-40 has_qualify detection).
        let after_qualify = if sel.has_qualify {
            LogicalPlan::Qualify {
                input: Box::new(after_intersect),
            }
        } else {
            after_intersect
        };

        // WithCte wrapper (S3-WS1-41 has_with_cte detection).
        let after_with_cte = if sel.has_with_cte {
            LogicalPlan::WithCte {
                input: Box::new(after_qualify),
            }
        } else {
            after_qualify
        };

        // RecursiveCte wrapper (S3-WS1-42 has_recursive_cte detection).
        let after_recursive_cte = if sel.has_recursive_cte {
            LogicalPlan::RecursiveCte {
                input: Box::new(after_with_cte),
            }
        } else {
            after_with_cte
        };

        // NotExists wrapper (S3-WS1-43 has_not_exists detection): outermost node.
        if sel.has_not_exists {
            LogicalPlan::NotExists {
                input: Box::new(after_recursive_cte),
            }
        } else {
            after_recursive_cte
        }
    }

    fn plan_insert(ins: &InsertStatement) -> LogicalPlan {
        LogicalPlan::Insert {
            table: ins.table.clone(),
            columns: ins.columns.clone(),
            value_count: ins.values.len(),
        }
    }

    fn plan_update(upd: &UpdateStatement) -> LogicalPlan {
        LogicalPlan::Update {
            table: upd.table.clone(),
            assignments: upd.assignments.clone(),
            filter: upd.where_clause.clone(),
        }
    }

    fn plan_delete(del: &DeleteStatement) -> LogicalPlan {
        LogicalPlan::Delete {
            table: del.table.clone(),
            filter: del.where_clause.clone(),
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use voltnuerongrid_sql::parse_one;

    fn plan(sql: &str) -> LogicalPlan {
        let stmt = parse_one(sql).expect("parse failed");
        QueryPlanner::plan(&stmt)
    }

    fn cost(sql: &str) -> CostEstimate {
        let p = plan(sql);
        QueryPlanner::estimate_cost(&p)
    }

    // ── Plan shape tests ────────────────────────────────────────────────────

    #[test]
    fn planner_select_star_produces_scan() {
        let p = plan("SELECT * FROM orders");
        assert!(matches!(p, LogicalPlan::Scan { .. }));
        assert_eq!(p.primary_table(), Some("orders"));
    }

    #[test]
    fn planner_select_columns_produces_project_over_scan() {
        let p = plan("SELECT id, name FROM users");
        assert!(matches!(&p, LogicalPlan::Project { columns, .. } if columns == &["id", "name"]));
        assert_eq!(p.primary_table(), Some("users"));
    }

    #[test]
    fn planner_select_with_where_produces_filter() {
        let p = plan("SELECT * FROM customers WHERE active = 1");
        // Scan → Filter (no projection because SELECT *)
        assert!(matches!(p, LogicalPlan::Filter { .. }));
        assert!(!p.has_aggregation());
    }

    #[test]
    fn planner_group_by_produces_aggregate() {
        let p = plan("SELECT region FROM sales GROUP BY region");
        assert!(p.has_aggregation());
        assert_eq!(p.primary_table(), Some("sales"));
    }

    #[test]
    fn planner_select_with_limit_produces_limit_node() {
        // SELECT * avoids the Project wrapper so Limit is the outermost node
        let p = plan("SELECT * FROM t LIMIT 10");
        assert!(matches!(p, LogicalPlan::Limit { count: 10, .. }));
    }

    #[test]
    fn planner_insert_produces_insert_node() {
        let p = plan("INSERT INTO orders VALUES ('o1', 200)");
        assert!(matches!(
            &p,
            LogicalPlan::Insert { table, value_count: 1, .. } if table == "orders"
        ));
        assert!(p.is_read_only() == false);
    }

    #[test]
    fn planner_update_produces_update_node() {
        let p = plan("UPDATE products SET price = 99 WHERE id = 'p1'");
        assert!(matches!(&p, LogicalPlan::Update { table, .. } if table == "products"));
        assert!(!p.is_read_only());
    }

    #[test]
    fn planner_delete_produces_delete_node() {
        let p = plan("DELETE FROM orders WHERE id = 'o1'");
        assert!(matches!(&p, LogicalPlan::Delete { table, .. } if table == "orders"));
        assert!(!p.is_read_only());
    }

    #[test]
    fn planner_create_table_produces_create_node() {
        let p = plan("CREATE TABLE events (id INTEGER, ts BIGINT)");
        assert!(
            matches!(&p, LogicalPlan::CreateTable { table, column_count: 2 } if table == "events")
        );
    }

    #[test]
    fn planner_begin_commit_rollback() {
        assert_eq!(plan("BEGIN"), LogicalPlan::Begin);
        assert_eq!(plan("COMMIT"), LogicalPlan::Commit);
        assert_eq!(plan("ROLLBACK"), LogicalPlan::Rollback);
    }

    // ── Cost model tests ────────────────────────────────────────────────────

    #[test]
    fn cost_full_scan_is_olap_path() {
        let c = cost("SELECT * FROM big_table");
        assert_eq!(c.recommended_path, QueryPath::Olap);
        assert!(c.estimated_rows > 1_000);
    }

    #[test]
    fn cost_filter_select_is_oltp_path() {
        let c = cost("SELECT id FROM users WHERE id = 1");
        assert_eq!(c.recommended_path, QueryPath::Oltp);
    }

    #[test]
    fn cost_aggregate_query_is_olap_path() {
        let c = cost("SELECT region FROM sales GROUP BY region");
        assert_eq!(c.recommended_path, QueryPath::Olap);
        assert!(c.relative_cost > 1.0);
    }

    #[test]
    fn cost_insert_is_cheap_oltp() {
        let c = cost("INSERT INTO orders VALUES ('o1', 100)");
        assert_eq!(c.recommended_path, QueryPath::Oltp);
        assert!(c.relative_cost < 1.0);
    }

    #[test]
    fn cost_limit_reduces_estimated_rows() {
        let c = cost("SELECT id FROM t LIMIT 5");
        assert_eq!(c.estimated_rows, 5);
    }

    #[test]
    fn cost_unknown_path_for_unrecognised_statement() {
        let c = cost("TRUNCATE TABLE foo");
        assert_eq!(c.recommended_path, QueryPath::Unknown);
    }

    // ── S3-WS1-05: JOIN planner tests ───────────────────────────────────────

    #[test]
    fn planner_select_with_join_produces_join_node() {
        let p = plan("SELECT * FROM orders JOIN customers ON orders.customer_id = customers.id");
        assert!(matches!(&p, LogicalPlan::Join { join_table, .. } if join_table == "customers"), "expected Join node, got {p:?}");
        assert_eq!(p.primary_table(), Some("orders"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_join_query_is_hybrid_path() {
        let c = cost("SELECT * FROM orders JOIN customers ON orders.customer_id = customers.id");
        assert_eq!(c.recommended_path, QueryPath::Hybrid);
        assert!(c.relative_cost > 3.0, "join should have extra cost");
    }

    // ── S3-WS1-05: Union plan node tests ─────────────────────────────────────

    #[test]
    fn planner_union_select_produces_union_node() {
        let p = plan("SELECT * FROM orders UNION SELECT * FROM archived_orders");
        assert!(
            matches!(&p, LogicalPlan::Union { .. }),
            "expected Union node for UNION query, got {p:?}"
        );
        assert_eq!(p.primary_table(), Some("orders"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_union_query_is_olap_path() {
        let c = cost("SELECT * FROM t1 UNION SELECT * FROM t2");
        assert_eq!(c.recommended_path, QueryPath::Olap, "UNION should route to OLAP");
        assert!(c.relative_cost > 2.0, "union should carry extra cost");
    }

    // ── S3-WS1-05: WindowFn plan node tests ──────────────────────────────────

    #[test]
    fn planner_window_fn_produces_window_fn_node() {
        let p = plan("SELECT id, RANK() OVER (PARTITION BY dept ORDER BY salary DESC) AS rnk FROM employees");
        assert!(
            matches!(&p, LogicalPlan::WindowFn { window_func, .. } if window_func == "OVER"),
            "expected WindowFn node for window function query, got {p:?}"
        );
        assert_eq!(p.primary_table(), Some("employees"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_window_fn_query_is_olap_path() {
        let c = cost("SELECT region, SUM(revenue) OVER(PARTITION BY region) AS total FROM sales");
        assert_eq!(c.recommended_path, QueryPath::Olap, "window function should route to OLAP");
        assert!(c.relative_cost > 2.5, "window fn should carry extra cost >= 2.5");
    }

    #[test]
    fn planner_distinct_wraps_outermost_in_distinct_node() {
        let p = plan("SELECT DISTINCT id FROM users");
        assert!(
            matches!(&p, LogicalPlan::Distinct { .. }),
            "expected Distinct node for SELECT DISTINCT, got {p:?}"
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_distinct_query_routes_to_oltp() {
        let c = cost("SELECT DISTINCT name FROM employees");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "DISTINCT should route to OLTP");
    }

    #[test]
    fn planner_select_with_offset_produces_offset_node() {
        let plan = plan("SELECT * FROM t LIMIT 10 OFFSET 5");
        // The outermost plan node should be Offset wrapping a Limit
        assert!(
            matches!(&plan, LogicalPlan::Offset { offset, .. } if *offset == 5),
            "LIMIT 10 OFFSET 5 should produce an Offset node with offset=5, got: {:?}", plan
        );
    }

    #[test]
    fn cost_offset_query_routes_to_oltp() {
        let c = cost("SELECT * FROM t LIMIT 10 OFFSET 5");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "OFFSET query should route to OLTP");
    }

    #[test]
    fn planner_having_produces_having_node() {
        // SELECT * avoids Project wrapper so Having is outermost plan node.
        let p = plan("SELECT * FROM employees GROUP BY dept HAVING COUNT(*) > 5");
        assert!(
            matches!(&p, LogicalPlan::Having { condition, .. } if condition.to_uppercase().contains("COUNT")),
            "GROUP BY ... HAVING should produce a Having node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("employees"));
    }

    #[test]
    fn cost_having_query_routes_to_olap() {
        let c = cost("SELECT * FROM orders GROUP BY region HAVING SUM(sales) > 100");
        assert_eq!(c.recommended_path, QueryPath::Olap, "HAVING query should route to OLAP");
        assert!(c.relative_cost >= 1.0, "HAVING should carry cost >= 1.0");
    }

    #[test]
    fn planner_topn_produced_when_order_by_and_limit() {
        let p = plan("SELECT * FROM employees ORDER BY salary DESC LIMIT 5");
        assert!(
            matches!(&p, LogicalPlan::TopN { count, .. } if *count == 5),
            "ORDER BY … LIMIT should produce TopN node; got {p:?}"
        );
        assert_eq!(p.primary_table(), Some("employees"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_topn_query_routes_to_oltp() {
        let c = cost("SELECT * FROM orders ORDER BY created_at LIMIT 20");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "TopN query should route to OLTP");
        assert_eq!(c.estimated_rows, 20, "estimated rows capped at TopN count");
    }

    #[test]
    fn planner_subquery_produces_subquery_node() {
        let p = plan("SELECT id FROM orders WHERE id = (SELECT MAX(id) FROM recent_orders)");
        assert!(
            matches!(&p, LogicalPlan::Subquery { .. }),
            "query with scalar subquery should produce outermost Subquery node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("orders"));
    }

    #[test]
    fn cost_subquery_routes_to_hybrid() {
        let c = cost("SELECT id FROM orders WHERE id = (SELECT MAX(id) FROM recent_orders)");
        assert_eq!(c.recommended_path, QueryPath::Hybrid, "subquery should route to Hybrid");
        assert!(c.relative_cost >= 2.0, "subquery carries cost >= 2.0 overhead");
    }

    #[test]
    fn planner_in_list_select_produces_in_list_node() {
        let p = plan("SELECT id FROM users WHERE id IN (1, 2, 3)");
        assert!(
            matches!(&p, LogicalPlan::InList { .. }),
            "IN list query should produce outermost InList node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_in_list_query_routes_to_oltp() {
        let c = cost("SELECT id FROM users WHERE id IN (1, 2, 3)");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "IN list should route to OLTP");
        assert!(c.relative_cost >= 0.5, "InList must carry at least 0.5 cost overhead");
    }

    #[test]
    fn planner_between_select_produces_between_node() {
        let p = plan("SELECT id FROM users WHERE age BETWEEN 18 AND 65");
        assert!(
            matches!(&p, LogicalPlan::Between { .. }),
            "BETWEEN query should produce outermost Between node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_between_query_routes_to_oltp() {
        let c = cost("SELECT id FROM users WHERE age BETWEEN 18 AND 65");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "BETWEEN should route to OLTP");
        assert!(c.relative_cost >= 0.4, "Between must carry at least 0.4 cost overhead");
    }

    #[test]
    fn planner_like_select_produces_like_node() {
        let p = plan("SELECT name FROM users WHERE name LIKE '%Alice%'");
        assert!(
            matches!(&p, LogicalPlan::Like { .. }),
            "LIKE query should produce outermost Like node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_like_query_routes_to_olap() {
        let c = cost("SELECT name FROM users WHERE name LIKE '%Alice%'");
        assert_eq!(c.recommended_path, QueryPath::Olap, "LIKE should route to OLAP (full scan)");
        assert!(c.relative_cost >= 1.2, "Like must carry at least 1.2 cost overhead");
    }

    #[test]
    fn planner_not_select_produces_not_node() {
        let p = plan("SELECT id FROM users WHERE NOT (id = 0)");
        assert!(
            matches!(&p, LogicalPlan::Not { .. }),
            "NOT predicate query should produce outermost Not node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_not_query_routes_to_oltp() {
        let c = cost("SELECT id FROM users WHERE NOT (id = 0)");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "NOT predicate should route to OLTP");
        assert!(c.relative_cost >= 0.6, "Not must carry at least 0.6 cost overhead");
    }

    #[test]
    fn planner_case_select_produces_case_node() {
        let p = plan("SELECT id, CASE WHEN age > 18 THEN 'adult' ELSE 'minor' END AS cat FROM users");
        assert!(
            matches!(&p, LogicalPlan::Case { .. }),
            "CASE WHEN query should produce outermost Case node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_case_query_routes_to_olap() {
        let c = cost("SELECT id, CASE WHEN age > 18 THEN 'adult' ELSE 'minor' END FROM users");
        assert_eq!(c.recommended_path, QueryPath::Olap, "CASE WHEN should route to OLAP");
        assert!(c.relative_cost >= 1.5, "Case must carry at least 1.5 cost overhead");
    }

    #[test]
    fn planner_coalesce_select_produces_coalesce_node() {
        let p = plan("SELECT COALESCE(name, 'unknown') FROM users");
        assert!(
            matches!(&p, LogicalPlan::Coalesce { .. }),
            "COALESCE() query should produce outermost Coalesce node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_coalesce_query_routes_to_oltp() {
        let c = cost("SELECT COALESCE(name, 'unknown') FROM users");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "COALESCE should route to OLTP");
        assert!(c.relative_cost >= 0.3, "Coalesce must carry at least 0.3 cost overhead");
    }

    #[test]
    fn planner_cast_select_produces_cast_node() {
        let p = plan("SELECT CAST(amount AS TEXT) FROM orders");
        assert!(
            matches!(&p, LogicalPlan::Cast { .. }),
            "CAST() query should produce outermost Cast node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("orders"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_cast_query_routes_to_oltp() {
        let c = cost("SELECT CAST(amount AS TEXT) FROM orders");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "CAST should route to OLTP");
        assert!(c.relative_cost >= 0.2, "Cast must carry at least 0.2 cost overhead");
    }

    #[test]
    fn planner_nullif_select_produces_nullif_node() {
        let p = plan("SELECT NULLIF(score, 0) FROM results");
        assert!(
            matches!(&p, LogicalPlan::Nullif { .. }),
            "NULLIF() query should produce outermost Nullif node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("results"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_nullif_query_routes_to_oltp() {
        let c = cost("SELECT NULLIF(score, 0) FROM results");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "NULLIF should route to OLTP");
        assert!(c.relative_cost >= 0.15, "Nullif must carry at least 0.15 cost overhead");
    }

    #[test]
    fn planner_string_fn_select_produces_string_fn_node() {
        let p = plan("SELECT UPPER(name) FROM users");
        assert!(
            matches!(&p, LogicalPlan::StringFn { .. }),
            "UPPER() query should produce outermost StringFn node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_string_fn_query_routes_to_oltp() {
        let c = cost("SELECT LENGTH(name) FROM users");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "string fn should route to OLTP");
        assert!(c.relative_cost >= 0.1, "StringFn must carry at least 0.1 cost overhead");
    }

    #[test]
    fn planner_date_fn_select_produces_date_fn_node() {
        let p = plan("SELECT NOW() FROM dual");
        assert!(
            matches!(&p, LogicalPlan::DateFn { .. }),
            "NOW() query should produce outermost DateFn node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("dual"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_date_fn_query_routes_to_oltp() {
        let c = cost("SELECT EXTRACT(year FROM created_at) FROM events");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "date fn should route to OLTP");
        assert!(c.relative_cost >= 0.12, "DateFn must carry at least 0.12 cost overhead");
    }

    #[test]
    fn planner_concat_select_produces_concat_node() {
        let p = plan("SELECT CONCAT(first_name, last_name) FROM users");
        assert!(
            matches!(&p, LogicalPlan::Concat { .. }),
            "CONCAT() query should produce outermost Concat node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_concat_query_routes_to_oltp() {
        let c = cost("SELECT CONCAT(first_name, last_name) FROM users");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "CONCAT should route to OLTP");
        assert!(c.relative_cost >= 0.08, "Concat must carry at least 0.08 cost overhead");
    }

    #[test]
    fn planner_math_fn_select_produces_math_fn_node() {
        let p = plan("SELECT ABS(balance) FROM accounts");
        assert!(
            matches!(&p, LogicalPlan::MathFn { .. }),
            "ABS() query should produce outermost MathFn node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("accounts"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_math_fn_query_routes_to_oltp() {
        let c = cost("SELECT ROUND(price, 2) FROM products");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "math fn should route to OLTP");
        assert!(c.relative_cost >= 0.09, "MathFn must carry at least 0.09 cost overhead");
    }

    #[test]
    fn planner_exists_select_produces_exists_node() {
        let p = plan("SELECT id FROM orders WHERE EXISTS (SELECT 1 FROM items WHERE items.order_id = orders.id)");
        assert!(
            matches!(&p, LogicalPlan::Exists { .. }),
            "EXISTS subquery must produce outermost Exists node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("orders"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_exists_query_routes_to_olap() {
        let c = cost("SELECT id FROM orders WHERE EXISTS (SELECT 1 FROM items WHERE items.order_id = orders.id)");
        assert_eq!(c.recommended_path, QueryPath::Olap, "EXISTS subquery should route to OLAP");
        assert!(c.relative_cost >= 1.2, "Exists must carry at least 1.2 cost overhead");
    }

    #[test]
    fn planner_any_all_select_produces_any_all_node() {
        let p = plan("SELECT id FROM products WHERE price > ANY(SELECT price FROM discounts)");
        assert!(
            matches!(&p, LogicalPlan::AnyAll { .. }),
            "ANY quantifier must produce outermost AnyAll node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("products"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_any_all_query_routes_to_olap() {
        let c = cost("SELECT name FROM employees WHERE salary >= ALL(SELECT salary FROM managers)");
        assert_eq!(c.recommended_path, QueryPath::Olap, "ANY/ALL quantifier should route to OLAP");
        assert!(c.relative_cost >= 0.6, "AnyAll must carry at least 0.6 cost overhead");
    }

    #[test]
    fn planner_not_in_select_produces_not_in_node() {
        let p = plan("SELECT id FROM orders WHERE status NOT IN ('cancelled', 'failed')");
        assert!(
            matches!(&p, LogicalPlan::NotIn { .. }),
            "NOT IN predicate must produce outermost NotIn node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("orders"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_not_in_query_routes_to_olap() {
        let c = cost("SELECT name FROM users WHERE id NOT IN (SELECT user_id FROM bans)");
        assert_eq!(c.recommended_path, QueryPath::Olap, "NOT IN should route to OLAP");
        assert!(c.relative_cost >= 0.4, "NotIn must carry at least 0.4 cost overhead");
    }

    // ── S3-WS1-22: Trim node tests ────────────────────────────────────────────

    #[test]
    fn planner_trim_select_produces_trim_node() {
        let p = plan("SELECT TRIM(name) FROM users");
        assert!(
            matches!(&p, LogicalPlan::Trim { .. }),
            "TRIM() call must produce outermost Trim node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_trim_query_routes_to_oltp() {
        let c = cost("SELECT LTRIM(email) FROM contacts");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "TRIM functions should route to OLTP");
        assert!(c.relative_cost >= 0.05, "Trim must carry at least 0.05 cost overhead");
    }

    // ── S3-WS1-23: Interval node tests ───────────────────────────────────────

    #[test]
    fn planner_interval_select_produces_interval_node() {
        let p = plan("SELECT created_at + INTERVAL '7 days' FROM events");
        assert!(
            matches!(&p, LogicalPlan::Interval { .. }),
            "INTERVAL expression must produce outermost Interval node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("events"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_interval_query_routes_to_olap() {
        let c = cost("SELECT * FROM logs WHERE ts > NOW() - INTERVAL '1 hour'");
        assert_eq!(c.recommended_path, QueryPath::Olap, "INTERVAL expressions should route to OLAP");
        assert!(c.relative_cost >= 0.3, "Interval must carry at least 0.3 cost overhead");
    }
    // ── S3-WS1-24: InSubquery node tests ─────────────────────────────────────

    #[test]
    fn planner_in_subquery_select_produces_in_subquery_node() {
        let p = plan("SELECT id FROM orders WHERE user_id IN (SELECT id FROM users)");
        assert!(
            matches!(&p, LogicalPlan::InSubquery { .. }),
            "IN (SELECT ...) predicate must produce outermost InSubquery node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("orders"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_in_subquery_query_routes_to_olap() {
        let c = cost("SELECT name FROM products WHERE cat_id IN (SELECT id FROM cats)");
        assert_eq!(c.recommended_path, QueryPath::Olap, "IN subquery should route to OLAP");
        assert!(c.relative_cost >= 0.8, "InSubquery must carry at least 0.8 cost overhead");
    }

    // ── S3-WS1-25: IsNull node tests ─────────────────────────────────────────

    #[test]
    fn planner_is_null_select_produces_is_null_node() {
        let p = plan("SELECT id FROM users WHERE email IS NULL");
        assert!(
            matches!(&p, LogicalPlan::IsNull { .. }),
            "IS NULL predicate must produce outermost IsNull node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_is_null_query_routes_to_oltp() {
        let c = cost("SELECT name FROM customers WHERE deleted_at IS NOT NULL");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "IS NULL check should route to OLTP");
        assert!(c.relative_cost >= 0.1, "IsNull must carry at least 0.1 cost overhead");
    }

    // ── S3-WS1-26: Regexp node tests ─────────────────────────────────────────

    #[test]
    fn planner_regexp_select_produces_regexp_node() {
        let p = plan("SELECT id FROM users WHERE email REGEXP '^[a-z]+'");
        assert!(
            matches!(&p, LogicalPlan::Regexp { .. }),
            "REGEXP predicate must produce outermost Regexp node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_regexp_query_routes_to_olap() {
        let c = cost("SELECT name FROM logs WHERE message RLIKE 'error'");
        assert_eq!(c.recommended_path, QueryPath::Olap, "REGEXP pattern match should route to OLAP");
        assert!(c.relative_cost >= 0.5, "Regexp must carry at least 0.5 cost overhead");
    }

    // ── S3-WS1-27: JsonOp node tests ─────────────────────────────────────────

    #[test]
    fn planner_json_op_select_produces_json_op_node() {
        let p = plan("SELECT data -> '$.name' FROM users");
        assert!(
            matches!(&p, LogicalPlan::JsonOp { .. }),
            "JSON -> operator must produce outermost JsonOp node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_json_op_query_routes_to_olap() {
        let c = cost("SELECT JSON_EXTRACT(data, '$.age') FROM profiles");
        assert_eq!(c.recommended_path, QueryPath::Olap, "JSON operator query should route to OLAP");
        assert!(c.relative_cost >= 0.4, "JsonOp must carry at least 0.4 cost overhead");
    }

    // ── S3-WS1-28: WindowAgg node tests ───────────────────────────────────────

    #[test]
    fn planner_window_agg_select_produces_window_agg_node() {
        let p = plan("SELECT COUNT(id) OVER (PARTITION BY dept) FROM employees");
        assert!(
            matches!(&p, LogicalPlan::WindowAgg { .. }),
            "COUNT() OVER must produce outermost WindowAgg node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("employees"));
        assert!(p.is_read_only());
        assert!(p.has_aggregation(), "WindowAgg must report has_aggregation = true");
    }

    #[test]
    fn cost_window_agg_query_routes_to_olap() {
        let c = cost("SELECT ROW_NUMBER() OVER (ORDER BY salary DESC) FROM staff");
        assert_eq!(c.recommended_path, QueryPath::Olap, "Window aggregate should route to OLAP");
        assert!(c.relative_cost >= 1.5, "WindowAgg must carry at least 1.5 cost overhead");
    }

    // ── S3-WS1-29: Lateral node tests ─────────────────────────────────────────

    #[test]
    fn planner_lateral_select_produces_lateral_node() {
        let p = plan("SELECT u.name, o.total FROM users u JOIN LATERAL (SELECT SUM(amount) AS total FROM orders WHERE orders.user_id = u.id) o ON true");
        assert!(
            matches!(&p, LogicalPlan::Lateral { .. }),
            "LATERAL join must produce outermost Lateral node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("users"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_lateral_query_routes_to_olap() {
        let c = cost("SELECT a.id, b.val FROM accounts a, LATERAL (SELECT val FROM history WHERE history.acct = a.id LIMIT 1) b");
        assert_eq!(c.recommended_path, QueryPath::Olap, "LATERAL subquery should route to OLAP");
        assert!(c.relative_cost >= 0.7, "Lateral must carry at least 0.7 cost overhead");
    }

    // ── S3-WS1-30: Pivot node tests ──────────────────────────────────────────

    #[test]
    fn planner_pivot_select_produces_pivot_node() {
        let p = plan("SELECT * FROM sales PIVOT (SUM(amount) FOR region IN ('East', 'West'))");
        assert!(
            matches!(&p, LogicalPlan::Pivot { .. }),
            "PIVOT clause must produce outermost Pivot node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("sales"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_pivot_query_routes_to_olap() {
        let c = cost("SELECT product, region, sales FROM quarterly_sales UNPIVOT (sales FOR region IN (q1, q2, q3, q4))");
        assert_eq!(c.recommended_path, QueryPath::Olap, "PIVOT/UNPIVOT query should route to OLAP");
        assert!(c.relative_cost >= 0.8, "Pivot must carry at least 0.8 cost overhead");
    }

    // ── S3-WS1-31: Fetch node tests ──────────────────────────────────────────

    #[test]
    fn planner_fetch_select_produces_fetch_node() {
        let p = plan("SELECT id FROM orders ORDER BY id OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY");
        assert!(
            matches!(&p, LogicalPlan::Fetch { .. }),
            "FETCH NEXT must produce outermost Fetch node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("orders"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_fetch_query_routes_to_oltp() {
        let c = cost("SELECT name FROM employees ORDER BY salary DESC FETCH FIRST 10 ROWS ONLY");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "FETCH pagination should route to OLTP");
        assert!(c.relative_cost >= 0.05, "Fetch must carry at least 0.05 cost overhead");
    }

    // ── S3-WS1-32: Values node tests ──────────────────────────────────────────

    #[test]
    fn planner_values_select_produces_values_node() {
        let p = plan("SELECT col FROM (VALUES (10),(20),(30)) AS t(col)");
        assert!(
            matches!(&p, LogicalPlan::Values { .. }),
            "VALUES row source must produce outermost Values node; got: {:?}", p
        );
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_values_query_routes_to_oltp() {
        let c = cost("SELECT a, b FROM (VALUES (1,2),(3,4)) AS v(a,b)");
        assert_eq!(c.recommended_path, QueryPath::Oltp, "VALUES row source should route to OLTP");
        assert!(c.relative_cost >= 0.02, "Values must carry at least 0.02 cost overhead");
    }

    // ── S3-WS1-33: CrossJoin node tests ──────────────────────────────────────

    #[test]
    fn planner_cross_join_select_produces_cross_join_node() {
        let p = plan("SELECT a.id, b.name FROM products a CROSS JOIN categories b");
        assert!(
            matches!(&p, LogicalPlan::CrossJoin { .. }),
            "CROSS JOIN must produce outermost CrossJoin node; got: {:?}", p
        );
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_cross_join_query_routes_to_olap() {
        let c = cost("SELECT x, y FROM t1 CROSS JOIN t2 WHERE t1.id < 10");
        assert_eq!(c.recommended_path, QueryPath::Olap, "CROSS JOIN should route to OLAP");
        assert!(c.relative_cost >= 0.30, "CrossJoin must carry at least 0.30 cost overhead");
    }

    // ── S3-WS1-34: FullTextSearch node tests ───────────────────────────────────

    #[test]
    fn planner_full_text_search_produces_full_text_search_node() {
        let p = plan("SELECT id, title FROM articles WHERE MATCH (title, body) AGAINST ('database engine')");
        assert!(
            matches!(&p, LogicalPlan::FullTextSearch { .. }),
            "MATCH/AGAINST must produce outermost FullTextSearch node; got: {:?}", p
        );
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_full_text_search_routes_to_olap() {
        let c = cost("SELECT id FROM docs WHERE to_tsvector(content) @@ plainto_tsquery('search')");
        assert_eq!(c.recommended_path, QueryPath::Olap, "full-text search should route to OLAP");
        assert!(c.relative_cost >= 0.60, "FullTextSearch must carry at least 0.60 cost overhead");
    }

    // ── S3-WS1-35: GroupingSets node tests ───────────────────────────────────

    #[test]
    fn planner_grouping_sets_select_produces_grouping_sets_node() {
        let p = plan("SELECT region, product, SUM(amount) FROM sales GROUP BY GROUPING SETS ((region), (product))");
        assert!(
            matches!(&p, LogicalPlan::GroupingSets { .. }),
            "GROUPING SETS must produce outermost GroupingSets node; got: {:?}", p
        );
        assert_eq!(p.primary_table(), Some("sales"));
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_grouping_sets_routes_to_olap() {
        let c = cost("SELECT dept, role, COUNT(*) FROM staff GROUP BY GROUPING SETS ((dept), (role))");
        assert_eq!(c.recommended_path, QueryPath::Olap, "GROUPING SETS should route to OLAP");
        assert!(c.relative_cost >= 0.70, "GroupingSets must carry at least 0.70 cost overhead");
    }

    // ── S3-WS1-36: NaturalJoin node tests ────────────────────────────────────

    #[test]
    fn planner_natural_join_select_produces_natural_join_node() {
        let p = plan("SELECT c.id, o.total FROM customers c NATURAL JOIN orders o");
        assert!(
            matches!(&p, LogicalPlan::NaturalJoin { .. }),
            "NATURAL JOIN must produce outermost NaturalJoin node; got: {:?}", p
        );
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_natural_join_routes_to_olap() {
        let c = cost("SELECT p.id FROM products p NATURAL JOIN inventory i WHERE p.active = 1");
        assert_eq!(c.recommended_path, QueryPath::Olap, "NATURAL JOIN should route to OLAP");
        assert!(c.relative_cost >= 0.35, "NaturalJoin must carry at least 0.35 cost overhead");
    }

    // ── S3-WS1-37: UsingJoin node tests ──────────────────────────────────────

    #[test]
    fn planner_using_join_select_produces_using_join_node() {
        let p = plan("SELECT c.id, o.total FROM customers c JOIN orders o USING (customer_id)");
        assert!(
            matches!(&p, LogicalPlan::UsingJoin { .. }),
            "JOIN ... USING must produce outermost UsingJoin node; got: {:?}", p
        );
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_using_join_routes_to_olap() {
        let c = cost("SELECT u.id, p.title FROM users u LEFT JOIN posts p USING (user_id)");
        assert_eq!(c.recommended_path, QueryPath::Olap, "JOIN ... USING should route to OLAP");
        assert!(c.relative_cost >= 0.25, "UsingJoin must carry at least 0.25 cost overhead");
    }

    // ── S3-WS1-38: Except node tests ─────────────────────────────────────────

    #[test]
    fn planner_except_select_produces_except_node() {
        let p = plan("SELECT id FROM active_users EXCEPT SELECT id FROM banned_users");
        assert!(
            matches!(&p, LogicalPlan::Except { .. }),
            "EXCEPT must produce outermost Except node; got: {:?}", p
        );
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_except_routes_to_olap() {
        let c = cost("SELECT id FROM s1 EXCEPT ALL SELECT id FROM s2");
        assert_eq!(c.recommended_path, QueryPath::Olap, "EXCEPT should route to OLAP");
        assert!(c.relative_cost >= 0.45, "Except must carry at least 0.45 cost overhead");
    }

    // ── S3-WS1-39: Intersect node tests ──────────────────────────────────────

    #[test]
    fn planner_intersect_select_produces_intersect_node() {
        let p = plan("SELECT id FROM active_users INTERSECT SELECT id FROM premium_users");
        assert!(
            matches!(&p, LogicalPlan::Intersect { .. }),
            "INTERSECT must produce outermost Intersect node; got: {:?}", p
        );
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_intersect_routes_to_olap() {
        let c = cost("SELECT id FROM s1 INTERSECT ALL SELECT id FROM s2");
        assert_eq!(c.recommended_path, QueryPath::Olap, "INTERSECT should route to OLAP");
        assert!(c.relative_cost >= 0.50, "Intersect must carry at least 0.50 cost overhead");
    }

    // ── S3-WS1-40: Qualify node tests ────────────────────────────────────────

    #[test]
    fn planner_qualify_select_produces_qualify_node() {
        let p = plan("SELECT user_id FROM events QUALIFY score > 0.95");
        assert!(
            matches!(&p, LogicalPlan::Qualify { .. }),
            "QUALIFY must produce outermost Qualify node; got: {:?}", p
        );
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_qualify_routes_to_olap() {
        let c = cost("SELECT user_id FROM events QUALIFY rank <= 3");
        assert_eq!(c.recommended_path, QueryPath::Olap, "QUALIFY should route to OLAP");
        assert!(c.relative_cost >= 0.20, "Qualify must carry at least 0.20 cost overhead");
    }

    // ── S3-WS1-41: WithCte node tests ───────────────────────────────────────

    #[test]
    fn planner_with_cte_select_produces_with_cte_node() {
        let p = plan("WITH recent AS (SELECT id FROM orders) SELECT id FROM recent");
        assert!(
            matches!(&p, LogicalPlan::WithCte { .. }),
            "WITH CTE must produce outermost WithCte node; got: {:?}", p
        );
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_with_cte_routes_to_olap() {
        let c = cost("WITH x AS (SELECT id FROM events) SELECT id FROM x");
        assert_eq!(c.recommended_path, QueryPath::Olap, "WITH CTE should route to OLAP");
        assert!(c.relative_cost >= 0.15, "WithCte must carry at least 0.15 cost overhead");
    }

    // ── S3-WS1-42: RecursiveCte node tests ──────────────────────────────────

    #[test]
    fn planner_recursive_cte_select_produces_recursive_cte_node() {
        let p = plan("WITH RECURSIVE t AS (SELECT 1 AS n) SELECT n FROM t");
        assert!(
            matches!(&p, LogicalPlan::RecursiveCte { .. }),
            "WITH RECURSIVE must produce outermost RecursiveCte node; got: {:?}", p
        );
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_recursive_cte_routes_to_hybrid() {
        let c = cost("WITH RECURSIVE t AS (SELECT 1 AS n) SELECT n FROM t");
        assert_eq!(c.recommended_path, QueryPath::Hybrid, "WITH RECURSIVE should route to Hybrid");
        assert!(c.relative_cost >= 3.00, "RecursiveCte must carry at least 3.00 cost overhead");
    }

    // ── S3-WS1-43: NotExists node tests ─────────────────────────────────────

    #[test]
    fn planner_not_exists_select_produces_not_exists_node() {
        let p = plan("SELECT id FROM users u WHERE NOT EXISTS (SELECT 1 FROM bans b WHERE b.user_id = u.id)");
        assert!(
            matches!(&p, LogicalPlan::NotExists { .. }),
            "NOT EXISTS must produce outermost NotExists node; got: {:?}", p
        );
        assert!(p.is_read_only());
    }

    #[test]
    fn cost_not_exists_routes_to_olap() {
        let c = cost("SELECT id FROM users WHERE NOT EXISTS (SELECT 1 FROM sessions)");
        assert_eq!(c.recommended_path, QueryPath::Olap, "NOT EXISTS should route to OLAP");
        assert!(c.relative_cost >= 2.00, "NotExists must carry at least 2.00 cost overhead");
    }
}