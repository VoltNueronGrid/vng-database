//! Segment metadata and statistics for HTAP storage.
//!
//! Segments are logical containers within a partition that hold row data
//! in either mutable tail (OLTP) or immutable base (OLAP) form.

use crate::types::{PartitionId, SegmentId, SnapshotTs, VersionId, RowId};

/// A reference to a segment within the HTAP storage hierarchy.
#[derive(Debug, Clone)]
pub struct HtapSegmentRef {
    /// Table or relation name
    pub table_name: String,
    /// Partition this segment belongs to
    pub partition_id: PartitionId,
    /// Unique segment identifier within the partition
    pub segment_id: SegmentId,
}

impl HtapSegmentRef {
    /// Create a new segment reference.
    pub fn new(table_name: String, partition_id: PartitionId, segment_id: SegmentId) -> Self {
        HtapSegmentRef { table_name, partition_id, segment_id }
    }
}

/// Metadata for a single storage segment.
///
/// Contains size, version, and statistics needed for query planning,
/// merge policies, and segment lifecycle management.
#[derive(Debug, Clone)]
pub struct SegmentMetadata {
    /// Unique identifier for this segment
    pub segment_id: SegmentId,
    /// Partition this segment belongs to
    pub partition_id: PartitionId,
    /// Table or relation name
    pub table_name: String,
    /// Current row count in this segment
    pub row_count: u64,
    /// High-water mark of snapshot timestamps visible in this segment
    pub tail_watermark: SnapshotTs,
    /// If Some, this segment has a frozen base version
    pub base_version_id: Option<VersionId>,
    /// Optional min/max statistics for column pruning
    pub min_max_stats: Option<MinMaxStats>,
    /// Creation time (milliseconds since epoch)
    pub created_at: u64,
    /// Last time a merge involving this segment completed
    pub last_merge_at: Option<u64>,
}

impl SegmentMetadata {
    /// Create a new segment metadata entry.
    pub fn new(
        segment_id: SegmentId,
        partition_id: PartitionId,
        table_name: String,
        created_at: u64,
    ) -> Self {
        SegmentMetadata {
            segment_id,
            partition_id,
            table_name,
            row_count: 0,
            tail_watermark: SnapshotTs(0),
            base_version_id: None,
            min_max_stats: None,
            created_at,
            last_merge_at: None,
        }
    }
}

/// Statistics about a segment's tail (mutable) portion.
#[derive(Debug, Clone)]
pub struct SegmentStats {
    /// Current number of rows in the segment
    pub row_count: u64,
    /// Highest snapshot timestamp visible in the tail
    pub tail_watermark: SnapshotTs,
    /// Number of entries in the mutable tail portion
    pub tail_length: usize,
    /// Estimated bytes of merge backlog (overhead from old versions)
    pub merge_backlog_bytes: u64,
}

/// Column-level statistics for storage segments (min/max values per column).
///
/// Used for column pruning and predicate pushdown during query planning.
#[derive(Debug, Clone)]
pub struct MinMaxStats {
    /// Minimum value for each column (as string)
    pub min_values: Vec<String>,
    /// Maximum value for each column (as string)
    pub max_values: Vec<String>,
}

/// A single immutable version of a row in the MVCC storage.
///
/// Versions are chained together for a logical row, allowing queries
/// to navigate the version history and select the correct snapshot.
#[derive(Debug, Clone)]
pub struct TailVersion {
    /// Row identifier this version belongs to
    pub row_id: RowId,
    /// Unique version ID in the row's chain
    pub version_id: VersionId,
    /// Transaction ID that created this version
    pub begin_ts: CommitTs,
    /// Transaction ID that deleted this version (if tombstone)
    pub end_ts: Option<CommitTs>,
    /// ID of the previous version in the chain (if any)
    pub prev_version: Option<VersionId>,
    /// True if this is a delete tombstone
    pub tombstone: bool,
    /// Serialized row data (empty for tombstones)
    pub payload: Vec<u8>,
}

impl TailVersion {
    /// Create a new row version.
    pub fn new(
        row_id: RowId,
        version_id: VersionId,
        begin_ts: CommitTs,
        payload: Vec<u8>,
    ) -> Self {
        TailVersion {
            row_id,
            version_id,
            begin_ts,
            end_ts: None,
            prev_version: None,
            tombstone: false,
            payload,
        }
    }

    /// Create a delete tombstone version.
    pub fn tombstone(
        row_id: RowId,
        version_id: VersionId,
        delete_ts: CommitTs,
    ) -> Self {
        TailVersion {
            row_id,
            version_id,
            begin_ts: delete_ts,
            end_ts: Some(delete_ts),
            prev_version: None,
            tombstone: true,
            payload: Vec::new(),
        }
    }
}

use crate::types::CommitTs;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_htap_segment_ref_creation() {
        let seg_ref = HtapSegmentRef::new(
            "users".to_string(),
            PartitionId(1),
            SegmentId(10),
        );
        assert_eq!(seg_ref.table_name, "users");
        assert_eq!(seg_ref.partition_id, PartitionId(1));
        assert_eq!(seg_ref.segment_id, SegmentId(10));
    }

    #[test]
    fn test_segment_metadata_creation() {
        let meta = SegmentMetadata::new(
            SegmentId(5),
            PartitionId(1),
            "orders".to_string(),
            1000,
        );
        assert_eq!(meta.segment_id, SegmentId(5));
        assert_eq!(meta.partition_id, PartitionId(1));
        assert_eq!(meta.table_name, "orders");
        assert_eq!(meta.created_at, 1000);
        assert_eq!(meta.row_count, 0);
        assert!(meta.base_version_id.is_none());
        assert!(meta.last_merge_at.is_none());
    }

    #[test]
    fn test_tail_version_creation() {
        let ver = TailVersion::new(
            RowId(100),
            VersionId(1),
            CommitTs(50),
            vec![1, 2, 3],
        );
        assert_eq!(ver.row_id, RowId(100));
        assert_eq!(ver.version_id, VersionId(1));
        assert_eq!(ver.begin_ts, CommitTs(50));
        assert!(ver.end_ts.is_none());
        assert!(!ver.tombstone);
        assert_eq!(ver.payload, vec![1, 2, 3]);
    }

    #[test]
    fn test_tail_version_tombstone() {
        let tomb = TailVersion::tombstone(
            RowId(200),
            VersionId(2),
            CommitTs(100),
        );
        assert_eq!(tomb.row_id, RowId(200));
        assert!(tomb.tombstone);
        assert_eq!(tomb.begin_ts, CommitTs(100));
        assert_eq!(tomb.end_ts, Some(CommitTs(100)));
        assert!(tomb.payload.is_empty());
    }

    #[test]
    fn test_segment_stats_creation() {
        let stats = SegmentStats {
            row_count: 1000,
            tail_watermark: SnapshotTs(500),
            tail_length: 50,
            merge_backlog_bytes: 5000,
        };
        assert_eq!(stats.row_count, 1000);
        assert_eq!(stats.tail_watermark, SnapshotTs(500));
        assert_eq!(stats.tail_length, 50);
        assert_eq!(stats.merge_backlog_bytes, 5000);
    }

    #[test]
    fn test_minmax_stats_creation() {
        let stats = MinMaxStats {
            min_values: vec!["a".to_string(), "1".to_string()],
            max_values: vec!["z".to_string(), "100".to_string()],
        };
        assert_eq!(stats.min_values.len(), 2);
        assert_eq!(stats.max_values.len(), 2);
    }
}
