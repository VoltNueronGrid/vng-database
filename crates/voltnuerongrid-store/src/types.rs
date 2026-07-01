//! Type definitions for HTAP storage identifiers and timestamps.
//!
//! This module defines newtype wrappers for key storage concepts:
//! - Partition and segment identification
//! - Row and version tracking
//! - Commit and snapshot timestamp semantics

/// Partition identifier — distinguishes logical data partitions.
/// Used for partition-aware segment catalog lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartitionId(pub u32);

/// Segment identifier — uniquely identifies a logical storage segment
/// within a partition. Segments hold rows in either tail (mutable) or
/// base (immutable columnar) form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SegmentId(pub u32);

/// Row identifier — globally unique within a segment.
/// Maps to the logical row address in MVCC storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowId(pub u64);

/// Commit timestamp — transaction ID of the operation that created
/// or deleted a row version. Used for MVCC visibility checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommitTs(pub u64);

/// Snapshot timestamp — the watermark at which a read is taken.
/// All versions with `begin_ts <= snapshot_ts` may be visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SnapshotTs(pub u64);

/// Version identifier — unique identifier for each version in a row's
/// version chain. Used to track predecessor/successor relationships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VersionId(pub u64);

// Implement From/Into conversions for common patterns
impl From<u32> for PartitionId {
    fn from(v: u32) -> Self {
        PartitionId(v)
    }
}

impl From<u32> for SegmentId {
    fn from(v: u32) -> Self {
        SegmentId(v)
    }
}

impl From<u64> for RowId {
    fn from(v: u64) -> Self {
        RowId(v)
    }
}

impl From<u64> for CommitTs {
    fn from(v: u64) -> Self {
        CommitTs(v)
    }
}

impl From<u64> for SnapshotTs {
    fn from(v: u64) -> Self {
        SnapshotTs(v)
    }
}

impl From<u64> for VersionId {
    fn from(v: u64) -> Self {
        VersionId(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_derive_correctly() {
        let part_id = PartitionId(1);
        let seg_id = SegmentId(42);
        let row_id = RowId(1000);
        let commit_ts = CommitTs(100);
        let snap_ts = SnapshotTs(50);
        let ver_id = VersionId(500);

        // Test Clone
        let _ = part_id.clone();
        let _ = seg_id.clone();
        let _ = row_id.clone();
        let _ = commit_ts.clone();
        let _ = snap_ts.clone();
        let _ = ver_id.clone();

        // Test Copy (by passing to a function)
        fn copy_test(_: PartitionId) {}
        copy_test(part_id);

        // Test Eq/PartialEq
        assert_eq!(part_id, PartitionId(1));
        assert_ne!(seg_id, SegmentId(41));

        // Test Ord
        assert!(PartitionId(1) < PartitionId(2));
        assert!(RowId(100) < RowId(200));
    }

    #[test]
    fn test_from_conversions() {
        assert_eq!(PartitionId::from(5u32), PartitionId(5));
        assert_eq!(SegmentId::from(10u32), SegmentId(10));
        assert_eq!(RowId::from(1000u64), RowId(1000));
        assert_eq!(CommitTs::from(200u64), CommitTs(200));
        assert_eq!(SnapshotTs::from(150u64), SnapshotTs(150));
        assert_eq!(VersionId::from(999u64), VersionId(999));
    }

    #[test]
    fn test_hash_compatibility() {
        use std::collections::HashSet;

        let mut ids = HashSet::new();
        ids.insert(PartitionId(1));
        ids.insert(PartitionId(2));

        assert!(ids.contains(&PartitionId(1)));
        assert!(!ids.contains(&PartitionId(3)));
        
        let mut seg_ids = HashSet::new();
        seg_ids.insert(SegmentId(42));
        seg_ids.insert(SegmentId(43));
        
        assert!(seg_ids.contains(&SegmentId(42)));
        assert!(!seg_ids.contains(&SegmentId(44)));
    }
}
