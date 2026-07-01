//! H9-14: Adaptive storage controller.
//!
//! Rule-based controller that observes per-segment workload statistics and
//! emits policy decisions for projection-cache gating, merge-frequency
//! tuning, and freshness-priority adjustment.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::types::{PartitionId, SegmentId};

// ──────────────────────────────────────────────────────────────────────────────
// Core types
// ──────────────────────────────────────────────────────────────────────────────

/// Per-segment workload statistics observed over a rolling window.
#[derive(Debug, Clone)]
pub struct SegmentWorkloadStats {
    pub segment_id: SegmentId,
    pub partition_id: PartitionId,
    pub table_name: String,
    pub read_ops_per_sec: f64,
    pub write_ops_per_sec: f64,
    /// Scans per minute.
    pub scan_frequency: f64,
    pub tail_version_count: u64,
    pub tail_bytes: u64,
    /// Cache hit rate in the range 0.0..=1.0.
    pub cache_hit_rate: f64,
    /// Average cost of the last three merges (milliseconds).
    pub merge_cost_ms: u64,
    /// Freshness lag behind the write-head (milliseconds).
    pub freshness_lag_ms: u64,
    /// Wall-clock epoch milliseconds at which these stats were captured.
    pub observed_at_ms: u64,
}

/// Policy decision for a single segment.
#[derive(Debug, Clone, PartialEq)]
pub enum PolicyDecision {
    NoChange,
    EnableProjectionCache { reason: String },
    DisableProjectionCache { reason: String },
    IncreaseMergeFrequency { new_threshold_ms: u64, reason: String },
    DecreaseMergeFrequency { new_threshold_ms: u64, reason: String },
    ElevateFreshnessPriority { reason: String },
    ReduceFreshnessPriority { reason: String },
}

/// A single policy-change recommendation with audit metadata.
#[derive(Debug, Clone)]
pub struct PolicyChange {
    pub segment_id: SegmentId,
    pub decision: PolicyDecision,
    /// Wall-clock epoch milliseconds when the recommendation was produced.
    pub timestamp_ms: u64,
    /// Confidence score in the range 0.0..=1.0.
    pub confidence: f64,
    /// Whether the caller has acknowledged / applied this recommendation.
    pub applied: bool,
}

/// Adaptive controller configuration.
#[derive(Debug, Clone)]
pub struct AdaptiveControllerConfig {
    /// Scans/min above which the projection cache should be enabled.
    pub cache_enable_scan_freq_threshold: f64,
    /// Hit-rate floor below which a low-scan cache is considered wasteful.
    pub cache_disable_hit_rate_floor: f64,
    /// `tail_version_count` above which merges should happen more often.
    pub merge_increase_tail_threshold: u64,
    /// `tail_version_count` below which merges can be scheduled less often.
    pub merge_decrease_tail_threshold: u64,
    /// Default merge period (milliseconds).
    pub merge_base_threshold_ms: u64,
    /// Freshness lag above which priority should be elevated (milliseconds).
    pub freshness_elevate_lag_threshold_ms: u64,
    /// Maximum number of `PolicyChange` records to keep in the audit ring.
    pub max_audit_history: usize,
}

impl Default for AdaptiveControllerConfig {
    fn default() -> Self {
        Self {
            cache_enable_scan_freq_threshold: 10.0,
            cache_disable_hit_rate_floor: 0.3,
            merge_increase_tail_threshold: 500,
            merge_decrease_tail_threshold: 50,
            merge_base_threshold_ms: 5_000,
            freshness_elevate_lag_threshold_ms: 2_000,
            max_audit_history: 1_000,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Metrics
// ──────────────────────────────────────────────────────────────────────────────

/// Snapshot of controller-level counters.
#[derive(Debug, Clone)]
pub struct ControllerMetrics {
    pub decisions_total: u64,
    pub no_change_total: u64,
    pub changes_applied_total: u64,
    pub pending_changes_count: usize,
    pub audit_history_size: usize,
}

// ──────────────────────────────────────────────────────────────────────────────
// Controller
// ──────────────────────────────────────────────────────────────────────────────

/// Rule-based adaptive storage controller.
///
/// Evaluates per-segment workload statistics and emits policy decisions for
/// projection-cache gating, merge-frequency tuning, and freshness-priority
/// adjustment.  All state is internally synchronized; the controller can be
/// shared across threads via `Arc`.
pub struct AdaptiveStorageController {
    config: AdaptiveControllerConfig,
    policy_history: Arc<Mutex<Vec<PolicyChange>>>,
    decisions_total: Arc<AtomicU64>,
    no_change_total: Arc<AtomicU64>,
    changes_applied_total: Arc<AtomicU64>,
}

impl AdaptiveStorageController {
    /// Create a controller with the supplied configuration.
    pub fn new(config: AdaptiveControllerConfig) -> Self {
        Self {
            config,
            policy_history: Arc::new(Mutex::new(Vec::new())),
            decisions_total: Arc::new(AtomicU64::new(0)),
            no_change_total: Arc::new(AtomicU64::new(0)),
            changes_applied_total: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Create a controller with `AdaptiveControllerConfig::default()`.
    pub fn with_defaults() -> Self {
        Self::new(AdaptiveControllerConfig::default())
    }

    // ─── Rule engine ──────────────────────────────────────────────────────────

    /// Evaluate a single segment and produce a policy decision.
    ///
    /// Rules are checked in priority order; the first matching rule wins.
    pub fn evaluate_segment(&self, stats: &SegmentWorkloadStats) -> PolicyDecision {
        let cfg = &self.config;

        // 1. Cache enablement — high scan frequency with low hit rate.
        if stats.scan_frequency > cfg.cache_enable_scan_freq_threshold
            && stats.cache_hit_rate < 0.5
        {
            return PolicyDecision::EnableProjectionCache {
                reason: format!(
                    "scan_frequency {:.1} > threshold {:.1} and hit_rate {:.2} < 0.5",
                    stats.scan_frequency, cfg.cache_enable_scan_freq_threshold,
                    stats.cache_hit_rate
                ),
            };
        }

        // 2. Cache disable — low scan activity and low hit rate means cache wastes memory.
        if stats.scan_frequency < cfg.cache_enable_scan_freq_threshold * 0.5
            && stats.cache_hit_rate < cfg.cache_disable_hit_rate_floor
        {
            return PolicyDecision::DisableProjectionCache {
                reason: format!(
                    "scan_frequency {:.1} < half-threshold {:.1} and hit_rate {:.2} < floor {:.2}",
                    stats.scan_frequency,
                    cfg.cache_enable_scan_freq_threshold * 0.5,
                    stats.cache_hit_rate,
                    cfg.cache_disable_hit_rate_floor
                ),
            };
        }

        // 3. Merge increase — tail is growing too large.
        if stats.tail_version_count > cfg.merge_increase_tail_threshold {
            let new_threshold_ms = cfg.merge_base_threshold_ms / 2;
            return PolicyDecision::IncreaseMergeFrequency {
                new_threshold_ms,
                reason: format!(
                    "tail_version_count {} > threshold {}; halving merge period to {}ms",
                    stats.tail_version_count, cfg.merge_increase_tail_threshold, new_threshold_ms
                ),
            };
        }

        // 4. Merge decrease — tail is small but merges are expensive.
        if stats.tail_version_count < cfg.merge_decrease_tail_threshold
            && stats.merge_cost_ms > 100
        {
            let new_threshold_ms = cfg.merge_base_threshold_ms * 2;
            return PolicyDecision::DecreaseMergeFrequency {
                new_threshold_ms,
                reason: format!(
                    "tail_version_count {} < threshold {} and merge_cost_ms {} > 100; \
                     doubling merge period to {}ms",
                    stats.tail_version_count, cfg.merge_decrease_tail_threshold,
                    stats.merge_cost_ms, new_threshold_ms
                ),
            };
        }

        // 5. Elevate freshness — segment is falling behind the write-head.
        if stats.freshness_lag_ms > cfg.freshness_elevate_lag_threshold_ms {
            return PolicyDecision::ElevateFreshnessPriority {
                reason: format!(
                    "freshness_lag_ms {} > threshold {}ms",
                    stats.freshness_lag_ms, cfg.freshness_elevate_lag_threshold_ms
                ),
            };
        }

        // 6. Reduce freshness — segment is well ahead of SLA and scan load is low.
        let low_scan_threshold = cfg.cache_enable_scan_freq_threshold * 0.25;
        if stats.freshness_lag_ms < cfg.freshness_elevate_lag_threshold_ms / 4
            && stats.scan_frequency < low_scan_threshold
        {
            return PolicyDecision::ReduceFreshnessPriority {
                reason: format!(
                    "freshness_lag_ms {} < quarter-threshold {}ms and scan_frequency {:.1} is low",
                    stats.freshness_lag_ms,
                    cfg.freshness_elevate_lag_threshold_ms / 4,
                    stats.scan_frequency
                ),
            };
        }

        PolicyDecision::NoChange
    }

    /// Confidence score for a given decision.
    fn confidence_for(decision: &PolicyDecision, stats: &SegmentWorkloadStats) -> f64 {
        match decision {
            PolicyDecision::NoChange => 1.0,
            PolicyDecision::EnableProjectionCache { .. } => {
                if stats.cache_hit_rate < 0.2 { 0.9 } else { 0.8 }
            }
            PolicyDecision::DisableProjectionCache { .. } => 0.7,
            PolicyDecision::IncreaseMergeFrequency { .. } => 0.85,
            PolicyDecision::DecreaseMergeFrequency { .. } => 0.6,
            PolicyDecision::ElevateFreshnessPriority { .. } => 0.9,
            PolicyDecision::ReduceFreshnessPriority { .. } => 0.5,
        }
    }

    // ─── Batch evaluation ─────────────────────────────────────────────────────

    /// Evaluate multiple segments and return a `PolicyChange` for each.
    ///
    /// Results with `NoChange` are still recorded in the audit history so that
    /// the absence of action is traceable.
    pub fn evaluate_batch(&self, stats: &[SegmentWorkloadStats]) -> Vec<PolicyChange> {
        let now_ms = now_epoch_ms();
        let mut results = Vec::with_capacity(stats.len());

        for s in stats {
            let decision = self.evaluate_segment(s);
            let confidence = Self::confidence_for(&decision, s);

            self.decisions_total.fetch_add(1, Ordering::Relaxed);
            if decision == PolicyDecision::NoChange {
                self.no_change_total.fetch_add(1, Ordering::Relaxed);
            }

            let change = PolicyChange {
                segment_id: s.segment_id,
                decision,
                timestamp_ms: now_ms,
                confidence,
                applied: false,
            };
            self.append_history(change.clone());
            results.push(change);
        }

        results
    }

    // ─── Audit / history helpers ──────────────────────────────────────────────

    /// Mark a previously recorded recommendation as applied.
    ///
    /// Matches on `segment_id` **and** `timestamp_ms`.  If multiple records
    /// share those values the most-recently inserted one is marked.
    pub fn mark_applied(&self, segment_id: SegmentId, timestamp_ms: u64) {
        if let Ok(mut history) = self.policy_history.lock() {
            // Walk in reverse so we mark the latest matching entry first.
            for change in history.iter_mut().rev() {
                if change.segment_id == segment_id && change.timestamp_ms == timestamp_ms {
                    if !change.applied {
                        change.applied = true;
                        self.changes_applied_total.fetch_add(1, Ordering::Relaxed);
                    }
                    break;
                }
            }
        }
    }

    /// Return the full audit history (cloned snapshot).
    pub fn audit_history(&self) -> Vec<PolicyChange> {
        self.policy_history
            .lock()
            .map(|h| h.clone())
            .unwrap_or_default()
    }

    /// Return only applied changes.
    pub fn applied_changes(&self) -> Vec<PolicyChange> {
        self.policy_history
            .lock()
            .map(|h| h.iter().filter(|c| c.applied).cloned().collect())
            .unwrap_or_default()
    }

    /// Return only pending (not-yet-applied) changes.
    pub fn pending_changes(&self) -> Vec<PolicyChange> {
        self.policy_history
            .lock()
            .map(|h| h.iter().filter(|c| !c.applied).cloned().collect())
            .unwrap_or_default()
    }

    /// Return a snapshot of controller-level counters.
    pub fn metrics(&self) -> ControllerMetrics {
        let (history_size, pending_count) = self
            .policy_history
            .lock()
            .map(|h| (h.len(), h.iter().filter(|c| !c.applied).count()))
            .unwrap_or((0, 0));

        ControllerMetrics {
            decisions_total: self.decisions_total.load(Ordering::Relaxed),
            no_change_total: self.no_change_total.load(Ordering::Relaxed),
            changes_applied_total: self.changes_applied_total.load(Ordering::Relaxed),
            pending_changes_count: pending_count,
            audit_history_size: history_size,
        }
    }

    /// Record a `NoChange` sentinel in the audit trail for a segment, e.g.
    /// after an operator manually resets its settings to defaults.
    pub fn revert_segment(&self, segment_id: SegmentId) {
        let change = PolicyChange {
            segment_id,
            decision: PolicyDecision::NoChange,
            timestamp_ms: now_epoch_ms(),
            confidence: 1.0,
            applied: true,
        };
        self.decisions_total.fetch_add(1, Ordering::Relaxed);
        self.no_change_total.fetch_add(1, Ordering::Relaxed);
        self.changes_applied_total.fetch_add(1, Ordering::Relaxed);
        self.append_history(change);
    }

    // ─── Private helpers ──────────────────────────────────────────────────────

    /// Append a `PolicyChange` to the bounded audit ring.
    fn append_history(&self, change: PolicyChange) {
        if let Ok(mut history) = self.policy_history.lock() {
            if history.len() >= self.config.max_audit_history {
                history.remove(0);
            }
            history.push(change);
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Utility
// ──────────────────────────────────────────────────────────────────────────────

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_stats(segment_id: u32) -> SegmentWorkloadStats {
        SegmentWorkloadStats {
            segment_id: SegmentId(segment_id),
            partition_id: PartitionId(1),
            table_name: format!("tbl_{segment_id}"),
            read_ops_per_sec: 100.0,
            write_ops_per_sec: 10.0,
            scan_frequency: 5.0,        // below enable threshold (10.0)
            tail_version_count: 100,    // between decrease (50) and increase (500)
            tail_bytes: 1_024 * 1_024,
            cache_hit_rate: 0.8,        // good hit rate
            merge_cost_ms: 50,          // cheap merges
            freshness_lag_ms: 500,      // well within SLA (2000ms)
            observed_at_ms: now_epoch_ms(),
        }
    }

    fn controller() -> AdaptiveStorageController {
        AdaptiveStorageController::with_defaults()
    }

    // ── Individual rule tests ────────────────────────────────────────────────

    #[test]
    fn test_no_change_for_normal_segment() {
        let ctrl = controller();
        let stats = default_stats(1);
        assert_eq!(ctrl.evaluate_segment(&stats), PolicyDecision::NoChange);
    }

    #[test]
    fn test_enable_cache_when_high_scan_freq() {
        let ctrl = controller();
        let stats = SegmentWorkloadStats {
            scan_frequency: 20.0,   // > threshold 10.0
            cache_hit_rate: 0.3,    // < 0.5
            ..default_stats(2)
        };
        let decision = ctrl.evaluate_segment(&stats);
        assert!(
            matches!(decision, PolicyDecision::EnableProjectionCache { .. }),
            "expected EnableProjectionCache, got {decision:?}"
        );
    }

    #[test]
    fn test_disable_cache_when_low_hit_rate_and_low_scan() {
        let ctrl = controller();
        let stats = SegmentWorkloadStats {
            scan_frequency: 2.0,    // < 10.0 * 0.5 = 5.0
            cache_hit_rate: 0.1,    // < floor 0.3
            ..default_stats(3)
        };
        let decision = ctrl.evaluate_segment(&stats);
        assert!(
            matches!(decision, PolicyDecision::DisableProjectionCache { .. }),
            "expected DisableProjectionCache, got {decision:?}"
        );
    }

    #[test]
    fn test_increase_merge_when_tail_too_large() {
        let ctrl = controller();
        let stats = SegmentWorkloadStats {
            tail_version_count: 600, // > threshold 500
            ..default_stats(4)
        };
        let decision = ctrl.evaluate_segment(&stats);
        assert!(
            matches!(decision, PolicyDecision::IncreaseMergeFrequency { .. }),
            "expected IncreaseMergeFrequency, got {decision:?}"
        );
        // Verify the new threshold is half the base.
        if let PolicyDecision::IncreaseMergeFrequency { new_threshold_ms, .. } = decision {
            assert_eq!(new_threshold_ms, 5_000 / 2);
        }
    }

    #[test]
    fn test_decrease_merge_when_tail_small_and_expensive() {
        let ctrl = controller();
        let stats = SegmentWorkloadStats {
            tail_version_count: 20,  // < threshold 50
            merge_cost_ms: 200,      // > 100ms
            ..default_stats(5)
        };
        let decision = ctrl.evaluate_segment(&stats);
        assert!(
            matches!(decision, PolicyDecision::DecreaseMergeFrequency { .. }),
            "expected DecreaseMergeFrequency, got {decision:?}"
        );
        if let PolicyDecision::DecreaseMergeFrequency { new_threshold_ms, .. } = decision {
            assert_eq!(new_threshold_ms, 5_000 * 2);
        }
    }

    #[test]
    fn test_elevate_freshness_priority_when_lagging() {
        let ctrl = controller();
        let stats = SegmentWorkloadStats {
            freshness_lag_ms: 3_000, // > threshold 2_000
            ..default_stats(6)
        };
        let decision = ctrl.evaluate_segment(&stats);
        assert!(
            matches!(decision, PolicyDecision::ElevateFreshnessPriority { .. }),
            "expected ElevateFreshnessPriority, got {decision:?}"
        );
    }

    #[test]
    fn test_reduce_freshness_when_well_ahead_and_low_scan() {
        let ctrl = controller();
        // lag < 2000/4 = 500 AND scan_frequency < 10.0 * 0.25 = 2.5
        let stats = SegmentWorkloadStats {
            freshness_lag_ms: 100,
            scan_frequency: 1.0,
            ..default_stats(7)
        };
        let decision = ctrl.evaluate_segment(&stats);
        assert!(
            matches!(decision, PolicyDecision::ReduceFreshnessPriority { .. }),
            "expected ReduceFreshnessPriority, got {decision:?}"
        );
    }

    // ── Batch and history tests ──────────────────────────────────────────────

    #[test]
    fn test_batch_evaluation_produces_per_segment_decisions() {
        let ctrl = controller();
        let batch: Vec<_> = (1u32..=4).map(default_stats).collect();
        let changes = ctrl.evaluate_batch(&batch);
        assert_eq!(changes.len(), 4);
        for (i, change) in changes.iter().enumerate() {
            assert_eq!(change.segment_id, SegmentId(i as u32 + 1));
        }
    }

    #[test]
    fn test_audit_history_records_changes() {
        let ctrl = controller();
        let batch: Vec<_> = (1u32..=3).map(default_stats).collect();
        ctrl.evaluate_batch(&batch);
        let history = ctrl.audit_history();
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_mark_applied_updates_change_status() {
        let ctrl = controller();
        let stats = vec![default_stats(10)];
        let changes = ctrl.evaluate_batch(&stats);
        let ts = changes[0].timestamp_ms;

        // Not applied yet.
        assert!(!changes[0].applied);

        ctrl.mark_applied(SegmentId(10), ts);

        let history = ctrl.audit_history();
        let entry = history.iter().find(|c| c.segment_id == SegmentId(10)).unwrap();
        assert!(entry.applied);
    }

    #[test]
    fn test_pending_changes_excludes_applied() {
        let ctrl = controller();
        let batch: Vec<_> = (20u32..=22).map(default_stats).collect();
        let changes = ctrl.evaluate_batch(&batch);

        // Mark the first segment as applied.
        ctrl.mark_applied(SegmentId(20), changes[0].timestamp_ms);

        let pending = ctrl.pending_changes();
        assert!(pending.iter().all(|c| c.segment_id != SegmentId(20)));
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn test_applied_changes_includes_only_applied() {
        let ctrl = controller();
        let batch: Vec<_> = (30u32..=32).map(default_stats).collect();
        let changes = ctrl.evaluate_batch(&batch);

        ctrl.mark_applied(SegmentId(31), changes[1].timestamp_ms);

        let applied = ctrl.applied_changes();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].segment_id, SegmentId(31));
    }

    #[test]
    fn test_metrics_track_decision_counts() {
        let ctrl = controller();

        // All three are "normal" → NoChange.
        let batch: Vec<_> = (40u32..=42).map(default_stats).collect();
        let changes = ctrl.evaluate_batch(&batch);

        // Mark one applied.
        ctrl.mark_applied(SegmentId(40), changes[0].timestamp_ms);

        let m = ctrl.metrics();
        assert_eq!(m.decisions_total, 3);
        assert_eq!(m.no_change_total, 3);
        assert_eq!(m.changes_applied_total, 1);
        assert_eq!(m.pending_changes_count, 2);
        assert_eq!(m.audit_history_size, 3);
    }

    #[test]
    fn test_audit_ring_bounded_by_max_history() {
        let ctrl = AdaptiveStorageController::new(AdaptiveControllerConfig {
            max_audit_history: 5,
            ..Default::default()
        });
        let batch: Vec<_> = (1u32..=10).map(default_stats).collect();
        ctrl.evaluate_batch(&batch);
        assert_eq!(ctrl.audit_history().len(), 5);
    }

    #[test]
    fn test_revert_segment_adds_noop_applied_entry() {
        let ctrl = controller();
        ctrl.revert_segment(SegmentId(99));
        let history = ctrl.audit_history();
        let entry = history.iter().find(|c| c.segment_id == SegmentId(99)).unwrap();
        assert_eq!(entry.decision, PolicyDecision::NoChange);
        assert!(entry.applied);
    }

    #[test]
    fn test_confidence_higher_for_very_low_hit_rate() {
        let ctrl = controller();
        let low_hit = SegmentWorkloadStats {
            scan_frequency: 20.0,
            cache_hit_rate: 0.1, // < 0.2 → confidence 0.9
            ..default_stats(50)
        };
        let high_hit = SegmentWorkloadStats {
            scan_frequency: 20.0,
            cache_hit_rate: 0.4, // >= 0.2 → confidence 0.8
            ..default_stats(51)
        };
        let c_low = ctrl.evaluate_batch(&[low_hit]);
        let c_high = ctrl.evaluate_batch(&[high_hit]);
        assert!(c_low[0].confidence > c_high[0].confidence);
    }
}
