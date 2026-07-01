//! H9-7: Tail Garbage Collection / Version Reclamation
//!
//! This module provides garbage collection for obsolete tail versions, enabling
//! automatic reclamation of versions that are no longer visible to any active snapshot.
//!
//! Key responsibilities:
//! - Track the oldest active snapshot timestamp to define the safe reclamation boundary
//! - Mark tail versions as obsolete after merge completion
//! - Reclaim obsolete versions only when safe (end_ts < oldest_active_snapshot_ts)
//! - Report GC metrics for observability

use crate::types::{SegmentId, CommitTs, SnapshotTs};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Metrics for tail garbage collection operations
#[derive(Debug, Clone, Copy)]
pub struct GcMetrics {
    /// Total number of versions reclaimed
    pub tail_gc_reclaimed_versions: u64,
    /// Total bytes reclaimed from tail store
    pub tail_gc_reclaimed_bytes: u64,
    /// Age of the oldest active snapshot in milliseconds
    pub oldest_snapshot_age_ms: u64,
    /// Number of tail records marked as obsolete but not yet reclaimed
    pub tail_records_obsolete: u64,
    /// Number of tail records that are eligible for reclamation
    pub tail_records_reclaimable: u64,
}

/// Collector for managing tail version garbage collection
///
/// TailGcCollector tracks obsolete versions and reclaims them when they are
/// no longer visible to any active snapshot. It enforces the invariant that
/// a version can only be reclaimed if its end_ts is strictly less than the
/// oldest active snapshot's timestamp.
#[derive(Debug)]
pub struct TailGcCollector {
    /// Oldest active snapshot timestamp — versions with end_ts < this can be reclaimed
    oldest_active_snapshot_ts: Arc<Mutex<SnapshotTs>>,
    /// Total number of versions reclaimed
    reclaimed_versions: Arc<AtomicU64>,
    /// Total bytes reclaimed
    reclaimed_bytes: Arc<AtomicU64>,
    /// Number of tail records marked as obsolete
    tail_records_marked_obsolete: Arc<AtomicU64>,
    /// Map of segment_id -> set of obsolete version indices (for tracking)
    obsolete_versions_map: Arc<Mutex<HashMap<SegmentId, Vec<u64>>>>,
}

impl TailGcCollector {
    /// Create a new TailGcCollector with initial snapshot timestamp.
    ///
    /// # Arguments
    /// * `initial_snapshot_ts` - The initial oldest active snapshot timestamp
    ///
    /// # Returns
    /// A new TailGcCollector instance
    pub fn new(initial_snapshot_ts: SnapshotTs) -> Self {
        TailGcCollector {
            oldest_active_snapshot_ts: Arc::new(Mutex::new(initial_snapshot_ts)),
            reclaimed_versions: Arc::new(AtomicU64::new(0)),
            reclaimed_bytes: Arc::new(AtomicU64::new(0)),
            tail_records_marked_obsolete: Arc::new(AtomicU64::new(0)),
            obsolete_versions_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Update the oldest active snapshot timestamp.
    ///
    /// Called by the SnapshotManager when snapshots are created or released.
    /// This updates the safe reclamation boundary — only versions with end_ts
    /// strictly less than this timestamp can be reclaimed.
    ///
    /// # Arguments
    /// * `snapshot_ts` - The new oldest active snapshot timestamp
    pub fn update_oldest_active_snapshot(&self, snapshot_ts: SnapshotTs) {
        if let Ok(mut oldest) = self.oldest_active_snapshot_ts.lock() {
            // Only update if the new timestamp is older (smaller value)
            if snapshot_ts.0 < oldest.0 {
                *oldest = snapshot_ts;
            }
        }
    }

    /// Mark tail records as obsolete after a merge completes.
    ///
    /// Called by MergeManager after successfully merging versions into the base store.
    /// Versions are not immediately reclaimed; they are marked and later reclaimed
    /// when safe to do so.
    ///
    /// # Arguments
    /// * `segment_id` - The segment containing the obsolete versions
    /// * `version_indices` - List of version indices that are now obsolete
    pub fn mark_tail_records_obsolete(&self, segment_id: SegmentId, version_indices: Vec<u64>) {
        let count = version_indices.len() as u64;
        self.tail_records_marked_obsolete.fetch_add(count, Ordering::Release);

        if let Ok(mut map) = self.obsolete_versions_map.lock() {
            map.entry(segment_id)
                .or_insert_with(Vec::new)
                .extend(version_indices);
        }
    }

    /// Attempt to reclaim obsolete versions for a segment.
    ///
    /// Versions are only reclaimed if their end_ts is strictly less than
    /// the oldest active snapshot timestamp. This ensures that no active
    /// snapshot can see a reclaimed version.
    ///
    /// # Arguments
    /// * `segment_id` - The segment to reclaim versions from
    /// * `version_end_timestamps` - Map of version_index -> end_ts
    ///
    /// # Returns
    /// A tuple of (version_count_reclaimed, bytes_reclaimed)
    pub fn reclaim_obsolete_versions(
        &self,
        segment_id: SegmentId,
        version_end_timestamps: &HashMap<u64, (CommitTs, u64)>, // version_idx -> (end_ts, byte_size)
    ) -> (u64, u64) {
        let oldest_snapshot = self
            .oldest_active_snapshot_ts
            .lock()
            .map(|ts| ts.0)
            .unwrap_or(u64::MAX);

        let mut reclaimable_count = 0;
        let mut reclaimable_bytes = 0;

        if let Ok(mut map) = self.obsolete_versions_map.lock() {
            if let Some(obsolete) = map.get_mut(&segment_id) {
                obsolete.retain(|version_idx| {
                    if let Some(&(end_ts, byte_size)) = version_end_timestamps.get(version_idx) {
                        if end_ts.0 < oldest_snapshot {
                            // Safe to reclaim
                            reclaimable_count += 1;
                            reclaimable_bytes += byte_size;
                            false // Remove from obsolete list
                        } else {
                            // Not yet safe to reclaim
                            true // Keep in obsolete list
                        }
                    } else {
                        // Version not in end_timestamps map, remove from tracking
                        false
                    }
                });

                // Clean up empty entries
                if obsolete.is_empty() {
                    map.remove(&segment_id);
                }
            }
        }

        // Update metrics with acquire/release semantics
        self.reclaimed_versions
            .fetch_add(reclaimable_count, Ordering::Release);
        self.reclaimed_bytes
            .fetch_add(reclaimable_bytes, Ordering::Release);
        self.tail_records_marked_obsolete
            .fetch_sub(reclaimable_count, Ordering::Release);

        (reclaimable_count, reclaimable_bytes)
    }

    /// Check if a version can be safely reclaimed.
    ///
    /// Returns true if the version's end_ts is strictly less than the
    /// oldest active snapshot timestamp.
    ///
    /// # Arguments
    /// * `end_ts` - The end timestamp of the version
    ///
    /// # Returns
    /// true if the version is safe to reclaim, false otherwise
    pub fn can_reclaim_version(&self, end_ts: CommitTs) -> bool {
        self.oldest_active_snapshot_ts
            .lock()
            .map(|oldest| end_ts.0 < oldest.0)
            .unwrap_or(false)
    }

    /// Get current garbage collection metrics.
    ///
    /// # Returns
    /// A GcMetrics struct containing current GC statistics
    pub fn get_metrics(&self) -> GcMetrics {
        let obsolete_count = self.obsolete_versions_map
            .lock()
            .map(|map| map.values().map(|v| v.len() as u64).sum::<u64>())
            .unwrap_or(0);

        GcMetrics {
            tail_gc_reclaimed_versions: self.reclaimed_versions.load(Ordering::Acquire),
            tail_gc_reclaimed_bytes: self.reclaimed_bytes.load(Ordering::Acquire),
            oldest_snapshot_age_ms: 0, // Placeholder for actual age calculation
            tail_records_obsolete: self.tail_records_marked_obsolete.load(Ordering::Acquire),
            tail_records_reclaimable: obsolete_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tail_gc_collector_creation() {
        let initial_ts = SnapshotTs(1000);
        let gc = TailGcCollector::new(initial_ts);

        let metrics = gc.get_metrics();
        assert_eq!(metrics.tail_gc_reclaimed_versions, 0);
        assert_eq!(metrics.tail_gc_reclaimed_bytes, 0);
        assert_eq!(metrics.tail_records_obsolete, 0);
        assert_eq!(metrics.tail_records_reclaimable, 0);
    }

    #[test]
    fn test_tail_gc_mark_obsolete() {
        let gc = TailGcCollector::new(SnapshotTs(1000));
        let segment_id = SegmentId(1);

        gc.mark_tail_records_obsolete(segment_id, vec![0, 1, 2]);

        let metrics = gc.get_metrics();
        assert_eq!(metrics.tail_records_obsolete, 3);
        assert_eq!(metrics.tail_records_reclaimable, 3);
    }

    #[test]
    fn test_tail_gc_reclaim_safe_after_oldest_snapshot() {
        let gc = TailGcCollector::new(SnapshotTs(1000));
        let segment_id = SegmentId(1);

        // Mark versions as obsolete
        gc.mark_tail_records_obsolete(segment_id, vec![0, 1, 2]);

        // Create version end timestamps
        let mut end_timestamps = HashMap::new();
        end_timestamps.insert(0u64, (CommitTs(500), 1024u64));  // Safe to reclaim
        end_timestamps.insert(1u64, (CommitTs(800), 1024u64));  // Safe to reclaim
        end_timestamps.insert(2u64, (CommitTs(999), 1024u64));  // Safe to reclaim

        // Reclaim with oldest_snapshot at 1000
        let (count, bytes) = gc.reclaim_obsolete_versions(segment_id, &end_timestamps);

        assert_eq!(count, 3);
        assert_eq!(bytes, 3 * 1024);

        let metrics = gc.get_metrics();
        assert_eq!(metrics.tail_gc_reclaimed_versions, 3);
        assert_eq!(metrics.tail_gc_reclaimed_bytes, 3 * 1024);
        assert_eq!(metrics.tail_records_obsolete, 0);
    }

    #[test]
    fn test_tail_gc_never_reclaims_active_snapshot_versions() {
        let gc = TailGcCollector::new(SnapshotTs(1000));
        let segment_id = SegmentId(1);

        // Mark versions as obsolete
        gc.mark_tail_records_obsolete(segment_id, vec![0, 1, 2]);

        // Create version end timestamps where some are >= oldest_snapshot
        let mut end_timestamps = HashMap::new();
        end_timestamps.insert(0u64, (CommitTs(500), 1024u64));   // Safe to reclaim
        end_timestamps.insert(1u64, (CommitTs(1000), 1024u64));  // NOT safe (== oldest)
        end_timestamps.insert(2u64, (CommitTs(1500), 1024u64));  // NOT safe (> oldest)

        // Attempt to reclaim
        let (count, bytes) = gc.reclaim_obsolete_versions(segment_id, &end_timestamps);

        // Only the first version should be reclaimed
        assert_eq!(count, 1);
        assert_eq!(bytes, 1024);

        let metrics = gc.get_metrics();
        assert_eq!(metrics.tail_gc_reclaimed_versions, 1);
        assert_eq!(metrics.tail_records_obsolete, 2);
        assert_eq!(metrics.tail_records_reclaimable, 2);
    }

    #[test]
    fn test_tail_gc_updates_oldest_active_snapshot() {
        let gc = TailGcCollector::new(SnapshotTs(2000));

        // Update to older snapshot
        gc.update_oldest_active_snapshot(SnapshotTs(1000));
        assert!(gc.can_reclaim_version(CommitTs(999)));
        assert!(!gc.can_reclaim_version(CommitTs(1000)));

        // Try to update to newer snapshot (should not change)
        gc.update_oldest_active_snapshot(SnapshotTs(3000));
        assert!(gc.can_reclaim_version(CommitTs(999)));
        assert!(!gc.can_reclaim_version(CommitTs(1000)));
    }

    #[test]
    fn test_tail_gc_metrics_tracking() {
        let gc = TailGcCollector::new(SnapshotTs(1000));
        let segment_id = SegmentId(1);

        // Mark multiple batches of obsolete versions
        gc.mark_tail_records_obsolete(segment_id, vec![0, 1]);
        gc.mark_tail_records_obsolete(segment_id, vec![2, 3, 4]);

        let metrics = gc.get_metrics();
        assert_eq!(metrics.tail_records_obsolete, 5);
        assert_eq!(metrics.tail_records_reclaimable, 5);

        // Reclaim some
        let mut end_timestamps = HashMap::new();
        end_timestamps.insert(0u64, (CommitTs(500), 512u64));
        end_timestamps.insert(1u64, (CommitTs(600), 512u64));
        end_timestamps.insert(2u64, (CommitTs(800), 512u64));
        end_timestamps.insert(3u64, (CommitTs(900), 512u64));
        end_timestamps.insert(4u64, (CommitTs(950), 512u64));

        gc.reclaim_obsolete_versions(segment_id, &end_timestamps);

        let metrics = gc.get_metrics();
        assert_eq!(metrics.tail_gc_reclaimed_versions, 5);
        assert_eq!(metrics.tail_gc_reclaimed_bytes, 5 * 512);
        assert_eq!(metrics.tail_records_obsolete, 0);
        assert_eq!(metrics.tail_records_reclaimable, 0);
    }

    #[test]
    fn test_tail_gc_reclaimed_bytes_calculation() {
        let gc = TailGcCollector::new(SnapshotTs(1000));
        let segment_id = SegmentId(1);

        gc.mark_tail_records_obsolete(segment_id, vec![0, 1, 2]);

        let mut end_timestamps = HashMap::new();
        end_timestamps.insert(0u64, (CommitTs(100), 1024u64));
        end_timestamps.insert(1u64, (CommitTs(200), 2048u64));
        end_timestamps.insert(2u64, (CommitTs(300), 4096u64));

        let (count, bytes) = gc.reclaim_obsolete_versions(segment_id, &end_timestamps);

        assert_eq!(count, 3);
        assert_eq!(bytes, 1024 + 2048 + 4096);

        let metrics = gc.get_metrics();
        assert_eq!(metrics.tail_gc_reclaimed_bytes, 7168);
    }

    #[test]
    fn test_tail_gc_concurrent_snapshot_updates() {
        let gc = Arc::new(TailGcCollector::new(SnapshotTs(1000)));
        let mut handles = vec![];

        // Spawn multiple threads updating oldest snapshot
        for i in 0..5 {
            let gc_clone = Arc::clone(&gc);
            let handle = std::thread::spawn(move || {
                let new_ts = SnapshotTs((i + 1) * 100);
                gc_clone.update_oldest_active_snapshot(new_ts);
            });
            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // The oldest should be the minimum
        assert!(gc.can_reclaim_version(CommitTs(99)));
        assert!(!gc.can_reclaim_version(CommitTs(100)));
    }

    #[test]
    fn test_tail_gc_multiple_segments() {
        let gc = TailGcCollector::new(SnapshotTs(1000));

        let seg1 = SegmentId(1);
        let seg2 = SegmentId(2);

        gc.mark_tail_records_obsolete(seg1, vec![0, 1]);
        gc.mark_tail_records_obsolete(seg2, vec![0, 1, 2]);

        let metrics = gc.get_metrics();
        assert_eq!(metrics.tail_records_obsolete, 5);
        assert_eq!(metrics.tail_records_reclaimable, 5);

        // Reclaim from segment 1
        let mut end_timestamps1 = HashMap::new();
        end_timestamps1.insert(0u64, (CommitTs(100), 512u64));
        end_timestamps1.insert(1u64, (CommitTs(200), 512u64));

        let (count1, bytes1) = gc.reclaim_obsolete_versions(seg1, &end_timestamps1);
        assert_eq!(count1, 2);
        assert_eq!(bytes1, 1024);

        // Reclaim from segment 2
        let mut end_timestamps2 = HashMap::new();
        end_timestamps2.insert(0u64, (CommitTs(300), 256u64));
        end_timestamps2.insert(1u64, (CommitTs(400), 256u64));
        end_timestamps2.insert(2u64, (CommitTs(500), 256u64));

        let (count2, bytes2) = gc.reclaim_obsolete_versions(seg2, &end_timestamps2);
        assert_eq!(count2, 3);
        assert_eq!(bytes2, 768);

        let metrics = gc.get_metrics();
        assert_eq!(metrics.tail_gc_reclaimed_versions, 5);
        assert_eq!(metrics.tail_gc_reclaimed_bytes, 1024 + 768);
        assert_eq!(metrics.tail_records_obsolete, 0);
    }

    #[test]
    fn test_tail_gc_respects_snapshot_age_threshold() {
        let gc = TailGcCollector::new(SnapshotTs(100));
        let segment_id = SegmentId(1);

        gc.mark_tail_records_obsolete(segment_id, vec![0, 1, 2, 3, 4]);

        let mut end_timestamps = HashMap::new();
        end_timestamps.insert(0u64, (CommitTs(50), 1024u64));
        end_timestamps.insert(1u64, (CommitTs(60), 1024u64));
        end_timestamps.insert(2u64, (CommitTs(70), 1024u64));
        end_timestamps.insert(3u64, (CommitTs(80), 1024u64));
        end_timestamps.insert(4u64, (CommitTs(99), 1024u64));

        // All should be reclaimable since end_ts < 100
        let (count, bytes) = gc.reclaim_obsolete_versions(segment_id, &end_timestamps);
        assert_eq!(count, 5);
        assert_eq!(bytes, 5 * 1024);

        // Add more obsolete versions
        gc.mark_tail_records_obsolete(segment_id, vec![5, 6]);

        let mut new_end_timestamps = HashMap::new();
        new_end_timestamps.insert(5u64, (CommitTs(100), 1024u64)); // Not reclaimable
        new_end_timestamps.insert(6u64, (CommitTs(150), 1024u64)); // Not reclaimable

        let (count2, bytes2) = gc.reclaim_obsolete_versions(segment_id, &new_end_timestamps);
        assert_eq!(count2, 0);
        assert_eq!(bytes2, 0);

        let metrics = gc.get_metrics();
        assert_eq!(metrics.tail_records_obsolete, 2);
    }

    #[test]
    fn test_tail_gc_can_reclaim_version() {
        let gc = TailGcCollector::new(SnapshotTs(1000));

        // Should be able to reclaim versions before oldest snapshot
        assert!(gc.can_reclaim_version(CommitTs(999)));
        assert!(gc.can_reclaim_version(CommitTs(500)));

        // Should not reclaim at or after oldest snapshot
        assert!(!gc.can_reclaim_version(CommitTs(1000)));
        assert!(!gc.can_reclaim_version(CommitTs(1001)));

        // Update oldest snapshot
        gc.update_oldest_active_snapshot(SnapshotTs(800));

        // Now should only reclaim before 800
        assert!(gc.can_reclaim_version(CommitTs(799)));
        assert!(!gc.can_reclaim_version(CommitTs(800)));
        assert!(!gc.can_reclaim_version(CommitTs(1000)));
    }
}
