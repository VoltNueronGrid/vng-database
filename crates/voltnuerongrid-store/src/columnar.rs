//! Columnar batch data layout — Sprint 4, S4-WS3-03.
//!
//! Provides [`ColumnBatch`] and [`ColumnVector`] for vectorized in-memory
//! representation of database rows read from the row store.  This is the
//! foundation for the OLAP vectorized execution engine.

#![forbid(unsafe_code)]

use std::collections::HashMap;

// ─── Column value types ───────────────────────────────────────────────────────

/// A typed column vector.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnVector {
    /// 64-bit integer column.
    Int64(Vec<i64>),
    /// 64-bit float column.
    Float64(Vec<f64>),
    /// Boolean column.
    Bool(Vec<bool>),
    /// UTF-8 string column.
    Utf8(Vec<String>),
    /// Null / missing column (all values null).
    Null(usize),
}

impl ColumnVector {
    /// Number of elements in this column.
    pub fn len(&self) -> usize {
        match self {
            ColumnVector::Int64(v) => v.len(),
            ColumnVector::Float64(v) => v.len(),
            ColumnVector::Bool(v) => v.len(),
            ColumnVector::Utf8(v) => v.len(),
            ColumnVector::Null(n) => *n,
        }
    }

    /// `true` when the column has no rows.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the best-effort string representation of element at `idx`.
    pub fn value_as_str(&self, idx: usize) -> Option<String> {
        match self {
            ColumnVector::Int64(v) => v.get(idx).map(|n| n.to_string()),
            ColumnVector::Float64(v) => v.get(idx).map(|f| f.to_string()),
            ColumnVector::Bool(v) => v.get(idx).map(|b| b.to_string()),
            ColumnVector::Utf8(v) => v.get(idx).cloned(),
            ColumnVector::Null(_) => Some("null".to_string()),
        }
    }
}

// ─── Column batch ─────────────────────────────────────────────────────────────

/// A batch of columns representing a set of database rows.
///
/// Columns are stored by name; all columns must have the same `len()`.
/// The `row_keys` vector holds the primary-key string for each row position.
#[derive(Debug, Clone, Default)]
pub struct ColumnBatch {
    /// Ordered column names.
    pub column_names: Vec<String>,
    /// Column data indexed by name.
    pub columns: HashMap<String, ColumnVector>,
    /// Primary-key strings in row order.
    pub row_keys: Vec<String>,
}

impl ColumnBatch {
    /// Create an empty batch.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of rows in this batch.
    pub fn row_count(&self) -> usize {
        self.row_keys.len()
    }

    /// `true` when the batch has no rows.
    pub fn is_empty(&self) -> bool {
        self.row_keys.is_empty()
    }

    /// Add a column by name.  The caller is responsible for ensuring the
    /// vector length equals the current row count (or adds the first column).
    pub fn add_column(&mut self, name: impl Into<String>, vec: ColumnVector) {
        let name = name.into();
        if !self.column_names.contains(&name) {
            self.column_names.push(name.clone());
        }
        self.columns.insert(name, vec);
    }

    /// Add a row key (used when building the batch row-by-row via
    /// [`ColumnBatchBuilder`]).
    pub fn push_row_key(&mut self, key: impl Into<String>) {
        self.row_keys.push(key.into());
    }
}

// ─── Builder for row-oriented input ──────────────────────────────────────────

/// Builds a [`ColumnBatch`] from row-oriented `HashMap<String, String>` input,
/// auto-infering column types (i64 → f64 → bool → Utf8).
#[derive(Debug, Default)]
pub struct ColumnBatchBuilder {
    row_keys: Vec<String>,
    /// Column name → raw string values in row order.
    raw: HashMap<String, Vec<String>>,
    column_order: Vec<String>,
}

impl ColumnBatchBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a row to the builder.
    pub fn push_row(&mut self, key: impl Into<String>, fields: &HashMap<String, String>) {
        self.row_keys.push(key.into());
        for (col, val) in fields {
            let col_vals = self.raw.entry(col.clone()).or_insert_with(|| {
                // Pad new columns with empty strings for existing rows.
                let n = self.row_keys.len() - 1;
                Vec::from(vec!["".to_string(); n])
            });
            if !self.column_order.contains(col) {
                self.column_order.push(col.clone());
            }
            col_vals.push(val.clone());
        }
        // Ensure every known column has a value for this row.
        let n = self.row_keys.len();
        for col_vals in self.raw.values_mut() {
            while col_vals.len() < n {
                col_vals.push(String::new());
            }
        }
    }

    /// Consume the builder and produce a [`ColumnBatch`] with type inference.
    pub fn finish(self) -> ColumnBatch {
        let mut batch = ColumnBatch {
            column_names: self.column_order.clone(),
            columns: HashMap::new(),
            row_keys: self.row_keys,
        };
        for col in &self.column_order {
            let vals = match self.raw.get(col) {
                Some(v) => v,
                None => continue,
            };
            let vec = infer_column_type(vals);
            batch.columns.insert(col.clone(), vec);
        }
        batch
    }
}

/// Infer the best column type from string values (i64 > f64 > bool > Utf8).
fn infer_column_type(vals: &[String]) -> ColumnVector {
    // Try i64 first.
    if vals.iter().all(|v| v.is_empty() || v.parse::<i64>().is_ok()) {
        return ColumnVector::Int64(
            vals.iter()
                .map(|v| v.parse::<i64>().unwrap_or(0))
                .collect(),
        );
    }
    // Try f64.
    if vals.iter().all(|v| v.is_empty() || v.parse::<f64>().is_ok()) {
        return ColumnVector::Float64(
            vals.iter()
                .map(|v| v.parse::<f64>().unwrap_or(0.0))
                .collect(),
        );
    }
    // Try bool.
    if vals.iter().all(|v| {
        let l = v.to_ascii_lowercase();
        l.is_empty() || l == "true" || l == "false"
    }) {
        return ColumnVector::Bool(
            vals.iter()
                .map(|v| v.to_ascii_lowercase() == "true")
                .collect(),
        );
    }
    // Default to Utf8.
    ColumnVector::Utf8(vals.to_vec())
}

// ─── Vectorized scan operator ─────────────────────────────────────────────────

/// Statistics produced by a vectorized scan.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorizedScanStats {
    pub rows_scanned: usize,
    pub columns_materialized: usize,
    pub elapsed_us: u128,
}

/// Run a vectorized scan over a slice of `(key, RowData)` pairs building a
/// [`ColumnBatch`].
///
/// `row_data` should come from `PagedRowStore::scan_at_snapshot()`; this
/// function does not hold the store lock so the caller must have already
/// collected the rows.
pub fn vectorized_scan(
    rows: &[(String, HashMap<String, String>)],
    limit: usize,
) -> (ColumnBatch, VectorizedScanStats) {
    use std::time::Instant;
    let t0 = Instant::now();
    let mut builder = ColumnBatchBuilder::new();
    for (key, fields) in rows.iter().take(limit) {
        builder.push_row(key.as_str(), fields);
    }
    let batch = builder.finish();
    let stats = VectorizedScanStats {
        rows_scanned: batch.row_count(),
        columns_materialized: batch.column_names.len(),
        elapsed_us: t0.elapsed().as_micros(),
    };
    (batch, stats)
}

// ─── Vectorized aggregation operators ────────────────────────────────────────

/// An aggregation operation that can be applied to a [`ColumnVector`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorizedAggOp {
    /// Sum of all numeric values.
    Sum,
    /// Count of rows (non-null).
    Count,
    /// Arithmetic mean.
    Avg,
    /// Minimum numeric value.
    Min,
    /// Maximum numeric value.
    Max,
}

/// The result of applying a [`VectorizedAggOp`] to a column.
#[derive(Debug, Clone, PartialEq)]
pub struct AggResult {
    pub op: VectorizedAggOp,
    /// The computed scalar value as a UTF-8 string so callers can serialise it.
    pub value: String,
    /// Number of rows that contributed to the aggregate.
    pub row_count: usize,
}

/// Apply a single aggregation `op` to a column vector.
///
/// For `Sum`/`Avg`/`Min`/`Max`, only `Int64` and `Float64` columns are
/// supported; other column types return `value: "null"`.
pub fn aggregate_column(col: &ColumnVector, op: VectorizedAggOp) -> AggResult {
    match op {
        VectorizedAggOp::Count => {
            let n = col.len();
            AggResult { op, value: n.to_string(), row_count: n }
        }
        VectorizedAggOp::Sum => match col {
            ColumnVector::Int64(v) => {
                let s: i64 = v.iter().sum();
                AggResult { op, value: s.to_string(), row_count: v.len() }
            }
            ColumnVector::Float64(v) => {
                let s: f64 = v.iter().sum();
                AggResult { op, value: s.to_string(), row_count: v.len() }
            }
            _ => AggResult { op, value: "null".to_string(), row_count: col.len() },
        },
        VectorizedAggOp::Avg => match col {
            ColumnVector::Int64(v) if !v.is_empty() => {
                let avg = v.iter().sum::<i64>() as f64 / v.len() as f64;
                AggResult { op, value: avg.to_string(), row_count: v.len() }
            }
            ColumnVector::Float64(v) if !v.is_empty() => {
                let avg = v.iter().sum::<f64>() / v.len() as f64;
                AggResult { op, value: avg.to_string(), row_count: v.len() }
            }
            _ => AggResult { op, value: "null".to_string(), row_count: col.len() },
        },
        VectorizedAggOp::Min => match col {
            ColumnVector::Int64(v) => {
                let m = v.iter().copied().min();
                AggResult { op, value: m.map_or("null".into(), |x| x.to_string()), row_count: v.len() }
            }
            ColumnVector::Float64(v) => {
                let m = v.iter().copied().fold(f64::INFINITY, f64::min);
                AggResult { op, value: if v.is_empty() { "null".into() } else { m.to_string() }, row_count: v.len() }
            }
            _ => AggResult { op, value: "null".to_string(), row_count: col.len() },
        },
        VectorizedAggOp::Max => match col {
            ColumnVector::Int64(v) => {
                let m = v.iter().copied().max();
                AggResult { op, value: m.map_or("null".into(), |x| x.to_string()), row_count: v.len() }
            }
            ColumnVector::Float64(v) => {
                let m = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                AggResult { op, value: if v.is_empty() { "null".into() } else { m.to_string() }, row_count: v.len() }
            }
            _ => AggResult { op, value: "null".to_string(), row_count: col.len() },
        },
    }
}

/// Apply a set of aggregation operations over all columns in `batch`.
///
/// `ops` maps column name → desired aggregation.  Columns not present in
/// `ops` are ignored.  Returns a map of column name → [`AggResult`].
pub fn aggregate_batch(
    batch: &ColumnBatch,
    ops: &HashMap<String, VectorizedAggOp>,
) -> HashMap<String, AggResult> {
    ops.iter()
        .filter_map(|(col, &op)| {
            batch.columns.get(col).map(|vec| (col.clone(), aggregate_column(vec, op)))
        })
        .collect()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rows() -> Vec<(String, HashMap<String, String>)> {
        vec![
            (
                "row-1".to_string(),
                [("age".to_string(), "30".to_string()), ("name".to_string(), "alice".to_string())]
                    .into_iter()
                    .collect(),
            ),
            (
                "row-2".to_string(),
                [("age".to_string(), "25".to_string()), ("name".to_string(), "bob".to_string())]
                    .into_iter()
                    .collect(),
            ),
            (
                "row-3".to_string(),
                [("age".to_string(), "40".to_string()), ("name".to_string(), "carol".to_string())]
                    .into_iter()
                    .collect(),
            ),
        ]
    }

    #[test]
    fn columnar_scan_produces_correct_row_count() {
        let rows = sample_rows();
        let (batch, stats) = vectorized_scan(&rows, 100);
        assert_eq!(batch.row_count(), 3);
        assert_eq!(stats.rows_scanned, 3);
    }

    #[test]
    fn columnar_scan_infers_int64_for_age_column() {
        let rows = sample_rows();
        let (batch, _) = vectorized_scan(&rows, 100);
        match batch.columns.get("age") {
            Some(ColumnVector::Int64(v)) => assert_eq!(v.len(), 3),
            other => panic!("expected Int64 column, got {:?}", other),
        }
    }

    #[test]
    fn columnar_scan_infers_utf8_for_name_column() {
        let rows = sample_rows();
        let (batch, _) = vectorized_scan(&rows, 100);
        match batch.columns.get("name") {
            Some(ColumnVector::Utf8(v)) => assert_eq!(v.len(), 3),
            other => panic!("expected Utf8 column, got {:?}", other),
        }
    }

    #[test]
    fn columnar_scan_respects_limit() {
        let rows = sample_rows();
        let (batch, stats) = vectorized_scan(&rows, 2);
        assert_eq!(batch.row_count(), 2);
        assert_eq!(stats.rows_scanned, 2);
    }

    #[test]
    fn columnar_scan_empty_input() {
        let (batch, stats) = vectorized_scan(&[], 100);
        assert!(batch.is_empty());
        assert_eq!(stats.rows_scanned, 0);
        assert_eq!(stats.columns_materialized, 0);
    }

    #[test]
    fn column_vector_value_as_str_int64() {
        let cv = ColumnVector::Int64(vec![1, 2, 3]);
        assert_eq!(cv.value_as_str(0), Some("1".to_string()));
        assert_eq!(cv.value_as_str(5), None);
    }

    #[test]
    fn infer_type_picks_float64() {
        let vals = vec!["1.5".to_string(), "2.7".to_string()];
        match infer_column_type(&vals) {
            ColumnVector::Float64(v) => assert_eq!(v.len(), 2),
            other => panic!("expected Float64, got {:?}", other),
        }
    }

    #[test]
    fn infer_type_picks_bool() {
        let vals = vec!["true".to_string(), "false".to_string()];
        match infer_column_type(&vals) {
            ColumnVector::Bool(v) => {
                assert_eq!(v[0], true);
                assert_eq!(v[1], false);
            }
            other => panic!("expected Bool, got {:?}", other),
        }
    }

    // ─── Aggregation operator tests ─────────────────────────────────────────

    #[test]
    fn aggregate_sum_int64_column() {
        let rows = sample_rows(); // ages: 30, 25, 40
        let (batch, _) = vectorized_scan(&rows, 100);
        let result = aggregate_column(batch.columns.get("age").unwrap(), VectorizedAggOp::Sum);
        assert_eq!(result.value, "95");
        assert_eq!(result.row_count, 3);
    }

    #[test]
    fn aggregate_count_returns_row_count() {
        let rows = sample_rows();
        let (batch, _) = vectorized_scan(&rows, 100);
        let result = aggregate_column(batch.columns.get("name").unwrap(), VectorizedAggOp::Count);
        assert_eq!(result.value, "3");
        assert_eq!(result.row_count, 3);
    }

    #[test]
    fn aggregate_avg_int64_column() {
        let rows = sample_rows(); // ages: 30, 25, 40 → avg = 31.666...
        let (batch, _) = vectorized_scan(&rows, 100);
        let result = aggregate_column(batch.columns.get("age").unwrap(), VectorizedAggOp::Avg);
        let avg: f64 = result.value.parse().unwrap();
        assert!((avg - 31.666_666_666_666_668_f64).abs() < 0.001);
    }

    #[test]
    fn aggregate_min_max_int64_column() {
        let rows = sample_rows(); // ages: 30, 25, 40
        let (batch, _) = vectorized_scan(&rows, 100);
        let min_r = aggregate_column(batch.columns.get("age").unwrap(), VectorizedAggOp::Min);
        let max_r = aggregate_column(batch.columns.get("age").unwrap(), VectorizedAggOp::Max);
        assert_eq!(min_r.value, "25");
        assert_eq!(max_r.value, "40");
    }

    #[test]
    fn aggregate_batch_returns_map_for_requested_columns() {
        let rows = sample_rows();
        let (batch, _) = vectorized_scan(&rows, 100);
        let mut ops = HashMap::new();
        ops.insert("age".to_string(), VectorizedAggOp::Sum);
        ops.insert("name".to_string(), VectorizedAggOp::Count);
        let results = aggregate_batch(&batch, &ops);
        assert_eq!(results.len(), 2);
        assert_eq!(results["age"].value, "95");
        assert_eq!(results["name"].value, "3");
    }

    #[test]
    fn aggregate_non_numeric_column_sum_returns_null() {
        let rows = sample_rows();
        let (batch, _) = vectorized_scan(&rows, 100);
        // "name" is a Utf8 column — sum should return "null"
        let result = aggregate_column(batch.columns.get("name").unwrap(), VectorizedAggOp::Sum);
        assert_eq!(result.value, "null");
    }
}
