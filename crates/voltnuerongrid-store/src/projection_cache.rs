//! Row-Projection Cache for Hot Segments
//!
//! H9-5 implements an efficient caching layer for frequently-accessed column
//! projections (subsets of columns). This module provides:
//!
//! - `RowProjectionCache`: Cache structure tracking hot segments and their projections
//! - `ProjectionCacheMetrics`: Performance telemetry (hits, misses, rebuilds, memory)
//! - Lazy rebuild on access threshold to avoid stale data
//! - Invalidation on base segment swap (full snapshot replacement)
//!
//! The cache is optional and zero-cost when unused (no allocations if not instantiated).

use crate::types::{SegmentId, RowId};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Metrics for row projection cache performance monitoring.
///
/// Tracks hits, misses, rebuilds, and memory footprint to assess cache effectiveness.
#[derive(Debug, Clone, Copy)]
pub struct ProjectionCacheMetrics {
    /// Number of successful cache lookups (projection found).
    pub cache_hits: u64,
    /// Number of failed cache lookups (projection not found).
    pub cache_misses: u64,
    /// Number of times the cache was completely rebuilt.
    pub cache_rebuilds: u64,
    /// Number of times individual rows were invalidated.
    pub cache_invalidations: u64,
    /// Approximate memory footprint of cached projections (bytes).
    pub memory_footprint_bytes: u64,
}

impl ProjectionCacheMetrics {
    /// Create a new zero-initialized metrics struct.
    pub fn new() -> Self {
        Self {
            cache_hits: 0,
            cache_misses: 0,
            cache_rebuilds: 0,
            cache_invalidations: 0,
            memory_footprint_bytes: 0,
        }
    }

    /// Calculate hit ratio as a percentage (0.0 to 100.0).
    pub fn hit_ratio(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            (self.cache_hits as f64 / total as f64) * 100.0
        }
    }
}

impl Default for ProjectionCacheMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Row-Projection Cache for hot segments.
///
/// Caches frequently-accessed column projections to avoid repeated scans or serialization.
/// The cache rebuilds lazily when access count exceeds the threshold, ensuring warm data
/// is always available while avoiding stale projections.
#[derive(Debug)]
pub struct RowProjectionCache {
    /// Segment this cache belongs to.
    segment_id: SegmentId,
    /// Maps row ID to cached projection data (or None if row is deleted).
    cache: HashMap<RowId, Option<Vec<u8>>>,
    /// Counts accesses to the segment; triggers rebuild at threshold.
    hot_access_count: usize,
    /// UNIX timestamp when the cache was last built.
    built_at_ts: u64,
    /// Performance metrics.
    metrics: ProjectionCacheMetrics,
    /// Default threshold for triggering a rebuild (configurable in tests).
    rebuild_threshold: usize,
}

impl RowProjectionCache {
    /// Create a new projection cache for a segment.
    ///
    /// The cache starts empty with 0 accesses. It will rebuild after the default
    /// threshold (100 accesses) is reached.
    pub fn new(segment_id: SegmentId) -> Self {
        Self {
            segment_id,
            cache: HashMap::new(),
            hot_access_count: 0,
            built_at_ts: current_timestamp(),
            metrics: ProjectionCacheMetrics::new(),
            rebuild_threshold: 100,
        }
    }

    /// Create a new projection cache with a custom rebuild threshold.
    ///
    /// Useful for testing or tuning cache behavior.
    pub fn with_threshold(segment_id: SegmentId, threshold: usize) -> Self {
        let mut cache = Self::new(segment_id);
        cache.rebuild_threshold = threshold;
        cache
    }

    /// Retrieve a cached projection for a row, or None if not cached or deleted.
    ///
    /// Increments access count and metrics. Returns the cached projection data
    /// if available.
    pub fn get(&mut self, row_id: RowId) -> Option<Vec<u8>> {
        self.hot_access_count += 1;
        match self.cache.get(&row_id) {
            Some(Some(projection)) => {
                self.metrics.cache_hits += 1;
                Some(projection.clone())
            }
            Some(None) => {
                self.metrics.cache_hits += 1;
                None
            }
            None => {
                self.metrics.cache_misses += 1;
                None
            }
        }
    }

    /// Cache a row projection (or None to mark row as deleted).
    ///
    /// Stores the serialized projection data for this row. Passing None indicates
    /// the row has been deleted and won't return data on future `get()` calls.
    pub fn put(&mut self, row_id: RowId, projection: Option<Vec<u8>>) {
        if let Some(ref p) = projection {
            self.metrics.memory_footprint_bytes = self.metrics
                .memory_footprint_bytes
                .saturating_add(p.len() as u64);
        }
        self.cache.insert(row_id, projection);
    }

    /// Invalidate all cached projections.
    ///
    /// Clears the entire cache without resetting access counters. Used when
    /// invalidating after a base segment swap or compaction.
    pub fn invalidate(&mut self) {
        self.cache.clear();
        self.metrics.cache_invalidations += 1;
        self.metrics.memory_footprint_bytes = 0;
    }

    /// Invalidate the projection for a single row.
    ///
    /// Removes the cached projection for this row after an update or delete.
    pub fn invalidate_row(&mut self, row_id: RowId) {
        if self.cache.remove(&row_id).is_some() {
            self.metrics.cache_invalidations += 1;
        }
    }

    /// Check if a rebuild should occur and reset counters if so.
    ///
    /// Returns true if the access count exceeded the rebuild threshold.
    /// When this returns true, the caller should rebuild the cache from scratch.
    pub fn should_rebuild(&mut self) -> bool {
        if self.hot_access_count >= self.rebuild_threshold {
            self.hot_access_count = 0;
            self.metrics.cache_rebuilds += 1;
            true
        } else {
            false
        }
    }

    /// Perform a rebuild if the hot-segment threshold is exceeded.
    ///
    /// Clears the cache and resets the access counter. The caller is responsible
    /// for repopulating the cache with fresh data.
    pub fn rebuild_if_hot(&mut self) {
        if self.should_rebuild() {
            self.invalidate();
            self.built_at_ts = current_timestamp();
        }
    }

    /// Mark cache for rebuild after a base segment swap (full snapshot replacement).
    ///
    /// Clears all cached data since the underlying segment has been replaced.
    /// Called when a new BaseSegmentManifest is installed.
    pub fn on_base_segment_swap(&mut self) {
        self.invalidate();
        self.hot_access_count = 0;
        self.built_at_ts = current_timestamp();
    }

    /// Retrieve the current cache metrics.
    pub fn get_metrics(&self) -> ProjectionCacheMetrics {
        self.metrics
    }

    /// Retrieve the segment ID this cache is associated with.
    pub fn segment_id(&self) -> SegmentId {
        self.segment_id
    }

    /// Retrieve the build timestamp (UNIX epoch seconds).
    pub fn built_at_ts(&self) -> u64 {
        self.built_at_ts
    }

    /// Retrieve the current access count (resets after rebuild).
    pub fn access_count(&self) -> usize {
        self.hot_access_count
    }

    /// Retrieve the number of cached rows.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }
}

/// Get the current UNIX timestamp in seconds.
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_projection_cache_creation() {
        let seg_id = SegmentId(42);
        let cache = RowProjectionCache::new(seg_id);

        assert_eq!(cache.segment_id(), seg_id);
        assert_eq!(cache.access_count(), 0);
        assert_eq!(cache.cache_size(), 0);

        let metrics = cache.get_metrics();
        assert_eq!(metrics.cache_hits, 0);
        assert_eq!(metrics.cache_misses, 0);
        assert_eq!(metrics.cache_rebuilds, 0);
    }

    #[test]
    fn test_projection_cache_get_put() {
        let seg_id = SegmentId(1);
        let mut cache = RowProjectionCache::new(seg_id);

        let row_id = RowId(100);
        let projection = vec![1, 2, 3, 4, 5];

        cache.put(row_id, Some(projection.clone()));
        assert_eq!(cache.cache_size(), 1);

        let retrieved = cache.get(row_id);
        assert_eq!(retrieved, Some(projection));

        let metrics = cache.get_metrics();
        assert_eq!(metrics.cache_hits, 1);
        assert_eq!(metrics.cache_misses, 0);
    }

    #[test]
    fn test_projection_cache_get_miss() {
        let seg_id = SegmentId(1);
        let mut cache = RowProjectionCache::new(seg_id);

        let row_id = RowId(100);
        let retrieved = cache.get(row_id);
        assert_eq!(retrieved, None);

        let metrics = cache.get_metrics();
        assert_eq!(metrics.cache_hits, 0);
        assert_eq!(metrics.cache_misses, 1);
    }

    #[test]
    fn test_projection_cache_invalidate() {
        let seg_id = SegmentId(1);
        let mut cache = RowProjectionCache::new(seg_id);

        let row_id = RowId(100);
        let projection = vec![1, 2, 3];

        cache.put(row_id, Some(projection));
        assert_eq!(cache.cache_size(), 1);

        cache.invalidate();
        assert_eq!(cache.cache_size(), 0);

        let metrics = cache.get_metrics();
        assert_eq!(metrics.cache_invalidations, 1);
    }

    #[test]
    fn test_projection_cache_invalidate_row() {
        let seg_id = SegmentId(1);
        let mut cache = RowProjectionCache::new(seg_id);

        let row_id_1 = RowId(100);
        let row_id_2 = RowId(101);
        let projection = vec![1, 2, 3];

        cache.put(row_id_1, Some(projection.clone()));
        cache.put(row_id_2, Some(projection));
        assert_eq!(cache.cache_size(), 2);

        cache.invalidate_row(row_id_1);
        assert_eq!(cache.cache_size(), 1);

        let metrics = cache.get_metrics();
        assert_eq!(metrics.cache_invalidations, 1);
    }

    #[test]
    fn test_projection_cache_rebuild_on_hot_threshold() {
        let seg_id = SegmentId(1);
        let mut cache = RowProjectionCache::with_threshold(seg_id, 5);

        let projection = vec![1, 2, 3];
        cache.put(RowId(100), Some(projection.clone()));
        cache.put(RowId(101), Some(projection.clone()));

        // Access 5 times to reach threshold
        for _ in 0..5 {
            let _ = cache.get(RowId(100));
        }

        assert!(cache.should_rebuild());
        assert_eq!(cache.access_count(), 0);

        let metrics = cache.get_metrics();
        assert_eq!(metrics.cache_rebuilds, 1);
    }

    #[test]
    fn test_projection_cache_rebuild_if_hot() {
        let seg_id = SegmentId(1);
        let mut cache = RowProjectionCache::with_threshold(seg_id, 3);

        let projection = vec![1, 2, 3];
        cache.put(RowId(100), Some(projection));
        assert_eq!(cache.cache_size(), 1);

        // Trigger rebuild
        for _ in 0..3 {
            let _ = cache.get(RowId(100));
        }

        cache.rebuild_if_hot();
        assert_eq!(cache.cache_size(), 0);

        let metrics = cache.get_metrics();
        assert_eq!(metrics.cache_rebuilds, 1);
    }

    #[test]
    fn test_projection_cache_metrics_hits_misses() {
        let seg_id = SegmentId(1);
        let mut cache = RowProjectionCache::new(seg_id);

        let projection = vec![1, 2, 3];
        cache.put(RowId(100), Some(projection));

        // 3 hits
        let _ = cache.get(RowId(100));
        let _ = cache.get(RowId(100));
        let _ = cache.get(RowId(100));

        // 2 misses
        let _ = cache.get(RowId(999));
        let _ = cache.get(RowId(998));

        let metrics = cache.get_metrics();
        assert_eq!(metrics.cache_hits, 3);
        assert_eq!(metrics.cache_misses, 2);
        assert_eq!(metrics.hit_ratio(), 60.0);
    }

    #[test]
    fn test_projection_cache_update_on_tail_append() {
        let seg_id = SegmentId(1);
        let mut cache = RowProjectionCache::new(seg_id);

        let row_id = RowId(100);
        let projection_v1 = vec![1, 2, 3];

        cache.put(row_id, Some(projection_v1.clone()));
        assert_eq!(cache.get(row_id), Some(projection_v1));

        // Simulate tail append by updating projection
        let projection_v2 = vec![1, 2, 3, 4];
        cache.put(row_id, Some(projection_v2.clone()));
        assert_eq!(cache.get(row_id), Some(projection_v2));
    }

    #[test]
    fn test_projection_cache_memory_footprint() {
        let seg_id = SegmentId(1);
        let mut cache = RowProjectionCache::new(seg_id);

        let projection_1 = vec![1, 2, 3, 4, 5]; // 5 bytes
        let projection_2 = vec![6, 7, 8]; // 3 bytes

        cache.put(RowId(100), Some(projection_1));
        cache.put(RowId(101), Some(projection_2));

        let metrics = cache.get_metrics();
        assert_eq!(metrics.memory_footprint_bytes, 8);
    }

    #[test]
    fn test_projection_cache_deleted_row() {
        let seg_id = SegmentId(1);
        let mut cache = RowProjectionCache::new(seg_id);

        let row_id = RowId(100);

        // Mark row as deleted (None)
        cache.put(row_id, None);
        assert_eq!(cache.cache_size(), 1);

        let retrieved = cache.get(row_id);
        assert_eq!(retrieved, None);

        let metrics = cache.get_metrics();
        assert_eq!(metrics.cache_hits, 1); // Still a cache hit (we knew it was deleted)
    }

    #[test]
    fn test_projection_cache_on_base_segment_swap() {
        let seg_id = SegmentId(1);
        let mut cache = RowProjectionCache::with_threshold(seg_id, 10);

        let projection = vec![1, 2, 3];
        cache.put(RowId(100), Some(projection));
        for _ in 0..5 {
            let _ = cache.get(RowId(100));
        }

        assert_eq!(cache.cache_size(), 1);
        assert_eq!(cache.access_count(), 5);

        // Simulate base segment swap
        cache.on_base_segment_swap();

        assert_eq!(cache.cache_size(), 0);
        assert_eq!(cache.access_count(), 0);

        let metrics = cache.get_metrics();
        assert_eq!(metrics.cache_invalidations, 1);
    }

    #[test]
    fn test_projection_cache_metrics_defaults() {
        let metrics = ProjectionCacheMetrics::new();
        assert_eq!(metrics.cache_hits, 0);
        assert_eq!(metrics.cache_misses, 0);
        assert_eq!(metrics.cache_rebuilds, 0);
        assert_eq!(metrics.cache_invalidations, 0);
        assert_eq!(metrics.memory_footprint_bytes, 0);
        assert_eq!(metrics.hit_ratio(), 0.0);
    }

    #[test]
    fn test_projection_cache_hit_ratio() {
        let mut metrics = ProjectionCacheMetrics::new();
        metrics.cache_hits = 3;
        metrics.cache_misses = 7;

        assert_eq!(metrics.hit_ratio(), 30.0);
    }
}
