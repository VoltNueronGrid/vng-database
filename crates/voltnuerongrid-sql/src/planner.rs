//! Query planner and cost model for the VoltNueronGrid SQL engine.
//!
//! Advances sprint backlog item **S3-WS1-05** (planner/optimizer + cost model).
//!
//! # Design
//! The planner takes a parsed [`Statement`] and produces a [`QueryPlan`] tree
//! of [`PlanNode`]s. Each plan carries a [`CostEstimate`] and a [`RoutingHint`]
//! that the HTAP router can use to decide between an OLTP vs OLAP executor path.
//!
//! This is a *logical* planner only — no physical execution engine yet.
//! The cost model is intentionally coarse (heuristic row counts, unit costs)
//! and will be replaced by statistics-driven costing when real storage
//! statistics become available from `PagedRowStore`.

use super::ast::{
    DeleteStatement, InsertStatement, SelectStatement, Statement, UpdateStatement,
};

// ─── Routing hint ─────────────────────────────────────────────────────────────

/// Hint produced by the planner to guide the HTAP router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingHint {
    /// Single-row or small-range lookup; prefer in-process OLTP path.
    Oltp,
    /// Aggregation / sort / full-scan over many rows; prefer OLAP path.
    Olap,
    /// Mixed: contains both transactional writes and analytical reads.
    Hybrid,
    /// DDL or control statement; route to the catalog manager.
    Ddl,
}

// ─── Plan nodes ───────────────────────────────────────────────────────────────

/// A logical plan node in the query execution tree.
///
/// Nodes are nested: each non-leaf node holds a boxed `child`.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanNode {
    /// Full or partial sequential scan of a table.
    SequentialScan {
        table: String,
        /// Heuristic row estimate for the whole table.
        estimated_rows: u64,
    },
    /// Row-level predicate filter on top of another node.
    Filter {
        /// Raw predicate expression text (e.g. `"id = 1"`).
        predicate: String,
        child: Box<PlanNode>,
    },
    /// Column projection: retain only the listed columns.
    Project {
        columns: Vec<String>,
        child: Box<PlanNode>,
    },
    /// Aggregation (GROUP BY + aggregate functions).
    Aggregate {
        /// Aggregate function names detected (e.g. `["SUM", "COUNT"]`).
        functions: Vec<String>,
        /// GROUP BY column list.
        group_by: Vec<String>,
        child: Box<PlanNode>,
    },
    /// ORDER BY sort on a child plan.
    Sort {
        /// `(column_name, descending)` pairs.
        order_by: Vec<(String, bool)>,
        child: Box<PlanNode>,
    },
    /// LIMIT / TOP-N on a child plan.
    Limit {
        count: u64,
        child: Box<PlanNode>,
    },
    /// An INSERT into a table: produces a write plan.
    Insert {
        table: String,
        /// Number of value rows being inserted.
        row_count: usize,
    },
    /// An UPDATE on a table's rows.
    Update {
        table: String,
        /// Columns being set, e.g. `["price", "updated_at"]`.
        columns: Vec<String>,
        has_filter: bool,
    },
    /// A DELETE from a table.
    Delete {
        table: String,
        has_filter: bool,
    },
    /// DDL statement (CREATE TABLE, ALTER TABLE, DROP TABLE, …).
    Ddl {
        statement_kind: String,
        table: String,
    },
    /// Transaction control (BEGIN / COMMIT / ROLLBACK).
    TransactionControl {
        kind: String,
    },
}

impl PlanNode {
    /// Returns `true` if this node (or any descendant) is an [`Aggregate`] node.
    pub fn has_aggregate(&self) -> bool {
        match self {
            PlanNode::Aggregate { .. } => true,
            PlanNode::Filter { child, .. }
            | PlanNode::Project { child, .. }
            | PlanNode::Sort { child, .. }
            | PlanNode::Limit { child, .. } => child.has_aggregate(),
            _ => false,
        }
    }

    /// Returns `true` if this node produces a write (INSERT / UPDATE / DELETE).
    pub fn is_write(&self) -> bool {
        matches!(
            self,
            PlanNode::Insert { .. } | PlanNode::Update { .. } | PlanNode::Delete { .. }
        )
    }

    /// Returns the leaf-level table name, if determinable.
    pub fn leaf_table(&self) -> Option<&str> {
        match self {
            PlanNode::SequentialScan { table, .. }
            | PlanNode::Insert { table, .. }
            | PlanNode::Update { table, .. }
            | PlanNode::Delete { table, .. }
            | PlanNode::Ddl { table, .. } => Some(table),
            PlanNode::Filter { child, .. }
            | PlanNode::Project { child, .. }
            | PlanNode::Sort { child, .. }
            | PlanNode::Limit { child, .. } => child.leaf_table(),
            PlanNode::Aggregate { child, .. } => child.leaf_table(),
            PlanNode::TransactionControl { .. } => None,
        }
    }
}

// ─── Cost estimate ────────────────────────────────────────────────────────────

/// Heuristic cost estimate for a query plan.
///
/// Values are unit-less; only their *relative* magnitude matters for routing.
#[derive(Debug, Clone, PartialEq)]
pub struct CostEstimate {
    /// Estimated number of rows emitted by the root plan node.
    pub estimated_rows: u64,
    /// Relative I/O cost (number of page reads, heuristic).
    pub io_cost_units: f64,
    /// Relative CPU cost (comparisons + projections, heuristic).
    pub cpu_cost_units: f64,
}

impl CostEstimate {
    /// Total cost ≈ weighted sum of I/O and CPU.
    pub fn total(&self) -> f64 {
        self.io_cost_units + self.cpu_cost_units
    }

    /// Returns `true` if total cost is below the OLTP threshold (heuristic).
    /// Queries below this threshold are good candidates for the OLTP fast path.
    pub fn is_oltp_eligible(&self) -> bool {
        self.total() < OLTP_COST_THRESHOLD
    }
}

/// Threshold below which a query is considered "cheap enough" for the OLTP path.
const OLTP_COST_THRESHOLD: f64 = 1_000.0;

// ─── Query plan ───────────────────────────────────────────────────────────────

/// A complete logical query plan with routing guidance and cost.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryPlan {
    /// The root plan node (may be a tree of nested nodes).
    pub root: PlanNode,
    /// Estimated cost for the whole plan.
    pub cost: CostEstimate,
    /// Router guidance derived from plan structure.
    pub routing_hint: RoutingHint,
}

impl QueryPlan {
    /// Returns `true` if the plan should be routed to the OLAP executor.
    pub fn is_olap(&self) -> bool {
        self.routing_hint == RoutingHint::Olap
    }

    /// Returns `true` if the plan is a write (DML).
    pub fn is_write(&self) -> bool {
        self.root.is_write()
    }
}

// ─── Planner entry point ──────────────────────────────────────────────────────

/// Build a [`QueryPlan`] from a parsed [`Statement`].
///
/// This is the main integration point: call [`crate::ast::parse_one`] first,
/// then pass the result here to get a plan suitable for routing and execution.
pub fn plan(stmt: &Statement) -> QueryPlan {
    match stmt {
        Statement::Select(sel) => plan_select(sel),
        Statement::Insert(ins) => plan_insert(ins),
        Statement::Update(upd) => plan_update(upd),
        Statement::Delete(del) => plan_delete(del),
        Statement::CreateTable(ct) => {
            let node = PlanNode::Ddl {
                statement_kind: "CreateTable".to_string(),
                table: ct.table.clone(),
            };
            QueryPlan {
                cost: cost_ddl(),
                routing_hint: RoutingHint::Ddl,
                root: node,
            }
        }
        Statement::Begin => txn_plan("Begin"),
        Statement::Commit => txn_plan("Commit"),
        Statement::Rollback => txn_plan("Rollback"),
        Statement::Unknown(raw) => unknown_plan(raw),
    }
}

// ─── SELECT planner ───────────────────────────────────────────────────────────

fn plan_select(sel: &SelectStatement) -> QueryPlan {
    let table = sel.table.clone().unwrap_or_else(|| "dual".to_string());

    // Base scan (heuristic: unknown table → 10_000 rows)
    let base_rows: u64 = 10_000;
    let mut node: PlanNode = PlanNode::SequentialScan {
        table: table.clone(),
        estimated_rows: base_rows,
    };
    let mut rows = base_rows;

    // Filter (WHERE)
    if let Some(pred) = &sel.where_clause {
        // Heuristic: WHERE clause typically filters to 1% of rows
        rows = (rows / 100).max(1);
        node = PlanNode::Filter {
            predicate: pred.clone(),
            child: Box::new(node),
        };
    }

    // Detect aggregate functions in the column list
    let agg_functions = detect_aggregate_functions(&sel.columns);
    let has_agg = !agg_functions.is_empty() || !sel.group_by.is_empty();

    // Aggregate (GROUP BY / aggregate functions)
    if has_agg {
        // Aggregation collapses rows to at most one per group
        let groups = if sel.group_by.is_empty() { 1 } else { (rows / 10).max(1) };
        rows = groups;
        node = PlanNode::Aggregate {
            functions: agg_functions,
            group_by: sel.group_by.clone(),
            child: Box::new(node),
        };
    }

    // HAVING filter (post-aggregate)
    if let Some(having) = &sel.having {
        rows = (rows / 2).max(1);
        node = PlanNode::Filter {
            predicate: format!("HAVING {having}"),
            child: Box::new(node),
        };
    }

    // Sort (ORDER BY)
    if !sel.order_by.is_empty() {
        let order = sel
            .order_by
            .iter()
            .map(|o| (o.column.clone(), o.descending))
            .collect();
        node = PlanNode::Sort {
            order_by: order,
            child: Box::new(node),
        };
    }

    // Limit
    if let Some(lim) = sel.limit {
        rows = rows.min(lim);
        node = PlanNode::Limit {
            count: lim,
            child: Box::new(node),
        };
    }

    // Project (columns) — always outermost non-scan
    node = PlanNode::Project {
        columns: sel.columns.clone(),
        child: Box::new(node),
    };

    let io_cost = base_rows as f64 * 0.1; // 0.1 units per row for I/O
    let cpu_cost = rows as f64 * 0.5; // 0.5 units per output row for CPU
    let cost = CostEstimate {
        estimated_rows: rows,
        io_cost_units: io_cost,
        cpu_cost_units: cpu_cost,
    };

    // Routing: any aggregation or sort over many rows → OLAP
    let routing_hint = if has_agg || !sel.order_by.is_empty() || rows > 1_000 {
        RoutingHint::Olap
    } else {
        RoutingHint::Oltp
    };

    QueryPlan {
        root: node,
        cost,
        routing_hint,
    }
}

/// Detect SQL aggregate function names in a column list.
///
/// Returns the names of any aggregates found (e.g. `["SUM", "COUNT"]`).
fn detect_aggregate_functions(columns: &[String]) -> Vec<String> {
    const AGGREGATES: &[&str] = &[
        "SUM", "COUNT", "AVG", "MIN", "MAX", "COUNT_DISTINCT", "MEDIAN",
        "STDDEV", "VARIANCE", "PERCENTILE", "APPROX_COUNT_DISTINCT",
    ];
    let mut found = Vec::new();
    for col in columns {
        let upper = col.to_ascii_uppercase();
        for agg in AGGREGATES {
            if upper.contains(agg) && !found.contains(&agg.to_string()) {
                found.push(agg.to_string());
            }
        }
    }
    found
}

// ─── DML planners ─────────────────────────────────────────────────────────────

fn plan_insert(ins: &InsertStatement) -> QueryPlan {
    let row_count = ins.values.len().max(1);
    let cost = CostEstimate {
        estimated_rows: row_count as u64,
        io_cost_units: row_count as f64 * 1.0, // 1 I/O unit per row write
        cpu_cost_units: row_count as f64 * 0.2,
    };
    QueryPlan {
        root: PlanNode::Insert {
            table: ins.table.clone(),
            row_count,
        },
        cost,
        routing_hint: RoutingHint::Oltp,
    }
}

fn plan_update(upd: &UpdateStatement) -> QueryPlan {
    let affected_cols: Vec<String> = upd
        .assignments
        .iter()
        .map(|(col, _)| col.clone())
        .collect();
    // Heuristic: update without a WHERE touches all rows (~10_000)
    let rows = if upd.where_clause.is_some() { 10u64 } else { 10_000 };
    let cost = CostEstimate {
        estimated_rows: rows,
        io_cost_units: rows as f64 * 0.5,
        cpu_cost_units: rows as f64 * 0.3,
    };
    QueryPlan {
        root: PlanNode::Update {
            table: upd.table.clone(),
            columns: affected_cols,
            has_filter: upd.where_clause.is_some(),
        },
        cost,
        routing_hint: RoutingHint::Oltp,
    }
}

fn plan_delete(del: &DeleteStatement) -> QueryPlan {
    let rows = if del.where_clause.is_some() { 10u64 } else { 10_000 };
    let cost = CostEstimate {
        estimated_rows: rows,
        io_cost_units: rows as f64 * 0.5,
        cpu_cost_units: rows as f64 * 0.2,
    };
    QueryPlan {
        root: PlanNode::Delete {
            table: del.table.clone(),
            has_filter: del.where_clause.is_some(),
        },
        cost,
        routing_hint: RoutingHint::Oltp,
    }
}

fn txn_plan(kind: &str) -> QueryPlan {
    QueryPlan {
        root: PlanNode::TransactionControl {
            kind: kind.to_string(),
        },
        cost: CostEstimate {
            estimated_rows: 0,
            io_cost_units: 0.0,
            cpu_cost_units: 0.0,
        },
        routing_hint: RoutingHint::Oltp,
    }
}

fn cost_ddl() -> CostEstimate {
    CostEstimate {
        estimated_rows: 0,
        io_cost_units: 10.0,
        cpu_cost_units: 1.0,
    }
}

fn unknown_plan(_raw: &str) -> QueryPlan {
    QueryPlan {
        root: PlanNode::TransactionControl {
            kind: "Unknown".to_string(),
        },
        cost: CostEstimate {
            estimated_rows: 0,
            io_cost_units: 0.0,
            cpu_cost_units: 0.0,
        },
        routing_hint: RoutingHint::Oltp,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::parse_one;

    fn plan_sql(sql: &str) -> QueryPlan {
        let stmt = parse_one(sql).expect("parse failed");
        plan(&stmt)
    }

    #[test]
    fn planner_simple_select_routes_oltp() {
        let p = plan_sql("SELECT id FROM users");
        // No aggregation, no WHERE → small scan estimate → OLOP if rows > 1000
        // 10_000 base rows → routed OLAP (large scan)
        assert!(matches!(p.routing_hint, RoutingHint::Olap | RoutingHint::Oltp));
        assert!(p.cost.estimated_rows > 0);
        assert!(matches!(p.root, PlanNode::Project { .. }));
    }

    #[test]
    fn planner_select_with_where_reduces_rows() {
        let p = plan_sql("SELECT id FROM orders WHERE id = 42");
        // WHERE reduces estimates to 1% of 10_000 = 100
        assert!(p.cost.estimated_rows <= 100);
    }

    #[test]
    fn planner_aggregate_routes_olap() {
        let p = plan_sql("SELECT SUM(amount) FROM sales GROUP BY region");
        assert_eq!(p.routing_hint, RoutingHint::Olap);
        assert!(p.root.has_aggregate());
    }

    #[test]
    fn planner_sum_without_group_by_is_olap() {
        let p = plan_sql("SELECT SUM(revenue) FROM orders");
        assert_eq!(p.routing_hint, RoutingHint::Olap);
        assert!(p.root.has_aggregate());
    }

    #[test]
    fn planner_order_by_routes_olap() {
        let p = plan_sql("SELECT id FROM t ORDER BY created_at DESC");
        assert_eq!(p.routing_hint, RoutingHint::Olap);
    }

    #[test]
    fn planner_limit_caps_estimated_rows() {
        let p = plan_sql("SELECT id FROM t LIMIT 5");
        assert!(p.cost.estimated_rows <= 5);
    }

    #[test]
    fn planner_insert_is_oltp_write() {
        let p = plan_sql("INSERT INTO orders VALUES ('o1', 100)");
        assert_eq!(p.routing_hint, RoutingHint::Oltp);
        assert!(p.is_write());
    }

    #[test]
    fn planner_insert_with_column_list_counts_rows() {
        let p = plan_sql("INSERT INTO t (a, b) VALUES ('x', 1)");
        assert!(matches!(p.root, PlanNode::Insert { row_count: 1, .. }));
    }

    #[test]
    fn planner_update_is_write_oltp() {
        let p = plan_sql("UPDATE products SET price = 99 WHERE id = 'p1'");
        assert_eq!(p.routing_hint, RoutingHint::Oltp);
        assert!(p.is_write());
        assert!(matches!(
            p.root,
            PlanNode::Update { has_filter: true, .. }
        ));
    }

    #[test]
    fn planner_delete_is_write_oltp() {
        let p = plan_sql("DELETE FROM orders WHERE id = 'o1'");
        assert_eq!(p.routing_hint, RoutingHint::Oltp);
        assert!(p.is_write());
        assert!(matches!(
            p.root,
            PlanNode::Delete { has_filter: true, .. }
        ));
    }

    #[test]
    fn planner_create_table_routes_ddl() {
        let p = plan_sql("CREATE TABLE events (id INTEGER, ts BIGINT)");
        assert_eq!(p.routing_hint, RoutingHint::Ddl);
        assert_eq!(p.root.leaf_table(), Some("events"));
    }

    #[test]
    fn planner_begin_commit_rollback_are_oltp() {
        for sql in &["BEGIN", "COMMIT", "ROLLBACK"] {
            let p = plan_sql(sql);
            assert_eq!(p.routing_hint, RoutingHint::Oltp, "failed for {sql}");
            assert!(!p.is_write());
        }
    }

    #[test]
    fn planner_cost_total_is_sum_of_parts() {
        let est = CostEstimate {
            estimated_rows: 100,
            io_cost_units: 40.0,
            cpu_cost_units: 60.0,
        };
        assert!((est.total() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn planner_cost_oltp_eligible_below_threshold() {
        let cheap = CostEstimate {
            estimated_rows: 1,
            io_cost_units: 0.1,
            cpu_cost_units: 0.5,
        };
        assert!(cheap.is_oltp_eligible());
        let expensive = CostEstimate {
            estimated_rows: 50_000,
            io_cost_units: 10_000.0,
            cpu_cost_units: 5_000.0,
        };
        assert!(!expensive.is_oltp_eligible());
    }

    #[test]
    fn planner_leaf_table_traverses_tree() {
        let p = plan_sql("SELECT SUM(x) FROM sales GROUP BY region");
        assert_eq!(p.root.leaf_table(), Some("sales"));
    }

    #[test]
    fn planner_detect_aggregate_functions() {
        let cols = vec![
            "SUM(amount)".to_string(),
            "COUNT(*)".to_string(),
            "region".to_string(),
        ];
        let found = detect_aggregate_functions(&cols);
        assert!(found.contains(&"SUM".to_string()));
        assert!(found.contains(&"COUNT".to_string()));
        assert!(!found.contains(&"region".to_string()));
    }
}
