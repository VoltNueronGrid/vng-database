//! MVCC (Multi-Version Concurrency Control) page-based row store.
//!
//! Advances S2-WS2-04 in status-tracker-v2.md from **TODO** → **PARTIAL**.
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
                return;
            }
        }
        // New row — append to current tail page (allocate new page if full).
        self.ensure_tail_capacity();
        let mut row = MvccRow::new(key);
        row.push_version(xid, false, data);
        self.pages.last_mut().unwrap().rows.push(row);
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
}
