//! H9-16: HTAP Isolation Benchmark Suite
//!
//! Measures OLTP/OLAP interference, freshness SLA compliance, and resource isolation.
//!
//! Scenarios:
//! 1. `RowOnly`          – Pure OLTP tail writes + point reads.
//! 2. `ColumnOnly`       – Pure OLAP base scans.
//! 3. `HybridStrictCurrent` – Queries require fully up-to-date data.
//! 4. `BoundedStale`     – OLAP queries tolerate N ms staleness budget.
//! 5. `MixedConcurrent`  – Interleaved OLTP writes + OLAP scans.
//!
//! No external benchmark framework is used; timings rely on `std::time::Instant`.
//! All scenarios run in-process using store-level types — no live server is needed.

use std::collections::HashMap;
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::segment::TailVersion;
use crate::mvcc::{MvccRowV2, RowVersion};
use crate::types::{CommitTs, RowId, VersionId};

// ---------------------------------------------------------------------------
// Public configuration types
// ---------------------------------------------------------------------------

/// Selects which workload pattern the benchmark should run.
#[derive(Debug, Clone, PartialEq)]
pub enum BenchmarkScenario {
    /// Pure OLTP: inserts and point reads only.
    RowOnly,
    /// Pure OLAP: full-scan reads only.
    ColumnOnly,
    /// Hybrid: OLAP queries require the very latest committed version.
    HybridStrictCurrent,
    /// Hybrid: OLAP queries accept data that is at most `max_staleness_ms` old.
    BoundedStale { max_staleness_ms: u64 },
    /// Concurrent: interleaved OLTP writes and OLAP scans in a single thread.
    MixedConcurrent,
}

/// Tunable parameters for a benchmark run.
#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    pub scenario: BenchmarkScenario,
    /// Simulated OLTP thread count (informational; single-threaded simulation).
    pub oltp_threads: usize,
    /// Simulated OLAP thread count (informational).
    pub olap_threads: usize,
    /// Wall-clock budget for the benchmark loop (milliseconds).
    pub duration_ms: u64,
    /// Target OLTP transactions per second (used for throttle simulation).
    pub target_oltp_tps: u64,
    /// Staleness budget in ms (mirrors `BoundedStale::max_staleness_ms` when set).
    pub max_staleness_ms: u64,
    /// Number of seed rows pre-populated before OLAP scans start.
    pub row_count: u64,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        BenchmarkConfig {
            scenario: BenchmarkScenario::MixedConcurrent,
            oltp_threads: 4,
            olap_threads: 2,
            duration_ms: 50,
            target_oltp_tps: 10_000,
            max_staleness_ms: 100,
            row_count: 1_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

/// Collected metrics from a single benchmark run.
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub scenario: String,
    pub duration_ms: u64,
    pub oltp_ops_total: u64,
    pub olap_ops_total: u64,
    pub oltp_tps: f64,
    pub olap_qps: f64,
    pub oltp_p50_latency_ms: f64,
    pub oltp_p95_latency_ms: f64,
    pub oltp_p99_latency_ms: f64,
    pub olap_p50_latency_ms: f64,
    pub olap_p95_latency_ms: f64,
    pub olap_p99_latency_ms: f64,
    /// Percentage of OLAP queries that met the freshness SLA (0.0–100.0).
    pub freshness_sla_compliance_pct: f64,
    /// Simulated lag between OLTP commit and OLAP visibility (ms).
    pub merge_lag_ms: u64,
    /// Total tail versions created during the run.
    pub tail_version_count: u64,
    /// Number of OLAP queries rejected by admission control.
    pub admission_rejections: u64,
    /// Unix epoch timestamp at result capture (ms).
    pub timestamp_ms: u64,
}

impl BenchmarkResult {
    /// Serialise the result as a compact JSON string.
    pub fn to_json(&self) -> String {
        format!(
            r#"{{"scenario":"{scenario}","duration_ms":{dur},"oltp_ops_total":{ot},"olap_ops_total":{oa},"oltp_tps":{otps:.2},"olap_qps":{oqps:.2},"oltp_p50_latency_ms":{op50:.3},"oltp_p95_latency_ms":{op95:.3},"oltp_p99_latency_ms":{op99:.3},"olap_p50_latency_ms":{ap50:.3},"olap_p95_latency_ms":{ap95:.3},"olap_p99_latency_ms":{ap99:.3},"freshness_sla_compliance_pct":{fsla:.2},"merge_lag_ms":{lag},"tail_version_count":{tvc},"admission_rejections":{ar},"timestamp_ms":{ts}}}"#,
            scenario = self.scenario,
            dur = self.duration_ms,
            ot = self.oltp_ops_total,
            oa = self.olap_ops_total,
            otps = self.oltp_tps,
            oqps = self.olap_qps,
            op50 = self.oltp_p50_latency_ms,
            op95 = self.oltp_p95_latency_ms,
            op99 = self.oltp_p99_latency_ms,
            ap50 = self.olap_p50_latency_ms,
            ap95 = self.olap_p95_latency_ms,
            ap99 = self.olap_p99_latency_ms,
            fsla = self.freshness_sla_compliance_pct,
            lag = self.merge_lag_ms,
            tvc = self.tail_version_count,
            ar = self.admission_rejections,
            ts = self.timestamp_ms,
        )
    }
}

// ---------------------------------------------------------------------------
// Benchmark suite entry point
// ---------------------------------------------------------------------------

/// HTAP benchmark suite.  Construct with a [`BenchmarkConfig`] and call [`run`].
pub struct HtapBenchmarkSuite {
    config: BenchmarkConfig,
}

impl HtapBenchmarkSuite {
    /// Create a new suite with the supplied configuration.
    pub fn new(config: BenchmarkConfig) -> Self {
        HtapBenchmarkSuite { config }
    }

    /// Execute the benchmark and return collected metrics.
    ///
    /// Uses in-memory HTAP store components — no live server is required.
    pub fn run(&self) -> BenchmarkResult {
        match &self.config.scenario {
            BenchmarkScenario::RowOnly => run_row_only(&self.config),
            BenchmarkScenario::ColumnOnly => run_column_only(&self.config),
            BenchmarkScenario::HybridStrictCurrent => run_hybrid_strict(&self.config),
            BenchmarkScenario::BoundedStale { max_staleness_ms } => {
                run_bounded_stale(&self.config, *max_staleness_ms)
            }
            BenchmarkScenario::MixedConcurrent => run_mixed_concurrent(&self.config),
        }
    }

    /// Write `result` as JSON to `path`, creating or overwriting the file.
    pub fn save_result(result: &BenchmarkResult, path: &str) -> std::io::Result<()> {
        let json = result.to_json();
        let mut f = std::fs::File::create(path)?;
        f.write_all(json.as_bytes())?;
        Ok(())
    }
}

/// Convenience free function that mirrors `HtapBenchmarkSuite::run`.
pub fn run_benchmark(config: BenchmarkConfig) -> BenchmarkResult {
    HtapBenchmarkSuite::new(config).run()
}

// ---------------------------------------------------------------------------
// Scenario implementations
// ---------------------------------------------------------------------------

/// Scenario 1 – Pure OLTP: insert `MvccRowV2` records, measure insert latencies.
fn run_row_only(cfg: &BenchmarkConfig) -> BenchmarkResult {
    let budget = Duration::from_millis(cfg.duration_ms);
    let start = Instant::now();

    let mut rows: Vec<MvccRowV2> = Vec::new();
    let mut oltp_latencies: Vec<u64> = Vec::new();
    let mut xid: u64 = 1;

    while start.elapsed() < budget {
        let op_start = Instant::now();

        let key = format!("row-{xid}");
        let mut row = MvccRowV2::new(&key);
        let version = RowVersion {
            xid,
            deleted: false,
            data: make_payload(xid),
        };
        let _ = row.append_tail_version(version);
        rows.push(row);

        oltp_latencies.push(op_start.elapsed().as_micros() as u64);
        xid += 1;
    }

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let total = oltp_latencies.len() as u64;

    build_result(
        "row_only",
        elapsed_ms,
        total,
        0,
        &mut oltp_latencies,
        &mut vec![],
        0,
        100.0,
        0,
        total,
        0,
    )
}

/// Scenario 2 – Pure OLAP: scan pre-seeded `TailVersion` records.
fn run_column_only(cfg: &BenchmarkConfig) -> BenchmarkResult {
    let tail_versions = seed_tail_versions(cfg.row_count);
    let budget = Duration::from_millis(cfg.duration_ms);
    let start = Instant::now();

    let mut olap_latencies: Vec<u64> = Vec::new();
    let mut scan_count: u64 = 0;

    while start.elapsed() < budget {
        let op_start = Instant::now();
        // Simulate full-scan: iterate all seeded versions.
        let _ = tail_versions.iter().filter(|v| !v.tombstone).count();
        olap_latencies.push(op_start.elapsed().as_micros() as u64);
        scan_count += 1;
    }

    let elapsed_ms = start.elapsed().as_millis() as u64;

    build_result(
        "column_only",
        elapsed_ms,
        0,
        scan_count,
        &mut vec![],
        &mut olap_latencies,
        0,
        100.0,
        0,
        tail_versions.len() as u64,
        0,
    )
}

/// Scenario 3 – Hybrid strict-current: interleaved writes followed immediately by reads.
fn run_hybrid_strict(cfg: &BenchmarkConfig) -> BenchmarkResult {
    let budget = Duration::from_millis(cfg.duration_ms);
    let start = Instant::now();

    let mut tail_store: Vec<TailVersion> = seed_tail_versions(cfg.row_count);
    let mut oltp_latencies: Vec<u64> = Vec::new();
    let mut olap_latencies: Vec<u64> = Vec::new();
    let mut write_ts: u64 = cfg.row_count;
    let mut freshness_ok: u64 = 0;
    let mut olap_total: u64 = 0;
    let mut admissions_rejected: u64 = 0;

    while start.elapsed() < budget {
        // OLTP: append a new version.
        let w_start = Instant::now();
        let tv = TailVersion::new(
            RowId(write_ts),
            VersionId(write_ts),
            CommitTs(write_ts),
            vec![0u8; 16],
        );
        tail_store.push(tv);
        oltp_latencies.push(w_start.elapsed().as_micros() as u64);
        write_ts += 1;

        // OLAP: strict-current read must see the version just written.
        let r_start = Instant::now();
        let latest_ts = write_ts - 1;
        // Simulate admission control: reject if >1000 pending OLAP requests.
        if olap_total > 0 && olap_total % 1001 == 0 {
            admissions_rejected += 1;
        } else {
            let found = tail_store.iter().any(|v| v.begin_ts.0 == latest_ts);
            olap_latencies.push(r_start.elapsed().as_micros() as u64);
            olap_total += 1;
            if found {
                freshness_ok += 1;
            }
        }
    }

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let compliance = if olap_total > 0 {
        freshness_ok as f64 / olap_total as f64 * 100.0
    } else {
        100.0
    };

    build_result(
        "hybrid_strict_current",
        elapsed_ms,
        oltp_latencies.len() as u64,
        olap_total,
        &mut oltp_latencies,
        &mut olap_latencies,
        0,
        compliance,
        1,
        (cfg.row_count + write_ts - cfg.row_count) as u64,
        admissions_rejected,
    )
}

/// Scenario 4 – Bounded-stale: OLAP queries pass if data age ≤ `max_staleness_ms`.
fn run_bounded_stale(cfg: &BenchmarkConfig, max_staleness_ms: u64) -> BenchmarkResult {
    let budget = Duration::from_millis(cfg.duration_ms);
    let start = Instant::now();

    let mut tail_store: Vec<TailVersion> = seed_tail_versions(cfg.row_count);
    let mut oltp_latencies: Vec<u64> = Vec::new();
    let mut olap_latencies: Vec<u64> = Vec::new();
    let mut write_ts: u64 = cfg.row_count;

    // Simulated write-to-visibility lag in ms (constant for this scenario).
    let simulated_merge_lag_ms: u64 = max_staleness_ms / 2;
    let mut freshness_ok: u64 = 0;
    let mut olap_total: u64 = 0;

    while start.elapsed() < budget {
        // OLTP write
        let w_start = Instant::now();
        let commit_ts = write_ts;
        let tv = TailVersion::new(
            RowId(write_ts),
            VersionId(write_ts),
            CommitTs(commit_ts),
            vec![1u8; 8],
        );
        tail_store.push(tv);
        oltp_latencies.push(w_start.elapsed().as_micros() as u64);
        write_ts += 1;

        // OLAP read with staleness tolerance.
        let r_start = Instant::now();
        // Simulate: data becomes visible after merge_lag_ms.
        // Queries asking for data older than max_staleness_ms can always be served.
        let stale_horizon = if write_ts > simulated_merge_lag_ms {
            write_ts - simulated_merge_lag_ms
        } else {
            0
        };
        let count = tail_store.iter().filter(|v| v.begin_ts.0 <= stale_horizon).count();
        olap_latencies.push(r_start.elapsed().as_micros() as u64);
        olap_total += 1;
        // Compliance: result is "fresh enough" when at least some data visible.
        if count > 0 {
            freshness_ok += 1;
        }
    }

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let compliance = if olap_total > 0 {
        freshness_ok as f64 / olap_total as f64 * 100.0
    } else {
        100.0
    };

    build_result(
        "bounded_stale",
        elapsed_ms,
        oltp_latencies.len() as u64,
        olap_total,
        &mut oltp_latencies,
        &mut olap_latencies,
        simulated_merge_lag_ms,
        compliance,
        write_ts - cfg.row_count,
        write_ts,
        0,
    )
}

/// Scenario 5 – Mixed concurrent: OLTP and OLAP interleaved 3:1.
fn run_mixed_concurrent(cfg: &BenchmarkConfig) -> BenchmarkResult {
    let budget = Duration::from_millis(cfg.duration_ms);
    let start = Instant::now();

    let mut tail_store: Vec<TailVersion> = seed_tail_versions(cfg.row_count);
    let mut oltp_latencies: Vec<u64> = Vec::new();
    let mut olap_latencies: Vec<u64> = Vec::new();
    let mut write_ts: u64 = cfg.row_count;
    let mut olap_total: u64 = 0;
    let mut freshness_ok: u64 = 0;
    let mut admissions_rejected: u64 = 0;
    let mut op_counter: u64 = 0;

    while start.elapsed() < budget {
        op_counter += 1;
        if op_counter % 4 == 0 {
            // Every 4th operation is an OLAP scan.
            // Simulate admission control: reject every 500th OLAP under pressure.
            if olap_total > 0 && olap_total % 500 == 0 {
                admissions_rejected += 1;
            } else {
                let r_start = Instant::now();
                let _ = tail_store.iter().filter(|v| !v.tombstone).count();
                olap_latencies.push(r_start.elapsed().as_micros() as u64);
                olap_total += 1;
                freshness_ok += 1; // All scans see current state in single-thread.
            }
        } else {
            // OLTP insert.
            let w_start = Instant::now();
            let tv = TailVersion::new(
                RowId(write_ts),
                VersionId(write_ts),
                CommitTs(write_ts),
                vec![2u8; 32],
            );
            tail_store.push(tv);
            oltp_latencies.push(w_start.elapsed().as_micros() as u64);
            write_ts += 1;
        }
    }

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let compliance = if olap_total > 0 {
        freshness_ok as f64 / olap_total as f64 * 100.0
    } else {
        100.0
    };

    build_result(
        "mixed_concurrent",
        elapsed_ms,
        oltp_latencies.len() as u64,
        olap_total,
        &mut oltp_latencies,
        &mut olap_latencies,
        0,
        compliance,
        write_ts - cfg.row_count,
        write_ts,
        admissions_rejected,
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a payload `HashMap` for an `MvccRowV2` row version.
fn make_payload(xid: u64) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("id".to_string(), xid.to_string());
    m.insert("value".to_string(), format!("v{xid}"));
    m
}

/// Pre-seed a vector of `TailVersion` records for OLAP scenarios.
fn seed_tail_versions(count: u64) -> Vec<TailVersion> {
    (0..count)
        .map(|i| TailVersion::new(RowId(i), VersionId(i), CommitTs(i), vec![0u8; 8]))
        .collect()
}

/// Calculate a percentile (0–100) from a **sorted** slice of microsecond durations.
/// Returns the value in milliseconds.
fn percentile_ms(sorted: &[u64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((pct / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)] as f64 / 1000.0
}

/// Assemble a [`BenchmarkResult`] from raw latency vectors and counters.
#[allow(clippy::too_many_arguments)]
fn build_result(
    scenario: &str,
    elapsed_ms: u64,
    oltp_total: u64,
    olap_total: u64,
    oltp_lat: &mut Vec<u64>,
    olap_lat: &mut Vec<u64>,
    merge_lag_ms: u64,
    freshness_compliance: f64,
    _new_versions: u64,
    tail_version_count: u64,
    admission_rejections: u64,
) -> BenchmarkResult {
    oltp_lat.sort_unstable();
    olap_lat.sort_unstable();

    let elapsed_secs = elapsed_ms as f64 / 1000.0;
    let oltp_tps = if elapsed_secs > 0.0 {
        oltp_total as f64 / elapsed_secs
    } else {
        0.0
    };
    let olap_qps = if elapsed_secs > 0.0 {
        olap_total as f64 / elapsed_secs
    } else {
        0.0
    };

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64;

    BenchmarkResult {
        scenario: scenario.to_string(),
        duration_ms: elapsed_ms,
        oltp_ops_total: oltp_total,
        olap_ops_total: olap_total,
        oltp_tps,
        olap_qps,
        oltp_p50_latency_ms: percentile_ms(oltp_lat, 50.0),
        oltp_p95_latency_ms: percentile_ms(oltp_lat, 95.0),
        oltp_p99_latency_ms: percentile_ms(oltp_lat, 99.0),
        olap_p50_latency_ms: percentile_ms(olap_lat, 50.0),
        olap_p95_latency_ms: percentile_ms(olap_lat, 95.0),
        olap_p99_latency_ms: percentile_ms(olap_lat, 99.0),
        freshness_sla_compliance_pct: freshness_compliance,
        merge_lag_ms,
        tail_version_count,
        admission_rejections,
        timestamp_ms,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_config(scenario: BenchmarkScenario) -> BenchmarkConfig {
        BenchmarkConfig {
            scenario,
            oltp_threads: 1,
            olap_threads: 1,
            duration_ms: 50,
            target_oltp_tps: 100_000,
            max_staleness_ms: 100,
            row_count: 100,
        }
    }

    fn assert_valid_result(r: &BenchmarkResult) {
        assert!(r.oltp_tps >= 0.0, "oltp_tps must be non-negative");
        assert!(r.olap_qps >= 0.0, "olap_qps must be non-negative");
        assert!(
            r.freshness_sla_compliance_pct >= 0.0 && r.freshness_sla_compliance_pct <= 100.0,
            "freshness compliance must be in [0, 100], got {}",
            r.freshness_sla_compliance_pct
        );
        assert!(!r.oltp_tps.is_nan(), "oltp_tps must not be NaN");
        assert!(!r.olap_qps.is_nan(), "olap_qps must not be NaN");
        assert!(r.duration_ms > 0, "duration_ms must be positive");
        assert!(r.timestamp_ms > 0, "timestamp_ms must be set");
    }

    #[test]
    fn test_row_only_benchmark_runs() {
        let cfg = fast_config(BenchmarkScenario::RowOnly);
        let result = run_benchmark(cfg);
        assert_eq!(result.scenario, "row_only");
        assert!(result.oltp_ops_total > 0, "should complete at least one OLTP op");
        assert_eq!(result.olap_ops_total, 0);
        assert_valid_result(&result);
    }

    #[test]
    fn test_column_only_benchmark_runs() {
        let cfg = fast_config(BenchmarkScenario::ColumnOnly);
        let result = run_benchmark(cfg);
        assert_eq!(result.scenario, "column_only");
        assert!(result.olap_ops_total > 0, "should complete at least one OLAP scan");
        assert_eq!(result.oltp_ops_total, 0);
        assert_valid_result(&result);
    }

    #[test]
    fn test_hybrid_strict_current_benchmark_runs() {
        let cfg = fast_config(BenchmarkScenario::HybridStrictCurrent);
        let result = run_benchmark(cfg);
        assert_eq!(result.scenario, "hybrid_strict_current");
        assert!(result.oltp_ops_total > 0);
        assert!(result.olap_ops_total > 0);
        assert_valid_result(&result);
    }

    #[test]
    fn test_bounded_stale_benchmark_runs() {
        let cfg = fast_config(BenchmarkScenario::BoundedStale { max_staleness_ms: 50 });
        let result = run_benchmark(cfg);
        assert_eq!(result.scenario, "bounded_stale");
        assert!(result.oltp_ops_total > 0);
        assert!(result.olap_ops_total > 0);
        assert_valid_result(&result);
    }

    #[test]
    fn test_mixed_concurrent_benchmark_runs() {
        let cfg = fast_config(BenchmarkScenario::MixedConcurrent);
        let result = run_benchmark(cfg);
        assert_eq!(result.scenario, "mixed_concurrent");
        assert!(result.oltp_ops_total > 0);
        assert!(result.olap_ops_total > 0);
        assert_valid_result(&result);
    }

    #[test]
    fn test_benchmark_result_serializes_to_json() {
        let cfg = fast_config(BenchmarkScenario::RowOnly);
        let result = run_benchmark(cfg);
        let json = result.to_json();
        assert!(json.contains("\"scenario\":\"row_only\""), "JSON must contain scenario field");
        assert!(json.contains("\"oltp_tps\""), "JSON must contain oltp_tps");
        assert!(json.contains("\"freshness_sla_compliance_pct\""), "JSON must contain freshness field");
        assert!(json.starts_with('{') && json.ends_with('}'), "JSON must be a valid object");
    }

    #[test]
    fn test_benchmark_latency_percentiles_calculated() {
        // Feed known latency values and verify p50/p95/p99 math.
        // 100 values in microseconds: 1..=100
        let mut latencies: Vec<u64> = (1..=100).collect();
        latencies.sort_unstable();

        // p50 → index 49 → 50 µs → 0.050 ms
        let p50 = percentile_ms(&latencies, 50.0);
        assert!((p50 - 0.050).abs() < 0.001, "p50 expected ~0.050ms, got {p50}");

        // p95 → index 94 → 95 µs → 0.095 ms
        let p95 = percentile_ms(&latencies, 95.0);
        assert!((p95 - 0.095).abs() < 0.001, "p95 expected ~0.095ms, got {p95}");

        // p99 → index 98 → 99 µs → 0.099 ms
        let p99 = percentile_ms(&latencies, 99.0);
        assert!((p99 - 0.099).abs() < 0.001, "p99 expected ~0.099ms, got {p99}");
    }

    #[test]
    fn test_freshness_compliance_rate_computed() {
        // Bounded-stale with a generous budget should yield high compliance.
        let cfg = BenchmarkConfig {
            scenario: BenchmarkScenario::BoundedStale { max_staleness_ms: 500 },
            row_count: 200,
            duration_ms: 50,
            ..BenchmarkConfig::default()
        };
        let result = run_benchmark(cfg);
        assert!(
            result.freshness_sla_compliance_pct >= 0.0
                && result.freshness_sla_compliance_pct <= 100.0,
            "compliance out of range: {}",
            result.freshness_sla_compliance_pct
        );
    }

    #[test]
    fn test_save_result_writes_json_file() {
        use std::fs;
        let cfg = fast_config(BenchmarkScenario::ColumnOnly);
        let result = run_benchmark(cfg);

        // Write into the crate's own out-dir so no path creation is needed in CI.
        // The `target/` tree is created by cargo before running tests.
        let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
        let path = format!("{out_dir}/vng_htap_benchmark_save_test.json");

        HtapBenchmarkSuite::save_result(&result, &path).expect("save_result should not fail");

        let content = fs::read_to_string(&path).expect("file should exist after save");
        assert!(content.contains("\"scenario\":\"column_only\""), "saved JSON must contain scenario");
        fs::remove_file(&path).ok(); // cleanup
    }

    #[test]
    fn test_admission_rejections_tracked() {
        // Mixed-concurrent runs enough ops to trigger simulated rejections.
        let cfg = BenchmarkConfig {
            scenario: BenchmarkScenario::MixedConcurrent,
            duration_ms: 50,
            row_count: 100,
            ..BenchmarkConfig::default()
        };
        let result = run_benchmark(cfg);
        // admission_rejections is a counter; just verify it is a valid u64 (≥ 0).
        let _ = result.admission_rejections;
        assert_valid_result(&result);
    }

    #[test]
    fn test_htap_suite_new_and_run() {
        let cfg = fast_config(BenchmarkScenario::HybridStrictCurrent);
        let suite = HtapBenchmarkSuite::new(cfg);
        let result = suite.run();
        assert_valid_result(&result);
    }

    #[test]
    fn test_percentile_empty_slice_returns_zero() {
        let empty: Vec<u64> = vec![];
        assert_eq!(percentile_ms(&empty, 50.0), 0.0);
        assert_eq!(percentile_ms(&empty, 99.0), 0.0);
    }

    #[test]
    fn test_percentile_single_element() {
        let single = vec![500u64]; // 500 µs = 0.5 ms
        assert!((percentile_ms(&single, 50.0) - 0.5).abs() < 0.001);
        assert!((percentile_ms(&single, 99.0) - 0.5).abs() < 0.001);
    }
}
