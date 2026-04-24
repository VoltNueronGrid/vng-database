//! Memory profiling scaffolding for VoltNueronGrid benchmarks.
//!
//! Uses a simple monotonic counter approach for now — real allocator hooks
//! (e.g. jemalloc epoch stats) are a follow-on task documented in
//! `services/voltnuerongridd/reference/s8-memory-allocator-strategy.md`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

// ─── Global simulated allocator counter ──────────────────────────────────────

/// Simulated heap counter incremented by `MemoryProfiler::simulate_alloc`.
static SIMULATED_HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);
static SIMULATED_PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

// ─── Types ────────────────────────────────────────────────────────────────────

/// A point-in-time snapshot of heap usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySnapshot {
    /// Bytes currently tracked as allocated (simulated counter).
    pub heap_allocated_bytes: usize,
    /// Peak bytes observed since the profiler was started.
    pub peak_bytes: usize,
    /// Wall-clock timestamp in milliseconds since Unix epoch.
    pub timestamp_ms: u128,
}

/// Collects memory snapshots around a workload.
pub struct MemoryProfiler {
    snapshots: Vec<MemorySnapshot>,
}

impl MemoryProfiler {
    /// Create a new profiler and reset the simulated counters.
    pub fn new() -> Self {
        SIMULATED_HEAP_BYTES.store(0, Ordering::SeqCst);
        SIMULATED_PEAK_BYTES.store(0, Ordering::SeqCst);
        Self {
            snapshots: Vec::new(),
        }
    }

    /// Simulate allocating `bytes` (increments the global counter).
    pub fn simulate_alloc(&self, bytes: usize) {
        let prev = SIMULATED_HEAP_BYTES.fetch_add(bytes, Ordering::SeqCst);
        let new_val = prev + bytes;
        // Update peak if needed (best-effort, not strictly atomic).
        let mut peak = SIMULATED_PEAK_BYTES.load(Ordering::SeqCst);
        while new_val > peak {
            match SIMULATED_PEAK_BYTES.compare_exchange_weak(
                peak,
                new_val,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(current) => peak = current,
            }
        }
    }

    /// Simulate freeing `bytes` (decrements the global counter, floor 0).
    pub fn simulate_free(&self, bytes: usize) {
        SIMULATED_HEAP_BYTES.fetch_saturating_sub(bytes);
    }

    /// Capture a snapshot of current simulated heap state.
    pub fn take_snapshot(&mut self) -> MemorySnapshot {
        let snapshot = MemorySnapshot {
            heap_allocated_bytes: SIMULATED_HEAP_BYTES.load(Ordering::SeqCst),
            peak_bytes: SIMULATED_PEAK_BYTES.load(Ordering::SeqCst),
            timestamp_ms: current_timestamp_ms(),
        };
        self.snapshots.push(snapshot.clone());
        snapshot
    }

    /// Return all snapshots collected so far.
    pub fn snapshots(&self) -> &[MemorySnapshot] {
        &self.snapshots
    }
}

impl Default for MemoryProfiler {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Allocation report ────────────────────────────────────────────────────────

/// Summary of allocations across an entire workload.
#[derive(Debug, Clone)]
pub struct AllocationReport {
    /// All snapshots captured during the workload.
    pub snapshots: Vec<MemorySnapshot>,
    /// Peak bytes observed across all snapshots.
    pub peak_bytes: usize,
    /// Estimated bytes allocated per row processed (linear regression over snapshots).
    pub growth_rate_bytes_per_row: f64,
}

/// Summarise a slice of memory snapshots against the number of rows processed.
///
/// `rows_processed` is assumed to grow linearly between the first and last snapshot,
/// which is sufficient for the current simulated-counter model.
pub fn summarize(snapshots: &[MemorySnapshot], rows_processed: usize) -> AllocationReport {
    let peak_bytes = snapshots.iter().map(|s| s.peak_bytes).max().unwrap_or(0);

    let growth_rate_bytes_per_row = if snapshots.len() < 2 || rows_processed == 0 {
        0.0
    } else {
        let first = snapshots.first().unwrap().heap_allocated_bytes as f64;
        let last = snapshots.last().unwrap().heap_allocated_bytes as f64;
        (last - first) / rows_processed as f64
    };

    AllocationReport {
        snapshots: snapshots.to_vec(),
        peak_bytes,
        growth_rate_bytes_per_row,
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

fn current_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

trait AtomicSaturatingSub {
    fn fetch_saturating_sub(&self, val: usize) -> usize;
}

impl AtomicSaturatingSub for AtomicUsize {
    fn fetch_saturating_sub(&self, val: usize) -> usize {
        loop {
            let current = self.load(Ordering::SeqCst);
            let new_val = current.saturating_sub(val);
            match self.compare_exchange_weak(
                current,
                new_val,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return current,
                Err(_) => continue,
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_profiler_snapshot() {
        let mut profiler = MemoryProfiler::new();

        // Initially zero.
        let snap0 = profiler.take_snapshot();
        assert_eq!(snap0.heap_allocated_bytes, 0);
        assert_eq!(snap0.peak_bytes, 0);
        assert!(snap0.timestamp_ms > 0);

        // After simulating an allocation the counter should rise.
        profiler.simulate_alloc(1024);
        let snap1 = profiler.take_snapshot();
        assert_eq!(snap1.heap_allocated_bytes, 1024);
        assert_eq!(snap1.peak_bytes, 1024);

        // After freeing, heap should drop but peak stays.
        profiler.simulate_free(512);
        let snap2 = profiler.take_snapshot();
        assert_eq!(snap2.heap_allocated_bytes, 512);
        assert_eq!(snap2.peak_bytes, 1024);

        assert_eq!(profiler.snapshots().len(), 3);
    }

    #[test]
    fn test_allocation_report_summarize() {
        let snapshots = vec![
            MemorySnapshot {
                heap_allocated_bytes: 0,
                peak_bytes: 0,
                timestamp_ms: 1_000,
            },
            MemorySnapshot {
                heap_allocated_bytes: 500,
                peak_bytes: 500,
                timestamp_ms: 2_000,
            },
            MemorySnapshot {
                heap_allocated_bytes: 1_000,
                peak_bytes: 1_000,
                timestamp_ms: 3_000,
            },
        ];

        let report = summarize(&snapshots, 100);
        assert_eq!(report.peak_bytes, 1_000);
        // growth_rate = (1000 - 0) / 100 rows = 10.0 bytes/row
        assert!(
            (report.growth_rate_bytes_per_row - 10.0).abs() < f64::EPSILON,
            "expected 10.0 bytes/row, got {}",
            report.growth_rate_bytes_per_row
        );
        assert_eq!(report.snapshots.len(), 3);
    }

    #[test]
    fn test_allocation_report_empty_snapshots() {
        let report = summarize(&[], 0);
        assert_eq!(report.peak_bytes, 0);
        assert_eq!(report.growth_rate_bytes_per_row, 0.0);
    }

    #[test]
    fn test_allocation_report_zero_rows_processed() {
        let snapshots = vec![MemorySnapshot {
            heap_allocated_bytes: 256,
            peak_bytes: 512,
            timestamp_ms: 1_000,
        }];
        let report = summarize(&snapshots, 0);
        // With 0 rows processed growth rate is undefined → returns 0.0.
        assert_eq!(report.growth_rate_bytes_per_row, 0.0);
        assert_eq!(report.peak_bytes, 512);
    }
}
