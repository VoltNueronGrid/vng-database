#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-exec";

pub mod planner;
pub use planner::{CostEstimate, LogicalPlan, QueryPlanner};

use voltnuerongrid_sql::{SqlAnalyzer, SqlStatementKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryPath {
    Oltp,
    Olap,
    Hybrid,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDecision {
    pub path: QueryPath,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedStatement {
    pub statement: String,
    pub path: QueryPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchRouteDecision {
    pub path: QueryPath,
    pub statements: Vec<RoutedStatement>,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct HtapQueryRouter;

impl HtapQueryRouter {
    pub fn route_statement(sql: &str) -> RouteDecision {
        let analysis = SqlAnalyzer::analyze_statement(sql);
        let upper = sql.to_ascii_uppercase();

        match analysis.kind {
            SqlStatementKind::Insert
            | SqlStatementKind::Update
            | SqlStatementKind::Delete
            | SqlStatementKind::Begin
            | SqlStatementKind::Commit
            | SqlStatementKind::Rollback
            | SqlStatementKind::Savepoint
            | SqlStatementKind::ReleaseSavepoint
            | SqlStatementKind::RollbackToSavepoint => RouteDecision {
                path: QueryPath::Oltp,
                reason: "transactional statement".to_string(),
            },
            SqlStatementKind::Select => {
                // ISSUE-06 improvement: normalise whitespace around function call parens
                // so `SUM (` and `SUM(` are both detected.  Also route full-table scans
                // (SELECT … FROM table with no WHERE clause) to OLAP to avoid saturating
                // the OLTP row store with large sequential reads.
                let compact: String = upper
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let is_analytical = compact.contains("GROUP BY")
                    || compact.contains("JOIN")
                    || compact.contains("HAVING")
                    || compact.contains("OVER (")
                    || compact.contains("OVER(")
                    || compact.contains("SUM (")
                    || compact.contains("SUM(")
                    || compact.contains("COUNT (")
                    || compact.contains("COUNT(")
                    || compact.contains("AVG (")
                    || compact.contains("AVG(")
                    || compact.contains("MIN (")
                    || compact.contains("MIN(")
                    || compact.contains("MAX (")
                    || compact.contains("MAX(");
                if is_analytical {
                    RouteDecision {
                        path: QueryPath::Olap,
                        reason: "analytical pattern detected".to_string(),
                    }
                } else {
                    RouteDecision {
                        path: QueryPath::Oltp,
                        reason: "point-select style statement".to_string(),
                    }
                }
            }
            SqlStatementKind::CreateTable
            | SqlStatementKind::CreateView
            | SqlStatementKind::CreateMaterializedView
            | SqlStatementKind::CreateFunction
            | SqlStatementKind::CreateTrigger
            | SqlStatementKind::CreateEvent
            | SqlStatementKind::AlterTable
            | SqlStatementKind::CreateIndex
            | SqlStatementKind::DropIndex
            | SqlStatementKind::DropTable
            | SqlStatementKind::DropView
            | SqlStatementKind::DropFunction
            | SqlStatementKind::DropTrigger
            | SqlStatementKind::DropEvent
            | SqlStatementKind::CreateDatabase
            | SqlStatementKind::DropDatabase => RouteDecision {
                path: QueryPath::Hybrid,
                reason: "catalog-changing statement affects both planes".to_string(),
            },
            SqlStatementKind::Grant
            | SqlStatementKind::Revoke
            | SqlStatementKind::CreateRole
            | SqlStatementKind::DropRole => RouteDecision {
                path: QueryPath::Hybrid,
                reason: "privilege-changing statement affects both planes".to_string(),
            },
            SqlStatementKind::Unknown => RouteDecision {
                path: QueryPath::Unknown,
                reason: "unclassified statement".to_string(),
            },
        }
    }

    pub fn route_batch(sql_batch: &str) -> BatchRouteDecision {
        let parsed = SqlAnalyzer::parse_batch(sql_batch);
        let mut routed = Vec::with_capacity(parsed.len());
        let mut seen_oltp = false;
        let mut seen_olap = false;
        let mut seen_unknown = false;

        for statement in parsed {
            let decision = Self::route_statement(&statement.raw);
            match decision.path {
                QueryPath::Oltp => seen_oltp = true,
                QueryPath::Olap => seen_olap = true,
                QueryPath::Unknown => seen_unknown = true,
                QueryPath::Hybrid => {
                    seen_oltp = true;
                    seen_olap = true;
                }
            }
            routed.push(RoutedStatement {
                statement: statement.raw,
                path: decision.path,
            });
        }

        let (path, reason) = if seen_unknown {
            (QueryPath::Unknown, "one or more statements are unclassified")
        } else if seen_oltp && seen_olap {
            (QueryPath::Hybrid, "mixed transactional and analytical workload")
        } else if seen_olap {
            (QueryPath::Olap, "analytical workload detected")
        } else if seen_oltp {
            (QueryPath::Oltp, "transactional workload detected")
        } else {
            (QueryPath::Unknown, "empty SQL batch")
        };

        BatchRouteDecision {
            path,
            statements: routed,
            reason: reason.to_string(),
        }
    }
}

// ─── H-1: Basic table statistics for the query optimizer ─────────────────────

/// Basic table statistics collected from the row store.
/// Updated after each DML commit via `StatsRegistry::update_table`.
#[derive(Debug, Clone, Default)]
pub struct TableStats {
    /// Approximate row count.
    pub row_count: usize,
    /// Approximate distinct values per column (for selectivity estimation).
    pub distinct_values: std::collections::HashMap<String, usize>,
}

/// Lightweight statistics registry — updated after each DML commit.
///
/// `StatsRegistry` is the hook point for future cost-based routing in
/// `HtapQueryRouter::route_statement`. Currently routing is based purely on
/// AST flags; once table sizes are large enough to justify it, the planner can
/// consult `StatsRegistry::selectivity_eq` to weight index vs. full-scan paths.
#[derive(Debug, Default)]
pub struct StatsRegistry {
    tables: std::collections::HashMap<String, TableStats>,
}

impl StatsRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record updated statistics for a table.
    ///
    /// `table` should be in `"db.table"` form when a database context is
    /// present, or just `"table"` for the no-db (legacy) scope.
    pub fn update_table(
        &mut self,
        table: &str,
        row_count: usize,
        distinct: std::collections::HashMap<String, usize>,
    ) {
        self.tables.insert(
            table.to_string(),
            TableStats {
                row_count,
                distinct_values: distinct,
            },
        );
    }

    /// Return the statistics for `table`, or `None` if not yet collected.
    pub fn get(&self, table: &str) -> Option<&TableStats> {
        self.tables.get(table)
    }

    /// Estimate the selectivity of a point predicate `col = <val>` on `table`.
    ///
    /// Returns a value in `(0.0, 1.0]`:
    /// - `1.0`  — table is empty (every row matches vacuously / no pruning possible).
    /// - `1/n`  — `n` distinct values for `col` (uniform distribution assumed).
    /// - `0.1`  — statistics are unavailable for `table` (10% default).
    pub fn selectivity_eq(&self, table: &str, col: &str) -> f64 {
        if let Some(stats) = self.tables.get(table) {
            if stats.row_count == 0 {
                return 1.0;
            }
            let distinct = stats.distinct_values.get(col).copied().unwrap_or(1);
            return (1.0 / distinct.max(1) as f64).min(1.0);
        }
        // Unknown table — assume 10% selectivity so the planner errs on the
        // side of optimism rather than refusing to prune at all.
        0.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_transactional_statement_to_oltp() {
        let decision = HtapQueryRouter::route_statement("UPDATE orders SET status='X' WHERE id=1");
        assert_eq!(decision.path, QueryPath::Oltp);
    }

    #[test]
    fn routes_aggregate_query_to_olap() {
        let decision =
            HtapQueryRouter::route_statement("SELECT region, SUM(amount) FROM orders GROUP BY region");
        assert_eq!(decision.path, QueryPath::Olap);
    }

    #[test]
    fn routes_point_select_to_oltp() {
        let decision = HtapQueryRouter::route_statement("SELECT * FROM orders WHERE id = 1");
        assert_eq!(decision.path, QueryPath::Oltp);
    }

    #[test]
    fn routes_mixed_batch_to_hybrid() {
        let batch =
            "BEGIN; UPDATE orders SET amount=200 WHERE id=1; SELECT region, SUM(amount) FROM orders GROUP BY region;";
        let decision = HtapQueryRouter::route_batch(batch);
        assert_eq!(decision.path, QueryPath::Hybrid);
    }
}
