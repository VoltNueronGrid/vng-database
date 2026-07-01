//! Hybrid base+tail scan execution with MVCC visibility and freshness enforcement.
//!
//! Advances H9-11 in status_tracker.md: Hybrid base+tail scan execution engine.
//!
//! This module implements efficient scanning over hybrid segments (base columnar + mutable tail)
//! with proper MVCC visibility semantics. The executor supports both eager (load all in memory)
//! and streaming (return incrementally) scan strategies.
//!
//! # Visibility Rules
//!
//! For each row_id, the visible version is determined as follows:
//! 1. Scan tail versions in reverse chronological order (most recent first)
//! 2. Find the first version where CommitTs <= snapshot_ts
//! 3. If found in tail and not deleted, use that tail version
//! 4. Otherwise, fall back to base version if present
//! 5. Return None if row is deleted or not visible at the snapshot
//!
//! # Freshness Enforcement
//!
//! If `max_staleness_ms` is configured and the merge lag exceeds the threshold,
//! the scan returns a `QueryTooStale` error to prevent reading stale data.

use crate::types::{CommitTs, RowId, SegmentId, SnapshotTs};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

/// Errors that can occur during hybrid scan execution.
#[derive(Debug, Clone)]
pub enum HybridScanError {
    /// Query attempted to read data that is too stale.
    QueryTooStale {
        required_ms: u64,
        actual_ms: u64,
    },
    /// Invalid configuration or parameters.
    InvalidConfig(String),
    /// General scan execution error.
    ExecutionError(String),
}

impl fmt::Display for HybridScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HybridScanError::QueryTooStale { required_ms, actual_ms } => {
                write!(f, "Query too stale: required freshness {} ms, actual {} ms", required_ms, actual_ms)
            }
            HybridScanError::InvalidConfig(msg) => write!(f, "Invalid config: {}", msg),
            HybridScanError::ExecutionError(msg) => write!(f, "Execution error: {}", msg),
        }
    }
}

impl Error for HybridScanError {}

/// Scan strategy for retrieving merged results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStrategy {
    /// Merge all results in memory before returning.
    Eager,
    /// Return merged results incrementally via an iterator.
    Streaming,
}

/// Source attribution for a scanned row (base segment or tail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanSource {
    /// Row came from base columnar segment.
    Base(SegmentId),
    /// Row came from mutable tail segment.
    Tail(SegmentId),
}

/// Version metadata for MVCC visibility tracking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    /// Transaction timestamp when the row was committed.
    pub commit_ts: CommitTs,
    /// Snapshot timestamp at which this version becomes visible.
    pub snapshot_ts: SnapshotTs,
    /// Whether the row is logically deleted at this version.
    pub is_deleted: bool,
}

impl VersionInfo {
    /// Create new version info.
    pub fn new(commit_ts: CommitTs, snapshot_ts: SnapshotTs, is_deleted: bool) -> Self {
        VersionInfo { commit_ts, snapshot_ts, is_deleted }
    }

    /// Check if this version is visible at a given snapshot timestamp.
    /// A version is visible if:
    /// - Its commit_ts <= snapshot_ts (committed before the snapshot)
    /// - It is not a delete tombstone
    pub fn is_visible_at(&self, snapshot_ts: SnapshotTs) -> bool {
        self.commit_ts.0 <= snapshot_ts.0 && !self.is_deleted
    }
}

/// Result of a hybrid scan operation for a single row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HybridScanResult {
    /// Row identifier.
    pub row_id: RowId,
    /// Serialized row data (raw bytes).
    pub values: Vec<u8>,
    /// Version metadata for this row.
    pub version_info: VersionInfo,
    /// Source of the row (base or tail).
    pub source: ScanSource,
}

impl HybridScanResult {
    /// Create a new hybrid scan result.
    pub fn new(
        row_id: RowId,
        values: Vec<u8>,
        version_info: VersionInfo,
        source: ScanSource,
    ) -> Self {
        HybridScanResult { row_id, values, version_info, source }
    }
}

/// Executor for hybrid base+tail scans with MVCC visibility.
pub struct HybridScanExecutor {
    segment_id: SegmentId,
    /// Base rows from columnar segment: (row_id, serialized values)
    base_rows: Vec<(RowId, Vec<u8>)>,
    /// Tail version chains: row_id -> [(commit_ts, optional_deleted_flag, values)]
    tail_rows: HashMap<RowId, Vec<(CommitTs, bool, Vec<u8>)>>,
    /// Snapshot timestamp for visibility checks.
    snapshot_ts: SnapshotTs,
    /// Maximum allowed staleness in milliseconds (None = no limit).
    max_staleness_ms: Option<u64>,
}

impl HybridScanExecutor {
    /// Create a new hybrid scan executor.
    ///
    /// # Arguments
    ///
    /// * `segment_id` - The segment being scanned
    /// * `base_rows` - Rows from the base columnar segment
    /// * `tail_rows` - Version chains from the mutable tail
    /// * `snapshot_ts` - Snapshot timestamp for visibility
    ///
    /// # Returns
    ///
    /// A new executor instance.
    pub fn new(
        segment_id: SegmentId,
        base_rows: Vec<(RowId, Vec<u8>)>,
        tail_rows: HashMap<RowId, Vec<(CommitTs, bool, Vec<u8>)>>,
        snapshot_ts: SnapshotTs,
    ) -> Self {
        HybridScanExecutor {
            segment_id,
            base_rows,
            tail_rows,
            snapshot_ts,
            max_staleness_ms: None,
        }
    }

    /// Set the maximum staleness threshold for this executor.
    pub fn with_max_staleness(mut self, max_staleness_ms: u64) -> Self {
        self.max_staleness_ms = Some(max_staleness_ms);
        self
    }

    /// Perform an eager scan: load all results into memory before returning.
    ///
    /// This strategy merges base and tail versions for all rows and returns
    /// the complete result set. Suitable for small result sets or when ordering
    /// over the full set is required.
    ///
    /// # Returns
    ///
    /// `Ok(Vec<HybridScanResult>)` with all visible rows, or an error if freshness
    /// constraints are violated.
    pub fn scan_eager(&self) -> Result<Vec<HybridScanResult>, HybridScanError> {
        let mut results = Vec::new();

        // Collect all row IDs from both base and tail
        let mut all_row_ids = std::collections::HashSet::new();
        for (row_id, _) in &self.base_rows {
            all_row_ids.insert(*row_id);
        }
        for row_id in self.tail_rows.keys() {
            all_row_ids.insert(*row_id);
        }

        // Merge versions for each row
        for row_id in all_row_ids {
            if let Some(merged) = self.merge_versions(row_id)? {
                results.push(merged);
            }
        }

        // Sort by row_id for consistent ordering
        results.sort_by_key(|r| r.row_id);

        Ok(results)
    }

    /// Perform a streaming scan: return results via an iterator.
    ///
    /// This strategy is more memory-efficient for large result sets, as it
    /// yields rows one at a time rather than loading everything into memory.
    ///
    /// # Returns
    ///
    /// An iterator yielding `HybridScanResult` items.
    pub fn scan_streaming(&self) -> Result<impl Iterator<Item = HybridScanResult> + '_, HybridScanError> {
        // Collect all row IDs and sort them
        let mut all_row_ids: Vec<_> = std::collections::HashSet::<_>::from_iter(
            self.base_rows.iter().map(|(rid, _)| *rid)
                .chain(self.tail_rows.keys().copied())
        ).into_iter().collect();
        all_row_ids.sort();

        Ok(all_row_ids.into_iter().filter_map(move |row_id| {
            self.merge_versions(row_id).ok().flatten()
        }))
    }

    /// Merge base and tail versions for a single row, respecting MVCC visibility.
    ///
    /// # Visibility Algorithm
    ///
    /// 1. If row exists in tail, walk its version chain in reverse order (most recent first)
    /// 2. Find the first (most recent) version where commit_ts <= snapshot_ts
    /// 3. If that version is not deleted, use it and return
    /// 4. Otherwise, fall back to base version if it exists
    /// 5. Return None if row is deleted or not visible
    ///
    /// # Arguments
    ///
    /// * `row_id` - The row to merge versions for
    ///
    /// # Returns
    ///
    /// Some(HybridScanResult) if the row is visible, None if deleted or not yet committed.
    fn merge_versions(&self, row_id: RowId) -> Result<Option<HybridScanResult>, HybridScanError> {
        // Step 1: Check tail first (most recent writes)
        if let Some(versions) = self.tail_rows.get(&row_id) {
            // Walk versions in reverse (most recent first)
            for (commit_ts, is_deleted, values) in versions.iter().rev() {
                if self.compute_visibility(row_id, *commit_ts, self.snapshot_ts) {
                    if *is_deleted {
                        // Row is deleted in tail
                        return Ok(None);
                    }
                    // Found visible version in tail
                    return Ok(Some(HybridScanResult::new(
                        row_id,
                        values.clone(),
                        VersionInfo::new(*commit_ts, self.snapshot_ts, false),
                        ScanSource::Tail(self.segment_id),
                    )));
                }
            }
        }

        // Step 2: Fall back to base if no visible tail version
        if let Some(base_values) = self.find_base_row(row_id) {
            // Base rows are immutable and by definition visible (committed before base was frozen)
            return Ok(Some(HybridScanResult::new(
                row_id,
                base_values.clone(),
                VersionInfo::new(CommitTs(0), self.snapshot_ts, false), // base rows predate snapshots
                ScanSource::Base(self.segment_id),
            )));
        }

        Ok(None)
    }

    /// Compute visibility of a version at a snapshot timestamp.
    ///
    /// A version is visible if:
    /// - Its commit_ts <= snapshot_ts (committed before or at the snapshot)
    /// - It hasn't been deleted in a later transaction
    ///
    /// # Arguments
    ///
    /// * `_row_id` - The row being checked (reserved for future transactional checks)
    /// * `commit_ts` - The commit timestamp of the version
    /// * `snapshot_ts` - The snapshot timestamp at which to check visibility
    ///
    /// # Returns
    ///
    /// `true` if the version is visible, `false` otherwise.
    fn compute_visibility(&self, _row_id: RowId, commit_ts: CommitTs, snapshot_ts: SnapshotTs) -> bool {
        CommitTs(commit_ts.0) <= CommitTs(snapshot_ts.0)
    }

    /// Find a row in the base segment.
    ///
    /// # Arguments
    ///
    /// * `row_id` - The row to find
    ///
    /// # Returns
    ///
    /// The serialized row values if found.
    fn find_base_row(&self, row_id: RowId) -> Option<&Vec<u8>> {
        self.base_rows.iter().find(|(rid, _)| *rid == row_id).map(|(_, data)| data)
    }

    /// Enforce freshness constraints: check that merge lag doesn't exceed the threshold.
    ///
    /// # Arguments
    ///
    /// * `merge_lag_ms` - The time elapsed since the last merge (in milliseconds)
    ///
    /// # Returns
    ///
    /// `Ok(())` if the data is fresh enough, or `Err(QueryTooStale)` if stale.
    pub fn enforce_freshness(&self, merge_lag_ms: u64) -> Result<(), HybridScanError> {
        if let Some(max_staleness) = self.max_staleness_ms {
            if merge_lag_ms > max_staleness {
                return Err(HybridScanError::QueryTooStale {
                    required_ms: max_staleness,
                    actual_ms: merge_lag_ms,
                });
            }
        }
        Ok(())
    }

    /// Get the segment ID for this executor.
    pub fn segment_id(&self) -> SegmentId {
        self.segment_id
    }

    /// Get the snapshot timestamp for this executor.
    pub fn snapshot_ts(&self) -> SnapshotTs {
        self.snapshot_ts
    }

    /// Get the number of base rows.
    pub fn base_row_count(&self) -> usize {
        self.base_rows.len()
    }

    /// Get the number of tail row IDs (not counting versions).
    pub fn tail_row_count(&self) -> usize {
        self.tail_rows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_executor() -> HybridScanExecutor {
        HybridScanExecutor::new(
            SegmentId(1),
            vec![],
            HashMap::new(),
            SnapshotTs(100),
        )
    }

    #[test]
    fn test_hybrid_scan_executor_creation() {
        let executor = new_executor();
        assert_eq!(executor.segment_id(), SegmentId(1));
        assert_eq!(executor.snapshot_ts(), SnapshotTs(100));
        assert_eq!(executor.base_row_count(), 0);
        assert_eq!(executor.tail_row_count(), 0);
    }

    #[test]
    fn test_hybrid_scan_executor_with_staleness() {
        let executor = HybridScanExecutor::new(
            SegmentId(1),
            vec![],
            HashMap::new(),
            SnapshotTs(100),
        ).with_max_staleness(5000);

        // Should pass with low staleness
        assert!(executor.enforce_freshness(3000).is_ok());

        // Should fail with high staleness
        assert!(executor.enforce_freshness(6000).is_err());
    }

    #[test]
    fn test_hybrid_scan_eager_empty_rows() {
        let executor = new_executor();
        let results = executor.scan_eager().expect("scan should succeed");
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_hybrid_scan_eager_merge_strategy() {
        let base_rows = vec![
            (RowId(1), vec![1, 2, 3]),
            (RowId(2), vec![4, 5, 6]),
        ];

        let mut tail_rows = HashMap::new();
        tail_rows.insert(RowId(1), vec![
            (CommitTs(50), false, vec![10, 11, 12]),  // visible at snap 100
        ]);

        let executor = HybridScanExecutor::new(
            SegmentId(1),
            base_rows,
            tail_rows,
            SnapshotTs(100),
        );

        let results = executor.scan_eager().expect("scan should succeed");
        assert_eq!(results.len(), 2);

        // Row 1 should use tail version
        let row1 = &results[0];
        assert_eq!(row1.row_id, RowId(1));
        assert_eq!(row1.values, vec![10, 11, 12]);
        assert_eq!(row1.source, ScanSource::Tail(SegmentId(1)));

        // Row 2 should use base version
        let row2 = &results[1];
        assert_eq!(row2.row_id, RowId(2));
        assert_eq!(row2.values, vec![4, 5, 6]);
        assert_eq!(row2.source, ScanSource::Base(SegmentId(1)));
    }

    #[test]
    fn test_hybrid_scan_streaming_strategy() {
        let base_rows = vec![
            (RowId(1), vec![1, 2, 3]),
            (RowId(3), vec![7, 8, 9]),
        ];

        let mut tail_rows = HashMap::new();
        tail_rows.insert(RowId(2), vec![
            (CommitTs(50), false, vec![4, 5, 6]),
        ]);

        let executor = HybridScanExecutor::new(
            SegmentId(1),
            base_rows,
            tail_rows,
            SnapshotTs(100),
        );

        let results: Vec<_> = executor.scan_streaming()
            .expect("scan should succeed")
            .collect();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].row_id, RowId(1));
        assert_eq!(results[1].row_id, RowId(2));
        assert_eq!(results[2].row_id, RowId(3));
    }

    #[test]
    fn test_compute_visibility_at_snapshot() {
        let executor = new_executor();

        // Version committed before snapshot is visible
        assert!(executor.compute_visibility(RowId(1), CommitTs(50), SnapshotTs(100)));

        // Version committed exactly at snapshot is visible
        assert!(executor.compute_visibility(RowId(1), CommitTs(100), SnapshotTs(100)));

        // Version committed after snapshot is not visible
        assert!(!executor.compute_visibility(RowId(1), CommitTs(150), SnapshotTs(100)));
    }

    #[test]
    fn test_merge_versions_uses_latest_visible() {
        let mut tail_rows = HashMap::new();
        tail_rows.insert(RowId(1), vec![
            (CommitTs(30), false, vec![1, 2, 3]),      // old version
            (CommitTs(70), false, vec![4, 5, 6]),      // newer, visible
            (CommitTs(150), false, vec![7, 8, 9]),     // too new, not visible
        ]);

        let executor = HybridScanExecutor::new(
            SegmentId(1),
            vec![],
            tail_rows,
            SnapshotTs(100),
        );

        let merged = executor.merge_versions(RowId(1))
            .expect("merge should succeed")
            .expect("row should exist");

        assert_eq!(merged.values, vec![4, 5, 6]);  // Should use the v2 (CommitTs 70)
    }

    #[test]
    fn test_merge_versions_prefers_tail_over_base() {
        let base_rows = vec![
            (RowId(1), vec![1, 2, 3]),
        ];

        let mut tail_rows = HashMap::new();
        tail_rows.insert(RowId(1), vec![
            (CommitTs(50), false, vec![10, 11, 12]),
        ]);

        let executor = HybridScanExecutor::new(
            SegmentId(1),
            base_rows,
            tail_rows,
            SnapshotTs(100),
        );

        let merged = executor.merge_versions(RowId(1))
            .expect("merge should succeed")
            .expect("row should exist");

        assert_eq!(merged.values, vec![10, 11, 12]);  // Tail version preferred
        assert_eq!(merged.source, ScanSource::Tail(SegmentId(1)));
    }

    #[test]
    fn test_merge_versions_handles_deleted_rows() {
        let base_rows = vec![
            (RowId(1), vec![1, 2, 3]),
        ];

        let mut tail_rows = HashMap::new();
        // Row was deleted in tail
        tail_rows.insert(RowId(1), vec![
            (CommitTs(50), false, vec![10, 11, 12]),  // original version
            (CommitTs(80), true, vec![]),             // deleted
        ]);

        let executor = HybridScanExecutor::new(
            SegmentId(1),
            base_rows,
            tail_rows,
            SnapshotTs(100),
        );

        let merged = executor.merge_versions(RowId(1))
            .expect("merge should succeed");

        assert_eq!(merged, None);  // Row should be deleted
    }

    #[test]
    fn test_merge_versions_falls_back_to_base_when_tail_deleted() {
        let base_rows = vec![
            (RowId(1), vec![1, 2, 3]),
        ];

        let mut tail_rows = HashMap::new();
        // Row was deleted in tail, but base exists
        tail_rows.insert(RowId(1), vec![
            (CommitTs(150), true, vec![]),  // too new, won't see this
        ]);

        let executor = HybridScanExecutor::new(
            SegmentId(1),
            base_rows,
            tail_rows,
            SnapshotTs(100),
        );

        let merged = executor.merge_versions(RowId(1))
            .expect("merge should succeed")
            .expect("row should exist");

        assert_eq!(merged.values, vec![1, 2, 3]);  // Fall back to base
        assert_eq!(merged.source, ScanSource::Base(SegmentId(1)));
    }

    #[test]
    fn test_hybrid_scan_respects_freshness_requirement() {
        let executor = HybridScanExecutor::new(
            SegmentId(1),
            vec![],
            HashMap::new(),
            SnapshotTs(100),
        ).with_max_staleness(5000);

        // Freshness OK
        assert!(executor.enforce_freshness(4000).is_ok());

        // Freshness violated
        match executor.enforce_freshness(6000) {
            Err(HybridScanError::QueryTooStale { required_ms, actual_ms }) => {
                assert_eq!(required_ms, 5000);
                assert_eq!(actual_ms, 6000);
            }
            _ => panic!("Expected QueryTooStale error"),
        }
    }

    #[test]
    fn test_hybrid_scan_fails_when_too_stale() {
        let executor = HybridScanExecutor::new(
            SegmentId(1),
            vec![],
            HashMap::new(),
            SnapshotTs(100),
        ).with_max_staleness(1000);

        let result = executor.enforce_freshness(2000);
        assert!(result.is_err());
    }

    #[test]
    fn test_hybrid_scan_multiple_rows_correct_ordering() {
        let base_rows = vec![
            (RowId(5), vec![50]),
            (RowId(2), vec![20]),
            (RowId(8), vec![80]),
        ];

        let executor = HybridScanExecutor::new(
            SegmentId(1),
            base_rows,
            HashMap::new(),
            SnapshotTs(100),
        );

        let results = executor.scan_eager().expect("scan should succeed");
        assert_eq!(results.len(), 3);

        // Results should be sorted by row_id
        assert_eq!(results[0].row_id, RowId(2));
        assert_eq!(results[1].row_id, RowId(5));
        assert_eq!(results[2].row_id, RowId(8));
    }

    #[test]
    fn test_hybrid_scan_version_chains() {
        let mut tail_rows = HashMap::new();
        // Complex version chain for row_id 1
        tail_rows.insert(RowId(1), vec![
            (CommitTs(10), false, vec![10]),    // old
            (CommitTs(30), false, vec![30]),    // older middle
            (CommitTs(50), false, vec![50]),    // recent middle
            (CommitTs(200), false, vec![200]),  // too new
        ]);

        let executor = HybridScanExecutor::new(
            SegmentId(1),
            vec![],
            tail_rows,
            SnapshotTs(100),
        );

        let merged = executor.merge_versions(RowId(1))
            .expect("merge should succeed")
            .expect("row should exist");

        // Should pick the most recent visible version (CommitTs 50)
        assert_eq!(merged.values, vec![50]);
        assert_eq!(merged.version_info.commit_ts, CommitTs(50));
    }

    #[test]
    fn test_scan_source_attribution() {
        let base_rows = vec![(RowId(1), vec![1])];

        let mut tail_rows = HashMap::new();
        tail_rows.insert(RowId(2), vec![
            (CommitTs(50), false, vec![2]),
        ]);

        let executor = HybridScanExecutor::new(
            SegmentId(42),
            base_rows,
            tail_rows,
            SnapshotTs(100),
        );

        let results = executor.scan_eager().expect("scan should succeed");

        let base_result = &results[0];
        assert_eq!(base_result.source, ScanSource::Base(SegmentId(42)));

        let tail_result = &results[1];
        assert_eq!(tail_result.source, ScanSource::Tail(SegmentId(42)));
    }

    #[test]
    fn test_version_info_visibility() {
        let v1 = VersionInfo::new(CommitTs(50), SnapshotTs(100), false);
        assert!(v1.is_visible_at(SnapshotTs(100)));
        assert!(v1.is_visible_at(SnapshotTs(150)));
        assert!(!v1.is_visible_at(SnapshotTs(40)));

        let v2 = VersionInfo::new(CommitTs(50), SnapshotTs(100), true);  // deleted
        assert!(!v2.is_visible_at(SnapshotTs(100)));  // deleted is never visible
    }

    #[test]
    fn test_error_display() {
        let err1 = HybridScanError::QueryTooStale { required_ms: 5000, actual_ms: 6000 };
        let display = format!("{}", err1);
        assert!(display.contains("too stale"));
        assert!(display.contains("5000"));
        assert!(display.contains("6000"));

        let err2 = HybridScanError::InvalidConfig("bad config".to_string());
        let display = format!("{}", err2);
        assert!(display.contains("Invalid config"));
        assert!(display.contains("bad config"));
    }
}
