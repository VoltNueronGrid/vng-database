#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-exec";

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
            | SqlStatementKind::Rollback => RouteDecision {
                path: QueryPath::Oltp,
                reason: "transactional statement".to_string(),
            },
            SqlStatementKind::Select => {
                if upper.contains("GROUP BY")
                    || upper.contains("JOIN")
                    || upper.contains("SUM(")
                    || upper.contains("COUNT(")
                    || upper.contains("AVG(")
                    || upper.contains("MIN(")
                    || upper.contains("MAX(")
                {
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
            | SqlStatementKind::AlterTable
            | SqlStatementKind::DropTable => RouteDecision {
                path: QueryPath::Hybrid,
                reason: "catalog-changing statement affects both planes".to_string(),
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
    fn routes_mixed_batch_to_hybrid() {
        let batch =
            "BEGIN; UPDATE orders SET amount=200 WHERE id=1; SELECT region, SUM(amount) FROM orders GROUP BY region;";
        let decision = HtapQueryRouter::route_batch(batch);
        assert_eq!(decision.path, QueryPath::Hybrid);
    }
}
