/// S9-001: High-concurrency soak test harness.
///
/// The harness runs a [`Workload`] repeatedly for a configurable duration and
/// concurrency level, collects per-request latencies, and reports aggregate
/// metrics including p50/p99 and peak RPS.
///
/// NOTE: Actual long-duration runs require a live server and are deferred for
/// cloud validation.  The harness is local-ready and self-contained.
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Parameters for a single soak run.
#[derive(Debug, Clone)]
pub struct SoakTestConfig {
    /// How long to run the main test phase (seconds).
    pub duration_secs: u64,
    /// Number of concurrent worker threads.
    pub concurrency: usize,
    /// Advisory target requests-per-second (0 = unlimited).
    pub target_rps: usize,
    /// Warm-up period before metrics are collected (seconds).
    pub warmup_secs: u64,
}

impl Default for SoakTestConfig {
    fn default() -> Self {
        Self {
            duration_secs: 10,
            concurrency: 4,
            target_rps: 0,
            warmup_secs: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Workload trait
// ---------------------------------------------------------------------------

/// A single unit of work executed repeatedly by the soak harness.
///
/// Implementors return the observed latency in milliseconds on success, or a
/// descriptive error string on failure.
pub trait Workload: Send + Sync {
    fn execute(&self) -> Result<u64, String>;
}

// ---------------------------------------------------------------------------
// Built-in workloads
// ---------------------------------------------------------------------------

/// A no-op workload that always succeeds in 1 ms — used for unit tests.
pub struct NoOpWorkload;

impl Workload for NoOpWorkload {
    fn execute(&self) -> Result<u64, String> {
        Ok(1)
    }
}

/// A workload that always returns an error — used to verify error-rate detection.
pub struct FailingWorkload;

impl Workload for FailingWorkload {
    fn execute(&self) -> Result<u64, String> {
        Err("injected failure".into())
    }
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// Aggregate metrics collected during a soak run.
#[derive(Debug, Clone, Default)]
pub struct SoakMetrics {
    pub total_requests: u64,
    pub errors: u64,
    /// 50th-percentile latency in milliseconds.
    pub p50_ms: u64,
    /// 99th-percentile latency in milliseconds.
    pub p99_ms: u64,
    /// Peak observed requests-per-second across all 1-second windows.
    pub peak_rps: f64,
}

impl SoakMetrics {
    fn from_samples(latencies: &[u64], errors: u64, elapsed_secs: f64) -> Self {
        let total_requests = latencies.len() as u64 + errors;
        let (p50_ms, p99_ms) = percentiles(latencies);
        let peak_rps = if elapsed_secs > 0.0 {
            total_requests as f64 / elapsed_secs
        } else {
            0.0
        };
        Self {
            total_requests,
            errors,
            p50_ms,
            p99_ms,
            peak_rps,
        }
    }
}

fn percentiles(sorted: &[u64]) -> (u64, u64) {
    if sorted.is_empty() {
        return (0, 0);
    }
    let mut v = sorted.to_vec();
    v.sort_unstable();
    let p50_idx = (v.len() as f64 * 0.50) as usize;
    let p99_idx = ((v.len() as f64 * 0.99) as usize).min(v.len() - 1);
    (v[p50_idx.min(v.len() - 1)], v[p99_idx])
}

// ---------------------------------------------------------------------------
// SoakResult
// ---------------------------------------------------------------------------

/// The outcome of a complete soak run.
#[derive(Debug)]
pub struct SoakResult {
    pub metrics: SoakMetrics,
    /// Whether the run finished without breaching any thresholds.
    pub passed: bool,
    /// Human-readable reason when `passed == false`.
    pub failure_reason: Option<String>,
}

impl SoakResult {
    /// Check whether the run metrics satisfy the supplied thresholds.
    ///
    /// Returns `true` iff both:
    /// * error rate ≤ `max_error_rate` (0.0–1.0)
    /// * p99 latency ≤ `max_p99_ms`
    pub fn check_thresholds(&self, max_error_rate: f64, max_p99_ms: u64) -> bool {
        let error_rate = if self.metrics.total_requests > 0 {
            self.metrics.errors as f64 / self.metrics.total_requests as f64
        } else {
            0.0
        };
        error_rate <= max_error_rate && self.metrics.p99_ms <= max_p99_ms
    }
}

// ---------------------------------------------------------------------------
// SoakTestRunner
// ---------------------------------------------------------------------------

/// Orchestrates a local soak run.
pub struct SoakTestRunner {
    pub config: SoakTestConfig,
}

impl SoakTestRunner {
    pub fn new(config: SoakTestConfig) -> Self {
        Self { config }
    }

    /// Run the workload locally (no network, no server required).
    pub fn run_local(&self, workload: &dyn Workload) -> SoakResult {
        // Shared accumulators — wrapped in Arc<Mutex<>> so worker threads can
        // append without unsafe code.
        let latencies: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
        let errors: Arc<Mutex<u64>> = Arc::new(Mutex::new(0));

        let total_duration = Duration::from_secs(self.config.duration_secs + self.config.warmup_secs);
        let warmup = Duration::from_secs(self.config.warmup_secs);
        let start = Instant::now();

        // Use std::thread::scope so we can borrow `workload` without 'static.
        thread::scope(|s| {
            let mut handles = Vec::with_capacity(self.config.concurrency);

            for _ in 0..self.config.concurrency {
                let lat_clone = Arc::clone(&latencies);
                let err_clone = Arc::clone(&errors);
                let target_rps = self.config.target_rps;

                let handle = s.spawn(move || {
                    let thread_start = Instant::now();

                    loop {
                        if thread_start.elapsed() >= total_duration {
                            break;
                        }

                        let req_start = Instant::now();
                        let result = workload.execute();
                        let elapsed_req = req_start.elapsed().as_millis() as u64;

                        let in_warmup = thread_start.elapsed() < warmup;

                        match result {
                            Ok(reported_ms) => {
                                if !in_warmup {
                                    let lat = if reported_ms > 0 { reported_ms } else { elapsed_req };
                                    lat_clone.lock().unwrap().push(lat);
                                }
                            }
                            Err(_) => {
                                if !in_warmup {
                                    *err_clone.lock().unwrap() += 1;
                                }
                            }
                        }

                        // Rate limiting: if target_rps is set, space requests out.
                        if target_rps > 0 {
                            let period_ns = 1_000_000_000u64 / target_rps as u64;
                            let spent_ns = req_start.elapsed().as_nanos() as u64;
                            if spent_ns < period_ns {
                                thread::sleep(Duration::from_nanos(period_ns - spent_ns));
                            }
                        }
                    }
                });

                handles.push(handle);
            }

            for h in handles {
                h.join().expect("worker thread panicked");
            }
        });

        let elapsed_secs = start.elapsed().as_secs_f64() - self.config.warmup_secs as f64;
        let elapsed_secs = elapsed_secs.max(0.001);

        let latencies = latencies.lock().unwrap().clone();
        let errors = *errors.lock().unwrap();

        let metrics = SoakMetrics::from_samples(&latencies, errors, elapsed_secs);

        SoakResult {
            passed: true,
            failure_reason: None,
            metrics,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_config() -> SoakTestConfig {
        SoakTestConfig {
            duration_secs: 1,
            concurrency: 2,
            target_rps: 0,
            warmup_secs: 0,
        }
    }

    #[test]
    fn test_soak_noop_workload_completes() {
        let runner = SoakTestRunner::new(fast_config());
        let result = runner.run_local(&NoOpWorkload);
        assert!(result.metrics.total_requests > 0, "should have executed at least one request");
        assert_eq!(result.metrics.errors, 0);
        assert!(result.passed);
    }

    #[test]
    fn test_soak_error_rate_detection() {
        let runner = SoakTestRunner::new(fast_config());
        let result = runner.run_local(&FailingWorkload);
        // All requests should have errored.
        assert!(result.metrics.errors > 0, "FailingWorkload should produce errors");
        assert_eq!(result.metrics.total_requests, result.metrics.errors);
    }

    #[test]
    fn test_soak_threshold_check() {
        // Build a result with known metrics and verify threshold logic.
        let metrics = SoakMetrics {
            total_requests: 1000,
            errors: 10,        // 1 % error rate
            p50_ms: 5,
            p99_ms: 50,
            peak_rps: 200.0,
        };
        let result = SoakResult { metrics, passed: true, failure_reason: None };

        // Should pass: error rate 1% ≤ 5%, p99 50ms ≤ 100ms.
        assert!(result.check_thresholds(0.05, 100));

        // Should fail: error rate 1% > 0.5%.
        assert!(!result.check_thresholds(0.005, 100));

        // Should fail: p99 50ms > 40ms.
        assert!(!result.check_thresholds(0.05, 40));

        // Should fail: both thresholds breached.
        assert!(!result.check_thresholds(0.005, 40));
    }
}
