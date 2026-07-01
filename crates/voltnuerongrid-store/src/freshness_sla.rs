//! H9-9: Freshness SLA Contract and Enforcement
//!
//! This module implements freshness SLA (Service Level Agreement) tracking and enforcement
//! for table segments. It ensures that queries can validate freshness requirements and
//! makes merge scheduling decisions to maintain SLA compliance.
//!
//! Key concepts:
//! - **FreshnessSlaConfig**: Per-table SLA threshold (e.g., 5000ms)
//! - **FreshnessSlaRequest**: Per-query freshness requirement (e.g., max 2000ms stale)
//! - **ComplianceStatus**: Compliant, Warning, or Violated based on staleness
//! - **FreshnessSlaEnforcer**: Central coordinator tracking segment freshness metrics

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

/// Configuration for freshness SLA per table.
#[derive(Debug, Clone)]
pub struct FreshnessSlaConfig {
    /// Table-level freshness SLA in milliseconds (optional; uses default if None).
    pub table_sla_ms: Option<u64>,
    /// Default freshness SLA in milliseconds (fallback for tables without explicit SLA).
    pub default_sla_ms: u64,
}

impl FreshnessSlaConfig {
    /// Create a new FreshnessSlaConfig.
    pub fn new(table_sla_ms: Option<u64>, default_sla_ms: u64) -> Self {
        Self {
            table_sla_ms,
            default_sla_ms,
        }
    }

    /// Get the effective SLA in milliseconds.
    pub fn effective_sla_ms(&self) -> u64 {
        self.table_sla_ms.unwrap_or(self.default_sla_ms)
    }
}

impl Default for FreshnessSlaConfig {
    fn default() -> Self {
        Self {
            table_sla_ms: None,
            default_sla_ms: 5000, // 5 seconds default
        }
    }
}

/// Per-query freshness requirement for SLA enforcement.
#[derive(Debug, Clone)]
pub struct FreshnessSlaRequest {
    /// Maximum acceptable staleness in milliseconds (None = no strict requirement).
    pub max_staleness_ms: Option<u64>,
    /// If true, strictly enforce SLA; if false, best-effort.
    pub enforce_sla: bool,
    /// If true, reject stale queries; if false, allow hybrid scan.
    pub reject_if_stale: bool,
}

impl FreshnessSlaRequest {
    /// Create a new FreshnessSlaRequest.
    pub fn new(max_staleness_ms: Option<u64>, enforce_sla: bool, reject_if_stale: bool) -> Self {
        Self {
            max_staleness_ms,
            enforce_sla,
            reject_if_stale,
        }
    }

    /// Create a permissive request (no SLA enforcement).
    pub fn permissive() -> Self {
        Self {
            max_staleness_ms: None,
            enforce_sla: false,
            reject_if_stale: false,
        }
    }

    /// Create a strict request (enforce SLA, reject if stale).
    pub fn strict(max_staleness_ms: u64) -> Self {
        Self {
            max_staleness_ms: Some(max_staleness_ms),
            enforce_sla: true,
            reject_if_stale: true,
        }
    }
}

/// Compliance status of a segment relative to its SLA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComplianceStatus {
    /// Segment is within SLA.
    Compliant,
    /// Segment is approaching SLA breach (staleness_ms).
    Warning(u64),
    /// Segment has violated SLA (staleness_ms).
    Violated(u64),
}

/// Freshness metrics for a segment.
#[derive(Debug, Clone)]
pub struct FreshnessMetrics {
    /// Timestamp of the last merge (milliseconds).
    pub last_merge_ts_ms: u64,
    /// Age of the base segment (milliseconds).
    pub base_freshness_ms: u64,
    /// Time since the last write to tail (milliseconds).
    pub tail_freshness_ms: u64,
    /// Current compliance status.
    pub compliance_status: ComplianceStatus,
}

impl FreshnessMetrics {
    /// Create new freshness metrics.
    pub fn new(
        last_merge_ts_ms: u64,
        base_freshness_ms: u64,
        tail_freshness_ms: u64,
        compliance_status: ComplianceStatus,
    ) -> Self {
        Self {
            last_merge_ts_ms,
            base_freshness_ms,
            tail_freshness_ms,
            compliance_status,
        }
    }

    /// Get the current staleness (maximum of base and tail freshness).
    pub fn current_staleness_ms(&self) -> u64 {
        self.base_freshness_ms.max(self.tail_freshness_ms)
    }
}

/// SLA enforcement metrics and statistics.
#[derive(Debug, Clone)]
pub struct FreshnessSlaMetrics {
    /// Total number of SLO violations (cumulative).
    pub slo_violations_total: u64,
    /// Number of segments in compliant state.
    pub segments_compliant: usize,
    /// Number of segments in warning state.
    pub segments_warning: usize,
    /// Number of segments in violated state.
    pub segments_violated: usize,
    /// Average staleness across all segments (milliseconds).
    pub avg_staleness_ms: f64,
}

/// Central coordinator for freshness SLA tracking and enforcement.
pub struct FreshnessSlaEnforcer {
    /// Per-segment freshness metrics.
    segment_freshness: Arc<Mutex<HashMap<u64, FreshnessMetrics>>>,
    /// Per-table SLA configuration.
    table_sla_config: Arc<Mutex<HashMap<String, FreshnessSlaConfig>>>,
    /// Total number of SLO violations (atomic counter).
    slo_violation_total: Arc<AtomicU64>,
}

impl FreshnessSlaEnforcer {
    /// Create a new FreshnessSlaEnforcer.
    pub fn new() -> Self {
        Self {
            segment_freshness: Arc::new(Mutex::new(HashMap::new())),
            table_sla_config: Arc::new(Mutex::new(HashMap::new())),
            slo_violation_total: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Register table-level SLA configuration.
    pub fn register_table_sla(
        &self,
        table_name: impl Into<String>,
        config: FreshnessSlaConfig,
    ) -> Result<(), String> {
        let table_name = table_name.into();
        let mut configs = self.table_sla_config.lock().map_err(|e| e.to_string())?;
        configs.insert(table_name, config);
        Ok(())
    }

    /// Evaluate freshness compliance for a segment given current time and SLA threshold.
    pub fn evaluate_freshness(
        &self,
        segment_id: u64,
        sla_threshold_ms: u64,
        now_ms: u64,
    ) -> Result<ComplianceStatus, String> {
        let freshness = self
            .segment_freshness
            .lock()
            .map_err(|e| e.to_string())?
            .get(&segment_id)
            .cloned()
            .ok_or_else(|| format!("Segment {} not found", segment_id))?;

        let staleness_ms = now_ms.saturating_sub(freshness.last_merge_ts_ms);
        let status = if staleness_ms <= sla_threshold_ms {
            ComplianceStatus::Compliant
        } else if staleness_ms <= sla_threshold_ms + 1000 {
            // Warning: 1 second over SLA
            ComplianceStatus::Warning(staleness_ms)
        } else {
            // Violated: significantly over SLA
            ComplianceStatus::Violated(staleness_ms)
        };

        Ok(status)
    }

    /// Check if a query can execute on a table given SLA requirements.
    ///
    /// Returns:
    /// - Ok(true) if query can proceed
    /// - Ok(false) if query should be rejected (stale and reject_if_stale=true)
    /// - Err(...) if validation fails
    pub fn check_query_can_execute(
        &self,
        table_name: &str,
        segment_id: u64,
        sla_request: &FreshnessSlaRequest,
        now_ms: u64,
    ) -> Result<bool, String> {
        // No SLA check if not enforced
        if !sla_request.enforce_sla {
            return Ok(true);
        }

        // Get table SLA config
        let configs = self.table_sla_config.lock().map_err(|e| e.to_string())?;
        let config = configs
            .get(table_name)
            .cloned()
            .unwrap_or_default();
        let sla_threshold_ms = config.effective_sla_ms();

        // Get segment freshness
        let freshness = self
            .segment_freshness
            .lock()
            .map_err(|e| e.to_string())?
            .get(&segment_id)
            .cloned()
            .ok_or_else(|| format!("Segment {} not found", segment_id))?;

        let staleness_ms = now_ms.saturating_sub(freshness.last_merge_ts_ms);

        // Check against max_staleness_ms from request if specified
        if let Some(max_staleness) = sla_request.max_staleness_ms {
            if staleness_ms > max_staleness {
                if sla_request.reject_if_stale {
                    return Ok(false); // Reject stale query
                }
            }
        }

        // Check against table SLA
        if staleness_ms > sla_threshold_ms && sla_request.reject_if_stale {
            return Ok(false); // Reject if violating table SLA
        }

        Ok(true) // Query can proceed
    }

    /// Record merge completion, updating segment freshness.
    pub fn on_merge_completed(
        &self,
        segment_id: u64,
        new_freshness_ms: u64,
        now_ms: u64,
    ) -> Result<(), String> {
        let mut freshness_map = self.segment_freshness.lock().map_err(|e| e.to_string())?;

        let metrics = freshness_map
            .entry(segment_id)
            .or_insert_with(|| FreshnessMetrics::new(now_ms, 0, 0, ComplianceStatus::Compliant));

        metrics.last_merge_ts_ms = now_ms;
        metrics.base_freshness_ms = new_freshness_ms;

        Ok(())
    }

    /// Record query execution, updating freshness tracking stats.
    pub fn on_query_executed(
        &self,
        segment_id: u64,
        now_ms: u64,
    ) -> Result<(), String> {
        let mut freshness_map = self.segment_freshness.lock().map_err(|e| e.to_string())?;

        let metrics = freshness_map
            .entry(segment_id)
            .or_insert_with(|| FreshnessMetrics::new(now_ms, 0, 0, ComplianceStatus::Compliant));

        // Update tail freshness to now (last query time)
        metrics.tail_freshness_ms = 0; // Reset on query (most recent activity)

        Ok(())
    }

    /// Get overall SLA metrics and compliance statistics.
    pub fn get_metrics(&self) -> Result<FreshnessSlaMetrics, String> {
        let freshness_map = self.segment_freshness.lock().map_err(|e| e.to_string())?;

        let mut compliant = 0;
        let mut warning = 0;
        let mut violated = 0;
        let mut total_staleness = 0u64;

        for metrics in freshness_map.values() {
            match &metrics.compliance_status {
                ComplianceStatus::Compliant => compliant += 1,
                ComplianceStatus::Warning(_) => warning += 1,
                ComplianceStatus::Violated(_) => violated += 1,
            }
            total_staleness += metrics.current_staleness_ms();
        }

        let count = freshness_map.len();
        let avg_staleness_ms = if count > 0 {
            total_staleness as f64 / count as f64
        } else {
            0.0
        };

        Ok(FreshnessSlaMetrics {
            slo_violations_total: self.slo_violation_total.load(Ordering::SeqCst),
            segments_compliant: compliant,
            segments_warning: warning,
            segments_violated: violated,
            avg_staleness_ms,
        })
    }

    /// Record an SLO violation.
    pub fn record_slo_violation(&self) {
        self.slo_violation_total.fetch_add(1, Ordering::SeqCst);
    }

    /// Prioritize merge candidates nearing SLA breach.
    ///
    /// Returns a sorted vector of (segment_id, priority_score) where higher score
    /// means higher priority for merge scheduling.
    pub fn prioritize_merge_candidates(
        &self,
        sla_threshold_ms: u64,
        now_ms: u64,
    ) -> Result<Vec<(u64, f64)>, String> {
        let freshness_map = self.segment_freshness.lock().map_err(|e| e.to_string())?;

        let mut candidates = Vec::new();

        for (&segment_id, metrics) in freshness_map.iter() {
            let staleness_ms = now_ms.saturating_sub(metrics.last_merge_ts_ms);

            // Skip segments already compliant
            if staleness_ms <= sla_threshold_ms {
                continue;
            }

            // Priority score: how far beyond SLA (normalized to 0-1, then scaled)
            let overage = staleness_ms.saturating_sub(sla_threshold_ms);
            let priority = (overage as f64) / (sla_threshold_ms as f64);

            candidates.push((segment_id, priority));
        }

        // Sort by priority descending (highest overage first)
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(candidates)
    }

    /// Initialize segment freshness tracking.
    pub fn init_segment(
        &self,
        segment_id: u64,
        now_ms: u64,
    ) -> Result<(), String> {
        let mut freshness_map = self.segment_freshness.lock().map_err(|e| e.to_string())?;
        freshness_map.insert(
            segment_id,
            FreshnessMetrics::new(now_ms, 0, 0, ComplianceStatus::Compliant),
        );
        Ok(())
    }

    /// Update segment compliance status.
    pub fn update_compliance_status(
        &self,
        segment_id: u64,
        status: ComplianceStatus,
    ) -> Result<(), String> {
        let mut freshness_map = self.segment_freshness.lock().map_err(|e| e.to_string())?;
        if let Some(metrics) = freshness_map.get_mut(&segment_id) {
            metrics.compliance_status = status;
            Ok(())
        } else {
            Err(format!("Segment {} not found", segment_id))
        }
    }
}

impl Default for FreshnessSlaEnforcer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freshness_sla_config_creation() {
        let config = FreshnessSlaConfig::new(Some(3000), 5000);
        assert_eq!(config.table_sla_ms, Some(3000));
        assert_eq!(config.default_sla_ms, 5000);
        assert_eq!(config.effective_sla_ms(), 3000);
    }

    #[test]
    fn test_freshness_sla_config_default_fallback() {
        let config = FreshnessSlaConfig::new(None, 5000);
        assert_eq!(config.table_sla_ms, None);
        assert_eq!(config.effective_sla_ms(), 5000);
    }

    #[test]
    fn test_freshness_sla_config_default() {
        let config = FreshnessSlaConfig::default();
        assert_eq!(config.default_sla_ms, 5000);
        assert_eq!(config.effective_sla_ms(), 5000);
    }

    #[test]
    fn test_freshness_sla_register_table() {
        let enforcer = FreshnessSlaEnforcer::new();
        let config = FreshnessSlaConfig::new(Some(2000), 5000);
        
        let result = enforcer.register_table_sla("users", config.clone());
        assert!(result.is_ok());

        // Register another table
        let result = enforcer.register_table_sla("orders", FreshnessSlaConfig::new(Some(3000), 5000));
        assert!(result.is_ok());
    }

    #[test]
    fn test_freshness_sla_evaluate_compliant() {
        let enforcer = FreshnessSlaEnforcer::new();
        let now_ms = 10000;
        let segment_id = 1;

        enforcer.init_segment(segment_id, now_ms).unwrap();

        // Segment just merged: staleness = 0, should be compliant
        let status = enforcer.evaluate_freshness(segment_id, 5000, now_ms).unwrap();
        assert_eq!(status, ComplianceStatus::Compliant);
    }

    #[test]
    fn test_freshness_sla_evaluate_warning() {
        let enforcer = FreshnessSlaEnforcer::new();
        let now_ms = 10000;
        let segment_id = 1;

        // Segment merged 6000ms ago (1000ms over SLA of 5000ms)
        let merge_ts = now_ms - 6000;
        let mut freshness_map = enforcer.segment_freshness.lock().unwrap();
        freshness_map.insert(
            segment_id,
            FreshnessMetrics::new(merge_ts, 6000, 0, ComplianceStatus::Compliant),
        );
        drop(freshness_map);

        let status = enforcer.evaluate_freshness(segment_id, 5000, now_ms).unwrap();
        assert_eq!(status, ComplianceStatus::Warning(6000));
    }

    #[test]
    fn test_freshness_sla_evaluate_violated() {
        let enforcer = FreshnessSlaEnforcer::new();
        let now_ms = 10000;
        let segment_id = 1;

        // Segment merged 7000ms ago (2000ms over SLA of 5000ms)
        let merge_ts = now_ms - 7000;
        let mut freshness_map = enforcer.segment_freshness.lock().unwrap();
        freshness_map.insert(
            segment_id,
            FreshnessMetrics::new(merge_ts, 7000, 0, ComplianceStatus::Compliant),
        );
        drop(freshness_map);

        let status = enforcer.evaluate_freshness(segment_id, 5000, now_ms).unwrap();
        assert_eq!(status, ComplianceStatus::Violated(7000));
    }

    #[test]
    fn test_freshness_sla_check_query_compliant() {
        let enforcer = FreshnessSlaEnforcer::new();
        let now_ms = 10000;
        let segment_id = 1;
        let table_name = "users";

        enforcer.register_table_sla(table_name, FreshnessSlaConfig::new(Some(5000), 5000)).unwrap();
        enforcer.init_segment(segment_id, now_ms).unwrap();

        let request = FreshnessSlaRequest::strict(3000);
        let can_execute = enforcer.check_query_can_execute(table_name, segment_id, &request, now_ms).unwrap();
        assert!(can_execute); // Segment just merged, staleness = 0
    }

    #[test]
    fn test_freshness_sla_check_query_rejected_if_stale() {
        let enforcer = FreshnessSlaEnforcer::new();
        let now_ms = 10000;
        let segment_id = 1;
        let table_name = "users";

        enforcer.register_table_sla(table_name, FreshnessSlaConfig::new(Some(2000), 5000)).unwrap();

        // Segment merged 4000ms ago
        let merge_ts = now_ms - 4000;
        let mut freshness_map = enforcer.segment_freshness.lock().unwrap();
        freshness_map.insert(
            segment_id,
            FreshnessMetrics::new(merge_ts, 4000, 0, ComplianceStatus::Violated(4000)),
        );
        drop(freshness_map);

        let request = FreshnessSlaRequest::strict(2000); // Max 2000ms staleness
        let can_execute = enforcer.check_query_can_execute(table_name, segment_id, &request, now_ms).unwrap();
        assert!(!can_execute); // Should reject (4000ms staleness exceeds 2000ms limit)
    }

    #[test]
    fn test_freshness_sla_check_query_permissive() {
        let enforcer = FreshnessSlaEnforcer::new();
        let now_ms = 10000;
        let segment_id = 1;
        let table_name = "users";

        enforcer.register_table_sla(table_name, FreshnessSlaConfig::default()).unwrap();

        // Segment merged 10000ms ago
        let merge_ts = now_ms - 10000;
        let mut freshness_map = enforcer.segment_freshness.lock().unwrap();
        freshness_map.insert(
            segment_id,
            FreshnessMetrics::new(merge_ts, 10000, 0, ComplianceStatus::Violated(10000)),
        );
        drop(freshness_map);

        let request = FreshnessSlaRequest::permissive();
        let can_execute = enforcer.check_query_can_execute(table_name, segment_id, &request, now_ms).unwrap();
        assert!(can_execute); // Permissive mode always allows
    }

    #[test]
    fn test_freshness_sla_prioritize_merge_candidates() {
        let enforcer = FreshnessSlaEnforcer::new();
        let now_ms = 20000;
        let sla_threshold_ms = 5000;

        // Segment 1: 3000ms old (compliant)
        let mut freshness_map = enforcer.segment_freshness.lock().unwrap();
        freshness_map.insert(
            1,
            FreshnessMetrics::new(now_ms - 3000, 3000, 0, ComplianceStatus::Compliant),
        );

        // Segment 2: 7000ms old (1000ms over SLA)
        freshness_map.insert(
            2,
            FreshnessMetrics::new(now_ms - 7000, 7000, 0, ComplianceStatus::Warning(7000)),
        );

        // Segment 3: 10000ms old (5000ms over SLA)
        freshness_map.insert(
            3,
            FreshnessMetrics::new(now_ms - 10000, 10000, 0, ComplianceStatus::Violated(10000)),
        );
        drop(freshness_map);

        let candidates = enforcer.prioritize_merge_candidates(sla_threshold_ms, now_ms).unwrap();

        // Should return segments 2 and 3, sorted by priority (3 highest)
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].0, 3); // Segment 3 highest priority
        assert_eq!(candidates[1].0, 2); // Segment 2 second priority
        assert!(candidates[0].1 > candidates[1].1); // Priority scores are in order
    }

    #[test]
    fn test_freshness_sla_metrics_tracking() {
        let enforcer = FreshnessSlaEnforcer::new();
        let now_ms = 10000;

        // Record violations
        enforcer.record_slo_violation();
        enforcer.record_slo_violation();

        // Initialize segments with different states
        let mut freshness_map = enforcer.segment_freshness.lock().unwrap();
        freshness_map.insert(1, FreshnessMetrics::new(now_ms, 0, 0, ComplianceStatus::Compliant));
        freshness_map.insert(2, FreshnessMetrics::new(now_ms - 500, 500, 0, ComplianceStatus::Warning(500)));
        freshness_map.insert(3, FreshnessMetrics::new(now_ms - 2000, 2000, 0, ComplianceStatus::Violated(2000)));
        drop(freshness_map);

        let metrics = enforcer.get_metrics().unwrap();
        assert_eq!(metrics.slo_violations_total, 2);
        assert_eq!(metrics.segments_compliant, 1);
        assert_eq!(metrics.segments_warning, 1);
        assert_eq!(metrics.segments_violated, 1);
        assert!(metrics.avg_staleness_ms > 0.0);
    }

    #[test]
    fn test_freshness_sla_concurrent_updates() {
        use std::thread;
        use std::sync::Arc as StdArc;

        let enforcer = StdArc::new(FreshnessSlaEnforcer::new());
        let mut handles = vec![];

        // Spawn multiple threads updating different segments
        for i in 0..10 {
            let enforcer_clone = StdArc::clone(&enforcer);
            let handle = thread::spawn(move || {
                let segment_id = i as u64;
                let now_ms = 10000 + i as u64 * 100;
                
                // Initialize segment
                enforcer_clone.init_segment(segment_id, now_ms).unwrap();
                
                // Record a merge
                enforcer_clone.on_merge_completed(segment_id, 1000, now_ms + 1000).unwrap();
                
                // Record a query
                enforcer_clone.on_query_executed(segment_id, now_ms + 2000).unwrap();
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all segments were initialized
        let metrics = enforcer.get_metrics().unwrap();
        assert_eq!(metrics.segments_compliant, 10);
    }

    #[test]
    fn test_freshness_metrics_staleness_calculation() {
        let metrics = FreshnessMetrics::new(1000, 2000, 500, ComplianceStatus::Compliant);
        assert_eq!(metrics.current_staleness_ms(), 2000); // max(2000, 500)
    }

    #[test]
    fn test_freshness_sla_request_strict() {
        let request = FreshnessSlaRequest::strict(1000);
        assert_eq!(request.max_staleness_ms, Some(1000));
        assert!(request.enforce_sla);
        assert!(request.reject_if_stale);
    }

    #[test]
    fn test_freshness_sla_request_permissive() {
        let request = FreshnessSlaRequest::permissive();
        assert_eq!(request.max_staleness_ms, None);
        assert!(!request.enforce_sla);
        assert!(!request.reject_if_stale);
    }

    #[test]
    fn test_freshness_sla_on_merge_completed() {
        let enforcer = FreshnessSlaEnforcer::new();
        let segment_id = 1;
        let now_ms = 10000;

        enforcer.init_segment(segment_id, now_ms).unwrap();
        enforcer.on_merge_completed(segment_id, 500, now_ms + 1000).unwrap();

        let freshness_map = enforcer.segment_freshness.lock().unwrap();
        let metrics = freshness_map.get(&segment_id).unwrap();
        assert_eq!(metrics.last_merge_ts_ms, now_ms + 1000);
        assert_eq!(metrics.base_freshness_ms, 500);
    }

    #[test]
    fn test_freshness_sla_on_query_executed() {
        let enforcer = FreshnessSlaEnforcer::new();
        let segment_id = 1;
        let now_ms = 10000;

        enforcer.init_segment(segment_id, now_ms).unwrap();
        enforcer.on_query_executed(segment_id, now_ms + 1000).unwrap();

        let freshness_map = enforcer.segment_freshness.lock().unwrap();
        let metrics = freshness_map.get(&segment_id).unwrap();
        assert_eq!(metrics.tail_freshness_ms, 0); // Reset to 0 on query
    }

    #[test]
    fn test_freshness_sla_update_compliance_status() {
        let enforcer = FreshnessSlaEnforcer::new();
        let segment_id = 1;
        let now_ms = 10000;

        enforcer.init_segment(segment_id, now_ms).unwrap();
        enforcer.update_compliance_status(segment_id, ComplianceStatus::Warning(1000)).unwrap();

        let freshness_map = enforcer.segment_freshness.lock().unwrap();
        let metrics = freshness_map.get(&segment_id).unwrap();
        assert_eq!(metrics.compliance_status, ComplianceStatus::Warning(1000));
    }
}
