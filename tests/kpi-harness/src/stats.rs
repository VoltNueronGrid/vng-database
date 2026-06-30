//! Statistics for KPI measurement harnesses (E-1..E-6).
//!
//! Pure, deterministic computations — latency percentiles, throughput, scaling
//! efficiency, and success rate — with unit tests. The harness binaries feed
//! real measured samples into these functions and assert the README KPI targets.

/// Nearest-rank percentile (1-based rank) over a latency sample set, in the same
/// unit as the input (milliseconds). Returns 0.0 for an empty set.
///
/// `p` is a fraction in `[0.0, 1.0]` (e.g. `0.95` for p95).
pub fn percentile(samples: &[f64], p: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p = p.clamp(0.0, 1.0);
    // Nearest-rank: rank = ceil(p * n), clamped to [1, n].
    let n = sorted.len();
    let rank = (p * n as f64).ceil() as usize;
    let idx = rank.clamp(1, n) - 1;
    sorted[idx]
}

/// Median (p50).
pub fn median(samples: &[f64]) -> f64 {
    percentile(samples, 0.50)
}

/// Arithmetic mean, or 0.0 for an empty set.
pub fn mean(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().sum::<f64>() / samples.len() as f64
}

/// Operations per second given a completed-op count and an elapsed duration.
pub fn throughput_per_sec(ops: u64, elapsed_secs: f64) -> f64 {
    if elapsed_secs <= 0.0 {
        return 0.0;
    }
    ops as f64 / elapsed_secs
}

/// Parallel scaling efficiency for `n` workers:
///   `efficiency = (throughput_n / throughput_1) / n`.
/// A perfectly linear scale-out yields `1.0` (100%). Returns 0.0 when the
/// single-worker baseline is non-positive.
pub fn scaling_efficiency(throughput_1: f64, throughput_n: f64, n: usize) -> f64 {
    if throughput_1 <= 0.0 || n == 0 {
        return 0.0;
    }
    (throughput_n / throughput_1) / n as f64
}

/// Success rate as a fraction in `[0.0, 1.0]`: `successes / attempts`.
pub fn success_rate(successes: u64, attempts: u64) -> f64 {
    if attempts == 0 {
        return 0.0;
    }
    successes as f64 / attempts as f64
}

/// Round a float to `places` decimals (for stable JSON artifacts).
pub fn round(value: f64, places: u32) -> f64 {
    let f = 10f64.powi(places as i32);
    (value * f).round() / f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_nearest_rank() {
        let s: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        assert_eq!(percentile(&s, 0.50), 50.0);
        assert_eq!(percentile(&s, 0.95), 95.0);
        assert_eq!(percentile(&s, 0.99), 99.0);
        assert_eq!(percentile(&s, 1.0), 100.0);
    }

    #[test]
    fn percentile_handles_small_and_empty() {
        assert_eq!(percentile(&[], 0.95), 0.0);
        assert_eq!(percentile(&[7.0], 0.95), 7.0);
        // Unsorted input is sorted internally.
        assert_eq!(percentile(&[5.0, 1.0, 3.0, 2.0, 4.0], 0.50), 3.0);
    }

    #[test]
    fn median_and_mean() {
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), 2.0);
        assert_eq!(mean(&[2.0, 4.0, 6.0]), 4.0);
        assert_eq!(mean(&[]), 0.0);
    }

    #[test]
    fn throughput_division() {
        assert_eq!(throughput_per_sec(1000, 2.0), 500.0);
        assert_eq!(throughput_per_sec(1000, 0.0), 0.0);
    }

    #[test]
    fn scaling_efficiency_linear_and_sublinear() {
        // Perfectly linear: 4 workers do 4x → 100%.
        assert_eq!(scaling_efficiency(100.0, 400.0, 4), 1.0);
        // Sub-linear: 4 workers do 3x → 75%.
        assert_eq!(scaling_efficiency(100.0, 300.0, 4), 0.75);
        // Degenerate baselines.
        assert_eq!(scaling_efficiency(0.0, 400.0, 4), 0.0);
        assert_eq!(scaling_efficiency(100.0, 400.0, 0), 0.0);
    }

    #[test]
    fn success_rate_fraction() {
        assert_eq!(success_rate(9995, 10000), 0.9995);
        assert_eq!(success_rate(0, 0), 0.0);
        assert_eq!(success_rate(10, 10), 1.0);
    }

    #[test]
    fn round_places() {
        assert_eq!(round(0.99949, 4), 0.9995);
        assert_eq!(round(12.3456, 2), 12.35);
    }
}
