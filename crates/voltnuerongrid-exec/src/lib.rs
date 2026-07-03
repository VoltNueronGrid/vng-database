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
                let has_cross_dialect_scalar_fn = upper.contains(" IFNULL(")
                    || upper.contains(" NVL(")
                    || upper.contains(" NVL2(")
                    || upper.contains(" IFF(")
                    || upper.contains(" DECODE(")
                    || upper.contains(" DATEADD(")
                    || upper.contains(" DATEDIFF(")
                    || upper.contains(" ZEROIFNULL(")
                    || upper.contains(" NULLIFZERO(")
                    || upper.contains(" TO_TIMESTAMP_NTZ(")
                    || upper.contains(" TO_TIMESTAMP_TZ(")
                    || upper.contains(" JSON_EXTRACT(")
                    || upper.contains(" JSON_OBJECT(")
                    || upper.contains(" OBJECT_CONSTRUCT(")
                    || upper.contains(" ARRAY_CONSTRUCT(")
                    || upper.contains(" TRY_CAST(")
                    || upper.contains(" TO_VARCHAR(")
                    || upper.contains(" TO_NUMBER(")
                    || upper.contains(" PIVOT(")
                    || upper.contains(" TRY_TO_");

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
                    || compact.contains("IN (SELECT")
                    || compact.contains("IN(SELECT")
                    || compact.contains("EXISTS (SELECT")
                    || compact.contains("EXISTS(SELECT")
                    || compact.contains("SUM (")
                    || compact.contains("SUM(")
                    || compact.contains("COUNT (")
                    || compact.contains("COUNT(")
                    || compact.contains("AVG (")
                    || compact.contains("AVG(")
                    || compact.contains("MIN (")
                    || compact.contains("MIN(")
                    || compact.contains("MAX (")
                    || compact.contains("MAX(")
                    || has_cross_dialect_scalar_fn;
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
            | SqlStatementKind::DropDatabase
            | SqlStatementKind::CreateSchema
            | SqlStatementKind::DropSchema => RouteDecision {
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

    /// Q-1 / AR-2: Cost-based routing refinement.
    ///
    /// Starts from the pure AST routing decision in [`route_statement`], then
    /// consults `stats` to override the path when the table size makes the
    /// default choice a poor one:
    ///
    /// * A `SELECT` that *looks* analytical (GROUP BY, aggregate, …) but targets
    ///   a small table (`< OLAP_MIN_ROWS`) is routed back to **OLTP** — the
    ///   DataFusion set-up cost outweighs any scan benefit for tiny tables.
    /// * A point-style `SELECT` against a very large table (`>= OLAP_MIN_ROWS`)
    ///   with no usable equality predicate is promoted to **OLAP** so a large
    ///   sequential scan does not saturate the OLTP row store.
    ///
    /// `db` is the optional database scope used to build the `"db.table"` stats
    /// key; pass `None` for the legacy no-database scope.
    ///
    /// Only a shared read of `stats` is required — this never mutates the
    /// registry, so it is safe to call on the hot path under a read lock.
    pub fn route_with_stats(
        sql: &str,
        stats: &StatsRegistry,
        db: Option<&str>,
    ) -> RouteDecision {
        let base = Self::route_statement(sql);
        let analysis = SqlAnalyzer::analyze_statement(sql);
        if analysis.kind != SqlStatementKind::Select {
            return base;
        }
        let Some(table) = extract_primary_table(sql) else {
            return base;
        };
        let stats_key = match db {
            Some(d) if !d.is_empty() => format!("{}.{}", d.to_ascii_lowercase(), table),
            _ => table.clone(),
        };
        // Fall back to the bare table name when the db-qualified key is unknown.
        let row_count = stats
            .get(&stats_key)
            .or_else(|| stats.get(&table))
            .map(|s| s.row_count);
        let Some(row_count) = row_count else {
            // No statistics collected yet — keep the AST decision.
            return base;
        };
        let upper: String = sql.to_ascii_uppercase().split_whitespace().collect::<Vec<_>>().join(" ");
        let has_equality_predicate = upper.contains("WHERE ") && upper.contains('=');
        let has_cross_dialect_scalar_fn = upper.contains(" IFNULL(")
            || upper.contains(" NVL(")
            || upper.contains(" NVL2(")
            || upper.contains(" IFF(")
            || upper.contains(" DECODE(")
            || upper.contains(" DATEADD(")
            || upper.contains(" DATEDIFF(")
            || upper.contains(" ZEROIFNULL(")
            || upper.contains(" NULLIFZERO(")
            || upper.contains(" TO_TIMESTAMP_NTZ(")
            || upper.contains(" TO_TIMESTAMP_TZ(")
            || upper.contains(" JSON_EXTRACT(")
            || upper.contains(" JSON_OBJECT(")
            || upper.contains(" OBJECT_CONSTRUCT(")
            || upper.contains(" ARRAY_CONSTRUCT(")
            || upper.contains(" TRY_CAST(")
            || upper.contains(" TO_VARCHAR(")
            || upper.contains(" TO_NUMBER(")
            || upper.contains(" PIVOT(")
            || upper.contains(" TRY_TO_");
        // A JOIN (or set operation) can only be executed on the OLAP/DataFusion
        // plane — the OLTP row-store path has no join executor. Never demote
        // such queries to OLTP regardless of table size.
        let requires_olap_engine = upper.contains(" JOIN ")
            || upper.contains(" UNION ")
            || upper.contains(" INTERSECT ")
            || upper.contains(" EXCEPT ")
            || has_cross_dialect_scalar_fn;
        match base.path {
            QueryPath::Olap if row_count < OLAP_MIN_ROWS && !requires_olap_engine => {
                RouteDecision {
                    path: QueryPath::Oltp,
                    reason: format!(
                        "cost override: '{table}' has {row_count} rows (< {OLAP_MIN_ROWS}); OLTP avoids OLAP setup cost"
                    ),
                }
            }
            QueryPath::Oltp if row_count >= OLAP_MIN_ROWS && !has_equality_predicate => {
                RouteDecision {
                    path: QueryPath::Olap,
                    reason: format!(
                        "cost override: '{table}' has {row_count} rows (>= {OLAP_MIN_ROWS}) with no equality predicate; OLAP avoids large OLTP scan"
                    ),
                }
            }
            _ => base,
        }
    }
}

/// Q-1 / AR-2: Row-count threshold above which a full-table read is cheaper on
/// the OLAP (columnar/DataFusion) plane than on the OLTP row store.
pub const OLAP_MIN_ROWS: usize = 10_000;

/// Q-1: Extract the primary (first `FROM`) table name from a SELECT statement,
/// lower-cased and stripped of schema/alias decoration. Returns `None` when no
/// `FROM` clause is present (e.g. `SELECT 1`).
pub fn extract_primary_table(sql: &str) -> Option<String> {
    let lower = sql.to_ascii_lowercase();
    let from_pos = lower.find(" from ")?;
    let after = &lower[from_pos + 6..];
    let token = after
        .split_whitespace()
        .next()?
        .trim_matches(|c: char| c == '(' || c == ')' || c == ',' || c == ';');
    if token.is_empty() {
        return None;
    }
    // Strip schema/database qualifier: keep the last dotted segment.
    let table = token.rsplit('.').next().unwrap_or(token);
    Some(table.to_string())
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

    #[test]
    fn extracts_primary_table_from_select() {
        assert_eq!(
            extract_primary_table("SELECT * FROM orders WHERE id = 1").as_deref(),
            Some("orders")
        );
        assert_eq!(
            extract_primary_table("SELECT region, SUM(amount) FROM sales GROUP BY region").as_deref(),
            Some("sales")
        );
        assert_eq!(
            extract_primary_table("SELECT * FROM shop.orders").as_deref(),
            Some("orders")
        );
        assert_eq!(extract_primary_table("SELECT 1").as_deref(), None);
    }

    #[test]
    fn q1_small_table_aggregate_routes_to_oltp() {
        let mut stats = StatsRegistry::new();
        stats.update_table("orders", 10, std::collections::HashMap::new());
        let decision = HtapQueryRouter::route_with_stats(
            "SELECT region, SUM(amount) FROM orders GROUP BY region",
            &stats,
            None,
        );
        assert_eq!(
            decision.path,
            QueryPath::Oltp,
            "tiny table aggregate must avoid OLAP setup cost"
        );
    }

    #[test]
    fn q1_large_table_aggregate_stays_olap() {
        let mut stats = StatsRegistry::new();
        stats.update_table("orders", 1_000_000, std::collections::HashMap::new());
        let decision = HtapQueryRouter::route_with_stats(
            "SELECT region, SUM(amount) FROM orders GROUP BY region",
            &stats,
            None,
        );
        assert_eq!(decision.path, QueryPath::Olap, "large table aggregate stays OLAP");
    }

    #[test]
    fn q1_large_table_full_scan_promoted_to_olap() {
        let mut stats = StatsRegistry::new();
        stats.update_table("events", 500_000, std::collections::HashMap::new());
        // Point-style SELECT with no equality predicate over a huge table.
        let decision =
            HtapQueryRouter::route_with_stats("SELECT * FROM events", &stats, None);
        assert_eq!(
            decision.path,
            QueryPath::Olap,
            "large unfiltered scan must be promoted to OLAP"
        );
    }

    #[test]
    fn q1_point_select_with_predicate_stays_oltp() {
        let mut stats = StatsRegistry::new();
        stats.update_table("events", 500_000, std::collections::HashMap::new());
        let decision = HtapQueryRouter::route_with_stats(
            "SELECT * FROM events WHERE id = 42",
            &stats,
            None,
        );
        assert_eq!(
            decision.path,
            QueryPath::Oltp,
            "indexed point lookup stays OLTP regardless of table size"
        );
    }

    #[test]
    fn q1_unknown_stats_keep_ast_decision() {
        let stats = StatsRegistry::new();
        let decision = HtapQueryRouter::route_with_stats(
            "SELECT region, SUM(amount) FROM orders GROUP BY region",
            &stats,
            None,
        );
        // No stats → fall back to AST decision (OLAP for aggregate).
        assert_eq!(decision.path, QueryPath::Olap);
    }

    #[test]
    fn q1_db_qualified_stats_key_resolves() {
        let mut stats = StatsRegistry::new();
        stats.update_table("shop.orders", 5, std::collections::HashMap::new());
        let decision = HtapQueryRouter::route_with_stats(
            "SELECT region, SUM(amount) FROM orders GROUP BY region",
            &stats,
            Some("shop"),
        );
        assert_eq!(decision.path, QueryPath::Oltp);
    }

    #[test]
    fn routes_cross_dialect_scalar_alias_to_olap() {
        let decision = HtapQueryRouter::route_statement("SELECT IFNULL(amount, 0) FROM orders");
        assert_eq!(decision.path, QueryPath::Olap);
    }

    #[test]
    fn q1_small_table_cross_dialect_function_stays_olap() {
        let mut stats = StatsRegistry::new();
        stats.update_table("orders", 10, std::collections::HashMap::new());
        let decision = HtapQueryRouter::route_with_stats(
            "SELECT IFNULL(amount, 0) FROM orders",
            &stats,
            None,
        );
        assert_eq!(decision.path, QueryPath::Olap);
    }
}
