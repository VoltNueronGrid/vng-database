// S11-001 — Scenario R-01: ANSI SQL basic round-trip
//
// Verifies that `SELECT 1` classifies and analyzes correctly through `SqlAnalyzer`.
// This is the minimal acceptance check for R-01 (ANSI SQL support).
//
// Run: cargo test -p voltnuerongrid-sql r01_ansi_sql_basic

use voltnuerongrid_sql::{AnalysisResult, SqlAnalyzer, SqlStatementKind};

#[test]
fn scenario_passes() {
    r01_ansi_sql_basic_classification();
    r01_ansi_sql_basic_analysis();
    r01_ansi_sql_basic_batch_parse();
}

/// Step 1-3: `SELECT 1` classifies as a Query.
fn r01_ansi_sql_basic_classification() {
    let kind = SqlAnalyzer::classify_statement("SELECT 1");
    assert!(
        matches!(kind, SqlStatementKind::Query),
        "expected SqlStatementKind::Query for 'SELECT 1', got {:?}",
        kind
    );
}

/// Step 4-5: `SELECT 1` analyzes as read-only.
fn r01_ansi_sql_basic_analysis() {
    let result: AnalysisResult = SqlAnalyzer::analyze_statement("SELECT 1");
    assert!(
        result.is_read_only,
        "expected is_read_only=true for 'SELECT 1'"
    );
    assert!(
        !result.is_ddl,
        "expected is_ddl=false for a plain SELECT"
    );
}

/// Bonus: batch parse with multiple statements includes the SELECT.
fn r01_ansi_sql_basic_batch_parse() {
    let stmts = SqlAnalyzer::parse_batch("SELECT 1; SELECT 2");
    assert!(
        stmts.len() >= 1,
        "parse_batch should return at least one statement for 'SELECT 1; SELECT 2'"
    );
    // All statements in this batch should be queries.
    for stmt in &stmts {
        assert!(
            matches!(stmt.kind, SqlStatementKind::Query),
            "expected Query kind for SELECT in batch, got {:?}",
            stmt.kind
        );
    }
}
