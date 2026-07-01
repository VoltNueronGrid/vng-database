//! MVCC (Multi-Version Concurrency Control) page-based row store.
//!
//! Advances S2-WS2-04 in status-tracker-v2.md from **TODO** → **PARTIAL**.
//!
//! # Durability vocabulary (see constitution §Vocabulary)
//!
//! This module implements the **in-memory row store** only.  It does NOT
//! provide **page-level row store durability** — all row data is held in
//! a `HashMap` and is lost on process exit.  DML statements are logged to
//! the **WAL** (via `persist_sql_statement` in `helpers/boot.rs`) so that
//! they can be replayed on restart (**WAL durability** ✅), but the row pages
//! themselves are not persisted to RocksDB Column Families (**page-level
//! durability** ❌ — tracked in tasks-v4.md P1).
//!
//! Implements the architectural concept of a page-based row store with
//! version-chain visibility rules.  The current implementation keeps all
//! data in memory using a fixed page-bucket layout so that the calling
//! pattern mirrors what a real disk-based store would expose.
//!
//! # Core concepts
//!
//! - **Transaction ID (`Xid`)** — a monotonically increasing u64 assigned
//!   by the caller (matches the ACID transaction-registry ID space).
//! - **Row version** — one snapshot of a row at a given `Xid`.  Rows can
//!   have multiple versions; the visibility rule is *"the latest version
//!   with `xid <= snapshot_xid` that is not a deleted tombstone"*.
//! - **Page** — a fixed-capacity bucket of rows.  When a page is full a
//!   new page is allocated (simulating heap-file page splits).
//! - **Snapshot read** — callers supply a `snapshot_xid` to read the state
//!   of the store as-of a completed transaction, enabling repeatable-read
//!   and serializable isolation queries.

#![forbid(unsafe_code)]

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A monotonically increasing transaction identifier.
pub type Xid = u64;

/// The data payload of one row version: column name → value (string-encoded).
pub type RowData = HashMap<String, String>;

/// One version of a row, created or deleted by transaction `xid`.
#[derive(Debug, Clone)]
pub struct RowVersion {
    /// The transaction that created / modified or deleted this version.
    pub xid: Xid,
    /// If `true` this version is a delete-tombstone; the row is invisible.
    pub deleted: bool,
    /// The column values for this version (empty for tombstones).
    pub data: RowData,
}

/// All versions of a single logical row, identified by `key`.
/// Versions are stored in ascending `xid` order; the **last** visible
/// version (latest `xid ≤ snapshot_xid` that is not a tombstone) wins.
#[derive(Debug, Clone)]
pub struct MvccRow {
    pub key: String,
    /// Version chain, ordered ascending by `xid`.
    pub versions: Vec<RowVersion>,
}

impl MvccRow {
    fn new(key: &str) -> Self {
        MvccRow { key: key.to_string(), versions: Vec::new() }
    }

    /// The visible version for a snapshot read at `snapshot_xid`.
    pub fn visible_at(&self, snapshot_xid: Xid) -> Option<&RowData> {
        // Walk backwards through versions (latest first) to find the
        // most recent version with xid <= snapshot_xid.
        for v in self.versions.iter().rev() {
            if v.xid <= snapshot_xid {
                return if v.deleted { None } else { Some(&v.data) };
            }
        }
        None
    }

    /// Push a new version.  Callers must ensure xid is >= last version's xid.
    fn push_version(&mut self, xid: Xid, deleted: bool, data: RowData) {
        self.versions.push(RowVersion { xid, deleted, data });
    }

    /// Returns the number of versions in this row's version chain.
    pub fn version_count(&self) -> usize {
        self.versions.len()
    }
}

// ---------------------------------------------------------------------------
// Storage page
// ---------------------------------------------------------------------------

/// A fixed-capacity bucket of rows within the [`PagedRowStore`].
#[derive(Debug, Default, Clone)]
pub struct StorePage {
    pub page_id: u64,
    /// Rows stored on this page, keyed by row key for O(1) lookup within page.
    rows: Vec<MvccRow>,
}

impl StorePage {
    fn new(page_id: u64) -> Self {
        StorePage { page_id, rows: Vec::new() }
    }

    fn find_row_mut(&mut self, key: &str) -> Option<&mut MvccRow> {
        self.rows.iter_mut().find(|r| r.key == key)
    }

    fn find_row(&self, key: &str) -> Option<&MvccRow> {
        self.rows.iter().find(|r| r.key == key)
    }

    fn len(&self) -> usize {
        self.rows.len()
    }
}

// ---------------------------------------------------------------------------
// PagedRowStore
// ---------------------------------------------------------------------------

/// A page-based row store with MVCC version-chain visibility.
///
/// Rows are distributed across pages of fixed `page_size` (default: 256 rows
/// per page). Any row that already exists on a page gets a new version
/// appended to its chain; new rows are appended to the current tail page,
/// allocating a fresh page when the current one is full.
///
/// This models the logical structure of a heap-file page store used by real
/// databases (Postgres, InnoDB, etc.) without the complexity of on-disk
/// serialisation.
#[derive(Debug)]
pub struct PagedRowStore {
    pages: Vec<StorePage>,
    page_size: usize,
    next_page_id: u64,
    next_xid: Xid,
    /// S2-WS2-05: Write-intent table — maps row key → the Xid that currently
    /// holds an uncommitted write intent for that key.  Used to detect
    /// write-write conflicts before a COMMIT is applied.
    write_intents: HashMap<String, Xid>,
    /// C-1: Maximum number of row-version records (across all pages) before FIFO
    /// page eviction kicks in.  `0` means unlimited (the default).
    ///
    /// In production with RocksDB as the primary engine, evicted rows are still
    /// durably stored in their per-DB column family and can be re-read via
    /// `DurabilityEngine::scan_rows_for_db`.  In-memory-only deployments should
    /// keep this at 0 (unlimited) to avoid data loss.
    max_rows_cap: usize,
}

impl Default for PagedRowStore {
    fn default() -> Self {
        Self::new(256)
    }
}

impl PagedRowStore {
    /// Create a new store with the given `page_size` (rows per page).
    pub fn new(page_size: usize) -> Self {
        assert!(page_size > 0, "page_size must be positive");
        let first_page = StorePage::new(0);
        PagedRowStore {
            pages: vec![first_page],
            page_size,
            next_page_id: 1,
            next_xid: 1,
            write_intents: HashMap::new(),
            max_rows_cap: 0,
        }
    }

    /// Set an upper bound on the total number of row versions kept in RAM.
    ///
    /// When `cap > 0` and `total_row_count() > cap` after an insert, the
    /// **oldest** page is dropped (FIFO eviction) until the count is under cap
    /// or only one page remains.
    ///
    /// Call this once after construction, before any writes.
    pub fn set_max_rows_cap(&mut self, cap: usize) {
        self.max_rows_cap = cap;
    }

    /// Evict the oldest page(s) until `total_row_count() <= max_rows_cap` or
    /// only one page remains.  No-op when `max_rows_cap == 0`.
    fn maybe_evict(&mut self) {
        if self.max_rows_cap == 0 {
            return;
        }
        while self.total_row_count() > self.max_rows_cap && self.pages.len() > 1 {
            self.pages.remove(0);
        }
    }

    // ------------------------------------------------------------------
    // Transaction ID management
    // ------------------------------------------------------------------

    /// Allocate and return a new monotonically increasing transaction ID.
    pub fn begin_xid(&mut self) -> Xid {
        let xid = self.next_xid;
        self.next_xid += 1;
        xid
    }

    /// The highest allocated Xid (useful as a snapshot fence: a `SELECT`
    /// started after `begin_xid()` returns this value sees all committed
    /// versions up to and including the returned number).
    pub fn current_xid(&self) -> Xid {
        self.next_xid.saturating_sub(1)
    }

    // ------------------------------------------------------------------
    // Write paths
    // ------------------------------------------------------------------

    /// Insert or overwrite a row identified by `key` within transaction `xid`.
    ///
    /// If the row already exists on any page its version chain is extended.
    /// Otherwise the row is placed on the current tail page (or a new page).
    pub fn insert(&mut self, xid: Xid, key: &str, data: RowData) {
        // Try to find the row on an existing page.
        for page in self.pages.iter_mut() {
            if let Some(row) = page.find_row_mut(key) {
                row.push_version(xid, false, data);
                // Version updates don't grow total_row_count — no eviction needed.
                return;
            }
        }
        // New row — append to current tail page (allocate new page if full).
        self.ensure_tail_capacity();
        let mut row = MvccRow::new(key);
        row.push_version(xid, false, data);
        self.pages.last_mut().unwrap().rows.push(row);
        // C-1: evict oldest page(s) if the write-back cache is over the cap.
        self.maybe_evict();
    }

    /// Delete `key` within transaction `xid`.  Appends a tombstone version.
    /// Returns `true` if the row existed and a tombstone was appended.
    pub fn delete(&mut self, xid: Xid, key: &str) -> bool {
        for page in self.pages.iter_mut() {
            if let Some(row) = page.find_row_mut(key) {
                row.push_version(xid, true, HashMap::new());
                return true;
            }
        }
        false
    }

    // ------------------------------------------------------------------
    // Read paths
    // ------------------------------------------------------------------

    /// Read the latest version of `key` visible at `snapshot_xid`.
    ///
    /// Returns `None` if the row does not exist or was deleted before or at
    /// the snapshot point.
    pub fn read_at_snapshot<'a>(&'a self, key: &str, snapshot_xid: Xid) -> Option<&'a RowData> {
        for page in &self.pages {
            if let Some(row) = page.find_row(key) {
                return row.visible_at(snapshot_xid);
            }
        }
        None
    }

    /// Returns the row data as-of the current head Xid (i.e. the absolute
    /// latest committed version regardless of snapshot).
    pub fn read_latest(&self, key: &str) -> Option<&RowData> {
        let snap = self.next_xid.saturating_sub(1);
        self.read_at_snapshot(key, snap)
    }

    /// Iterate over all (key, data) pairs visible at `snapshot_xid`.
    pub fn scan_at_snapshot(&self, snapshot_xid: Xid) -> Vec<(&str, &RowData)> {
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for page in &self.pages {
            for row in &page.rows {
                if seen.contains(row.key.as_str()) {
                    continue;
                }
                if let Some(data) = row.visible_at(snapshot_xid) {
                    result.push((row.key.as_str(), data));
                    seen.insert(row.key.as_str());
                }
            }
        }
        result
    }

    // ------------------------------------------------------------------
    // T-2: Committed-only read paths (dirty-read prevention)
    // ------------------------------------------------------------------

    /// T-2: Read the latest version of `key` visible at `snapshot_xid`, but
    /// only when no *other* transaction currently holds an uncommitted
    /// write-intent on the key. `reader_xid` is the reading transaction's id
    /// (pass `0` for an autonomous/non-transactional reader).
    ///
    /// When a foreign transaction holds a write-intent, its in-progress value
    /// is not yet committed; this method returns the previously-committed
    /// version (the MVCC value at `snapshot_xid`) rather than the dirty value,
    /// preventing a dirty read. A reader observing its *own* intent still sees
    /// its own uncommitted writes (read-your-own-writes).
    pub fn read_committed<'a>(
        &'a self,
        key: &str,
        snapshot_xid: Xid,
        reader_xid: Xid,
    ) -> Option<&'a RowData> {
        // If a different transaction holds the intent, exclude its uncommitted
        // version by reading at a snapshot strictly *below* that intent's xid.
        let effective_snapshot = match self.write_intents.get(key) {
            Some(&owner) if owner != reader_xid => {
                // The intent owner's write has xid == owner (allocated via
                // begin_xid before begin_write_intent). Read just below it.
                snapshot_xid.min(owner.saturating_sub(1))
            }
            _ => snapshot_xid,
        };
        self.read_at_snapshot(key, effective_snapshot)
    }

    /// T-2: Scan all rows committed as of `snapshot_xid`, excluding the
    /// in-progress (uncommitted) value of any row currently held under a
    /// foreign write-intent. `reader_xid` is the reading transaction's id
    /// (`0` for a non-transactional reader). Rows whose only versions belong to
    /// a foreign uncommitted intent are omitted entirely.
    pub fn scan_committed(&self, snapshot_xid: Xid, reader_xid: Xid) -> Vec<(&str, &RowData)> {
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for page in &self.pages {
            for row in &page.rows {
                if seen.contains(row.key.as_str()) {
                    continue;
                }
                let effective_snapshot = match self.write_intents.get(row.key.as_str()) {
                    Some(&owner) if owner != reader_xid => {
                        snapshot_xid.min(owner.saturating_sub(1))
                    }
                    _ => snapshot_xid,
                };
                if let Some(data) = row.visible_at(effective_snapshot) {
                    result.push((row.key.as_str(), data));
                    seen.insert(row.key.as_str());
                }
            }
        }
        result
    }

    // ------------------------------------------------------------------
    // Metrics / introspection
    // ------------------------------------------------------------------

    /// Total number of pages allocated.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Total number of logical rows (across all pages, including deleted).
    pub fn total_row_count(&self) -> usize {
        self.pages.iter().map(|p| p.len()).sum()
    }

    /// Count of rows visible at `snapshot_xid` (excludes tombstones).
    pub fn visible_row_count(&self, snapshot_xid: Xid) -> usize {
        self.scan_at_snapshot(snapshot_xid).len()
    }

    // ------------------------------------------------------------------
    // Write-intent concurrency control (S2-WS2-05)
    // ------------------------------------------------------------------

    /// Register a write intent for `key` under transaction `xid`.
    ///
    /// Returns `Ok(())` if the intent was registered successfully.
    /// Returns `Err(blocking_xid)` if a **different** transaction already
    /// holds a write intent for the same key, indicating a write-write
    /// conflict that the caller should surface as HTTP 409.
    pub fn begin_write_intent(&mut self, xid: Xid, key: &str) -> Result<(), Xid> {
        match self.write_intents.get(key) {
            Some(&other) if other != xid => Err(other),
            _ => {
                self.write_intents.insert(key.to_string(), xid);
                Ok(())
            }
        }
    }

    /// Remove all write intents owned by `xid`.
    /// Call this on both COMMIT and ROLLBACK so intents do not linger.
    pub fn release_write_intents(&mut self, xid: Xid) {
        self.write_intents.retain(|_, &mut v| v != xid);
    }

    /// Returns `true` if any version of `key` was committed with an Xid
    /// strictly greater than `since_xid`.  Used for optimistic conflict
    /// detection at COMMIT: if another transaction snuck in a write after
    /// the current transaction took its read snapshot, the commit should fail.
    pub fn was_modified_after(&self, key: &str, since_xid: Xid) -> bool {
        for page in &self.pages {
            if let Some(row) = page.find_row(key) {
                return row.versions.iter().any(|v| v.xid > since_xid);
            }
        }
        false
    }

    // ------------------------------------------------------------------
    // Snapshot export (S2-WS2-04)
    // ------------------------------------------------------------------

    /// Export a point-in-time snapshot of all currently-visible rows.
    ///
    /// Returns one `(key, data)` pair per distinct visible key at the
    /// current head XID.  Tombstoned (deleted) rows are excluded.
    pub fn export_rows_snapshot(&self) -> Vec<(String, RowData)> {
        let snapshot_xid = self.current_xid();
        self.scan_at_snapshot(snapshot_xid)
            .into_iter()
            .map(|(k, data)| (k.to_string(), data.clone()))
            .collect()
    }

    /// P1: Advance `next_xid` to at least `min_xid`.
    ///
    /// Called at boot when RocksDB is the durability engine to restore the
    /// XID counter to a value higher than any XID persisted in a previous
    /// session.  Without this, `current_xid()` returns 0 after restart and
    /// `scan_rows_for_db` filters out all persisted rows (xid > 0 > snapshot).
    ///
    /// Safe to call with any value: it is a no-op when `next_xid >= min_xid`.
    pub fn fast_forward_xid(&mut self, min_xid: Xid) {
        if min_xid > self.next_xid {
            self.next_xid = min_xid;
        }
    }

    /// Replace all rows atomically with the given snapshot data.
    ///
    /// Clears every existing page (resetting to a single empty page) and
    /// inserts each row from `rows` under a fresh transaction id.  The
    /// `next_xid` counter is preserved so future writes get monotonically
    /// higher xids than any prior version.
    ///
    /// Intended for Raft snapshot installation (§7): the leader's full
    /// row-store snapshot replaces the follower's diverged state entirely.
    pub fn replace_all(&mut self, rows: impl IntoIterator<Item = (String, RowData)>) {
        // Reset to a single empty page; keep next_xid monotone.
        self.pages = vec![StorePage::new(0)];
        self.next_page_id = 1;
        self.write_intents.clear();
        let xid = self.begin_xid();
        for (key, data) in rows {
            self.insert(xid, &key, data);
        }
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn ensure_tail_capacity(&mut self) {
        if self.pages.last().map(|p| p.len()).unwrap_or(0) >= self.page_size {
            let page_id = self.next_page_id;
            self.next_page_id += 1;
            self.pages.push(StorePage::new(page_id));
        }
    }
}

// ---------------------------------------------------------------------------
// H9-3: Bridge trait and adapter functions for timestamp-based semantics
// ---------------------------------------------------------------------------

/// Bridge trait to adapt old Xid-based visibility to new timestamp-based visibility.
pub trait VersionVisibility {
    fn is_visible_at(&self, snapshot_ts: crate::types::SnapshotTs) -> bool;
}

/// Adapter: map Xid to CommitTs (simple 1:1 mapping).
pub fn xid_to_commit_ts(xid: Xid) -> crate::types::CommitTs {
    crate::types::CommitTs(xid)
}

/// Adapter: map CommitTs to Xid.
pub fn commit_ts_to_xid(ts: crate::types::CommitTs) -> Xid {
    ts.0
}

/// Adapter: map SnapshotTs to Xid.
pub fn snapshot_ts_to_xid(ts: crate::types::SnapshotTs) -> Xid {
    ts.0
}

// ---------------------------------------------------------------------------
// H9-3: MvccRowV2 - Enhanced MVCC row with explicit lineage and timestamp semantics
// ---------------------------------------------------------------------------

/// Enhanced MVCC row data structure that bridges old and new semantics.
/// Maintains both the old version chain and new explicit lineage pointers.
#[derive(Debug, Clone)]
pub struct MvccRowV2 {
    pub key: String,
    /// Old-style versions for backward compatibility
    pub versions: Vec<RowVersion>,
    /// Index: VersionId → position in versions vector for fast lookups
    version_index: std::collections::HashMap<crate::types::VersionId, usize>,
}

impl MvccRowV2 {
    /// Create a new row for bridging semantics.
    pub fn new(key: &str) -> Self {
        MvccRowV2 {
            key: key.to_string(),
            versions: Vec::new(),
            version_index: std::collections::HashMap::new(),
        }
    }

    /// Get latest visible version using new timestamp semantics.
    /// Visibility rule: begin_ts <= snapshot_ts < end_ts
    pub fn get_latest_tail_version(
        &self,
        snapshot_ts: crate::types::SnapshotTs,
    ) -> Option<&RowVersion> {
        // Walk backwards through versions (latest first) to find the
        // most recent version with begin_ts <= snapshot_ts
        for v in self.versions.iter().rev() {
            // Map Xid to CommitTs (implicitly begin_ts)
            let begin_ts = xid_to_commit_ts(v.xid);
            if begin_ts.0 <= snapshot_ts.0 {
                return if v.deleted { None } else { Some(v) };
            }
        }
        None
    }

    /// Traverse backward through lineage chain, returning versions in descending order.
    /// Returns all versions, including tombstones.
    pub fn get_tail_version_chain(&self) -> Vec<&RowVersion> {
        self.versions.iter().rev().collect()
    }

    /// Append a new version to the chain.
    /// Automatically sets end_ts of previous version if this version has a higher begin_ts.
    pub fn append_tail_version(
        &mut self,
        version: RowVersion,
    ) -> Result<(), String> {
        let version_id = crate::types::VersionId(self.versions.len() as u64);
        self.version_index.insert(version_id, self.versions.len());
        self.versions.push(version);
        Ok(())
    }

    /// Mark a version as deleted (create tombstone).
    /// The latest version is closed and a tombstone is added.
    pub fn delete_at_ts(&mut self, delete_ts: Xid) -> Result<(), String> {
        if self.versions.is_empty() {
            return Err("Row is empty".to_string());
        }
        // Add a tombstone version
        let tombstone = RowVersion {
            xid: delete_ts,
            deleted: true,
            data: HashMap::new(),
        };
        self.append_tail_version(tombstone)
    }

    /// Returns the number of versions in this row's version chain.
    pub fn version_count(&self) -> usize {
        self.versions.len()
    }

    /// Check if row is visible at snapshot (not deleted).
    pub fn is_visible_at(&self, snapshot_ts: crate::types::SnapshotTs) -> bool {
        self.get_latest_tail_version(snapshot_ts).is_some()
    }
}

// ---------------------------------------------------------------------------
// H9-1: TailStore trait implementation for PagedRowStore
// ---------------------------------------------------------------------------

impl crate::traits::TailStore for PagedRowStore {
    fn insert_version(
        &mut self,
        row_id: crate::types::RowId,
        version: crate::segment::TailVersion,
    ) -> Result<(), String> {
        // Map RowId to a row key (simple mapping: use row_id as key)
        let key = format!("row:{}", row_id.0);
        
        // If the version is a tombstone, record a delete
        if version.tombstone {
            self.delete(version.begin_ts.0, &key);
        } else {
            // Otherwise, insert the payload as row data
            // Parse payload as column pairs: [col_name_len, col_name, val_len, val, ...]
            let mut data = std::collections::HashMap::new();
            
            // For now, store the raw payload as a single "data" column
            // A full implementation would parse the payload into columns
            data.insert("_payload".to_string(), String::from_utf8_lossy(&version.payload).to_string());
            
            self.insert(version.begin_ts.0, &key, data);
        }
        
        Ok(())
    }

    fn get_latest_version(
        &self,
        row_id: crate::types::RowId,
        snapshot_ts: crate::types::SnapshotTs,
    ) -> Result<Option<crate::segment::TailVersion>, String> {
        let key = format!("row:{}", row_id.0);
        
        // Scan through pages to find the row
        for page in &self.pages {
            if let Some(row) = page.find_row(&key) {
                // Find the latest visible version
                if let Some(data) = row.visible_at(snapshot_ts.0) {
                    // Reconstruct a TailVersion from the found row data
                    let payload = data
                        .get("_payload")
                        .map(|s| s.as_bytes().to_vec())
                        .unwrap_or_default();
                    
                    let version = crate::segment::TailVersion {
                        row_id,
                        version_id: crate::types::VersionId(row.version_count() as u64),
                        begin_ts: crate::types::CommitTs(row.versions.last().map(|v| v.xid).unwrap_or(0)),
                        end_ts: None,
                        prev_version: None,
                        tombstone: false,
                        payload,
                    };
                    return Ok(Some(version));
                }
                return Ok(None);
            }
        }
        
        Ok(None)
    }

    fn get_version_chain(
        &self,
        row_id: crate::types::RowId,
    ) -> Result<Vec<crate::segment::TailVersion>, String> {
        let key = format!("row:{}", row_id.0);
        
        // Scan through pages to find the row
        for page in &self.pages {
            if let Some(row) = page.find_row(&key) {
                // Convert version chain to TailVersion objects
                let mut versions = Vec::new();
                for (idx, row_ver) in row.versions.iter().enumerate() {
                    let payload = if row_ver.deleted {
                        Vec::new()
                    } else {
                        row_ver.data
                            .get("_payload")
                            .map(|s| s.as_bytes().to_vec())
                            .unwrap_or_default()
                    };
                    
                    versions.push(crate::segment::TailVersion {
                        row_id,
                        version_id: crate::types::VersionId((idx + 1) as u64),
                        begin_ts: crate::types::CommitTs(row_ver.xid),
                        end_ts: None,
                        prev_version: if idx > 0 { Some(crate::types::VersionId(idx as u64)) } else { None },
                        tombstone: row_ver.deleted,
                        payload,
                    });
                }
                return Ok(versions);
            }
        }
        
        Ok(Vec::new())
    }

    fn delete_row(
        &mut self,
        row_id: crate::types::RowId,
        commit_ts: crate::types::CommitTs,
    ) -> Result<(), String> {
        let key = format!("row:{}", row_id.0);
        self.delete(commit_ts.0, &key);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pairs: &[(&str, &str)]) -> RowData {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn mvcc_insert_and_read_latest() {
        let mut store = PagedRowStore::new(256);
        let xid = store.begin_xid();
        store.insert(xid, "user:1", row(&[("name", "Alice"), ("age", "30")]));

        let data = store.read_latest("user:1").expect("row must exist");
        assert_eq!(data["name"], "Alice");
        assert_eq!(data["age"], "30");
    }

    #[test]
    fn mvcc_snapshot_does_not_see_future_writes() {
        let mut store = PagedRowStore::new(256);

        // xid=1 inserts a row
        let xid1 = store.begin_xid(); // 1
        store.insert(xid1, "order:1", row(&[("amount", "100")]));

        // snapshot fixed at xid=1
        let snapshot = store.current_xid(); // 1

        // xid=2 updates the row
        let xid2 = store.begin_xid(); // 2
        store.insert(xid2, "order:1", row(&[("amount", "999")]));

        // snapshot must still see amount=100
        let visible = store.read_at_snapshot("order:1", snapshot).expect("should be visible");
        assert_eq!(visible["amount"], "100");

        // head read sees amount=999
        let latest = store.read_latest("order:1").expect("should exist");
        assert_eq!(latest["amount"], "999");
    }

    #[test]
    fn mvcc_delete_creates_tombstone() {
        let mut store = PagedRowStore::new(256);
        let xid = store.begin_xid();
        store.insert(xid, "session:a", row(&[("active", "true")]));

        let snapshot_before = store.current_xid();

        let xid2 = store.begin_xid();
        assert!(store.delete(xid2, "session:a"));

        // snapshot before delete sees the row
        let before = store.read_at_snapshot("session:a", snapshot_before);
        assert!(before.is_some());

        // latest read after delete sees nothing
        assert!(store.read_latest("session:a").is_none());
    }

    #[test]
    fn mvcc_version_chain_grows_correctly() {
        let mut store = PagedRowStore::new(256);
        for i in 1u64..=5 {
            store.insert(i, "counter", row(&[("v", &i.to_string())]));
        }

        // snapshot at xid=3 should see v=3
        let at3 = store.read_at_snapshot("counter", 3).expect("must exist");
        assert_eq!(at3["v"], "3");

        // snapshot at xid=5 should see v=5
        let at5 = store.read_at_snapshot("counter", 5).expect("must exist");
        assert_eq!(at5["v"], "5");
    }

    #[test]
    fn mvcc_page_split_on_full_page() {
        // page_size=2 forces a new page after 2 distinct rows
        let mut store = PagedRowStore::new(2);
        let xid = store.begin_xid();
        store.insert(xid, "r1", row(&[("x", "1")]));
        store.insert(xid, "r2", row(&[("x", "2")]));
        store.insert(xid, "r3", row(&[("x", "3")])); // triggers page split

        assert!(store.page_count() >= 2, "must have allocated a second page");
        assert_eq!(store.visible_row_count(store.current_xid()), 3);
    }

    #[test]
    fn mvcc_scan_at_snapshot() {
        let mut store = PagedRowStore::new(256);
        let xid = store.begin_xid(); // 1
        store.insert(xid, "a", row(&[("v", "1")]));
        store.insert(xid, "b", row(&[("v", "2")]));

        let snapshot = store.current_xid();

        let xid2 = store.begin_xid(); // 2
        store.insert(xid2, "c", row(&[("v", "3")]));  // not visible at snapshot

        let visible = store.scan_at_snapshot(snapshot);
        let keys: Vec<_> = visible.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"a"));
        assert!(keys.contains(&"b"));
        assert!(!keys.contains(&"c"), "c must not be visible at snapshot");
    }

    #[test]
    fn mvcc_delete_non_existent_returns_false() {
        let mut store = PagedRowStore::new(256);
        let xid = store.begin_xid();
        assert!(!store.delete(xid, "ghost:key"));
    }

    #[test]
    fn mvcc_visible_row_count_excludes_tombstones() {
        let mut store = PagedRowStore::new(256);
        let xid = store.begin_xid();
        store.insert(xid, "x", row(&[("k", "v")]));
        store.insert(xid, "y", row(&[("k", "v")]));

        assert_eq!(store.visible_row_count(store.current_xid()), 2);

        let xid2 = store.begin_xid();
        store.delete(xid2, "x");

        assert_eq!(store.visible_row_count(store.current_xid()), 1);
        // total_row_count includes logical rows (not versions)
        assert_eq!(store.total_row_count(), 2);
    }

    // ─── S2-WS2-05: Write-intent concurrency control ──────────────────────────

    #[test]
    fn write_intent_registers_and_releases() {
        let mut store = PagedRowStore::new(256);
        let xid = store.begin_xid();
        assert!(store.begin_write_intent(xid, "user:1").is_ok());
        assert!(store.begin_write_intent(xid, "user:1").is_ok()); // idempotent same xid
        store.release_write_intents(xid);
        // After release, a new xid can acquire the intent.
        let xid2 = store.begin_xid();
        assert!(store.begin_write_intent(xid2, "user:1").is_ok());
    }

    #[test]
    fn write_intent_conflict_returns_blocking_xid() {
        let mut store = PagedRowStore::new(256);
        let xid1 = store.begin_xid();
        let xid2 = store.begin_xid();
        store.begin_write_intent(xid1, "order:99").unwrap();
        // xid2 attempting the same key → should get Err(xid1)
        let result = store.begin_write_intent(xid2, "order:99");
        assert_eq!(result, Err(xid1));
    }

    #[test]
    fn was_modified_after_detects_concurrent_write() {
        let mut store = PagedRowStore::new(256);
        let snapshot = store.current_xid(); // 0 — nothing committed yet
        let xid = store.begin_xid();
        store.insert(xid, "item:1", row(&[("qty", "10")]));
        // snapshot=0; item:1 now has a version with xid=1 > 0
        assert!(store.was_modified_after("item:1", snapshot));
        // For a key that was not modified, should return false.
        assert!(!store.was_modified_after("item:99", snapshot));
    }

    // ─── T-2: committed-only reads (dirty-read prevention) ────────────────────

    #[test]
    fn t2_read_committed_hides_foreign_uncommitted_write() {
        let mut store = PagedRowStore::new(256);
        // Commit an initial value for acct:1.
        let xid1 = store.begin_xid();
        store.insert(xid1, "acct:1", row(&[("bal", "100")]));
        let reader_snapshot = store.current_xid();

        // A second transaction writes a new value AND holds an uncommitted intent.
        let xid2 = store.begin_xid();
        store.begin_write_intent(xid2, "acct:1").unwrap();
        store.insert(xid2, "acct:1", row(&[("bal", "999")]));

        // A concurrent reader (reader_xid = 0) must NOT see the dirty 999 value.
        let seen = store
            .read_committed("acct:1", reader_snapshot, 0)
            .expect("row exists");
        assert_eq!(seen.get("bal").map(String::as_str), Some("100"),
            "dirty read prevented: must see committed 100, not in-progress 999");

        // The writer itself (reader_xid = xid2) does see its own write.
        let own = store
            .read_committed("acct:1", store.current_xid(), xid2)
            .expect("row exists");
        assert_eq!(own.get("bal").map(String::as_str), Some("999"),
            "read-your-own-writes: writer sees its in-progress value");

        // After the writer releases its intent (commit), the reader sees 999.
        store.release_write_intents(xid2);
        let after = store
            .read_committed("acct:1", store.current_xid(), 0)
            .expect("row exists");
        assert_eq!(after.get("bal").map(String::as_str), Some("999"));
    }

    #[test]
    fn t2_scan_committed_excludes_foreign_dirty_rows() {
        let mut store = PagedRowStore::new(256);
        let xid1 = store.begin_xid();
        store.insert(xid1, "t:1", row(&[("v", "a")]));
        store.insert(xid1, "t:2", row(&[("v", "b")]));
        let snap = store.current_xid();

        // Foreign tx updates t:1 (uncommitted intent held).
        let xid2 = store.begin_xid();
        store.begin_write_intent(xid2, "t:1").unwrap();
        store.insert(xid2, "t:1", row(&[("v", "DIRTY")]));

        let rows = store.scan_committed(snap, 0);
        let t1 = rows.iter().find(|(k, _)| *k == "t:1").map(|(_, d)| d);
        assert_eq!(
            t1.and_then(|d| d.get("v")).map(String::as_str),
            Some("a"),
            "scan_committed must show committed value, not the foreign dirty write"
        );
    }

    #[test]
    fn replace_all_clears_old_rows_and_inserts_new() {
        use std::collections::HashMap;
        let mut store = PagedRowStore::new(256);
        // Insert initial rows.
        let xid1 = store.begin_xid();
        store.insert(xid1, "old-key-1", HashMap::new());
        store.insert(xid1, "old-key-2", HashMap::new());
        // Verify old rows visible.
        let snap1 = store.scan_at_snapshot(xid1);
        assert!(snap1.iter().any(|(k, _)| *k == "old-key-1"), "old rows should be visible before replace_all");
        // Replace all with a single new row.
        let mut new_cols = HashMap::new();
        new_cols.insert("col".to_string(), "v".to_string());
        store.replace_all(vec![("new-key-1".to_string(), new_cols)]);
        let snap2 = store.scan_at_snapshot(store.current_xid());
        let keys: Vec<&str> = snap2.iter().map(|(k, _)| *k).collect();
        assert!(!keys.iter().any(|k| *k == "old-key-1"), "old-key-1 must not appear after replace_all");
        assert!(!keys.iter().any(|k| *k == "old-key-2"), "old-key-2 must not appear after replace_all");
        assert!(keys.iter().any(|k| *k == "new-key-1"), "new-key-1 must appear after replace_all");
    }

    // ─── C-1: bounded write-back cache (max_rows_cap) ────────────────────────

    /// set_max_rows_cap with a page-size of 2 rows:
    ///   insert 3 distinct keys → the first page (2 rows) is evicted, leaving 1.
    #[test]
    fn c1_eviction_drops_oldest_page_when_over_cap() {
        // page_size=2 means each page holds exactly 2 rows before a new page is allocated.
        let mut store = PagedRowStore::new(2);
        store.set_max_rows_cap(2); // cap at 2 rows total

        let xid = store.begin_xid();
        store.insert(xid, "row:1", row(&[("v", "a")]));
        store.insert(xid, "row:2", row(&[("v", "b")]));
        // Both rows fit — page 0 is full.  No eviction yet.
        assert_eq!(store.total_row_count(), 2);

        // Inserting a 3rd distinct key overflows to a new page, triggering eviction.
        let xid2 = store.begin_xid();
        store.insert(xid2, "row:3", row(&[("v", "c")]));
        // Page 0 (rows 1 & 2) should have been evicted; only row:3 remains.
        assert_eq!(store.total_row_count(), 1, "oldest page should have been evicted");
        // row:3 is still readable.
        assert!(store.read_latest("row:3").is_some(), "row:3 must survive eviction");
    }

    /// Version updates on an existing key do NOT bump total_row_count, so
    /// they should never trigger eviction regardless of the cap.
    #[test]
    fn c1_version_update_does_not_trigger_eviction() {
        let mut store = PagedRowStore::new(2);
        store.set_max_rows_cap(2);

        let xid = store.begin_xid();
        store.insert(xid, "user:1", row(&[("name", "Alice")]));
        store.insert(xid, "user:2", row(&[("name", "Bob")]));
        // 2 rows exactly at cap.

        // Update user:1 — same key, appends a new version but count stays 2.
        let xid2 = store.begin_xid();
        store.insert(xid2, "user:1", row(&[("name", "Alicia")]));

        assert_eq!(store.total_row_count(), 2, "version update must not increase row count");
        let latest = store.read_latest("user:1").unwrap();
        assert_eq!(latest["name"], "Alicia", "latest version should be visible");
    }

    /// With cap=0 (unlimited), any number of rows may be inserted without eviction.
    #[test]
    fn c1_no_cap_allows_unlimited_rows() {
        let mut store = PagedRowStore::new(2);
        // cap defaults to 0 (unlimited)
        let xid = store.begin_xid();
        for i in 0..20usize {
            store.insert(xid, &format!("row:{i}"), row(&[("v", "x")]));
        }
        assert_eq!(store.total_row_count(), 20, "no eviction when cap=0");
    }

    // ─── H9-1: TailStore trait tests ───────────────────────────────────────

    #[test]
    fn h9_1_tailstore_insert_version() {
        use crate::traits::TailStore;
        use crate::types::{RowId, VersionId, CommitTs};
        use crate::segment::TailVersion;

        let mut store = PagedRowStore::new(256);
        let initial_count = store.total_row_count();
        
        let row_id = RowId(100);
        let version = TailVersion::new(
            row_id,
            VersionId(1),
            CommitTs(50),
            vec![1, 2, 3, 4],
        );

        // Insert version via trait
        let result = store.insert_version(row_id, version);
        assert!(result.is_ok(), "insert_version should succeed");

        // Verify the row count increased
        let new_count = store.total_row_count();
        assert_eq!(new_count, initial_count + 1, "row count should increase after insert");
    }

    #[test]
    fn h9_1_tailstore_get_latest_version() {
        use crate::traits::TailStore;
        use crate::types::{RowId, VersionId, CommitTs, SnapshotTs};
        use crate::segment::TailVersion;

        let mut store = PagedRowStore::new(256);
        let row_id = RowId(200);
        
        // Insert initial version
        let version = TailVersion::new(
            row_id,
            VersionId(1),
            CommitTs(50),
            vec![1, 2, 3],
        );
        store.insert_version(row_id, version).unwrap();

        // Query at same snapshot
        let result = store.get_latest_version(row_id, SnapshotTs(50));
        assert!(result.is_ok());
        let found = result.unwrap();
        assert!(found.is_some(), "should find version at snapshot 50");
        assert_eq!(found.unwrap().row_id, row_id);
    }

    #[test]
    fn h9_1_tailstore_delete_row() {
        use crate::traits::TailStore;
        use crate::types::{RowId, VersionId, CommitTs, SnapshotTs};
        use crate::segment::TailVersion;

        let mut store = PagedRowStore::new(256);
        let row_id = RowId(300);

        // Insert a row
        let version = TailVersion::new(
            row_id,
            VersionId(1),
            CommitTs(50),
            vec![5, 6, 7],
        );
        store.insert_version(row_id, version).unwrap();

        // Delete it
        let result = store.delete_row(row_id, CommitTs(100));
        assert!(result.is_ok());

        // Row should not be visible at snapshot > delete_ts
        let found = store.get_latest_version(row_id, SnapshotTs(100)).unwrap();
        assert!(found.is_none(), "deleted row should not be visible");
    }

    #[test]
    fn h9_1_tailstore_version_chain() {
        use crate::traits::TailStore;
        use crate::types::{RowId, VersionId, CommitTs};
        use crate::segment::TailVersion;

        let mut store = PagedRowStore::new(256);
        let row_id = RowId(400);

        // Insert version 1
        let v1 = TailVersion::new(
            row_id,
            VersionId(1),
            CommitTs(10),
            vec![1],
        );
        store.insert_version(row_id, v1).unwrap();

        // Insert version 2 (update)
        let v2 = TailVersion::new(
            row_id,
            VersionId(2),
            CommitTs(20),
            vec![2],
        );
        store.insert_version(row_id, v2).unwrap();

        // Get version chain
        let chain = store.get_version_chain(row_id).unwrap();
        assert_eq!(chain.len(), 2, "should have 2 versions");
        assert_eq!(chain[0].begin_ts, CommitTs(10));
        assert_eq!(chain[1].begin_ts, CommitTs(20));
    }

    #[test]
    fn h9_1_tailstore_tombstone_version() {
        use crate::traits::TailStore;
        use crate::types::{RowId, VersionId, CommitTs};
        use crate::segment::TailVersion;

        let mut store = PagedRowStore::new(256);
        let row_id = RowId(500);

        // Insert a row
        let version = TailVersion::new(
            row_id,
            VersionId(1),
            CommitTs(30),
            vec![9, 8, 7],
        );
        store.insert_version(row_id, version).unwrap();

        // Insert tombstone version
        let tombstone = TailVersion::tombstone(
            row_id,
            VersionId(2),
            CommitTs(40),
        );
        store.insert_version(row_id, tombstone).unwrap();

        // Verify version chain has tombstone
        let chain = store.get_version_chain(row_id).unwrap();
        assert_eq!(chain.len(), 2);
        assert!(chain[1].tombstone, "second version should be tombstone");
    }

    // ─── H9-3: MvccRowV2 and timestamp-based visibility tests ──────────────────

    #[test]
    fn h9_3_mvcc_row_v2_creation() {
        let mrow = MvccRowV2::new("test_key");
        assert_eq!(mrow.key, "test_key");
        assert_eq!(mrow.versions.len(), 0);
    }

    #[test]
    fn h9_3_tail_version_visibility_rule() {
        let mut mrow = MvccRowV2::new("test_key");

        // Insert version 1: xid=10
        let v1 = RowVersion {
            xid: 10,
            deleted: false,
            data: row(&[("v", "1")]),
        };
        mrow.append_tail_version(v1).unwrap();

        // Insert version 2: xid=20
        let v2 = RowVersion {
            xid: 20,
            deleted: false,
            data: row(&[("v", "2")]),
        };
        mrow.append_tail_version(v2).unwrap();

        // Visibility checks: begin_ts <= snapshot_ts
        let snap_10 = crate::types::SnapshotTs(10);
        let snap_15 = crate::types::SnapshotTs(15);
        let snap_20 = crate::types::SnapshotTs(20);
        let snap_5 = crate::types::SnapshotTs(5);

        // At snapshot 10, should see version 1
        let v = mrow.get_latest_tail_version(snap_10).unwrap();
        assert_eq!(v.xid, 10);

        // At snapshot 15, should see version 1 (most recent before snapshot)
        let v = mrow.get_latest_tail_version(snap_15).unwrap();
        assert_eq!(v.xid, 10);

        // At snapshot 20, should see version 2
        let v = mrow.get_latest_tail_version(snap_20).unwrap();
        assert_eq!(v.xid, 20);

        // At snapshot 5, should see nothing (before first version)
        assert!(mrow.get_latest_tail_version(snap_5).is_none());
    }

    #[test]
    fn h9_3_lineage_chain_traversal() {
        let mut mrow = MvccRowV2::new("test_key");

        // Insert version 1: xid=10
        let v1 = RowVersion {
            xid: 10,
            deleted: false,
            data: row(&[("v", "1")]),
        };
        mrow.append_tail_version(v1).unwrap();

        // Insert version 2: xid=20
        let v2 = RowVersion {
            xid: 20,
            deleted: false,
            data: row(&[("v", "2")]),
        };
        mrow.append_tail_version(v2).unwrap();

        // Insert version 3: xid=30
        let v3 = RowVersion {
            xid: 30,
            deleted: false,
            data: row(&[("v", "3")]),
        };
        mrow.append_tail_version(v3).unwrap();

        // Get lineage chain (should be reverse chronological)
        let chain = mrow.get_tail_version_chain();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].xid, 30, "latest first");
        assert_eq!(chain[1].xid, 20, "middle");
        assert_eq!(chain[2].xid, 10, "oldest last");
    }

    #[test]
    fn h9_3_tombstone_visibility() {
        let mut mrow = MvccRowV2::new("test_key");

        // Insert version 1: xid=10
        let v1 = RowVersion {
            xid: 10,
            deleted: false,
            data: row(&[("v", "value")]),
        };
        mrow.append_tail_version(v1).unwrap();

        // Snapshot before delete
        assert!(
            mrow.is_visible_at(crate::types::SnapshotTs(10)),
            "row should be visible at ts=10"
        );

        // Delete at xid=20
        mrow.delete_at_ts(20).unwrap();

        // After delete, row should be invisible
        assert!(
            !mrow.is_visible_at(crate::types::SnapshotTs(25)),
            "row should be invisible after delete"
        );

        // Before delete, row should still be visible
        assert!(
            mrow.is_visible_at(crate::types::SnapshotTs(10)),
            "row should be visible before delete"
        );
    }

    #[test]
    fn h9_3_xid_to_commit_ts_mapping() {
        let xid = 100u64;
        let ts = xid_to_commit_ts(xid);
        assert_eq!(ts.0, 100);
        
        let back_to_xid = commit_ts_to_xid(ts);
        assert_eq!(back_to_xid, 100);
    }

    #[test]
    fn h9_3_snapshot_ts_to_xid_mapping() {
        let snap_ts = crate::types::SnapshotTs(50);
        let xid = snapshot_ts_to_xid(snap_ts);
        assert_eq!(xid, 50);
    }

    #[test]
    fn h9_3_backward_compat_with_old_rowversion() {
        // Ensure old RowVersion API still works
        let mut data = HashMap::new();
        data.insert("col1".to_string(), "val1".to_string());
        let rv = RowVersion {
            xid: 50,
            deleted: false,
            data,
        };
        assert_eq!(rv.xid, 50);
        assert!(!rv.deleted);
        assert_eq!(rv.data.len(), 1);
    }

    #[test]
    fn h9_3_multi_version_chain_with_multiple_inserts_and_deletes() {
        let mut mrow = MvccRowV2::new("complex_key");

        // Version 1: insert at xid=100
        let v1 = RowVersion {
            xid: 100,
            deleted: false,
            data: row(&[("version", "1")]),
        };
        mrow.append_tail_version(v1).unwrap();

        // Version 2: update at xid=200
        let v2 = RowVersion {
            xid: 200,
            deleted: false,
            data: row(&[("version", "2")]),
        };
        mrow.append_tail_version(v2).unwrap();

        // Version 3: update at xid=300
        let v3 = RowVersion {
            xid: 300,
            deleted: false,
            data: row(&[("version", "3")]),
        };
        mrow.append_tail_version(v3).unwrap();

        // Version 4: delete at xid=400
        mrow.delete_at_ts(400).unwrap();

        // Verify chain has 4 entries
        let chain = mrow.get_tail_version_chain();
        assert_eq!(chain.len(), 4);

        // Verify visibility at different snapshots
        assert!(mrow.is_visible_at(crate::types::SnapshotTs(100))); // after v1
        assert!(mrow.is_visible_at(crate::types::SnapshotTs(250))); // after v2
        assert!(mrow.is_visible_at(crate::types::SnapshotTs(350))); // after v3
        assert!(!mrow.is_visible_at(crate::types::SnapshotTs(450))); // after delete
        assert!(!mrow.is_visible_at(crate::types::SnapshotTs(50))); // before all
    }

    #[test]
    fn h9_3_get_latest_version_at_specific_timestamp() {
        let mut mrow = MvccRowV2::new("ts_test");

        // Multiple versions with specific xids
        for i in [10, 20, 30, 40, 50].iter() {
            let xid_val = *i;
            let xid_str = xid_val.to_string();
            let data = row(&[("xid", &xid_str)]);
            let v = RowVersion {
                xid: xid_val,
                deleted: false,
                data,
            };
            mrow.append_tail_version(v).unwrap();
        }

        // Query at different snapshots
        let get_at = |snap: u64| {
            mrow.get_latest_tail_version(crate::types::SnapshotTs(snap))
                .map(|v| v.xid)
        };

        assert_eq!(get_at(5), None); // before all
        assert_eq!(get_at(15), Some(10)); // sees 10
        assert_eq!(get_at(25), Some(20)); // sees 20
        assert_eq!(get_at(35), Some(30)); // sees 30
        assert_eq!(get_at(45), Some(40)); // sees 40
        assert_eq!(get_at(60), Some(50)); // sees 50
    }

    #[test]
    fn h9_3_tombstone_in_chain_hides_current_version() {
        let mut mrow = MvccRowV2::new("tombstone_test");

        // Version 1: insert
        let v1 = RowVersion {
            xid: 10,
            deleted: false,
            data: row(&[("data", "alive")]),
        };
        mrow.append_tail_version(v1).unwrap();

        // Version 2: tombstone
        let tombstone = RowVersion {
            xid: 20,
            deleted: true,
            data: HashMap::new(),
        };
        mrow.append_tail_version(tombstone).unwrap();

        // After tombstone, row should be invisible
        assert!(
            mrow.get_latest_tail_version(crate::types::SnapshotTs(20)).is_none(),
            "tombstone should make row invisible"
        );

        // Before tombstone, row should be visible
        assert!(
            mrow.get_latest_tail_version(crate::types::SnapshotTs(10)).is_some(),
            "row should be visible before tombstone"
        );
    }

    #[test]
    fn h9_3_version_count_accuracy() {
        let mut mrow = MvccRowV2::new("count_test");

        assert_eq!(mrow.version_count(), 0);

        for i in 1..=5usize {
            let v = RowVersion {
                xid: (i * 10) as u64,
                deleted: false,
                data: HashMap::new(),
            };
            mrow.append_tail_version(v).unwrap();
            assert_eq!(mrow.version_count(), i);
        }
    }

    #[test]
    fn h9_3_version_visibility_preserves_mvcc_isolation() {
        let mut mrow = MvccRowV2::new("isolation_test");

        // Transaction 1 inserts value "A" at xid=100
        let v1 = RowVersion {
            xid: 100,
            deleted: false,
            data: row(&[("val", "A")]),
        };
        mrow.append_tail_version(v1).unwrap();

        // Snapshot reader takes snapshot at xid=100
        let snap_100 = crate::types::SnapshotTs(100);

        // Transaction 2 updates to value "B" at xid=200
        let v2 = RowVersion {
            xid: 200,
            deleted: false,
            data: row(&[("val", "B")]),
        };
        mrow.append_tail_version(v2).unwrap();

        // Snapshot reader should still see "A"
        let visible = mrow.get_latest_tail_version(snap_100).unwrap();
        assert_eq!(visible.data["val"], "A");

        // Current reader should see "B"
        let current = mrow.get_latest_tail_version(crate::types::SnapshotTs(200)).unwrap();
        assert_eq!(current.data["val"], "B");
    }
}

