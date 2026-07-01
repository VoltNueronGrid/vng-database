//! H9-8: Snapshot Manager with lifecycle and freshness selection
//!
//! This module provides explicit snapshot object lifecycle management with:
//! - Reference counting to track active snapshot handles
//! - Freshness selection to reuse snapshots if max_staleness_ms is satisfied
//! - RAII guards for automatic cleanup
//! - Lifecycle metrics for observability

use crate::types::{SnapshotTs, VersionId, SegmentId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Unique identifier for a snapshot
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SnapshotId(pub u64);

/// Pinned base versions for a snapshot (segment_id -> base_version_id)
pub type PinnedBaseVersions = HashMap<SegmentId, VersionId>;

/// Handle to a logical snapshot with reference counting and freshness metadata
#[derive(Debug, Clone)]
pub struct SnapshotHandle {
    /// Unique snapshot identifier
    pub snapshot_id: SnapshotId,
    /// The timestamp this snapshot was taken at
    pub snapshot_ts: SnapshotTs,
    /// Pinned base segment versions to prevent GC
    pub pinned_base_versions: Arc<PinnedBaseVersions>,
    /// Reference count for this snapshot
    pub ref_count: Arc<AtomicU64>,
    /// Timestamp when this snapshot was created (ms since epoch)
    pub created_at_ms: u64,
}

impl SnapshotHandle {
    /// Create a new snapshot handle with initial ref_count=1
    pub fn new(
        snapshot_id: SnapshotId,
        snapshot_ts: SnapshotTs,
        pinned_base_versions: PinnedBaseVersions,
    ) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        SnapshotHandle {
            snapshot_id,
            snapshot_ts,
            pinned_base_versions: Arc::new(pinned_base_versions),
            ref_count: Arc::new(AtomicU64::new(1)),
            created_at_ms: now_ms,
        }
    }

    /// Get current reference count
    pub fn get_ref_count(&self) -> u64 {
        self.ref_count.load(Ordering::Acquire)
    }

    /// Increment reference count (acquire)
    pub fn acquire(&self) {
        self.ref_count.fetch_add(1, Ordering::Release);
    }

    /// Decrement reference count (release) and return previous count
    pub fn release(&self) -> u64 {
        self.ref_count.fetch_sub(1, Ordering::Release)
    }

    /// Age of this snapshot in milliseconds
    pub fn age_ms(&self) -> u64 {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        now_ms.saturating_sub(self.created_at_ms)
    }

    /// Check if this snapshot satisfies a freshness requirement
    pub fn satisfies_max_staleness_ms(&self, max_staleness_ms: Option<u64>) -> bool {
        if let Some(max_staleness) = max_staleness_ms {
            self.age_ms() <= max_staleness
        } else {
            true
        }
    }
}

/// Snapshot lifecycle metrics
#[derive(Debug, Clone, Default)]
pub struct SnapshotMetrics {
    /// Number of active snapshots
    pub active_count: u64,
    /// Total snapshots created
    pub total_created: u64,
    /// Sum of all reference counts
    pub total_ref_count: u64,
    /// Oldest active snapshot age (ms)
    pub oldest_age_ms: u64,
    /// Youngest active snapshot age (ms)
    pub youngest_age_ms: u64,
}

/// Request envelope with optional freshness requirement
#[derive(Debug, Clone)]
pub struct SnapshotRequest {
    /// Maximum allowed staleness (milliseconds)
    pub max_staleness_ms: Option<u64>,
    /// Query ID for tracing
    pub query_id: Option<String>,
}

impl SnapshotRequest {
    /// Create a new snapshot request with optional freshness bound
    pub fn new(max_staleness_ms: Option<u64>) -> Self {
        SnapshotRequest {
            max_staleness_ms,
            query_id: None,
        }
    }

    /// Add a query ID for tracing
    pub fn with_query_id(mut self, query_id: String) -> Self {
        self.query_id = Some(query_id);
        self
    }
}

/// Manages snapshot lifecycle: creation, reuse, release, expiration
pub struct SnapshotManager {
    /// All active snapshots (snapshot_id -> handle)
    active_snapshots: Arc<std::sync::Mutex<HashMap<SnapshotId, Arc<SnapshotHandle>>>>,
    /// Reusable snapshots (handles for reuse if freshness OK)
    reusable_cache: Arc<std::sync::Mutex<Vec<Arc<SnapshotHandle>>>>,
    /// Next snapshot ID counter
    next_id: Arc<AtomicU64>,
    /// Maximum reusable cache size
    max_cache_size: usize,
    /// Snapshot retention time (ms) before cleanup
    retention_ms: u64,
}

impl SnapshotManager {
    /// Create a new snapshot manager
    pub fn new(max_cache_size: usize, retention_ms: u64) -> Self {
        SnapshotManager {
            active_snapshots: Arc::new(std::sync::Mutex::new(HashMap::new())),
            reusable_cache: Arc::new(std::sync::Mutex::new(Vec::new())),
            next_id: Arc::new(AtomicU64::new(1)),
            max_cache_size,
            retention_ms,
        }
    }

    /// Create a new snapshot or reuse existing one if freshness allows
    pub fn create_or_reuse(
        &self,
        request: &SnapshotRequest,
        pinned_base_versions: PinnedBaseVersions,
    ) -> Result<Arc<SnapshotHandle>, String> {
        let max_staleness = request.max_staleness_ms;

        // Try to reuse a snapshot if freshness allows
        if let Ok(cache) = self.reusable_cache.lock() {
            for cached in cache.iter() {
                if cached.satisfies_max_staleness_ms(max_staleness) {
                    let handle = cached.clone();
                    handle.acquire();

                    // Update pinned versions if needed
                    if !pinned_base_versions.is_empty() {
                        // In a real implementation, merge pinned versions
                    }

                    return Ok(handle);
                }
            }
        }

        // Create new snapshot
        let snapshot_id = SnapshotId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let snapshot_ts = SnapshotTs(snapshot_id.0);

        let handle = Arc::new(SnapshotHandle::new(
            snapshot_id,
            snapshot_ts,
            pinned_base_versions,
        ));

        if let Ok(mut active) = self.active_snapshots.lock() {
            active.insert(snapshot_id, handle.clone());
        }

        Ok(handle)
    }

    /// Release a snapshot (decrement ref count, may move to reusable cache)
    pub fn release(&self, handle: &Arc<SnapshotHandle>) -> Result<(), String> {
        let count = handle.release();

        if count == 1 {
            // Last reference released, move to reusable cache
            if let Ok(mut cache) = self.reusable_cache.lock() {
                if cache.len() < self.max_cache_size {
                    cache.push(handle.clone());
                }
            }

            if let Ok(mut active) = self.active_snapshots.lock() {
                active.remove(&handle.snapshot_id);
            }
        }

        Ok(())
    }

    /// Get snapshot by ID
    pub fn get(&self, snapshot_id: SnapshotId) -> Result<Option<Arc<SnapshotHandle>>, String> {
        self.active_snapshots
            .lock()
            .map_err(|e| e.to_string())
            .map(|active| active.get(&snapshot_id).cloned())
    }

    /// Expire old snapshots from the reusable cache
    pub fn cleanup_expired(&self) -> Result<usize, String> {
        let mut cache = self
            .reusable_cache
            .lock()
            .map_err(|e| e.to_string())?;
        let before = cache.len();

        cache.retain(|handle| {
            let age = handle.age_ms();
            age < self.retention_ms
        });

        Ok(before - cache.len())
    }

    /// Get metrics for all snapshots
    pub fn get_metrics(&self) -> Result<SnapshotMetrics, String> {
        let active = self
            .active_snapshots
            .lock()
            .map_err(|e| e.to_string())?;
        let cache = self.reusable_cache.lock().map_err(|e| e.to_string())?;

        let active_count = active.len() + cache.len();
        let mut total_ref_count = 0u64;
        let mut oldest_age_ms = 0u64;
        let mut youngest_age_ms = u64::MAX;

        for handle in active.values() {
            total_ref_count += handle.get_ref_count();
            let age = handle.age_ms();
            oldest_age_ms = oldest_age_ms.max(age);
            youngest_age_ms = youngest_age_ms.min(age);
        }

        for handle in cache.iter() {
            let age = handle.age_ms();
            oldest_age_ms = oldest_age_ms.max(age);
            youngest_age_ms = youngest_age_ms.min(age);
        }

        if youngest_age_ms == u64::MAX {
            youngest_age_ms = 0;
        }

        Ok(SnapshotMetrics {
            active_count: active_count as u64,
            total_created: self.next_id.load(Ordering::SeqCst),
            total_ref_count,
            oldest_age_ms,
            youngest_age_ms,
        })
    }

    /// Get all active snapshots
    pub fn list_active(&self) -> Result<Vec<Arc<SnapshotHandle>>, String> {
        let active = self
            .active_snapshots
            .lock()
            .map_err(|e| e.to_string())?;
        Ok(active.values().cloned().collect())
    }
}

impl Default for SnapshotManager {
    fn default() -> Self {
        SnapshotManager::new(100, 60_000) // 100 cache, 60s retention
    }
}

/// RAII guard for snapshot lifecycle management
/// Automatically releases the snapshot when dropped
pub struct SnapshotGuard {
    manager: Arc<SnapshotManager>,
    handle: Arc<SnapshotHandle>,
}

impl SnapshotGuard {
    /// Create a new snapshot guard
    pub fn new(manager: Arc<SnapshotManager>, handle: Arc<SnapshotHandle>) -> Self {
        SnapshotGuard { manager, handle }
    }

    /// Get a reference to the snapshot handle
    pub fn handle(&self) -> &Arc<SnapshotHandle> {
        &self.handle
    }
}

impl Drop for SnapshotGuard {
    fn drop(&mut self) {
        let _ = self.manager.release(&self.handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_handle_creation() {
        let pinned = HashMap::new();
        let handle = SnapshotHandle::new(SnapshotId(1), SnapshotTs(100), pinned);
        assert_eq!(handle.snapshot_id, SnapshotId(1));
        assert_eq!(handle.snapshot_ts, SnapshotTs(100));
        assert_eq!(handle.get_ref_count(), 1);
    }

    #[test]
    fn test_snapshot_ref_counting() {
        let pinned = HashMap::new();
        let handle = Arc::new(SnapshotHandle::new(SnapshotId(1), SnapshotTs(100), pinned));

        assert_eq!(handle.get_ref_count(), 1);
        handle.acquire();
        assert_eq!(handle.get_ref_count(), 2);
        handle.release();
        assert_eq!(handle.get_ref_count(), 1);
    }

    #[test]
    fn test_snapshot_manager_create() {
        let mgr = SnapshotManager::new(10, 60_000);
        let req = SnapshotRequest::new(None);
        let pinned = HashMap::new();

        let handle = mgr.create_or_reuse(&req, pinned).unwrap();
        assert_eq!(handle.get_ref_count(), 1);
    }

    #[test]
    fn test_snapshot_manager_reuse() {
        let mgr = SnapshotManager::new(10, 60_000);
        let req = SnapshotRequest::new(Some(10000)); // 10 sec freshness

        let pinned1 = HashMap::new();
        let h1 = mgr.create_or_reuse(&req, pinned1).unwrap();
        
        // Release the first snapshot to make it reusable
        mgr.release(&h1).unwrap();

        let pinned2 = HashMap::new();
        let h2 = mgr.create_or_reuse(&req, pinned2).unwrap();

        // Same snapshot should be reused
        assert_eq!(h1.snapshot_id, h2.snapshot_id);
    }

    #[test]
    fn test_snapshot_manager_release() {
        let mgr = SnapshotManager::new(10, 60_000);
        let req = SnapshotRequest::new(None);
        let pinned = HashMap::new();

        let handle = mgr.create_or_reuse(&req, pinned).unwrap();
        
        // Before release, snapshot is active
        let metrics_before = mgr.get_metrics().unwrap();
        assert_eq!(metrics_before.active_count, 1);
        
        // Release the snapshot - it moves to reusable cache
        mgr.release(&handle).unwrap();

        // After release, it's in reusable cache (still counted as active)
        let metrics_after = mgr.get_metrics().unwrap();
        assert_eq!(metrics_after.active_count, 1); // Now in reusable cache
    }

    #[test]
    fn test_snapshot_freshness_check() {
        let pinned = HashMap::new();
        let handle = SnapshotHandle::new(SnapshotId(1), SnapshotTs(100), pinned);

        // Should satisfy any freshness requirement initially
        assert!(handle.satisfies_max_staleness_ms(None));
        assert!(handle.satisfies_max_staleness_ms(Some(10000)));
    }

    #[test]
    fn test_snapshot_guard_drop() {
        let mgr = Arc::new(SnapshotManager::new(10, 60_000));
        let req = SnapshotRequest::new(None);
        let pinned = HashMap::new();

        let handle = mgr.create_or_reuse(&req, pinned).unwrap();
        {
            let _guard = SnapshotGuard::new(mgr.clone(), handle);
            // Guard keeps reference
        }
        // Guard dropped, snapshot released
    }

    #[test]
    fn test_snapshot_manager_metrics() {
        let mgr = SnapshotManager::new(10, 60_000);
        let req = SnapshotRequest::new(None);

        let pinned = HashMap::new();
        let _h1 = mgr.create_or_reuse(&req, pinned).unwrap();

        let metrics = mgr.get_metrics().unwrap();
        assert_eq!(metrics.active_count, 1);
        assert!(metrics.total_created >= 1);
    }

    #[test]
    fn test_snapshot_request_with_query_id() {
        let req = SnapshotRequest::new(Some(5000)).with_query_id("query-123".to_string());
        assert_eq!(req.max_staleness_ms, Some(5000));
        assert_eq!(req.query_id, Some("query-123".to_string()));
    }

    #[test]
    fn test_snapshot_list_active() {
        let mgr = SnapshotManager::new(10, 60_000);
        let req = SnapshotRequest::new(None);

        let pinned1 = HashMap::new();
        let _h1 = mgr.create_or_reuse(&req, pinned1).unwrap();

        let pinned2 = HashMap::new();
        let req2 = SnapshotRequest::new(Some(1)); // very fresh
        let _h2 = mgr.create_or_reuse(&req2, pinned2).unwrap();

        let active = mgr.list_active().unwrap();
        assert!(active.len() >= 1); // At least one active snapshot
    }
}
