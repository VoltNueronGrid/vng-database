//! SCALE-2: Compute-Storage Separation Architecture
//!
//! Defines [`StorageNodeClient`] trait that abstracts over local in-process
//! storage and remote storage nodes.  Compute-tier handlers should call this
//! trait so the same logic works in both single-node and disaggregated topologies.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::mvcc::PagedRowStore;

/// Row data alias (column name → value).
pub type RowData = HashMap<String, String>;

/// Errors returned by [`StorageNodeClient`] operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageClientError {
    /// The requested row key was not found.
    NotFound(String),
    /// A transient network or RPC error occurred.
    Transport(String),
    /// The storage node returned an unexpected response.
    Protocol(String),
    /// Storage is at capacity.
    Capacity,
}

impl std::fmt::Display for StorageClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageClientError::NotFound(k) => write!(f, "not_found: {k}"),
            StorageClientError::Transport(m) => write!(f, "transport_error: {m}"),
            StorageClientError::Protocol(m) => write!(f, "protocol_error: {m}"),
            StorageClientError::Capacity => write!(f, "storage_capacity_exceeded"),
        }
    }
}

impl std::error::Error for StorageClientError {}

/// Abstraction over the row-level storage backend.
///
/// `LocalStorageNodeClient` wraps `PagedRowStore` for zero-overhead single-node
/// deployments.  `RemoteStorageNodeClient` (stub) sends HTTP requests to a
/// `VNG_STORAGE_NODE_URL` storage peer, enabling stateless compute nodes.
pub trait StorageNodeClient: Send + Sync {
    /// Read a single row by exact key.
    fn get_row(&self, key: &str) -> Result<RowData, StorageClientError>;

    /// Write (insert or overwrite) a row.
    fn store_row(&self, key: &str, data: RowData) -> Result<(), StorageClientError>;

    /// Delete a row.  Returns `Ok(true)` if the row existed, `Ok(false)` if not.
    fn delete_row(&self, key: &str) -> Result<bool, StorageClientError>;

    /// Scan all rows whose key begins with `prefix`.
    fn scan_prefix(&self, prefix: &str) -> Result<Vec<(String, RowData)>, StorageClientError>;

    /// Retrieve the backend type name (for observability).
    fn backend_type(&self) -> &'static str;
}

// ── LocalStorageNodeClient ────────────────────────────────────────────────────

/// Zero-overhead storage client that delegates directly to a `PagedRowStore`
/// mutex.  Used in single-node deployments (the default).
pub struct LocalStorageNodeClient {
    row_store: Arc<Mutex<PagedRowStore>>,
}

impl LocalStorageNodeClient {
    /// Create a local client wrapping the given `PagedRowStore`.
    pub fn new(row_store: Arc<Mutex<PagedRowStore>>) -> Self {
        Self { row_store }
    }
}

impl StorageNodeClient for LocalStorageNodeClient {
    fn get_row(&self, key: &str) -> Result<RowData, StorageClientError> {
        let rs = self.row_store.lock().map_err(|e| StorageClientError::Transport(e.to_string()))?;
        let xid = rs.current_xid();
        rs.read_at_snapshot(key, xid)
            .cloned()
            .ok_or_else(|| StorageClientError::NotFound(key.to_string()))
    }

    fn store_row(&self, key: &str, data: RowData) -> Result<(), StorageClientError> {
        let mut rs = self.row_store.lock().map_err(|e| StorageClientError::Transport(e.to_string()))?;
        let xid = rs.begin_xid();
        rs.insert(xid, key, data);
        Ok(())
    }

    fn delete_row(&self, key: &str) -> Result<bool, StorageClientError> {
        let mut rs = self.row_store.lock().map_err(|e| StorageClientError::Transport(e.to_string()))?;
        let xid = rs.begin_xid();
        Ok(rs.delete(xid, key))
    }

    fn scan_prefix(&self, prefix: &str) -> Result<Vec<(String, RowData)>, StorageClientError> {
        let rs = self.row_store.lock().map_err(|e| StorageClientError::Transport(e.to_string()))?;
        let xid = rs.current_xid();
        let results = rs
            .scan_at_snapshot(xid)
            .into_iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        Ok(results)
    }

    fn backend_type(&self) -> &'static str {
        "local"
    }
}

// ── RemoteStorageNodeClient ───────────────────────────────────────────────────

/// Stub HTTP-based storage client for a remote storage node.
///
/// In production this would use a typed HTTP/gRPC client to forward storage
/// operations to `VNG_STORAGE_NODE_URL`.  The stub returns `Transport` errors
/// so the compute tier can fall back gracefully until the full implementation
/// is wired.
pub struct RemoteStorageNodeClient {
    /// Base URL of the remote storage node, e.g. `http://storage-node-1:8090`.
    pub node_url: String,
}

impl RemoteStorageNodeClient {
    pub fn new(node_url: impl Into<String>) -> Self {
        Self { node_url: node_url.into() }
    }
}

impl StorageNodeClient for RemoteStorageNodeClient {
    fn get_row(&self, key: &str) -> Result<RowData, StorageClientError> {
        // SCALE-2 stub: full gRPC/HTTP transport not yet wired.
        Err(StorageClientError::Transport(format!(
            "remote_storage_not_connected: {} key={key}",
            self.node_url
        )))
    }

    fn store_row(&self, _key: &str, _data: RowData) -> Result<(), StorageClientError> {
        Err(StorageClientError::Transport(format!(
            "remote_storage_not_connected: {}",
            self.node_url
        )))
    }

    fn delete_row(&self, _key: &str) -> Result<bool, StorageClientError> {
        Err(StorageClientError::Transport(format!(
            "remote_storage_not_connected: {}",
            self.node_url
        )))
    }

    fn scan_prefix(&self, _prefix: &str) -> Result<Vec<(String, RowData)>, StorageClientError> {
        Err(StorageClientError::Transport(format!(
            "remote_storage_not_connected: {}",
            self.node_url
        )))
    }

    fn backend_type(&self) -> &'static str {
        "remote"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_local_client() -> LocalStorageNodeClient {
        let rs = Arc::new(Mutex::new(PagedRowStore::new(256)));
        LocalStorageNodeClient::new(rs)
    }

    #[test]
    fn scale2_local_store_and_get_row() {
        let client = make_local_client();
        let mut data = HashMap::new();
        data.insert("name".to_string(), "Alice".to_string());
        client.store_row("users:u1", data.clone()).unwrap();
        let retrieved = client.get_row("users:u1").unwrap();
        assert_eq!(retrieved.get("name").map(|s| s.as_str()), Some("Alice"));
    }

    #[test]
    fn scale2_local_delete_row() {
        let client = make_local_client();
        let mut data = HashMap::new();
        data.insert("x".to_string(), "1".to_string());
        client.store_row("t:k1", data).unwrap();
        let deleted = client.delete_row("t:k1").unwrap();
        assert!(deleted, "should return true when row existed");
        let res = client.get_row("t:k1");
        assert!(res.is_err(), "row should be gone after delete");
    }

    #[test]
    fn scale2_local_scan_prefix() {
        let client = make_local_client();
        let mut d1 = HashMap::new(); d1.insert("v".to_string(), "a".to_string());
        let mut d2 = HashMap::new(); d2.insert("v".to_string(), "b".to_string());
        let mut d3 = HashMap::new(); d3.insert("v".to_string(), "c".to_string());
        client.store_row("orders:o1", d1).unwrap();
        client.store_row("orders:o2", d2).unwrap();
        client.store_row("users:u1", d3).unwrap();
        let rows = client.scan_prefix("orders:").unwrap();
        assert_eq!(rows.len(), 2, "only orders: rows should be returned");
    }

    #[test]
    fn scale2_remote_client_returns_transport_error() {
        let client = RemoteStorageNodeClient::new("http://storage-node:8090");
        assert!(matches!(
            client.get_row("any:key"),
            Err(StorageClientError::Transport(_))
        ));
    }

    #[test]
    fn scale2_backend_type_names() {
        let local = make_local_client();
        assert_eq!(local.backend_type(), "local");
        let remote = RemoteStorageNodeClient::new("http://x:8090");
        assert_eq!(remote.backend_type(), "remote");
    }
}
