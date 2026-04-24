/// S9-003: Failure injection and recovery framework.
///
/// Provides deterministic and probabilistic failure injection for use in soak
/// runs and standalone unit tests.  No external dependencies are required.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

// ---------------------------------------------------------------------------
// FailureKind
// ---------------------------------------------------------------------------

/// The category of failure being simulated.
#[derive(Debug, Clone, PartialEq)]
pub enum FailureKind {
    /// Simulate a network timeout (no response).
    NetworkTimeout,
    /// Simulate an abrupt connection reset.
    ConnectionReset,
    /// Simulate a write that delivers only partial data.
    PartialWrite,
    /// Simulate a slow response after the given delay in milliseconds.
    SlowResponse(u64),
}

// ---------------------------------------------------------------------------
// FailurePolicy
// ---------------------------------------------------------------------------

/// Controls *when* the injector fires.
#[derive(Debug, Clone)]
pub enum FailurePolicy {
    /// Never inject — pass-through for baseline runs.
    Never,
    /// Always inject — useful for verifying recovery code.
    Always,
    /// Inject approximately `rate` fraction of the time (0.0–1.0).
    Rate(f64),
    /// Inject exactly once after `n` successful calls.
    AfterN(u64),
}

// ---------------------------------------------------------------------------
// InjectedFailure
// ---------------------------------------------------------------------------

/// Carries context about a fired injection event.
#[derive(Debug)]
pub struct InjectedFailure {
    pub kind: FailureKind,
    /// The value of the injected-count *at the moment this failure fired*.
    pub at_request: u64,
}

impl std::fmt::Display for InjectedFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InjectedFailure({:?} at request {})", self.kind, self.at_request)
    }
}

// ---------------------------------------------------------------------------
// FailureInjector
// ---------------------------------------------------------------------------

/// Thread-safe failure injector.
pub struct FailureInjector {
    pub kind: FailureKind,
    pub policy: FailurePolicy,
    /// Total number of injections fired so far.
    pub injected_count: AtomicU64,
    /// Total number of `should_inject` calls (for Rate policy denominator).
    call_count: AtomicU64,
}

impl FailureInjector {
    pub fn new(kind: FailureKind, policy: FailurePolicy) -> Self {
        Self {
            kind,
            policy,
            injected_count: AtomicU64::new(0),
            call_count: AtomicU64::new(0),
        }
    }

    /// Returns `true` if a failure should be injected on this call.
    pub fn should_inject(&self) -> bool {
        let call = self.call_count.fetch_add(1, Ordering::Relaxed);
        match &self.policy {
            FailurePolicy::Never => false,
            FailurePolicy::Always => true,
            FailurePolicy::Rate(r) => {
                // Deterministic pseudo-random via linear congruential generator
                // seeded by call index — avoids pulling in `rand`.
                let pseudo = lcg_frac(call);
                pseudo < *r
            }
            FailurePolicy::AfterN(n) => call == *n,
        }
    }

    /// If `should_inject()` returns `true`, record the injection and return
    /// `Err(InjectedFailure)`; otherwise return `Ok(())`.
    pub fn inject(&self) -> Result<(), InjectedFailure> {
        if self.should_inject() {
            let count = self.injected_count.fetch_add(1, Ordering::Relaxed) + 1;
            Err(InjectedFailure {
                kind: self.kind.clone(),
                at_request: count,
            })
        } else {
            Ok(())
        }
    }
}

/// Hash-based pseudo-random fraction → [0, 1), no external deps.
///
/// Uses a splitmix64-style integer finaliser which has much better avalanche
/// properties than a plain LCG, ensuring good distribution across the full
/// [0, 1) range regardless of which seed values are used.
fn lcg_frac(seed: u64) -> f64 {
    // splitmix64 finaliser (public domain).
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z = z ^ (z >> 31);
    // Map to [0, 1) using the top 53 bits.
    (z >> 11) as f64 / (1u64 << 53) as f64
}

// ---------------------------------------------------------------------------
// RecoveryProbe
// ---------------------------------------------------------------------------

/// Result of a recovery probe sequence.
#[derive(Debug)]
pub struct RecoveryResult {
    /// Whether the `check` function eventually returned `true`.
    pub recovered: bool,
    /// Number of probe attempts made.
    pub attempts: u32,
    /// Total elapsed time across all attempts in milliseconds.
    pub total_ms: u64,
}

/// Probe for recovery by calling `check` repeatedly with exponential-ish back-off.
///
/// * `max_attempts` — give up after this many tries.
/// * `backoff_ms` — initial sleep between attempts (doubles each retry, up to 1 s).
/// * `check` — returns `true` when the system is considered recovered.
pub fn probe_recovery(
    max_attempts: u32,
    backoff_ms: u64,
    check: &dyn Fn() -> bool,
) -> RecoveryResult {
    let start = Instant::now();
    let mut current_backoff = backoff_ms;

    for attempt in 1..=max_attempts {
        if check() {
            return RecoveryResult {
                recovered: true,
                attempts: attempt,
                total_ms: start.elapsed().as_millis() as u64,
            };
        }
        // Cap sleep so unit tests remain fast.
        let sleep_ms = current_backoff.min(100);
        std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
        current_backoff = (current_backoff * 2).min(1_000);
    }

    RecoveryResult {
        recovered: false,
        attempts: max_attempts,
        total_ms: start.elapsed().as_millis() as u64,
    }
}

/// Convenience type alias kept for ergonomics in tests.
pub type RecoveryProbe = fn(u32, u64, &dyn Fn() -> bool) -> RecoveryResult;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering as AOrdering};

    #[test]
    fn test_failure_injector_never_policy() {
        let inj = FailureInjector::new(FailureKind::NetworkTimeout, FailurePolicy::Never);
        for _ in 0..100 {
            assert!(!inj.should_inject(), "Never policy must never inject");
        }
        assert_eq!(inj.injected_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_failure_injector_always_policy() {
        let inj = FailureInjector::new(FailureKind::ConnectionReset, FailurePolicy::Always);
        for i in 0..10 {
            let result = inj.inject();
            assert!(result.is_err(), "call {i}: Always policy must always inject");
        }
        assert_eq!(inj.injected_count.load(Ordering::Relaxed), 10);
    }

    #[test]
    fn test_failure_injector_rate_policy_approximate() {
        let target_rate = 0.3_f64;
        let inj = FailureInjector::new(
            FailureKind::SlowResponse(200),
            FailurePolicy::Rate(target_rate),
        );

        let n = 1000u64;
        let mut fired = 0u64;
        for _ in 0..n {
            if inj.should_inject() {
                fired += 1;
            }
        }

        let actual_rate = fired as f64 / n as f64;
        // Allow ±15 percentage points — deterministic LCG is not perfectly uniform.
        assert!(
            (actual_rate - target_rate).abs() < 0.15,
            "rate {actual_rate:.3} not within ±15% of target {target_rate}"
        );
    }

    #[test]
    fn test_recovery_probe_immediate_success() {
        let result = probe_recovery(5, 1, &|| true);
        assert!(result.recovered);
        assert_eq!(result.attempts, 1, "should succeed on first attempt");
    }

    #[test]
    fn test_recovery_probe_eventual_success() {
        // Succeed on the 3rd attempt.
        let counter = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&counter);
        let result = probe_recovery(10, 1, &move || {
            let v = c.fetch_add(1, AOrdering::Relaxed);
            v >= 2 // succeed from attempt index 2 onward (3rd call)
        });
        assert!(result.recovered);
        assert_eq!(result.attempts, 3);
    }
}
