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
use std::collections::HashMap;

/// Encoding strategy for a column block
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnEncoding {
    /// Raw uncompressed values
    Uncompressed,
    /// Dictionary-encoded (for low cardinality)
    Dictionary,
    /// Run-length encoded (for repetitive data)
    RunLength,
    /// Bit-packing for small integers
    BitPacked,
}

/// A single columnar block of data (one column, one segment)
#[derive(Debug, Clone)]
pub struct ColumnBlock {
    /// Column ID
    pub col_id: u32,
    /// Encoded values (serialized format)
    pub values: Vec<u8>,
    /// Encoding strategy used
    pub encoding: ColumnEncoding,
    /// Minimum value in this block (for pruning)
    pub min_value: Option<String>,
    /// Maximum value in this block (for pruning)
    pub max_value: Option<String>,
    /// Count of null values
    pub null_count: u64,
    /// Row count in this block
    pub row_count: u64,
}

impl ColumnBlock {
    /// Create a new column block.
    pub fn new(col_id: u32, encoding: ColumnEncoding, row_count: u64) -> Self {
        ColumnBlock {
            col_id,
            values: Vec::new(),
            encoding,
            min_value: None,
            max_value: None,
            null_count: 0,
            row_count,
        }
    }
}

/// A complete immutable base version of a segment (all columns)
#[derive(Debug, Clone)]
pub struct BaseSegmentVersion {
    /// Segment identifier
    pub segment_id: SegmentId,
    /// Unique version ID for this base
    pub version_id: VersionId,
    /// Minimum commit timestamp of rows in this version
    pub min_commit_ts: CommitTs,
    /// Maximum commit timestamp of rows in this version
    pub max_commit_ts: CommitTs,
    /// All column blocks (col_id -> ColumnBlock)
    pub columns: HashMap<u32, ColumnBlock>,
    /// Overall statistics
    pub stats: SegmentStats,
    /// Creation timestamp (milliseconds since epoch)
    pub created_at_ms: u64,
    /// Row IDs in this base version (for lookups)
    pub row_ids: Vec<RowId>,
}

impl BaseSegmentVersion {
    /// Create a new base segment version.
    pub fn new(
        segment_id: SegmentId,
        version_id: VersionId,
        min_commit_ts: CommitTs,
        max_commit_ts: CommitTs,
        created_at_ms: u64,
    ) -> Self {
        BaseSegmentVersion {
            segment_id,
            version_id,
            min_commit_ts,
            max_commit_ts,
            columns: HashMap::new(),
            stats: SegmentStats {
                row_count: 0,
                tail_watermark: SnapshotTs(max_commit_ts.0),
                tail_length: 0,
                merge_backlog_bytes: 0,
            },
            created_at_ms,
            row_ids: Vec::new(),
        }
    }

    /// Check if a row is visible in this base version
    pub fn contains_row(&self, row_id: RowId) -> bool {
        self.row_ids.contains(&row_id)
    }
}

/// Manifest for base versions of a segment
#[derive(Debug, Clone)]
pub struct BaseSegmentManifest {
    /// Current active base version (None if tail-only)
    pub current_version: Option<BaseSegmentVersion>,
    /// Previous versions (for point-in-time reads)
    pub history: Vec<BaseSegmentVersion>,
}

impl BaseSegmentManifest {
    /// Create a new base segment manifest.
    pub fn new() -> Self {
        BaseSegmentManifest { current_version: None, history: Vec::new() }
    }

    /// Atomically swap to a new base version.
    /// Guarantees: either old-or-new visible, never partial.
    pub fn swap_version(&mut self, new_version: BaseSegmentVersion) {
        if let Some(old) = self.current_version.take() {
            self.history.push(old);
        }
        self.current_version = Some(new_version);
    }
}

impl Default for BaseSegmentManifest {
    fn default() -> Self {
        Self::new()
    }
}

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

    #[test]
    fn test_column_encoding_variants() {
        let uncompressed = ColumnEncoding::Uncompressed;
        let dict = ColumnEncoding::Dictionary;
        let rle = ColumnEncoding::RunLength;
        let bitpacked = ColumnEncoding::BitPacked;

        assert_eq!(uncompressed, ColumnEncoding::Uncompressed);
        assert_eq!(dict, ColumnEncoding::Dictionary);
        assert_eq!(rle, ColumnEncoding::RunLength);
        assert_eq!(bitpacked, ColumnEncoding::BitPacked);
        assert_ne!(uncompressed, dict);
    }

    #[test]
    fn test_column_block_creation() {
        let block = ColumnBlock::new(1, ColumnEncoding::Uncompressed, 1000);
        assert_eq!(block.col_id, 1);
        assert_eq!(block.row_count, 1000);
        assert_eq!(block.encoding, ColumnEncoding::Uncompressed);
        assert_eq!(block.null_count, 0);
        assert!(block.min_value.is_none());
        assert!(block.max_value.is_none());
        assert!(block.values.is_empty());
    }

    #[test]
    fn test_base_segment_version_creation() {
        let version = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(100),
            CommitTs(50),
            CommitTs(150),
            1000,
        );
        assert_eq!(version.segment_id, SegmentId(1));
        assert_eq!(version.version_id, VersionId(100));
        assert_eq!(version.min_commit_ts, CommitTs(50));
        assert_eq!(version.max_commit_ts, CommitTs(150));
        assert_eq!(version.created_at_ms, 1000);
        assert_eq!(version.stats.row_count, 0);
        assert!(version.columns.is_empty());
        assert!(version.row_ids.is_empty());
    }

    #[test]
    fn test_base_segment_version_contains_row() {
        let mut version = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(1),
            CommitTs(10),
            CommitTs(20),
            1000,
        );
        version.row_ids.push(RowId(100));
        version.row_ids.push(RowId(200));

        assert!(version.contains_row(RowId(100)));
        assert!(version.contains_row(RowId(200)));
        assert!(!version.contains_row(RowId(300)));
    }

    #[test]
    fn test_base_segment_manifest_creation() {
        let manifest = BaseSegmentManifest::new();
        assert!(manifest.current_version.is_none());
        assert!(manifest.history.is_empty());
    }

    #[test]
    fn test_base_segment_manifest_swap_single_version() {
        let mut manifest = BaseSegmentManifest::new();

        let v1 = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(1),
            CommitTs(10),
            CommitTs(20),
            1000,
        );
        manifest.swap_version(v1);

        assert!(manifest.current_version.is_some());
        assert_eq!(manifest.history.len(), 0);
        assert_eq!(manifest.current_version.as_ref().unwrap().version_id, VersionId(1));
    }

    #[test]
    fn test_base_segment_manifest_swap_multiple_versions() {
        let mut manifest = BaseSegmentManifest::new();

        let v1 = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(1),
            CommitTs(10),
            CommitTs(20),
            1000,
        );
        manifest.swap_version(v1);
        assert_eq!(manifest.history.len(), 0);

        let v2 = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(2),
            CommitTs(30),
            CommitTs(40),
            2000,
        );
        manifest.swap_version(v2);

        assert!(manifest.current_version.is_some());
        assert_eq!(manifest.history.len(), 1);
        assert_eq!(manifest.current_version.as_ref().unwrap().version_id, VersionId(2));
        assert_eq!(manifest.history[0].version_id, VersionId(1));
    }

    #[test]
    fn test_base_segment_manifest_atomic_visibility() {
        let mut manifest = BaseSegmentManifest::new();

        let v1 = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(1),
            CommitTs(10),
            CommitTs(20),
            1000,
        );
        manifest.swap_version(v1);

        let v2 = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(2),
            CommitTs(30),
            CommitTs(40),
            2000,
        );
        manifest.swap_version(v2);

        let v3 = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(3),
            CommitTs(50),
            CommitTs(60),
            3000,
        );
        manifest.swap_version(v3);

        // Verify old-or-new guarantee: only current version is active
        assert_eq!(manifest.current_version.as_ref().unwrap().version_id, VersionId(3));
        assert_eq!(manifest.history.len(), 2);
        // Verify history is in order
        assert_eq!(manifest.history[0].version_id, VersionId(1));
        assert_eq!(manifest.history[1].version_id, VersionId(2));
    }
}

