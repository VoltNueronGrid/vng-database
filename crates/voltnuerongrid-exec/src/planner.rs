//! SQL logical planner and cost estimator.
//!
//! Converts a parsed [`voltnuerongrid_sql::Statement`] AST into a [`LogicalPlan`]
//! tree, then produces a [`CostEstimate`] to drive HTAP path selection.
//!
//! Advances sprint backlog item S3-WS1-05 (planner/optimizer + cost model).

use voltnuerongrid_sql::{
    DeleteStatement, InsertStatement, SelectStatement, Statement, UpdateStatement,
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
    /// Unrecognised or unparseable statement.
    Unknown(String),
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
            | LogicalPlan::Limit { input, .. } => input.primary_table(),
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
            | LogicalPlan::Limit { input, .. } => input.has_aggregation(),
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

        // Filter
        let after_filter = if let Some(pred) = &sel.where_clause {
            LogicalPlan::Filter {
                input: Box::new(scan),
                predicate: pred.clone(),
            }
        } else {
            scan
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

        // Sort (ORDER BY)
        let after_sort = if !sel.order_by.is_empty() {
            LogicalPlan::Sort {
                input: Box::new(after_agg),
                order_by: sel
                    .order_by
                    .iter()
                    .map(|o| (o.column.clone(), o.descending))
                    .collect(),
            }
        } else {
            after_agg
        };

        // Limit
        let after_limit = if let Some(n) = sel.limit {
            LogicalPlan::Limit {
                input: Box::new(after_sort),
                count: n,
            }
        } else {
            after_sort
        };

        // Project (only when not SELECT *)
        if sel.columns != vec!["*".to_string()] && !sel.columns.is_empty() {
            LogicalPlan::Project {
                input: Box::new(after_limit),
                columns: sel.columns.clone(),
            }
        } else {
            after_limit
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
}
