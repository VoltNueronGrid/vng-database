//! Phase 3 — full DataFusion executor for complex SQL.
//!
//! Handles everything the hand-rolled executor in [`crate`] returns
//! `ExecError::Unsupported` for: JOIN, GROUP BY, HAVING, window functions,
//! subqueries, and multi-table queries. Uses DataFusion 38 with Arrow
//! columnar execution under the hood.
//!
//! # DataFrame hydration
//!
//! [`PagedRowStore`] rows are `HashMap<String, String>` (all string values).
//! They're converted to Arrow `RecordBatch` with a `Utf8` column per distinct
//! key across all rows; missing values become `null`. The batch is registered
//! as a [`MemTable`] so DataFusion can plan over it.
//!
//! Each registered table also receives an internal `__vng_key` column that
//! holds the original row key. After query execution, `__vng_key` is stripped
//! from the output and used to populate [`ResultRow::key`].
//!
//! # Multi-table queries
//!
//! Use [`execute_select_multi`] and pass a `HashMap<&str, &PagedRowStore>`.
//! Each entry is registered under its map key as a separate MemTable so
//! JOINs work correctly. [`execute_select_single`] is a convenience wrapper
//! for the common single-table case.
//!
//! # Async
//!
//! Both entry points are `async`. From synchronous service code, wrap with
//! `tokio::runtime::Handle::current().block_on(...)` or
//! `tokio::task::spawn_blocking`.

#![cfg(feature = "datafusion")]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use datafusion::arrow::array::{ArrayRef, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::datasource::MemTable;
use datafusion::prelude::*;

use voltnuerongrid_store::mvcc::{PagedRowStore, RowData};

use crate::{AggregateCell, AggregateResult, ExecError, ResultRow, SelectOutput};

/// Internal column name that carries the original PagedRowStore row key.
/// Stripped from query results before returning.
const KEY_COL: &str = "__vng_key";

// ─────────────────────────────────────────────────────────────────────────────
// Public entry points
// ─────────────────────────────────────────────────────────────────────────────

/// Run `sql` against a single `PagedRowStore` registered as `table_name`.
///
/// This covers the common case where the SQL touches one table.
/// For JOINs across multiple tables, use [`execute_select_multi`].
pub async fn execute_select_single(
    sql: &str,
    table_name: &str,
    store: &PagedRowStore,
    max_rows: usize,
) -> Result<SelectOutput, ExecError> {
    let mut tables = HashMap::new();
    tables.insert(table_name, store);
    execute_select_multi(sql, &tables, max_rows).await
}

/// Run `sql` against pre-filtered rows for each table.
///
/// Use this when you have a single unified store (as in the service) and need
/// to pass rows already filtered by table prefix for each table in the query.
/// Each entry in `table_rows` is registered as a separate MemTable.
pub async fn execute_select_from_rows(
    sql: &str,
    table_rows: HashMap<String, Vec<(String, RowData)>>,
    max_rows: usize,
) -> Result<SelectOutput, ExecError> {
    let ctx = SessionContext::new();

    for (name, rows) in table_rows {
        let (schema, batch) = rows_to_batch(&name, rows)?;
        let provider = MemTable::try_new(schema, vec![vec![batch]])
            .map_err(|e| ExecError::Unsupported(format!("MemTable({name}): {e}")))?;
        ctx.register_table(name.as_str(), Arc::new(provider))
            .map_err(|e| ExecError::Unsupported(format!("register_table({name}): {e}")))?;
    }

    let df = ctx
        .sql(sql)
        .await
        .map_err(|e| ExecError::BadPredicate(format!("DataFusion parse/plan: {e}")))?;

    let batches = df
        .collect()
        .await
        .map_err(|e| ExecError::BadPredicate(format!("DataFusion execute: {e}")))?;

    batches_to_output(batches, max_rows)
}

/// Run `sql` against multiple `PagedRowStore`s, each registered under its map
/// key as a separate MemTable. This enables cross-table JOINs.
pub async fn execute_select_multi(
    sql: &str,
    tables: &HashMap<&str, &PagedRowStore>,
    max_rows: usize,
) -> Result<SelectOutput, ExecError> {
    let ctx = SessionContext::new();

    for (&name, store) in tables {
        let rows = store.export_rows_snapshot();
        let (schema, batch) = rows_to_batch(name, rows)?;
        let provider = MemTable::try_new(schema, vec![vec![batch]])
            .map_err(|e| ExecError::Unsupported(format!("MemTable({name}): {e}")))?;
        ctx.register_table(name, Arc::new(provider))
            .map_err(|e| ExecError::Unsupported(format!("register_table({name}): {e}")))?;
    }

    let df = ctx
        .sql(sql)
        .await
        .map_err(|e| ExecError::BadPredicate(format!("DataFusion parse/plan: {e}")))?;

    let batches = df
        .collect()
        .await
        .map_err(|e| ExecError::BadPredicate(format!("DataFusion execute: {e}")))?;

    batches_to_output(batches, max_rows)
}

// ─────────────────────────────────────────────────────────────────────────────
// Schema + batch construction
// ─────────────────────────────────────────────────────────────────────────────

/// Build an Arrow schema and `RecordBatch` from a snapshot of PagedRowStore rows.
///
/// Schema: one `Utf8` field per distinct column name across all rows, sorted
/// for determinism; plus `__vng_key` as the first field to carry the original
/// row key through query execution.
fn rows_to_batch(
    table_name: &str,
    rows: Vec<(String, RowData)>,
) -> Result<(SchemaRef, RecordBatch), ExecError> {
    let mut col_names: Vec<String> = rows
        .iter()
        .flat_map(|(_, data)| data.keys().cloned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    col_names.sort();

    let mut fields: Vec<Field> = Vec::with_capacity(col_names.len() + 1);
    fields.push(Field::new(KEY_COL, DataType::Utf8, false));
    for name in &col_names {
        fields.push(Field::new(name.as_str(), DataType::Utf8, true));
    }
    let schema = Arc::new(Schema::new(fields));

    if rows.is_empty() {
        let batch = RecordBatch::new_empty(schema.clone());
        return Ok((schema, batch));
    }

    let key_array: ArrayRef = Arc::new(StringArray::from(
        rows.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
    ));

    let mut arrays: Vec<ArrayRef> = vec![key_array];
    for col in &col_names {
        let vals: Vec<Option<&str>> = rows
            .iter()
            .map(|(_, data)| data.get(col).map(String::as_str))
            .collect();
        arrays.push(Arc::new(StringArray::from(vals)));
    }

    RecordBatch::try_new(schema.clone(), arrays)
        .map(|batch| (schema, batch))
        .map_err(|e| ExecError::Unsupported(format!("RecordBatch({table_name}): {e}")))
}

// ─────────────────────────────────────────────────────────────────────────────
// Result conversion
// ─────────────────────────────────────────────────────────────────────────────

fn batches_to_output(
    batches: Vec<RecordBatch>,
    max_rows: usize,
) -> Result<SelectOutput, ExecError> {
    if batches.is_empty() {
        return Ok(SelectOutput::Rows(Vec::new()));
    }

    let schema = batches[0].schema();
    let col_names: Vec<String> = schema
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();

    // Strip the internal key column; use it to populate ResultRow::key.
    // For aggregate/GROUP BY results, __vng_key is absent — fall back to
    // a synthetic "row_N" key so every result row is still addressable.
    let key_col_idx = col_names.iter().position(|n| n == KEY_COL);
    let output_cols: Vec<usize> = (0..col_names.len())
        .filter(|&i| Some(i) != key_col_idx)
        .collect();

    let mut out: Vec<ResultRow> = Vec::new();
    'outer: for batch in &batches {
        for row_idx in 0..batch.num_rows() {
            if out.len() >= max_rows {
                break 'outer;
            }
            let key = key_col_idx
                .and_then(|idx| string_value(batch, idx, row_idx))
                .unwrap_or_else(|| format!("row_{}", out.len()));

            let mut data = RowData::new();
            for &col_idx in &output_cols {
                if let Some(val) = string_value(batch, col_idx, row_idx) {
                    data.insert(col_names[col_idx].clone(), val);
                }
            }
            out.push(ResultRow { key, data });
        }
    }
    Ok(SelectOutput::Rows(out))
}

fn string_value(batch: &RecordBatch, col_idx: usize, row_idx: usize) -> Option<String> {
    let col = batch.column(col_idx);
    use datafusion::arrow::array::Array;
    if col.is_null(row_idx) {
        return None;
    }
    datafusion::arrow::util::display::array_value_to_string(col, row_idx).ok()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use voltnuerongrid_store::mvcc::PagedRowStore;

    fn store_with_rows(rows: &[(&str, &[(&str, &str)])]) -> PagedRowStore {
        let mut s = PagedRowStore::new(64);
        let xid = s.begin_xid();
        for (key, fields) in rows {
            let mut data = RowData::new();
            for (k, v) in *fields {
                data.insert(k.to_string(), v.to_string());
            }
            s.insert(xid, key, data);
        }
        s
    }

    // ── Basic single-table queries ───────────────────────────────────────────

    #[tokio::test]
    async fn select_all_rows() {
        let store = store_with_rows(&[
            ("k1", &[("id", "1"), ("name", "alice")]),
            ("k2", &[("id", "2"), ("name", "bob")]),
        ]);
        let out = execute_select_single("SELECT id, name FROM t", "t", &store, 100)
            .await
            .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("expected Rows, got {o:?}") };
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn where_clause_filters_rows() {
        let store = store_with_rows(&[
            ("k1", &[("id", "1"), ("name", "alice")]),
            ("k2", &[("id", "2"), ("name", "bob")]),
            ("k3", &[("id", "3"), ("name", "carol")]),
        ]);
        let out = execute_select_single(
            "SELECT name FROM t WHERE id = '2'", "t", &store, 100,
        )
        .await
        .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("{o:?}") };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].data.get("name").map(String::as_str), Some("bob"));
    }

    #[tokio::test]
    async fn order_by_name() {
        let store = store_with_rows(&[
            ("k3", &[("name", "carol")]),
            ("k1", &[("name", "alice")]),
            ("k2", &[("name", "bob")]),
        ]);
        let out = execute_select_single("SELECT name FROM t ORDER BY name", "t", &store, 100)
            .await
            .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("{o:?}") };
        let names: Vec<_> = rows.iter().map(|r| r.data["name"].as_str()).collect();
        assert_eq!(names, ["alice", "bob", "carol"]);
    }

    #[tokio::test]
    async fn limit_respected() {
        let store = store_with_rows(&[
            ("k1", &[("id", "1")]), ("k2", &[("id", "2")]),
            ("k3", &[("id", "3")]), ("k4", &[("id", "4")]),
        ]);
        let out = execute_select_single("SELECT id FROM t LIMIT 2", "t", &store, 100)
            .await
            .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("{o:?}") };
        assert_eq!(rows.len(), 2);
    }

    // ── GROUP BY ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn group_by_department_count() {
        let store = store_with_rows(&[
            ("e1", &[("dept", "eng"), ("name", "alice")]),
            ("e2", &[("dept", "eng"), ("name", "bob")]),
            ("e3", &[("dept", "hr"),  ("name", "carol")]),
            ("e4", &[("dept", "eng"), ("name", "dave")]),
            ("e5", &[("dept", "hr"),  ("name", "eve")]),
        ]);
        let out = execute_select_single(
            "SELECT dept, COUNT(*) AS cnt FROM t GROUP BY dept ORDER BY dept",
            "t", &store, 100,
        )
        .await
        .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("{o:?}") };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].data.get("dept").map(String::as_str), Some("eng"));
        assert_eq!(rows[1].data.get("dept").map(String::as_str), Some("hr"));
        assert_eq!(rows[0].data.get("cnt").map(String::as_str), Some("3"));
        assert_eq!(rows[1].data.get("cnt").map(String::as_str), Some("2"));
    }

    #[tokio::test]
    async fn group_by_unaliased_count_returns_all_groups() {
        // Regression: previously is_aggregate_schema fired on "count(*)" column name
        // and aggregate_output only took the first row, silently dropping all other groups.
        let store = store_with_rows(&[
            ("e1", &[("dept", "eng")]),
            ("e2", &[("dept", "eng")]),
            ("e3", &[("dept", "hr")]),
            ("e4", &[("dept", "hr")]),
            ("e5", &[("dept", "pm")]),
        ]);
        let out = execute_select_single(
            "SELECT dept, COUNT(*) FROM t GROUP BY dept ORDER BY dept",
            "t", &store, 100,
        )
        .await
        .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("{o:?}") };
        // Must get all 3 groups, not just the first.
        assert_eq!(rows.len(), 3, "all groups must survive — was truncated to 1 before fix");
        // dept column is present
        assert!(rows.iter().any(|r| r.data.get("dept").map(String::as_str) == Some("eng")));
        assert!(rows.iter().any(|r| r.data.get("dept").map(String::as_str) == Some("hr")));
        assert!(rows.iter().any(|r| r.data.get("dept").map(String::as_str) == Some("pm")));
    }

    #[tokio::test]
    async fn group_by_having_filters_groups() {
        let store = store_with_rows(&[
            ("e1", &[("dept", "eng"), ("salary", "90000")]),
            ("e2", &[("dept", "eng"), ("salary", "95000")]),
            ("e3", &[("dept", "hr"),  ("salary", "60000")]),
        ]);
        let out = execute_select_single(
            "SELECT dept, COUNT(*) AS cnt FROM t GROUP BY dept HAVING COUNT(*) > 1",
            "t", &store, 100,
        )
        .await
        .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("{o:?}") };
        assert_eq!(rows.len(), 1, "only eng has more than 1 row");
        assert_eq!(rows[0].data.get("dept").map(String::as_str), Some("eng"));
    }

    // ── JOIN ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn inner_join_across_two_tables() {
        let orders = store_with_rows(&[
            ("o1", &[("id", "1"), ("customer_id", "c1"), ("amount", "100")]),
            ("o2", &[("id", "2"), ("customer_id", "c2"), ("amount", "200")]),
            ("o3", &[("id", "3"), ("customer_id", "c1"), ("amount", "50")]),
        ]);
        let customers = store_with_rows(&[
            ("c1", &[("id", "c1"), ("name", "alice")]),
            ("c2", &[("id", "c2"), ("name", "bob")]),
        ]);
        let mut tables = HashMap::new();
        tables.insert("orders", &orders);
        tables.insert("customers", &customers);

        let out = execute_select_multi(
            "SELECT o.id, o.amount, c.name \
             FROM orders o JOIN customers c ON o.customer_id = c.id \
             ORDER BY o.id",
            &tables, 100,
        )
        .await
        .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("{o:?}") };
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].data.get("name").map(String::as_str), Some("alice"));
        assert_eq!(rows[1].data.get("name").map(String::as_str), Some("bob"));
        assert_eq!(rows[2].data.get("name").map(String::as_str), Some("alice"));
    }

    #[tokio::test]
    async fn left_join_includes_unmatched_rows() {
        let orders = store_with_rows(&[
            ("o1", &[("id", "1"), ("customer_id", "c1")]),
            ("o2", &[("id", "2"), ("customer_id", "c99")]),
        ]);
        let customers = store_with_rows(&[
            ("c1", &[("id", "c1"), ("name", "alice")]),
        ]);
        let mut tables = HashMap::new();
        tables.insert("orders", &orders);
        tables.insert("customers", &customers);

        let out = execute_select_multi(
            "SELECT o.id, c.name \
             FROM orders o LEFT JOIN customers c ON o.customer_id = c.id \
             ORDER BY o.id",
            &tables, 100,
        )
        .await
        .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("{o:?}") };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].data.get("name").map(String::as_str), Some("alice"));
        // o2 has no matching customer — name absent
        assert!(rows[1].data.get("name").is_none());
    }

    // ── Window functions ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn window_row_number() {
        let store = store_with_rows(&[
            ("r1", &[("dept", "eng"), ("salary", "90000")]),
            ("r2", &[("dept", "eng"), ("salary", "95000")]),
            ("r3", &[("dept", "hr"),  ("salary", "60000")]),
        ]);
        let out = execute_select_single(
            "SELECT dept, salary, \
             ROW_NUMBER() OVER (PARTITION BY dept ORDER BY salary DESC) AS rn \
             FROM t ORDER BY dept, rn",
            "t", &store, 100,
        )
        .await
        .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("{o:?}") };
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].data.get("dept").map(String::as_str), Some("eng"));
        assert_eq!(rows[0].data.get("rn").map(String::as_str), Some("1"));
        assert_eq!(rows[0].data.get("salary").map(String::as_str), Some("95000"));
        assert_eq!(rows[2].data.get("dept").map(String::as_str), Some("hr"));
        assert_eq!(rows[2].data.get("rn").map(String::as_str), Some("1"));
    }

    #[tokio::test]
    async fn window_rank_and_dense_rank() {
        let store = store_with_rows(&[
            ("r1", &[("score", "90")]), ("r2", &[("score", "90")]),
            ("r3", &[("score", "80")]), ("r4", &[("score", "70")]),
        ]);
        let out = execute_select_single(
            "SELECT score, \
             RANK() OVER (ORDER BY score DESC) AS rnk, \
             DENSE_RANK() OVER (ORDER BY score DESC) AS drnk \
             FROM t ORDER BY score DESC, rnk",
            "t", &store, 100,
        )
        .await
        .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("{o:?}") };
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].data.get("rnk").map(String::as_str), Some("1"));
        assert_eq!(rows[0].data.get("drnk").map(String::as_str), Some("1"));
        assert_eq!(rows[1].data.get("rnk").map(String::as_str), Some("1"));
        assert_eq!(rows[2].data.get("rnk").map(String::as_str), Some("3"));
        assert_eq!(rows[2].data.get("drnk").map(String::as_str), Some("2"));
        assert_eq!(rows[3].data.get("rnk").map(String::as_str), Some("4"));
        assert_eq!(rows[3].data.get("drnk").map(String::as_str), Some("3"));
    }

    #[tokio::test]
    async fn window_running_sum() {
        let store = store_with_rows(&[
            ("r1", &[("amount", "100")]),
            ("r2", &[("amount", "200")]),
            ("r3", &[("amount", "300")]),
        ]);
        let out = execute_select_single(
            "SELECT amount, \
             SUM(CAST(amount AS BIGINT)) OVER (ORDER BY amount) AS running \
             FROM t ORDER BY amount",
            "t", &store, 100,
        )
        .await
        .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("{o:?}") };
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].data.get("running").map(String::as_str), Some("100"));
        assert_eq!(rows[1].data.get("running").map(String::as_str), Some("300"));
        assert_eq!(rows[2].data.get("running").map(String::as_str), Some("600"));
    }

    // ── Subquery ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn subquery_in_where() {
        let store = store_with_rows(&[
            ("r1", &[("id", "1"), ("score", "90")]),
            ("r2", &[("id", "2"), ("score", "70")]),
            ("r3", &[("id", "3"), ("score", "80")]),
        ]);
        let out = execute_select_single(
            "SELECT id FROM t WHERE score > (SELECT AVG(CAST(score AS DOUBLE)) FROM t)",
            "t", &store, 100,
        )
        .await
        .unwrap();
        let rows = match out { SelectOutput::Rows(r) => r, o => panic!("{o:?}") };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].data.get("id").map(String::as_str), Some("1"));
    }

    // ── Empty table ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn empty_store_returns_empty_rows() {
        let store = PagedRowStore::new(64);
        let out = execute_select_single("SELECT * FROM t", "t", &store, 100)
            .await
            .unwrap();
        assert_eq!(out, SelectOutput::Rows(Vec::new()));
    }
}
