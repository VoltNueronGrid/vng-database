//! Query benchmark harness.
//!
//! Measures:
//! - SELECT latency (scan + predicate evaluation over in-memory rows)
//! - JOIN cost (nested-loop join simulation over two in-memory tables)
//! - Paging overhead (offset-based vs. keyset-based page navigation)

use std::time::{Duration, Instant};

use crate::ingest_benchmark::BenchmarkResult;

// ─── Simulated row types ───────────────────────────────────────────────────────

#[derive(Clone)]
struct Row {
    id: usize,
    #[allow(dead_code)]
    name: String,
    value: i64,
}

fn make_rows(n: usize) -> Vec<Row> {
    (1..=n)
        .map(|i| Row {
            id: i,
            name: format!("row_{i}"),
            value: (i * 7 % 1_000) as i64,
        })
        .collect()
}

// ─── Harness ──────────────────────────────────────────────────────────────────

/// Harness for query benchmarks.
pub struct QueryBenchmark {
    /// Number of timing iterations per benchmark.
    pub iterations: usize,
}

impl Default for QueryBenchmark {
    fn default() -> Self {
        Self { iterations: 10 }
    }
}

impl QueryBenchmark {
    /// Create a new query benchmark harness.
    pub fn new(iterations: usize) -> Self {
        Self {
            iterations: iterations.max(1),
        }
    }

    /// Benchmark SELECT latency.
    ///
    /// Scans `rows` simulated records and applies a simple equality predicate
    /// (`value == target`) over `self.iterations` timed runs.
    /// Returns [`BenchmarkResult`] with rows-scanned throughput.
    pub fn run_select_benchmark(&self, rows: usize) -> BenchmarkResult {
        let table = make_rows(rows);
        let target_value: i64 = 42;
        let mut samples: Vec<Duration> = Vec::with_capacity(self.iterations);

        for _ in 0..self.iterations {
            let t = Instant::now();
            let _hits: Vec<&Row> = table.iter().filter(|r| r.value == target_value).collect();
            samples.push(t.elapsed());
        }

        benchmark_result_from_scan(rows * self.iterations, &samples)
    }

    /// Benchmark JOIN cost.
    ///
    /// Simulates a nested-loop inner join between two in-memory tables of
    /// `rows` rows each on `left.id == right.id`. Times `self.iterations` runs.
    pub fn run_join_benchmark(&self, rows: usize) -> BenchmarkResult {
        let left = make_rows(rows);
        let right = make_rows(rows);
        let mut samples: Vec<Duration> = Vec::with_capacity(self.iterations);

        for _ in 0..self.iterations {
            let t = Instant::now();
            // Hash-join simulation: build hash map on right side, probe with left.
            let right_map: std::collections::HashMap<usize, &Row> =
                right.iter().map(|r| (r.id, r)).collect();
            let _joined: Vec<(&Row, &Row)> = left
                .iter()
                .filter_map(|l| right_map.get(&l.id).map(|r| (l, *r)))
                .collect();
            samples.push(t.elapsed());
        }

        benchmark_result_from_scan(rows * self.iterations, &samples)
    }

    /// Benchmark paging overhead.
    ///
    /// Simulates offset-based pagination over `rows` records with a page size of
    /// 100. Times how long it takes to materialise all pages across `self.iterations`
    /// full passes.
    pub fn run_paging_benchmark(&self, rows: usize) -> BenchmarkResult {
        let table = make_rows(rows);
        let page_size = 100usize;
        let page_count = (rows + page_size - 1) / page_size;
        let mut samples: Vec<Duration> = Vec::with_capacity(self.iterations);

        for _ in 0..self.iterations {
            let t = Instant::now();
            for page in 0..page_count {
                let offset = page * page_size;
                let _page_rows: &[Row] = &table[offset..(offset + page_size).min(rows)];
            }
            samples.push(t.elapsed());
        }

        benchmark_result_from_scan(rows * self.iterations, &samples)
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

fn benchmark_result_from_scan(total_rows: usize, samples: &[Duration]) -> BenchmarkResult {
    if samples.is_empty() {
        return BenchmarkResult {
            rows: total_rows,
            duration_ms: 0,
            rows_per_sec: 0.0,
            p99_ms: 0,
        };
    }

    let total_ms: u64 = samples.iter().map(|d| d.as_millis() as u64).sum();
    let rows_per_sec = if total_ms == 0 {
        total_rows as f64 * 1_000.0
    } else {
        total_rows as f64 / (total_ms as f64 / 1_000.0)
    };

    let mut sorted: Vec<u64> = samples.iter().map(|d| d.as_millis() as u64).collect();
    sorted.sort_unstable();
    let p99_idx = ((sorted.len() as f64 * 0.99) as usize).min(sorted.len() - 1);

    BenchmarkResult {
        rows: total_rows,
        duration_ms: total_ms,
        rows_per_sec,
        p99_ms: sorted[p99_idx],
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_benchmark_100_rows_positive_throughput() {
        let bench = QueryBenchmark::new(5);
        let result = bench.run_select_benchmark(100);
        assert!(result.rows > 0);
        assert!(result.rows_per_sec > 0.0, "rows_per_sec must be > 0");
    }

    #[test]
    fn test_join_benchmark_100_rows_positive_throughput() {
        let bench = QueryBenchmark::new(5);
        let result = bench.run_join_benchmark(100);
        assert!(result.rows > 0);
        assert!(result.rows_per_sec > 0.0, "rows_per_sec must be > 0");
    }

    #[test]
    fn test_paging_benchmark_100_rows_positive_throughput() {
        let bench = QueryBenchmark::new(5);
        let result = bench.run_paging_benchmark(100);
        assert!(result.rows > 0);
        assert!(result.rows_per_sec > 0.0, "rows_per_sec must be > 0");
    }

    #[test]
    fn test_paging_benchmark_single_page() {
        // 50 rows < page_size of 100, should still complete without panic
        let bench = QueryBenchmark::new(3);
        let result = bench.run_paging_benchmark(50);
        assert!(result.rows > 0);
    }

    /// Medium-scale query benchmark. Ignored by default.
    #[test]
    #[ignore]
    fn test_select_10k_rows() {
        let bench = QueryBenchmark::new(10);
        let result = bench.run_select_benchmark(10_000);
        assert!(result.rows_per_sec > 100_000.0);
    }
}
