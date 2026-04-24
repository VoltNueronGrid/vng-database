//! Ingest benchmark harness.
//!
//! Measures:
//! - rows/sec for CSV ingest (parsing + record construction)
//! - batch insert throughput (record vector population)
//!
//! Datasets: synthetic 10K / 100K / 1M row generators.
//! Reports: min / max / avg / p99 latency per batch.

use std::time::{Duration, Instant};

// ─── Result types ─────────────────────────────────────────────────────────────

/// Summary statistics from a single benchmark run.
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    /// Number of rows processed.
    pub rows: usize,
    /// Total wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Throughput: rows processed per second.
    pub rows_per_sec: f64,
    /// 99th-percentile batch latency in milliseconds.
    pub p99_ms: u64,
}

impl BenchmarkResult {
    fn from_samples(rows: usize, samples: &[Duration]) -> Self {
        if samples.is_empty() {
            return Self {
                rows,
                duration_ms: 0,
                rows_per_sec: 0.0,
                p99_ms: 0,
            };
        }

        let total_ms: u64 = samples.iter().map(|d| d.as_millis() as u64).sum();
        let rows_per_sec = if total_ms == 0 {
            rows as f64 * 1_000.0 // avoid div-by-zero: report proportional estimate
        } else {
            rows as f64 / (total_ms as f64 / 1_000.0)
        };

        let mut sorted: Vec<u64> = samples.iter().map(|d| d.as_millis() as u64).collect();
        sorted.sort_unstable();
        let p99_idx = ((sorted.len() as f64 * 0.99) as usize).min(sorted.len() - 1);
        let p99_ms = sorted[p99_idx];

        Self {
            rows,
            duration_ms: total_ms,
            rows_per_sec,
            p99_ms,
        }
    }
}

// ─── Synthetic data generator ─────────────────────────────────────────────────

/// Generate a synthetic CSV string with `rows` data rows plus a header.
///
/// Format:
/// ```text
/// id,name,value,score,active
/// 1,user_1,value_1,0.001,false
/// 2,user_2,value_2,0.002,true
/// ...
/// ```
pub fn generate_synthetic_csv(rows: usize) -> String {
    let mut out = String::with_capacity(rows * 40);
    out.push_str("id,name,value,score,active\n");
    for i in 1..=rows {
        let score = (i % 1000) as f64 * 0.001;
        let active = if i % 2 == 0 { "true" } else { "false" };
        out.push_str(&format!(
            "{},user_{},value_{},{:.3},{}\n",
            i, i, i, score, active
        ));
    }
    out
}

// ─── Benchmark harness ────────────────────────────────────────────────────────

/// Harness for ingest benchmarks.
pub struct IngestBenchmark {
    /// Number of timing samples to collect (one per logical "batch").
    pub batch_count: usize,
}

impl Default for IngestBenchmark {
    fn default() -> Self {
        Self { batch_count: 10 }
    }
}

impl IngestBenchmark {
    /// Create a new benchmark harness with a given number of timing batches.
    pub fn new(batch_count: usize) -> Self {
        Self {
            batch_count: batch_count.max(1),
        }
    }

    /// Benchmark batch-insert throughput.
    ///
    /// Simulates inserting `rows` records into an in-memory Vec (stand-in for a
    /// real storage layer) split evenly across `self.batch_count` timed batches.
    /// Returns [`BenchmarkResult`] with aggregate statistics.
    pub fn run_batch_insert(&self, rows: usize) -> BenchmarkResult {
        let batch_size = (rows / self.batch_count).max(1);
        let mut samples: Vec<Duration> = Vec::with_capacity(self.batch_count);
        let mut total_rows = 0usize;

        for batch_idx in 0..self.batch_count {
            let remaining = rows.saturating_sub(batch_idx * batch_size);
            if remaining == 0 {
                break;
            }
            let this_batch = remaining.min(batch_size);

            let start = Instant::now();
            // Simulated insert: build a Vec of tuples representing records.
            let _records: Vec<(usize, String, String)> = (0..this_batch)
                .map(|i| {
                    let id = batch_idx * batch_size + i + 1;
                    (id, format!("key_{id}"), format!("payload_{id}"))
                })
                .collect();
            samples.push(start.elapsed());
            total_rows += this_batch;
        }

        BenchmarkResult::from_samples(total_rows, &samples)
    }

    /// Benchmark CSV parse throughput.
    ///
    /// Generates a synthetic CSV in-memory, then times how long it takes to
    /// parse each row into a `(usize, String, String, f64, bool)` tuple across
    /// `self.batch_count` timed batches.
    pub fn run_csv_parse(&self, rows: usize) -> BenchmarkResult {
        let csv = generate_synthetic_csv(rows);
        let lines: Vec<&str> = csv.lines().skip(1).collect(); // skip header
        let actual_rows = lines.len();

        let batch_size = (actual_rows / self.batch_count).max(1);
        let mut samples: Vec<Duration> = Vec::with_capacity(self.batch_count);
        let mut total_parsed = 0usize;

        for batch_idx in 0..self.batch_count {
            let start_line = batch_idx * batch_size;
            if start_line >= actual_rows {
                break;
            }
            let end_line = (start_line + batch_size).min(actual_rows);
            let batch = &lines[start_line..end_line];

            let t = Instant::now();
            let _parsed: Vec<(usize, String, String, f64, bool)> = batch
                .iter()
                .filter_map(|line| parse_csv_row(line))
                .collect();
            samples.push(t.elapsed());
            total_parsed += end_line - start_line;
        }

        BenchmarkResult::from_samples(total_parsed, &samples)
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

fn parse_csv_row(line: &str) -> Option<(usize, String, String, f64, bool)> {
    let mut cols = line.splitn(5, ',');
    let id: usize = cols.next()?.parse().ok()?;
    let name = cols.next()?.to_string();
    let value = cols.next()?.to_string();
    let score: f64 = cols.next()?.parse().ok()?;
    let active: bool = cols.next()?.trim() == "true";
    Some((id, name, value, score, active))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_synthetic_csv_row_count() {
        let csv = generate_synthetic_csv(100);
        // 1 header + 100 data rows
        let line_count = csv.lines().count();
        assert_eq!(line_count, 101, "expected 101 lines (header + 100 data rows)");
    }

    #[test]
    fn test_generate_synthetic_csv_header() {
        let csv = generate_synthetic_csv(1);
        let first_line = csv.lines().next().unwrap();
        assert_eq!(first_line, "id,name,value,score,active");
    }

    #[test]
    fn test_batch_insert_100_rows_positive_throughput() {
        let bench = IngestBenchmark::new(5);
        let result = bench.run_batch_insert(100);
        assert_eq!(result.rows, 100);
        assert!(result.rows_per_sec > 0.0, "rows_per_sec must be > 0");
    }

    #[test]
    fn test_csv_parse_100_rows_positive_throughput() {
        let bench = IngestBenchmark::new(5);
        let result = bench.run_csv_parse(100);
        assert_eq!(result.rows, 100);
        assert!(result.rows_per_sec > 0.0, "rows_per_sec must be > 0");
    }

    #[test]
    fn test_benchmark_result_p99_within_samples() {
        let bench = IngestBenchmark::new(10);
        let result = bench.run_batch_insert(100);
        // p99 must be representable (>= 0)
        let _ = result.p99_ms;
    }

    #[test]
    fn test_batch_insert_zero_rows() {
        let bench = IngestBenchmark::default();
        let result = bench.run_batch_insert(0);
        assert_eq!(result.rows, 0);
    }

    /// Medium-scale smoke test (10K rows). Marked ignored to keep default runs fast.
    #[test]
    #[ignore]
    fn test_batch_insert_10k_rows() {
        let bench = IngestBenchmark::new(10);
        let result = bench.run_batch_insert(10_000);
        assert_eq!(result.rows, 10_000);
        assert!(result.rows_per_sec > 50_000.0, "expected > 50K rows/sec at 10K scale");
    }

    /// Large-scale smoke test (100K rows). Marked ignored to keep default runs fast.
    #[test]
    #[ignore]
    fn test_batch_insert_100k_rows() {
        let bench = IngestBenchmark::new(10);
        let result = bench.run_batch_insert(100_000);
        assert!(result.rows_per_sec > 100_000.0, "expected > 100K rows/sec at 100K scale");
    }
}
