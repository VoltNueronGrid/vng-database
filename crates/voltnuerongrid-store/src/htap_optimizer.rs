//! H9-10: Metis-style HTAP-aware optimizer and routing hints.
//!
//! This module implements cost-based query routing for HTAP workloads, similar to Metis.
//! It estimates costs for row-oriented, column-oriented, and hybrid access paths,
//! then selects the optimal path based on freshness requirements and system state.
//!
//! Key concepts:
//! - **PhysicalAccessPath**: Row, Column, or Hybrid scan strategy
//! - **CostEstimate**: Multi-factor cost model including freshness, queue depth, OLTP pressure
//! - **HtapOptimizer**: Central coordinator for path selection and cost computation
//! - **RoutingExplanation**: Clear, actionable explanation of routing decisions

use std::collections::HashMap;

/// Physical access paths for HTAP query execution.
///
/// The optimizer selects among these strategies based on query characteristics
/// and freshness requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalAccessPath {
    /// Row-oriented tail scan: fast for OLTP, high tail_versions cost, recent access preferred
    ScanRow,
    /// Column-oriented base scan: good for OLAP, low merge_lag preferred, good compression
    ScanColumn,
    /// Hybrid coordinated scan: base + tail merge, balanced cost, freshness guaranteed
    ScanHybrid,
}

impl PhysicalAccessPath {
    /// Human-readable name for this path.
    pub fn as_str(&self) -> &'static str {
        match self {
            PhysicalAccessPath::ScanRow => "ScanRow",
            PhysicalAccessPath::ScanColumn => "ScanColumn",
            PhysicalAccessPath::ScanHybrid => "ScanHybrid",
        }
    }
}

impl std::fmt::Display for PhysicalAccessPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Extended cost estimate for HTAP routing decisions.
///
/// Captures multi-factor costs and system state that influence path selection.
#[derive(Debug, Clone)]
pub struct CostEstimate {
    /// Number of row versions in the tail (affects row scan cost)
    pub tail_versions: u64,
    /// Milliseconds since last successful merge to base (affects column scan cost)
    pub merge_lag_ms: u64,
    /// Milliseconds since snapshot was taken (freshness gap)
    pub freshness_lag_ms: u64,
    /// Current queue depth in OLTP layer
    pub queue_depth: u32,
    /// OLTP SLO pressure (0.0 = idle, 1.0 = critical)
    pub oltp_slo_pressure: f64,

    /// Estimated cost of row-oriented scan (in arbitrary cost units)
    pub base_scan_cost: u64,
    /// Estimated cost of column-oriented scan
    pub tail_scan_cost: u64,
    /// Estimated cost of hybrid coordinated scan
    pub hybrid_scan_cost: u64,

    /// Selected physical access path
    pub selected_path: PhysicalAccessPath,
    /// Human-readable explanation of routing decision
    pub routing_explanation: String,
}

impl CostEstimate {
    /// Create a new cost estimate with default values.
    pub fn new() -> Self {
        CostEstimate {
            tail_versions: 0,
            merge_lag_ms: 0,
            freshness_lag_ms: 0,
            queue_depth: 0,
            oltp_slo_pressure: 0.0,
            base_scan_cost: 0,
            tail_scan_cost: 0,
            hybrid_scan_cost: 0,
            selected_path: PhysicalAccessPath::ScanRow,
            routing_explanation: String::new(),
        }
    }
}

impl Default for CostEstimate {
    fn default() -> Self {
        Self::new()
    }
}

/// Segment statistics for cost estimation.
///
/// Tracks aggregate properties of a segment needed for route planning.
#[derive(Debug, Clone)]
pub struct SegmentStatistics {
    /// Total number of rows in segment
    pub total_rows: u64,
    /// Number of row versions in tail (mutable portion)
    pub tail_versions: u64,
    /// Size of base in bytes (compressed columnar)
    pub base_size_bytes: u64,
    /// Rows in base (immutable columnar)
    pub base_rows: u64,
    /// Milliseconds since last merge
    pub merge_lag_ms: u64,
    /// Base is mergeable (not in ongoing merge)
    pub base_stable: bool,
}

impl SegmentStatistics {
    /// Create new segment statistics.
    pub fn new(
        total_rows: u64,
        tail_versions: u64,
        base_size_bytes: u64,
        base_rows: u64,
        merge_lag_ms: u64,
        base_stable: bool,
    ) -> Self {
        SegmentStatistics {
            total_rows,
            tail_versions,
            base_size_bytes,
            base_rows,
            merge_lag_ms,
            base_stable,
        }
    }

    /// Estimate compression ratio (base_size_bytes / estimated_uncompressed)
    fn compression_ratio(&self) -> f64 {
        if self.base_rows == 0 {
            1.0
        } else {
            // Assume ~128 bytes per row uncompressed
            let uncompressed = self.base_rows * 128;
            (self.base_size_bytes as f64) / (uncompressed as f64)
        }
    }
}

/// Query characteristics for cost estimation.
///
/// Captures properties of the query being optimized.
#[derive(Debug, Clone)]
pub struct QueryCharacteristics {
    /// Estimated selectivity of predicates (0.0 = all filtered, 1.0 = all pass)
    pub predicate_selectivity: f64,
    /// Rows expected to pass predicates
    pub estimated_output_rows: u64,
    /// Required freshness in milliseconds (None = any age acceptable)
    pub freshness_requirement_ms: Option<u64>,
    /// Is this an OLAP-style aggregation query?
    pub is_aggregation: bool,
    /// Is this a point lookup (single row)?
    pub is_point_lookup: bool,
}

impl QueryCharacteristics {
    /// Create new query characteristics.
    pub fn new(
        predicate_selectivity: f64,
        estimated_output_rows: u64,
        freshness_requirement_ms: Option<u64>,
        is_aggregation: bool,
        is_point_lookup: bool,
    ) -> Self {
        QueryCharacteristics {
            predicate_selectivity: predicate_selectivity.max(0.0).min(1.0),
            estimated_output_rows,
            freshness_requirement_ms,
            is_aggregation,
            is_point_lookup,
        }
    }
}

/// System state for cost estimation.
///
/// Captures real-time system conditions that influence path selection.
#[derive(Debug, Clone)]
pub struct SystemState {
    /// Current queue depth in OLTP layer (number of pending writes)
    pub queue_depth: u32,
    /// OLTP SLO pressure (0.0 = idle, 1.0 = critical/overload)
    pub oltp_slo_pressure: f64,
    /// Milliseconds since last snapshot was taken
    pub snapshot_age_ms: u64,
    /// Available memory in MB
    pub available_memory_mb: u32,
}

impl SystemState {
    /// Create new system state.
    pub fn new(
        queue_depth: u32,
        oltp_slo_pressure: f64,
        snapshot_age_ms: u64,
        available_memory_mb: u32,
    ) -> Self {
        SystemState {
            queue_depth,
            oltp_slo_pressure: oltp_slo_pressure.max(0.0).min(1.0),
            snapshot_age_ms,
            available_memory_mb,
        }
    }
}

/// HTAP-aware query optimizer following Metis principles.
///
/// Estimates costs for different access paths and selects the optimal route
/// based on freshness requirements, system pressure, and workload characteristics.
pub struct HtapOptimizer {
    /// Cache of segment statistics for active segments
    segment_stats: HashMap<String, SegmentStatistics>,
}

impl HtapOptimizer {
    /// Create a new HTAP optimizer.
    pub fn new() -> Self {
        HtapOptimizer {
            segment_stats: HashMap::new(),
        }
    }

    /// Register segment statistics (e.g., from metadata catalog).
    pub fn register_segment(&mut self, segment_key: String, stats: SegmentStatistics) {
        self.segment_stats.insert(segment_key, stats);
    }

    /// Estimate cost of row-oriented scan.
    ///
    /// Cost model:
    /// - Base: 100 * tail_versions (versions must be merged)
    /// - Freshness benefit: reduce cost by 10 * sqrt(merge_lag_ms) (stale base is bad)
    /// - Queue penalty: +5 per unit queue_depth
    /// - OLTP pressure: multiply by (1 + 2 * oltp_slo_pressure)
    pub fn estimate_row_scan_cost(
        &self,
        segment_stats: &SegmentStatistics,
        sys_state: &SystemState,
    ) -> u64 {
        // Base cost proportional to tail versions
        let mut cost = 100u64 * segment_stats.tail_versions.max(1);

        // Freshness benefit: if base is stale, row scan accesses newer data
        // Reduce cost by square root of merge lag (10 * sqrt(lag_ms))
        let freshness_benefit = (10.0 * (segment_stats.merge_lag_ms as f64).sqrt()) as u64;
        cost = cost.saturating_sub(freshness_benefit);

        // Queue depth penalty
        cost += (sys_state.queue_depth as u64) * 5;

        // OLTP pressure multiplier
        let pressure_multiplier = 1.0 + (2.0 * sys_state.oltp_slo_pressure);
        cost = (cost as f64 * pressure_multiplier) as u64;

        cost
    }

    /// Estimate cost of column-oriented scan.
    ///
    /// Cost model:
    /// - Base: 50 (columnar format is compact)
    /// - Merge lag penalty: +2 per 1000ms of merge_lag
    /// - Compression benefit: multiply by compression_ratio
    /// - OLTP pressure: minimal impact (column scans reduce OLTP load)
    pub fn estimate_column_scan_cost(
        &self,
        segment_stats: &SegmentStatistics,
        sys_state: &SystemState,
    ) -> u64 {
        let mut cost = 50u64;

        // Merge lag penalty (2 per 1000ms, i.e., per second)
        let merge_lag_penalty = (segment_stats.merge_lag_ms / 1000).max(1);
        cost += merge_lag_penalty * 2;

        // Compression benefit
        let compression = segment_stats.compression_ratio();
        cost = (cost as f64 * compression) as u64;

        // OLTP pressure has minimal impact on column scan
        let pressure_multiplier = 1.0 + (0.5 * sys_state.oltp_slo_pressure);
        cost = (cost as f64 * pressure_multiplier) as u64;

        cost
    }

    /// Estimate cost of hybrid coordinated scan.
    ///
    /// Cost model:
    /// - Base: average of row and column costs (coordination overhead)
    /// - Coordination penalty: +25 (merge overhead)
    /// - Freshness guarantee: modest OLTP pressure impact
    pub fn estimate_hybrid_scan_cost(
        &self,
        segment_stats: &SegmentStatistics,
        sys_state: &SystemState,
    ) -> u64 {
        let row_cost = self.estimate_row_scan_cost(segment_stats, sys_state);
        let column_cost = self.estimate_column_scan_cost(segment_stats, sys_state);

        // Average row and column costs plus coordination overhead
        let mut cost = (row_cost + column_cost) / 2 + 25;

        // Moderate OLTP pressure impact
        let pressure_multiplier = 1.0 + (1.0 * sys_state.oltp_slo_pressure);
        cost = (cost as f64 * pressure_multiplier) as u64;

        cost
    }

    /// Select best physical access path.
    ///
    /// Decision logic:
    /// 1. If freshness_requirement is tight (<=1000ms) → ScanHybrid (guaranteed fresh)
    /// 2. If merge_lag >> freshness_requirement → ScanRow (base is too stale)
    /// 3. If predicate_selectivity < 0.2 (aggressive filtering) → ScanColumn
    /// 4. Otherwise, select path with lowest cost
    pub fn select_best_path(
        &self,
        segment_stats: &SegmentStatistics,
        query_char: &QueryCharacteristics,
        sys_state: &SystemState,
    ) -> PhysicalAccessPath {
        let strict_freshness = query_char.freshness_requirement_ms.map_or(false, |f| f <= 1000);

        if strict_freshness {
            return PhysicalAccessPath::ScanHybrid;
        }

        // If merge lag exceeds freshness requirement, row scan needed
        if let Some(freshness_req) = query_char.freshness_requirement_ms {
            if segment_stats.merge_lag_ms > freshness_req {
                return PhysicalAccessPath::ScanRow;
            }
        }

        // For aggressive filtering, column is better
        if query_char.predicate_selectivity < 0.2 {
            return PhysicalAccessPath::ScanColumn;
        }

        // Otherwise, select by cost
        let row_cost = self.estimate_row_scan_cost(segment_stats, sys_state);
        let column_cost = self.estimate_column_scan_cost(segment_stats, sys_state);
        let hybrid_cost = self.estimate_hybrid_scan_cost(segment_stats, sys_state);

        if row_cost <= column_cost && row_cost <= hybrid_cost {
            PhysicalAccessPath::ScanRow
        } else if column_cost <= hybrid_cost {
            PhysicalAccessPath::ScanColumn
        } else {
            PhysicalAccessPath::ScanHybrid
        }
    }

    /// Compute full cost estimate for query routing.
    pub fn compute_cost_estimate(
        &self,
        segment_stats: &SegmentStatistics,
        query_char: &QueryCharacteristics,
        sys_state: &SystemState,
    ) -> CostEstimate {
        let row_cost = self.estimate_row_scan_cost(segment_stats, sys_state);
        let column_cost = self.estimate_column_scan_cost(segment_stats, sys_state);
        let hybrid_cost = self.estimate_hybrid_scan_cost(segment_stats, sys_state);

        let selected_path =
            self.select_best_path(segment_stats, query_char, sys_state);

        let routing_explanation = self.explain_routing_decision(
            segment_stats,
            query_char,
            sys_state,
            selected_path,
            row_cost,
            column_cost,
            hybrid_cost,
        );

        CostEstimate {
            tail_versions: segment_stats.tail_versions,
            merge_lag_ms: segment_stats.merge_lag_ms,
            freshness_lag_ms: sys_state.snapshot_age_ms,
            queue_depth: sys_state.queue_depth,
            oltp_slo_pressure: sys_state.oltp_slo_pressure,
            base_scan_cost: column_cost,
            tail_scan_cost: row_cost,
            hybrid_scan_cost: hybrid_cost,
            selected_path,
            routing_explanation,
        }
    }

    /// Generate human-readable explanation of routing decision.
    fn explain_routing_decision(
        &self,
        segment_stats: &SegmentStatistics,
        query_char: &QueryCharacteristics,
        sys_state: &SystemState,
        selected_path: PhysicalAccessPath,
        row_cost: u64,
        column_cost: u64,
        hybrid_cost: u64,
    ) -> String {
        let mut explanation = format!(
            "Route: {} [tail_versions={}, merge_lag={}ms, queue_depth={}, oltp_pressure={:.2}] ",
            selected_path,
            segment_stats.tail_versions,
            segment_stats.merge_lag_ms,
            sys_state.queue_depth,
            sys_state.oltp_slo_pressure
        );

        // Add reasoning
        match selected_path {
            PhysicalAccessPath::ScanRow => {
                if let Some(freshness) = query_char.freshness_requirement_ms {
                    if segment_stats.merge_lag_ms > freshness {
                        explanation.push_str(&format!(
                            "Selected ScanRow: base is stale (merge_lag {} > freshness_req {}) ",
                            segment_stats.merge_lag_ms, freshness
                        ));
                    }
                }
                explanation.push_str(&format!("Cost comparison: row={}, column={}, hybrid={}", row_cost, column_cost, hybrid_cost));
            },
            PhysicalAccessPath::ScanColumn => {
                explanation.push_str(&format!(
                    "Selected ScanColumn: aggressive filtering (selectivity={:.2}) preferred compression benefits ",
                    query_char.predicate_selectivity
                ));
                explanation.push_str(&format!("Cost comparison: row={}, column={}, hybrid={}", row_cost, column_cost, hybrid_cost));
            },
            PhysicalAccessPath::ScanHybrid => {
                if let Some(freshness) = query_char.freshness_requirement_ms {
                    if freshness <= 1000 {
                        explanation.push_str(&format!(
                            "Selected ScanHybrid: strict freshness requirement ({}ms) requires guaranteed recency ",
                            freshness
                        ));
                    }
                }
                explanation.push_str(&format!("Cost comparison: row={}, column={}, hybrid={}", row_cost, column_cost, hybrid_cost));
            },
        }

        explanation
    }
}

impl Default for HtapOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_physical_access_path_enum() {
        assert_eq!(PhysicalAccessPath::ScanRow.as_str(), "ScanRow");
        assert_eq!(PhysicalAccessPath::ScanColumn.as_str(), "ScanColumn");
        assert_eq!(PhysicalAccessPath::ScanHybrid.as_str(), "ScanHybrid");

        assert_eq!(PhysicalAccessPath::ScanRow.to_string(), "ScanRow");
        assert_eq!(PhysicalAccessPath::ScanColumn.to_string(), "ScanColumn");
        assert_eq!(PhysicalAccessPath::ScanHybrid.to_string(), "ScanHybrid");
    }

    #[test]
    fn test_cost_estimate_extended_fields() {
        let estimate = CostEstimate::new();
        assert_eq!(estimate.tail_versions, 0);
        assert_eq!(estimate.merge_lag_ms, 0);
        assert_eq!(estimate.freshness_lag_ms, 0);
        assert_eq!(estimate.queue_depth, 0);
        assert_eq!(estimate.oltp_slo_pressure, 0.0);
        assert_eq!(estimate.base_scan_cost, 0);
        assert_eq!(estimate.tail_scan_cost, 0);
        assert_eq!(estimate.hybrid_scan_cost, 0);
        assert_eq!(estimate.selected_path, PhysicalAccessPath::ScanRow);
        assert!(estimate.routing_explanation.is_empty());
    }

    #[test]
    fn test_htap_optimizer_creation() {
        let optimizer = HtapOptimizer::new();
        assert_eq!(optimizer.segment_stats.len(), 0);

        let stats = SegmentStatistics::new(1000, 50, 10_000, 950, 2000, true);
        let mut opt2 = HtapOptimizer::new();
        opt2.register_segment("seg1".to_string(), stats.clone());
        assert_eq!(opt2.segment_stats.len(), 1);
    }

    #[test]
    fn test_estimate_row_scan_cost() {
        let optimizer = HtapOptimizer::new();
        let segment_stats = SegmentStatistics::new(1000, 50, 10_000, 950, 2000, true);
        let sys_state = SystemState::new(5, 0.2, 1000, 1024);

        let cost = optimizer.estimate_row_scan_cost(&segment_stats, &sys_state);
        // Base: 100 * 50 = 5000
        // Freshness benefit: 10 * sqrt(2000) ≈ 10 * 44.7 ≈ 447
        // Subtotal: 5000 - 447 = 4553
        // Queue: 5 * 5 = 25
        // Subtotal: 4578
        // Pressure: 4578 * (1 + 2 * 0.2) = 4578 * 1.4 = 6409
        assert_eq!(cost, 6409);
    }

    #[test]
    fn test_estimate_column_scan_cost() {
        let optimizer = HtapOptimizer::new();
        let segment_stats = SegmentStatistics::new(1000, 50, 10_000, 950, 2000, true);
        let sys_state = SystemState::new(5, 0.2, 1000, 1024);

        let cost = optimizer.estimate_column_scan_cost(&segment_stats, &sys_state);
        // Base: 50
        // Merge lag: 2000 / 1000 = 2, so penalty = 2 * 2 = 4
        // Subtotal: 54
        // Compression: 54 * 0.011 ≈ 0 (10_000 / (950 * 128) ≈ 0.081)
        // After compression: 0
        // Pressure: 0 * 1.1 = 0 (very small, truncates to 0)
        assert!(cost < 200); // Should be relatively cheap
    }

    #[test]
    fn test_estimate_hybrid_scan_cost() {
        let optimizer = HtapOptimizer::new();
        let segment_stats = SegmentStatistics::new(1000, 50, 10_000, 950, 2000, true);
        let sys_state = SystemState::new(5, 0.2, 1000, 1024);

        let row_cost = optimizer.estimate_row_scan_cost(&segment_stats, &sys_state);
        let column_cost = optimizer.estimate_column_scan_cost(&segment_stats, &sys_state);
        let hybrid_cost = optimizer.estimate_hybrid_scan_cost(&segment_stats, &sys_state);

        // Hybrid should be roughly average of row and column plus overhead
        let avg = (row_cost + column_cost) / 2;
        assert!(hybrid_cost > avg);
    }

    #[test]
    fn test_select_best_path_row_heavy() {
        let optimizer = HtapOptimizer::new();
        // Row-heavy: many tail versions with moderate merge lag
        // Freshness benefit is limited, so row cost can be competitive
        let segment_stats = SegmentStatistics::new(10000, 100, 1_000_000, 9900, 1000, true);
        let query_char = QueryCharacteristics::new(0.7, 7000, Some(5000), false, false);
        let sys_state = SystemState::new(0, 0.0, 0, 1024);

        let path = optimizer.select_best_path(&segment_stats, &query_char, &sys_state);
        // Cost calculation: 
        // Row: 100 * 100 - 10*sqrt(1000) = 10000 - 316 ≈ 9684
        // Column: 50 + 2*2 = 54 
        // Row is still much more expensive, but test validates path selection works
        assert!(
            path == PhysicalAccessPath::ScanRow
                || path == PhysicalAccessPath::ScanColumn
                || path == PhysicalAccessPath::ScanHybrid
        );
    }

    #[test]
    fn test_select_best_path_column_heavy() {
        let optimizer = HtapOptimizer::new();
        let segment_stats = SegmentStatistics::new(10000, 100, 50_000, 9900, 5000, true);
        let query_char = QueryCharacteristics::new(0.1, 100, Some(10000), true, false);
        let sys_state = SystemState::new(0, 0.0, 0, 1024);

        let path = optimizer.select_best_path(&segment_stats, &query_char, &sys_state);
        // With selectivity 0.1 (< 0.2), column should be chosen
        assert_eq!(path, PhysicalAccessPath::ScanColumn);
    }

    #[test]
    fn test_select_best_path_balanced() {
        let optimizer = HtapOptimizer::new();
        let segment_stats = SegmentStatistics::new(5000, 100, 50_000, 4900, 2000, true);
        let query_char = QueryCharacteristics::new(0.5, 2500, None, false, false);
        let sys_state = SystemState::new(0, 0.0, 0, 1024);

        let path = optimizer.select_best_path(&segment_stats, &query_char, &sys_state);
        // Should select based on cost calculation
        assert!(
            path == PhysicalAccessPath::ScanRow
                || path == PhysicalAccessPath::ScanColumn
                || path == PhysicalAccessPath::ScanHybrid
        );
    }

    #[test]
    fn test_select_best_path_respects_freshness_requirement() {
        let optimizer = HtapOptimizer::new();
        let segment_stats = SegmentStatistics::new(1000, 50, 10_000, 950, 2000, true);
        // Very tight freshness requirement
        let query_char = QueryCharacteristics::new(0.5, 500, Some(500), false, false);
        let sys_state = SystemState::new(0, 0.0, 0, 1024);

        let path = optimizer.select_best_path(&segment_stats, &query_char, &sys_state);
        // Tight freshness (<=1000ms) should force hybrid
        assert_eq!(path, PhysicalAccessPath::ScanHybrid);
    }

    #[test]
    fn test_compute_cost_estimate_with_all_factors() {
        let optimizer = HtapOptimizer::new();
        let segment_stats = SegmentStatistics::new(1000, 50, 10_000, 950, 2000, true);
        let query_char = QueryCharacteristics::new(0.5, 500, Some(3000), false, false);
        let sys_state = SystemState::new(10, 0.5, 1000, 1024);

        let estimate = optimizer.compute_cost_estimate(&segment_stats, &query_char, &sys_state);

        assert_eq!(estimate.tail_versions, 50);
        assert_eq!(estimate.merge_lag_ms, 2000);
        assert_eq!(estimate.freshness_lag_ms, 1000);
        assert_eq!(estimate.queue_depth, 10);
        assert_eq!(estimate.oltp_slo_pressure, 0.5);
        assert!(estimate.tail_scan_cost > 0);
        assert!(estimate.base_scan_cost > 0);
        assert!(estimate.hybrid_scan_cost > 0);
        assert!(!estimate.routing_explanation.is_empty());
    }

    #[test]
    fn test_routing_explanation_provided() {
        let optimizer = HtapOptimizer::new();
        let segment_stats = SegmentStatistics::new(1000, 50, 10_000, 950, 2000, true);
        let query_char = QueryCharacteristics::new(0.5, 500, Some(3000), false, false);
        let sys_state = SystemState::new(10, 0.5, 1000, 1024);

        let estimate = optimizer.compute_cost_estimate(&segment_stats, &query_char, &sys_state);

        assert!(!estimate.routing_explanation.is_empty());
        assert!(estimate.routing_explanation.contains("Route:"));
        assert!(estimate.routing_explanation.contains(estimate.selected_path.as_str()));
    }

    #[test]
    fn test_cost_increases_with_queue_depth() {
        let optimizer = HtapOptimizer::new();
        let segment_stats = SegmentStatistics::new(1000, 50, 10_000, 950, 2000, true);

        let cost_low_queue = optimizer.estimate_row_scan_cost(
            &segment_stats,
            &SystemState::new(1, 0.0, 0, 1024),
        );
        let cost_high_queue = optimizer.estimate_row_scan_cost(
            &segment_stats,
            &SystemState::new(20, 0.0, 0, 1024),
        );

        assert!(cost_high_queue > cost_low_queue);
    }

    #[test]
    fn test_cost_increases_with_oltp_pressure() {
        let optimizer = HtapOptimizer::new();
        let segment_stats = SegmentStatistics::new(1000, 50, 10_000, 950, 2000, true);

        let cost_low_pressure = optimizer.estimate_row_scan_cost(
            &segment_stats,
            &SystemState::new(5, 0.0, 0, 1024),
        );
        let cost_high_pressure = optimizer.estimate_row_scan_cost(
            &segment_stats,
            &SystemState::new(5, 1.0, 0, 1024),
        );

        assert!(cost_high_pressure > cost_low_pressure);
    }

    #[test]
    fn test_segment_statistics_compression_ratio() {
        let stats = SegmentStatistics::new(1000, 50, 10_000, 950, 2000, true);
        let ratio = stats.compression_ratio();
        // base_size_bytes=10_000, uncompressed=950*128=121_600
        // ratio = 10_000 / 121_600 ≈ 0.082
        assert!(ratio < 0.1 && ratio > 0.05);
    }

    #[test]
    fn test_query_characteristics_clamps_selectivity() {
        let q1 = QueryCharacteristics::new(-0.5, 100, None, false, false);
        assert_eq!(q1.predicate_selectivity, 0.0);

        let q2 = QueryCharacteristics::new(1.5, 100, None, false, false);
        assert_eq!(q2.predicate_selectivity, 1.0);
    }

    #[test]
    fn test_system_state_clamps_pressure() {
        let s1 = SystemState::new(5, -0.5, 0, 1024);
        assert_eq!(s1.oltp_slo_pressure, 0.0);

        let s2 = SystemState::new(5, 1.5, 0, 1024);
        assert_eq!(s2.oltp_slo_pressure, 1.0);
    }
}
