//! Storage abstraction layer traits for HTAP architecture.
//!
//! Defines trait-based interfaces for the various storage components:
//! - TailStore: Mutable, in-memory row storage (OLTP-optimized)
//! - BaseColumnStore: Immutable columnar storage (OLAP-optimized)
//! - SegmentCatalog: Registry of partitions and segments
//! - RowProjectionCache: Efficient column subset caching
//! - MergeableSegmentStore: Support for L-Store-style compaction

use crate::segment::{SegmentMetadata, SegmentStats, TailVersion};
use crate::types::{PartitionId, SegmentId, RowId, SnapshotTs, CommitTs};
use std::collections::HashMap;

/// A `Row` is a map of column name to value (string-encoded).
pub type Row = HashMap<String, String>;

/// A batch of columns extracted from storage.
#[derive(Debug, Clone)]
pub struct ColumnBatch {
    /// Column ID → column values
    pub columns: HashMap<u32, Vec<String>>,
    /// Number of rows in this batch
    pub row_count: usize,
}

/// TailStore trait — writable, mutable row storage in a segment.
///
/// The "tail" is the mutable portion of a segment, holding recent OLTP writes.
/// It supports version chaining and MVCC visibility rules.
pub trait TailStore: Send + Sync {
    /// Insert or update a version in the tail.
    /// Returns error if the version already exists or storage is full.
    fn insert_version(&mut self, row_id: RowId, version: TailVersion) -> Result<(), String>;

    /// Get the latest visible version of a row as-of a snapshot timestamp.
    /// Navigates the version chain to find the most recent non-tombstone version.
    fn get_latest_version(
        &self,
        row_id: RowId,
        snapshot_ts: SnapshotTs,
    ) -> Result<Option<TailVersion>, String>;

    /// Get the entire version chain for a row.
    /// Returns all versions in ascending commit timestamp order.
    fn get_version_chain(&self, row_id: RowId) -> Result<Vec<TailVersion>, String>;

    /// Delete (tombstone) a row within a transaction.
    /// The row becomes invisible to reads after the delete transaction commits.
    fn delete_row(&mut self, row_id: RowId, commit_ts: CommitTs) -> Result<(), String>;
}

/// BaseColumnStore trait — immutable columnar segment storage.
///
/// The "base" is the frozen, columnar portion of a segment. It is
/// optimized for OLAP scans and supports fast column-level filtering.
pub trait BaseColumnStore: Send + Sync {
    /// Scan specific columns, returning a batch of all rows.
    /// Column IDs are 0-indexed column positions.
    fn scan_columns(&self, col_ids: &[u32]) -> Result<ColumnBatch, String>;

    /// Retrieve a single row by ID from the base store.
    /// Returns None if the row does not exist.
    fn get_row_by_id(&self, row_id: RowId) -> Result<Option<Row>, String>;

    /// Return the total row count in this base store.
    fn row_count(&self) -> u64;
}

/// SegmentCatalog trait — registry of partition/segment metadata.
///
/// Maintains the mapping between logical partitions, segments, and their
/// associated metadata (sizes, versions, merge status).
pub trait SegmentCatalog: Send + Sync {
    /// Register a new segment in the catalog.
    /// Returns error if the segment ID already exists.
    fn register_segment(&mut self, meta: SegmentMetadata) -> Result<(), String>;

    /// Retrieve metadata for a segment by ID.
    /// Returns None if the segment does not exist.
    fn get_segment(&self, seg_id: SegmentId) -> Result<Option<SegmentMetadata>, String>;

    /// List all segments in a partition.
    /// Returns metadata for each segment, in creation order.
    fn list_segments_for_partition(&self, part_id: PartitionId) -> Result<Vec<SegmentMetadata>, String>;

    /// Update statistics for a segment (e.g., after a merge or compaction).
    /// Returns error if the segment does not exist.
    fn update_segment_stats(&mut self, seg_id: SegmentId, stats: SegmentStats) -> Result<(), String>;
}

/// RowProjectionCache trait — efficient column subset caching.
///
/// Caches frequently-accessed column projections to avoid re-scanning
/// or re-serializing the same subset repeatedly.
pub trait RowProjectionCache: Send + Sync {
    /// Retrieve a cached row projection (subset of columns).
    /// Returns None if the projection is not in the cache.
    fn get(&self, row_id: RowId, columns: &[u32]) -> Result<Option<Row>, String>;

    /// Cache a row projection (subset of columns).
    /// Overwrites any existing entry for the same row and column set.
    fn put(&mut self, row_id: RowId, columns: &[u32], row: Row) -> Result<(), String>;

    /// Invalidate all cached projections for a row.
    /// Called after a row is updated or deleted.
    fn invalidate(&mut self, row_id: RowId) -> Result<(), String>;
}

/// MergeableSegmentStore trait — support for L-Store-style merges.
///
/// Segments can be merged to consolidate the tail into the base, reducing
/// the overhead of maintaining multiple versions and improving OLAP performance.
pub trait MergeableSegmentStore: Send + Sync {
    /// Get a list of segments in a partition that are eligible for merging.
    /// Merging strategy is implementation-defined (e.g., size, age, version count).
    fn get_mergeable_segments(&self, part_id: PartitionId) -> Result<Vec<SegmentId>, String>;

    /// Merge source segments into a target segment.
    /// The target segment's base is updated to contain all rows from sources.
    /// Returns the new row count in the target segment.
    fn merge_segments(&mut self, src_segs: &[SegmentId], target_id: SegmentId) -> Result<u64, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_batch_creation() {
        let mut batch = ColumnBatch {
            columns: HashMap::new(),
            row_count: 100,
        };
        batch.columns.insert(0, vec!["a".to_string(), "b".to_string()]);
        batch.columns.insert(1, vec!["1".to_string(), "2".to_string()]);

        assert_eq!(batch.row_count, 100);
        assert_eq!(batch.columns.len(), 2);
        assert_eq!(batch.columns[&0].len(), 2);
    }

    #[test]
    fn test_row_type() {
        let mut row: Row = HashMap::new();
        row.insert("name".to_string(), "Alice".to_string());
        row.insert("age".to_string(), "30".to_string());

        assert_eq!(row.len(), 2);
        assert_eq!(row.get("name").map(|s| s.as_str()), Some("Alice"));
    }
}
