#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap};

/// Describes the kind of index maintained by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexKind {
    BTree,
    Hash,
}

/// Metadata about a single index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDescriptor {
    pub name: String,
    pub table: String,
    pub column: String,
    pub kind: IndexKind,
    pub unique: bool,
}

/// A lightweight B-tree index that maps column values → sets of row keys.
#[derive(Debug, Clone)]
pub struct BTreeIndex {
    descriptor: IndexDescriptor,
    /// column_value → set of row_keys
    tree: BTreeMap<String, Vec<String>>,
}

impl BTreeIndex {
    pub fn new(descriptor: IndexDescriptor) -> Self {
        Self {
            descriptor,
            tree: BTreeMap::new(),
        }
    }

    pub fn descriptor(&self) -> &IndexDescriptor {
        &self.descriptor
    }

    /// Insert a mapping from `column_value` to `row_key`.
    /// Returns `Err` if the index is unique and the value already exists.
    pub fn insert(&mut self, column_value: &str, row_key: &str) -> Result<(), IndexError> {
        let entry = self.tree.entry(column_value.to_string()).or_default();
        if self.descriptor.unique && !entry.is_empty() {
            return Err(IndexError::UniqueViolation {
                index: self.descriptor.name.clone(),
                value: column_value.to_string(),
            });
        }
        if !entry.contains(&row_key.to_string()) {
            entry.push(row_key.to_string());
        }
        Ok(())
    }

    /// Remove a specific row_key from the given column_value bucket.
    pub fn remove(&mut self, column_value: &str, row_key: &str) {
        if let Some(keys) = self.tree.get_mut(column_value) {
            keys.retain(|k| k != row_key);
            if keys.is_empty() {
                self.tree.remove(column_value);
            }
        }
    }

    /// Exact lookup — returns row keys that match the given column value.
    pub fn lookup(&self, column_value: &str) -> Vec<&str> {
        self.tree
            .get(column_value)
            .map(|keys| keys.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Range scan (inclusive) — returns all row keys whose column values
    /// fall within `[start, end]` in lexicographic order.
    pub fn range_scan(&self, start: &str, end: &str) -> Vec<(&str, Vec<&str>)> {
        self.tree
            .range(start.to_string()..=end.to_string())
            .map(|(val, keys)| (val.as_str(), keys.iter().map(String::as_str).collect()))
            .collect()
    }

    pub fn entry_count(&self) -> usize {
        self.tree.values().map(|v| v.len()).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexError {
    UniqueViolation { index: String, value: String },
    IndexAlreadyExists(String),
    IndexNotFound(String),
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UniqueViolation { index, value } => {
                write!(f, "unique constraint violation on index '{index}' for value '{value}'")
            }
            Self::IndexAlreadyExists(name) => write!(f, "index '{name}' already exists"),
            Self::IndexNotFound(name) => write!(f, "index '{name}' not found"),
        }
    }
}

/// Manages a collection of indexes for a storage engine.
#[derive(Debug, Default)]
pub struct IndexManager {
    indexes: HashMap<String, BTreeIndex>,
}

impl IndexManager {
    pub fn new() -> Self {
        Self {
            indexes: HashMap::new(),
        }
    }

    pub fn create_index(&mut self, descriptor: IndexDescriptor) -> Result<(), IndexError> {
        if self.indexes.contains_key(&descriptor.name) {
            return Err(IndexError::IndexAlreadyExists(descriptor.name.clone()));
        }
        let name = descriptor.name.clone();
        self.indexes.insert(name, BTreeIndex::new(descriptor));
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<IndexDescriptor, IndexError> {
        self.indexes
            .remove(name)
            .map(|idx| idx.descriptor.clone())
            .ok_or_else(|| IndexError::IndexNotFound(name.to_string()))
    }

    pub fn get(&self, name: &str) -> Option<&BTreeIndex> {
        self.indexes.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut BTreeIndex> {
        self.indexes.get_mut(name)
    }

    pub fn list_indexes(&self) -> Vec<&IndexDescriptor> {
        self.indexes.values().map(|idx| idx.descriptor()).collect()
    }

    pub fn index_count(&self) -> usize {
        self.indexes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_descriptor(name: &str, unique: bool) -> IndexDescriptor {
        IndexDescriptor {
            name: name.to_string(),
            table: "orders".to_string(),
            column: "customer_id".to_string(),
            kind: IndexKind::BTree,
            unique,
        }
    }

    #[test]
    fn btree_index_insert_and_lookup() {
        let mut idx = BTreeIndex::new(test_descriptor("idx_customer", false));
        idx.insert("C100", "row-1").unwrap();
        idx.insert("C100", "row-2").unwrap();
        idx.insert("C200", "row-3").unwrap();

        assert_eq!(idx.lookup("C100"), vec!["row-1", "row-2"]);
        assert_eq!(idx.lookup("C200"), vec!["row-3"]);
        assert!(idx.lookup("C999").is_empty());
    }

    #[test]
    fn btree_index_unique_violation_rejected() {
        let mut idx = BTreeIndex::new(test_descriptor("idx_pk", true));
        idx.insert("PK1", "row-1").unwrap();
        let err = idx.insert("PK1", "row-2").unwrap_err();
        assert_eq!(
            err,
            IndexError::UniqueViolation {
                index: "idx_pk".to_string(),
                value: "PK1".to_string()
            }
        );
    }

    #[test]
    fn btree_index_remove_cleans_empty_buckets() {
        let mut idx = BTreeIndex::new(test_descriptor("idx_rm", false));
        idx.insert("V1", "row-1").unwrap();
        idx.insert("V1", "row-2").unwrap();
        idx.remove("V1", "row-1");
        assert_eq!(idx.lookup("V1"), vec!["row-2"]);
        idx.remove("V1", "row-2");
        assert!(idx.lookup("V1").is_empty());
        assert_eq!(idx.entry_count(), 0);
    }

    #[test]
    fn btree_index_range_scan_returns_ordered_results() {
        let mut idx = BTreeIndex::new(test_descriptor("idx_range", false));
        idx.insert("A", "r1").unwrap();
        idx.insert("B", "r2").unwrap();
        idx.insert("C", "r3").unwrap();
        idx.insert("D", "r4").unwrap();

        let results = idx.range_scan("B", "C");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "B");
        assert_eq!(results[1].0, "C");
    }

    #[test]
    fn index_manager_create_drop_lifecycle() {
        let mut mgr = IndexManager::new();
        mgr.create_index(test_descriptor("idx1", false)).unwrap();
        mgr.create_index(test_descriptor("idx2", true)).unwrap();
        assert_eq!(mgr.index_count(), 2);

        let dropped = mgr.drop_index("idx1").unwrap();
        assert_eq!(dropped.name, "idx1");
        assert_eq!(mgr.index_count(), 1);
    }

    #[test]
    fn index_manager_rejects_duplicate_names() {
        let mut mgr = IndexManager::new();
        mgr.create_index(test_descriptor("idx1", false)).unwrap();
        let err = mgr.create_index(test_descriptor("idx1", false)).unwrap_err();
        assert_eq!(err, IndexError::IndexAlreadyExists("idx1".to_string()));
    }

    #[test]
    fn index_manager_drop_unknown_returns_not_found() {
        let mut mgr = IndexManager::new();
        let err = mgr.drop_index("nope").unwrap_err();
        assert_eq!(err, IndexError::IndexNotFound("nope".to_string()));
    }
}
