// H9-13: HTAP observability and SLO metrics
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

// ─── Histogram helper ────────────────────────────────────────────────────────

fn percentile(sorted: &[u64], pct: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 * pct / 100.0).ceil() as usize).saturating_sub(1);
    sorted[idx.min(sorted.len() - 1)]
}

fn histogram_summary(values: &[u64]) -> HistogramSummary {
    if values.is_empty() {
        return HistogramSummary { p50: 0, p95: 0, p99: 0, count: 0 };
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    HistogramSummary {
        p50: percentile(&sorted, 50.0),
        p95: percentile(&sorted, 95.0),
        p99: percentile(&sorted, 99.0),
        count: sorted.len() as u64,
    }
}

// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramSummary {
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub count: u64,
}

/// Per-table HTAP diagnostics for the HTTP response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableDiagnostics {
    pub table_name: String,
    pub tail_version_count: u64,
    pub estimated_tail_bytes: u64,
    pub last_merge_lag_ms: u64,
    pub freshness_slo_status: String,
    pub active_snapshot_count: u64,
}

/// Point-in-time snapshot of all atomic metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HtapMetricsSnapshot {
    // counters
    pub merge_attempts_total: u64,
    pub merge_failures_total: u64,
    pub merge_completions_total: u64,
    pub snapshot_creates_total: u64,
    pub snapshot_releases_total: u64,
    pub freshness_slo_violations_total: u64,
    pub admission_rejections_total: u64,
    pub admission_accepts_total: u64,
    pub hybrid_scan_total: u64,
    pub hybrid_scan_errors_total: u64,
    // gauges
    pub tail_versions_count: u64,
    pub tail_bytes_estimate: u64,
    pub merge_lag_ms: u64,
    pub snapshot_age_ms: u64,
    pub admission_queue_depth: u64,
    pub oltp_slo_pressure_pct: u64,
    // histograms
    pub merge_duration_ms: HistogramSummary,
    pub scan_duration_ms: HistogramSummary,
    pub snapshot_create_ms: HistogramSummary,
}

/// Full diagnostics payload returned by `GET /api/v1/htap/diagnostics`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HtapDiagnostics {
    pub timestamp_ms: u64,
    pub per_table: HashMap<String, TableDiagnostics>,
    pub system_snapshot: HtapMetricsSnapshot,
}

// ─── HtapMetrics ─────────────────────────────────────────────────────────────

/// Central lock-free metrics collector for the HTAP subsystem.
#[derive(Debug)]
pub struct HtapMetrics {
    // counters
    merge_attempts_total: Arc<AtomicU64>,
    merge_failures_total: Arc<AtomicU64>,
    merge_completions_total: Arc<AtomicU64>,
    snapshot_creates_total: Arc<AtomicU64>,
    snapshot_releases_total: Arc<AtomicU64>,
    freshness_slo_violations_total: Arc<AtomicU64>,
    admission_rejections_total: Arc<AtomicU64>,
    admission_accepts_total: Arc<AtomicU64>,
    hybrid_scan_total: Arc<AtomicU64>,
    hybrid_scan_errors_total: Arc<AtomicU64>,
    // gauges
    tail_versions_count: Arc<AtomicU64>,
    tail_bytes_estimate: Arc<AtomicU64>,
    merge_lag_ms: Arc<AtomicU64>,
    snapshot_age_ms: Arc<AtomicU64>,
    admission_queue_depth: Arc<AtomicU64>,
    oltp_slo_pressure_pct: Arc<AtomicU64>,
    // histograms
    merge_duration_ms: Arc<Mutex<Vec<u64>>>,
    scan_duration_ms: Arc<Mutex<Vec<u64>>>,
    snapshot_create_ms: Arc<Mutex<Vec<u64>>>,
}

impl HtapMetrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            merge_attempts_total: Arc::new(AtomicU64::new(0)),
            merge_failures_total: Arc::new(AtomicU64::new(0)),
            merge_completions_total: Arc::new(AtomicU64::new(0)),
            snapshot_creates_total: Arc::new(AtomicU64::new(0)),
            snapshot_releases_total: Arc::new(AtomicU64::new(0)),
            freshness_slo_violations_total: Arc::new(AtomicU64::new(0)),
            admission_rejections_total: Arc::new(AtomicU64::new(0)),
            admission_accepts_total: Arc::new(AtomicU64::new(0)),
            hybrid_scan_total: Arc::new(AtomicU64::new(0)),
            hybrid_scan_errors_total: Arc::new(AtomicU64::new(0)),
            tail_versions_count: Arc::new(AtomicU64::new(0)),
            tail_bytes_estimate: Arc::new(AtomicU64::new(0)),
            merge_lag_ms: Arc::new(AtomicU64::new(0)),
            snapshot_age_ms: Arc::new(AtomicU64::new(0)),
            admission_queue_depth: Arc::new(AtomicU64::new(0)),
            oltp_slo_pressure_pct: Arc::new(AtomicU64::new(0)),
            merge_duration_ms: Arc::new(Mutex::new(Vec::new())),
            scan_duration_ms: Arc::new(Mutex::new(Vec::new())),
            snapshot_create_ms: Arc::new(Mutex::new(Vec::new())),
        })
    }

    // ── Counters ──────────────────────────────────────────────────────────────

    pub fn record_merge_attempt(&self) {
        self.merge_attempts_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_merge_failure(&self) {
        self.merge_failures_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_merge_completion(&self, duration_ms: u64) {
        self.merge_completions_total.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut v) = self.merge_duration_ms.lock() {
            v.push(duration_ms);
        }
    }

    pub fn record_snapshot_create(&self, age_ms: u64) {
        self.snapshot_creates_total.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut v) = self.snapshot_create_ms.lock() {
            v.push(age_ms);
        }
    }

    pub fn record_snapshot_release(&self) {
        self.snapshot_releases_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_freshness_violation(&self) {
        self.freshness_slo_violations_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_admission_rejected(&self) {
        self.admission_rejections_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_admission_accepted(&self) {
        self.admission_accepts_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_hybrid_scan(&self, duration_ms: u64) {
        self.hybrid_scan_total.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut v) = self.scan_duration_ms.lock() {
            v.push(duration_ms);
        }
    }

    pub fn record_hybrid_scan_error(&self) {
        self.hybrid_scan_errors_total.fetch_add(1, Ordering::Relaxed);
    }

    // ── Gauges ────────────────────────────────────────────────────────────────

    pub fn set_tail_versions(&self, count: u64) {
        self.tail_versions_count.store(count, Ordering::Relaxed);
    }

    pub fn set_tail_bytes(&self, bytes: u64) {
        self.tail_bytes_estimate.store(bytes, Ordering::Relaxed);
    }

    pub fn set_merge_lag(&self, lag_ms: u64) {
        self.merge_lag_ms.store(lag_ms, Ordering::Relaxed);
    }

    pub fn set_snapshot_age(&self, age_ms: u64) {
        self.snapshot_age_ms.store(age_ms, Ordering::Relaxed);
    }

    pub fn set_admission_queue_depth(&self, depth: u64) {
        self.admission_queue_depth.store(depth, Ordering::Relaxed);
    }

    pub fn set_oltp_slo_pressure(&self, pct_times_100: u64) {
        self.oltp_slo_pressure_pct.store(pct_times_100, Ordering::Relaxed);
    }

    // ── Snapshot & Diagnostics ────────────────────────────────────────────────

    /// Capture a consistent point-in-time snapshot of all metrics.
    pub fn snapshot(&self) -> HtapMetricsSnapshot {
        let merge_dur = self.merge_duration_ms.lock()
            .map(|v| histogram_summary(&v))
            .unwrap_or(HistogramSummary { p50: 0, p95: 0, p99: 0, count: 0 });
        let scan_dur = self.scan_duration_ms.lock()
            .map(|v| histogram_summary(&v))
            .unwrap_or(HistogramSummary { p50: 0, p95: 0, p99: 0, count: 0 });
        let snap_create = self.snapshot_create_ms.lock()
            .map(|v| histogram_summary(&v))
            .unwrap_or(HistogramSummary { p50: 0, p95: 0, p99: 0, count: 0 });

        HtapMetricsSnapshot {
            merge_attempts_total: self.merge_attempts_total.load(Ordering::Relaxed),
            merge_failures_total: self.merge_failures_total.load(Ordering::Relaxed),
            merge_completions_total: self.merge_completions_total.load(Ordering::Relaxed),
            snapshot_creates_total: self.snapshot_creates_total.load(Ordering::Relaxed),
            snapshot_releases_total: self.snapshot_releases_total.load(Ordering::Relaxed),
            freshness_slo_violations_total: self.freshness_slo_violations_total.load(Ordering::Relaxed),
            admission_rejections_total: self.admission_rejections_total.load(Ordering::Relaxed),
            admission_accepts_total: self.admission_accepts_total.load(Ordering::Relaxed),
            hybrid_scan_total: self.hybrid_scan_total.load(Ordering::Relaxed),
            hybrid_scan_errors_total: self.hybrid_scan_errors_total.load(Ordering::Relaxed),
            tail_versions_count: self.tail_versions_count.load(Ordering::Relaxed),
            tail_bytes_estimate: self.tail_bytes_estimate.load(Ordering::Relaxed),
            merge_lag_ms: self.merge_lag_ms.load(Ordering::Relaxed),
            snapshot_age_ms: self.snapshot_age_ms.load(Ordering::Relaxed),
            admission_queue_depth: self.admission_queue_depth.load(Ordering::Relaxed),
            oltp_slo_pressure_pct: self.oltp_slo_pressure_pct.load(Ordering::Relaxed),
            merge_duration_ms: merge_dur,
            scan_duration_ms: scan_dur,
            snapshot_create_ms: snap_create,
        }
    }

    /// Assemble a full `HtapDiagnostics` from a caller-supplied list of table stats.
    pub fn diagnostics(&self, table_stats: Vec<TableDiagnostics>) -> HtapDiagnostics {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let per_table: HashMap<String, TableDiagnostics> = table_stats
            .into_iter()
            .map(|t| (t.table_name.clone(), t))
            .collect();

        HtapDiagnostics {
            timestamp_ms,
            per_table,
            system_snapshot: self.snapshot(),
        }
    }
}

impl Default for HtapMetrics {
    fn default() -> Self {
        Arc::try_unwrap(Self::new()).unwrap_or_else(|a| {
            // Safety: this path is unreachable at construction time (Arc refcount == 1),
            // but we need to satisfy the type system.
            drop(a);
            unreachable!("HtapMetrics::default: unexpected extra Arc reference")
        })
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn counters_increment_independently() {
        let m = HtapMetrics::new();
        m.record_merge_attempt();
        m.record_merge_attempt();
        m.record_merge_failure();
        let s = m.snapshot();
        assert_eq!(s.merge_attempts_total, 2);
        assert_eq!(s.merge_failures_total, 1);
        assert_eq!(s.merge_completions_total, 0);
    }

    #[test]
    fn snapshot_counter_records_correctly() {
        let m = HtapMetrics::new();
        m.record_snapshot_create(100);
        m.record_snapshot_create(200);
        m.record_snapshot_release();
        let s = m.snapshot();
        assert_eq!(s.snapshot_creates_total, 2);
        assert_eq!(s.snapshot_releases_total, 1);
    }

    #[test]
    fn admission_counters_work() {
        let m = HtapMetrics::new();
        m.record_admission_accepted();
        m.record_admission_accepted();
        m.record_admission_rejected();
        let s = m.snapshot();
        assert_eq!(s.admission_accepts_total, 2);
        assert_eq!(s.admission_rejections_total, 1);
    }

    #[test]
    fn hybrid_scan_counters_work() {
        let m = HtapMetrics::new();
        m.record_hybrid_scan(50);
        m.record_hybrid_scan_error();
        let s = m.snapshot();
        assert_eq!(s.hybrid_scan_total, 1);
        assert_eq!(s.hybrid_scan_errors_total, 1);
    }

    #[test]
    fn freshness_violation_counter_increments() {
        let m = HtapMetrics::new();
        m.record_freshness_violation();
        m.record_freshness_violation();
        m.record_freshness_violation();
        assert_eq!(m.snapshot().freshness_slo_violations_total, 3);
    }

    #[test]
    fn gauges_set_and_read_correctly() {
        let m = HtapMetrics::new();
        m.set_tail_versions(42);
        m.set_tail_bytes(1024);
        m.set_merge_lag(300);
        m.set_snapshot_age(5000);
        m.set_admission_queue_depth(7);
        m.set_oltp_slo_pressure(8500);
        let s = m.snapshot();
        assert_eq!(s.tail_versions_count, 42);
        assert_eq!(s.tail_bytes_estimate, 1024);
        assert_eq!(s.merge_lag_ms, 300);
        assert_eq!(s.snapshot_age_ms, 5000);
        assert_eq!(s.admission_queue_depth, 7);
        assert_eq!(s.oltp_slo_pressure_pct, 8500);
    }

    #[test]
    fn gauge_overwrite_reflects_latest_value() {
        let m = HtapMetrics::new();
        m.set_merge_lag(100);
        m.set_merge_lag(999);
        assert_eq!(m.snapshot().merge_lag_ms, 999);
    }

    #[test]
    fn histogram_percentiles_calculated_correctly() {
        let m = HtapMetrics::new();
        // record 10 merge completions: 10, 20, …, 100 ms
        for i in 1..=10u64 {
            m.record_merge_completion(i * 10);
        }
        let s = m.snapshot();
        // sorted: [10,20,30,40,50,60,70,80,90,100]
        assert_eq!(s.merge_duration_ms.count, 10);
        assert_eq!(s.merge_duration_ms.p50, 50);
        assert_eq!(s.merge_duration_ms.p95, 100);
        assert_eq!(s.merge_duration_ms.p99, 100);
    }

    #[test]
    fn scan_histogram_tracks_hybrid_scans() {
        let m = HtapMetrics::new();
        for ms in [5, 10, 15, 20, 25] {
            m.record_hybrid_scan(ms);
        }
        let s = m.snapshot();
        assert_eq!(s.scan_duration_ms.count, 5);
        assert_eq!(s.scan_duration_ms.p50, 15);
    }

    #[test]
    fn empty_histogram_returns_zero_percentiles() {
        let m = HtapMetrics::new();
        let s = m.snapshot();
        assert_eq!(s.merge_duration_ms.p50, 0);
        assert_eq!(s.merge_duration_ms.p99, 0);
        assert_eq!(s.merge_duration_ms.count, 0);
    }

    #[test]
    fn diagnostics_assembles_per_table_correctly() {
        let m = HtapMetrics::new();
        m.record_merge_attempt();
        let table = TableDiagnostics {
            table_name: "orders".to_string(),
            tail_version_count: 5,
            estimated_tail_bytes: 2048,
            last_merge_lag_ms: 120,
            freshness_slo_status: "compliant".to_string(),
            active_snapshot_count: 2,
        };
        let diag = m.diagnostics(vec![table]);
        assert!(diag.timestamp_ms > 0);
        assert!(diag.per_table.contains_key("orders"));
        let t = &diag.per_table["orders"];
        assert_eq!(t.tail_version_count, 5);
        assert_eq!(t.freshness_slo_status, "compliant");
        assert_eq!(diag.system_snapshot.merge_attempts_total, 1);
    }

    #[test]
    fn diagnostics_multiple_tables_keyed_by_name() {
        let m = HtapMetrics::new();
        let tables = vec![
            TableDiagnostics {
                table_name: "events".to_string(),
                tail_version_count: 10,
                estimated_tail_bytes: 4096,
                last_merge_lag_ms: 50,
                freshness_slo_status: "warning".to_string(),
                active_snapshot_count: 1,
            },
            TableDiagnostics {
                table_name: "users".to_string(),
                tail_version_count: 3,
                estimated_tail_bytes: 512,
                last_merge_lag_ms: 10,
                freshness_slo_status: "violated".to_string(),
                active_snapshot_count: 0,
            },
        ];
        let diag = m.diagnostics(tables);
        assert_eq!(diag.per_table.len(), 2);
        assert_eq!(diag.per_table["events"].freshness_slo_status, "warning");
        assert_eq!(diag.per_table["users"].freshness_slo_status, "violated");
    }

    #[test]
    fn thread_safety_concurrent_counter_increments() {
        let m = HtapMetrics::new();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let m2 = Arc::clone(&m);
                thread::spawn(move || {
                    for _ in 0..100 {
                        m2.record_merge_attempt();
                        m2.record_hybrid_scan(5);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("thread panicked");
        }
        let s = m.snapshot();
        assert_eq!(s.merge_attempts_total, 800);
        assert_eq!(s.hybrid_scan_total, 800);
        assert_eq!(s.scan_duration_ms.count, 800);
    }
}
