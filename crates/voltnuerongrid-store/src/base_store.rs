//! Base column store implementation for immutable, persistent columnar segments.
//!
//! Provides in-memory column store with atomic versioning and manifest tracking.
//! All versions are immutable after publication, supporting old-or-new visibility guarantees.

use crate::segment::{BaseSegmentVersion, BaseSegmentManifest};
use crate::traits::{BaseColumnStore, ColumnBatch, Row};
use crate::types::{SegmentId, RowId, VersionId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// In-memory base column store with manifest versioning.
///
/// Maintains immutable column data organized by segment, with atomic version swaps.
/// Guarantees: old-or-new visibility (never partial), point-in-time consistency.
pub struct InMemoryBaseColumnStore {
    /// Segment ID -> manifest (thread-safe)
    manifests: HashMap<SegmentId, Arc<Mutex<BaseSegmentManifest>>>,
}

impl InMemoryBaseColumnStore {
    /// Create a new empty base column store.
    pub fn new() -> Self {
        InMemoryBaseColumnStore { manifests: HashMap::new() }
    }

    /// Register a segment (allocate manifest).
    pub fn register_segment(&mut self, segment_id: SegmentId) -> Result<(), String> {
        self.manifests.insert(segment_id, Arc::new(Mutex::new(BaseSegmentManifest::new())));
        Ok(())
    }

    /// Atomically install a new base version.
    /// Guarantees: either old-or-new visible, never mixed.
    pub fn install_base_version(
        &mut self,
        segment_id: SegmentId,
        version: BaseSegmentVersion,
    ) -> Result<(), String> {
        let manifest = self
            .manifests
            .get(&segment_id)
            .ok_or_else(|| format!("Segment {} not registered", segment_id.0))?;
        manifest.lock().map_err(|e| e.to_string())?.swap_version(version);
        Ok(())
    }

    /// Get the current base version for a segment.
    pub fn get_current_version(
        &self,
        segment_id: SegmentId,
    ) -> Result<Option<BaseSegmentVersion>, String> {
        let manifest = self
            .manifests
            .get(&segment_id)
            .ok_or_else(|| format!("Segment {} not registered", segment_id.0))?;
        Ok(manifest.lock().map_err(|e| e.to_string())?.current_version.clone())
    }

    /// Get version history for point-in-time reads.
    pub fn get_version_history(&self, segment_id: SegmentId) -> Result<Vec<BaseSegmentVersion>, String> {
        let manifest = self
            .manifests
            .get(&segment_id)
            .ok_or_else(|| format!("Segment {} not registered", segment_id.0))?;
        let m = manifest.lock().map_err(|e| e.to_string())?;
        let mut history = m.history.clone();
        if let Some(ref current) = m.current_version {
            history.push(current.clone());
        }
        Ok(history)
    }

    /// Check if a row exists in the current base version of a segment.
    pub fn row_exists(&self, segment_id: SegmentId, row_id: RowId) -> Result<bool, String> {
        let manifest = self
            .manifests
            .get(&segment_id)
            .ok_or_else(|| format!("Segment {} not registered", segment_id.0))?;
        let m = manifest.lock().map_err(|e| e.to_string())?;
        Ok(m.current_version.as_ref().map_or(false, |v| v.contains_row(row_id)))
    }
}

impl Default for InMemoryBaseColumnStore {
    fn default() -> Self {
        Self::new()
    }
}

impl BaseColumnStore for InMemoryBaseColumnStore {
    fn scan_columns(&self, col_ids: &[u32]) -> Result<ColumnBatch, String> {
        let mut batch = ColumnBatch {
            columns: HashMap::new(),
            row_count: 0,
        };

        // Scan all segments and combine columns
        for manifest in self.manifests.values() {
            if let Some(ref version) = manifest.lock().map_err(|e| e.to_string())?.current_version {
                for col_id in col_ids {
                    if version.columns.contains_key(col_id) {
                        batch.columns.entry(*col_id).or_insert_with(Vec::new);
                    }
                }
                batch.row_count = batch.row_count.max(version.stats.row_count as usize);
            }
        }
        Ok(batch)
    }

    fn get_row_by_id(&self, row_id: RowId) -> Result<Option<Row>, String> {
        // Scan all segments for the row
        for manifest in self.manifests.values() {
            if let Some(ref version) = manifest.lock().map_err(|e| e.to_string())?.current_version {
                if version.contains_row(row_id) {
                    // Found the row, reconstruct from blocks
                    let mut row: Row = HashMap::new();
                    for (col_id, _block) in &version.columns {
                        // Placeholder: real implementation would decode the block
                        row.insert(col_id.to_string(), String::new());
                    }
                    return Ok(Some(row));
                }
            }
        }
        Ok(None)
    }

    fn row_count(&self) -> u64 {
        self.manifests
            .values()
            .map(|m| {
                m.lock()
                    .ok()
                    .and_then(|manifest| manifest.current_version.as_ref().map(|v| v.stats.row_count))
                    .unwrap_or(0)
            })
            .sum()
    }
}

/// Atomic segment version manager.
///
/// Manages version IDs across segments with atomic promotion.
/// Guarantees: version swaps are fully visible or not at all (no partial states).
pub struct AtomicVersionManager {
    // Segment ID -> (current_version_id, old_version_ids)
    versions: Arc<Mutex<HashMap<SegmentId, (VersionId, Vec<VersionId>)>>>,
}

impl AtomicVersionManager {
    /// Create a new version manager.
    pub fn new() -> Self {
        AtomicVersionManager { versions: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Atomically promote a new version as current.
    /// Guarantees: either old-or-new visible, never partial.
    pub fn promote_version(
        &self,
        segment_id: SegmentId,
        new_version_id: VersionId,
    ) -> Result<(), String> {
        let mut versions = self.versions.lock().map_err(|e| e.to_string())?;
        if let Some((old_id, mut history)) = versions.remove(&segment_id) {
            history.push(old_id);
            versions.insert(segment_id, (new_version_id, history));
        } else {
            versions.insert(segment_id, (new_version_id, vec![]));
        }
        Ok(())
    }

    /// Get the current version (atomic visibility).
    pub fn get_current_version(&self, segment_id: SegmentId) -> Result<Option<VersionId>, String> {
        let versions = self.versions.lock().map_err(|e| e.to_string())?;
        Ok(versions.get(&segment_id).map(|(v, _)| *v))
    }

    /// Get version history.
    pub fn get_version_history(&self, segment_id: SegmentId) -> Result<Vec<VersionId>, String> {
        let versions = self.versions.lock().map_err(|e| e.to_string())?;
        if let Some((current, history)) = versions.get(&segment_id) {
            let mut all = history.clone();
            all.push(*current);
            Ok(all)
        } else {
            Ok(Vec::new())
        }
    }

    /// Check if a version exists for a segment.
    pub fn has_version(&self, segment_id: SegmentId) -> Result<bool, String> {
        let versions = self.versions.lock().map_err(|e| e.to_string())?;
        Ok(versions.contains_key(&segment_id))
    }

    /// Initialize a segment with a version (for first version).
    pub fn init_version(
        &self,
        segment_id: SegmentId,
        initial_version_id: VersionId,
    ) -> Result<(), String> {
        let mut versions = self.versions.lock().map_err(|e| e.to_string())?;
        versions.insert(segment_id, (initial_version_id, Vec::new()));
        Ok(())
    }
}

impl Default for AtomicVersionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CommitTs;
    use crate::segment::{ColumnEncoding, ColumnBlock};

    #[test]
    fn test_inmemory_store_creation() {
        let store = InMemoryBaseColumnStore::new();
        assert_eq!(store.row_count(), 0);
    }

    #[test]
    fn test_register_segment() {
        let mut store = InMemoryBaseColumnStore::new();
        let result = store.register_segment(SegmentId(1));
        assert!(result.is_ok());
    }

    #[test]
    fn test_register_duplicate_segment() {
        let mut store = InMemoryBaseColumnStore::new();
        store.register_segment(SegmentId(1)).unwrap();
        // Registering again should replace (idempotent)
        let result = store.register_segment(SegmentId(1));
        assert!(result.is_ok());
    }

    #[test]
    fn test_install_base_version() {
        let mut store = InMemoryBaseColumnStore::new();
        store.register_segment(SegmentId(1)).unwrap();

        let version = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(1),
            CommitTs(10),
            CommitTs(20),
            1000,
        );

        let result = store.install_base_version(SegmentId(1), version);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_current_version() {
        let mut store = InMemoryBaseColumnStore::new();
        store.register_segment(SegmentId(1)).unwrap();

        let version = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(1),
            CommitTs(10),
            CommitTs(20),
            1000,
        );

        store.install_base_version(SegmentId(1), version).unwrap();

        let retrieved = store.get_current_version(SegmentId(1)).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().version_id, VersionId(1));
    }

    #[test]
    fn test_get_version_not_found() {
        let store = InMemoryBaseColumnStore::new();
        let result = store.get_current_version(SegmentId(999));
        assert!(result.is_err());
    }

    #[test]
    fn test_row_exists() {
        let mut store = InMemoryBaseColumnStore::new();
        store.register_segment(SegmentId(1)).unwrap();

        let mut version = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(1),
            CommitTs(10),
            CommitTs(20),
            1000,
        );
        version.row_ids.push(RowId(100));

        store.install_base_version(SegmentId(1), version).unwrap();

        assert!(store.row_exists(SegmentId(1), RowId(100)).unwrap());
        assert!(!store.row_exists(SegmentId(1), RowId(200)).unwrap());
    }

    #[test]
    fn test_scan_columns() {
        let mut store = InMemoryBaseColumnStore::new();
        store.register_segment(SegmentId(1)).unwrap();

        let mut version = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(1),
            CommitTs(10),
            CommitTs(20),
            1000,
        );
        version.columns.insert(0, ColumnBlock::new(0, ColumnEncoding::Uncompressed, 100));
        version.columns.insert(1, ColumnBlock::new(1, ColumnEncoding::Dictionary, 100));
        version.stats.row_count = 100;

        store.install_base_version(SegmentId(1), version).unwrap();

        let batch = store.scan_columns(&[0, 1]).unwrap();
        assert_eq!(batch.row_count, 100);
        assert_eq!(batch.columns.len(), 2);
    }

    #[test]
    fn test_row_count() {
        let mut store = InMemoryBaseColumnStore::new();
        store.register_segment(SegmentId(1)).unwrap();

        let mut version = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(1),
            CommitTs(10),
            CommitTs(20),
            1000,
        );
        version.stats.row_count = 1000;

        store.install_base_version(SegmentId(1), version).unwrap();

        assert_eq!(store.row_count(), 1000);
    }

    #[test]
    fn test_get_row_by_id() {
        let mut store = InMemoryBaseColumnStore::new();
        store.register_segment(SegmentId(1)).unwrap();

        let mut version = BaseSegmentVersion::new(
            SegmentId(1),
            VersionId(1),
            CommitTs(10),
            CommitTs(20),
            1000,
        );
        version.row_ids.push(RowId(100));
        version.columns.insert(0, ColumnBlock::new(0, ColumnEncoding::Uncompressed, 1));

        store.install_base_version(SegmentId(1), version).unwrap();

        let row = store.get_row_by_id(RowId(100)).unwrap();
        assert!(row.is_some());
    }

    #[test]
    fn test_version_manager_creation() {
        let mgr = AtomicVersionManager::new();
        let result = mgr.get_current_version(SegmentId(1));
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn test_promote_version_first_time() {
        let mgr = AtomicVersionManager::new();
        let result = mgr.promote_version(SegmentId(1), VersionId(1));
        assert!(result.is_ok());

        let current = mgr.get_current_version(SegmentId(1)).unwrap();
        assert_eq!(current, Some(VersionId(1)));
    }

    #[test]
    fn test_promote_version_multiple_times() {
        let mgr = AtomicVersionManager::new();

        mgr.promote_version(SegmentId(1), VersionId(1)).unwrap();
        assert_eq!(mgr.get_current_version(SegmentId(1)).unwrap(), Some(VersionId(1)));

        mgr.promote_version(SegmentId(1), VersionId(2)).unwrap();
        assert_eq!(mgr.get_current_version(SegmentId(1)).unwrap(), Some(VersionId(2)));

        mgr.promote_version(SegmentId(1), VersionId(3)).unwrap();
        assert_eq!(mgr.get_current_version(SegmentId(1)).unwrap(), Some(VersionId(3)));
    }

    #[test]
    fn test_atomic_visibility_guarantee() {
        let mgr = AtomicVersionManager::new();

        mgr.promote_version(SegmentId(1), VersionId(1)).unwrap();
        mgr.promote_version(SegmentId(1), VersionId(2)).unwrap();
        mgr.promote_version(SegmentId(1), VersionId(3)).unwrap();

        // Verify only current version is active
        let current = mgr.get_current_version(SegmentId(1)).unwrap();
        assert_eq!(current, Some(VersionId(3)));

        // Verify history is tracked
        let history = mgr.get_version_history(SegmentId(1)).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0], VersionId(1));
        assert_eq!(history[1], VersionId(2));
        assert_eq!(history[2], VersionId(3));
    }

    #[test]
    fn test_has_version() {
        let mgr = AtomicVersionManager::new();
        assert!(!mgr.has_version(SegmentId(1)).unwrap());

        mgr.promote_version(SegmentId(1), VersionId(1)).unwrap();
        assert!(mgr.has_version(SegmentId(1)).unwrap());
    }

    #[test]
    fn test_init_version() {
        let mgr = AtomicVersionManager::new();
        let result = mgr.init_version(SegmentId(1), VersionId(100));
        assert!(result.is_ok());

        let current = mgr.get_current_version(SegmentId(1)).unwrap();
        assert_eq!(current, Some(VersionId(100)));
    }

    #[test]
    fn test_multiple_segments_independence() {
        let mgr = AtomicVersionManager::new();

        mgr.promote_version(SegmentId(1), VersionId(1)).unwrap();
        mgr.promote_version(SegmentId(2), VersionId(10)).unwrap();
        mgr.promote_version(SegmentId(3), VersionId(100)).unwrap();

        assert_eq!(mgr.get_current_version(SegmentId(1)).unwrap(), Some(VersionId(1)));
        assert_eq!(mgr.get_current_version(SegmentId(2)).unwrap(), Some(VersionId(10)));
        assert_eq!(mgr.get_current_version(SegmentId(3)).unwrap(), Some(VersionId(100)));
    }
}
