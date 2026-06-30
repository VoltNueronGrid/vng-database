//! B-6: Multimodel / document (JSONB) support — path and containment operators
//! plus a GIN-like inverted index over top-level keys.
//!
//! JSONB values are stored as ordinary string columns (the engine's `RowData`
//! is `HashMap<String, String>`); this module interprets those strings as JSON
//! and implements the PostgreSQL-style operators:
//!
//! - `->`  path get, returns the child JSON value
//! - `->>` path get as text, returns the child rendered as a string
//! - `@>`  containment, true when the left document contains the right document

use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// `json -> key` / `json -> index`: return the child JSON value at a top-level
/// object key or array index. Returns `None` when absent or the operand type
/// does not match the JSON shape.
pub(crate) fn json_get(doc: &Value, key: &str) -> Option<Value> {
    match doc {
        Value::Object(map) => map.get(key).cloned(),
        Value::Array(arr) => key.parse::<usize>().ok().and_then(|i| arr.get(i).cloned()),
        _ => None,
    }
}

/// `json ->> key`: return the child at `key` rendered as text. Strings are
/// returned without surrounding quotes; other JSON scalars/containers use their
/// compact JSON rendering.
pub(crate) fn json_get_text(doc: &Value, key: &str) -> Option<String> {
    json_get(doc, key).map(|v| value_as_text(&v))
}

/// Render a JSON value as `->>`-style text.
pub(crate) fn value_as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

/// `left @> right`: containment. True when every key/value in `right` is present
/// in `left` (recursively for nested objects, and as a subset for arrays).
pub(crate) fn json_contains(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Object(l), Value::Object(r)) => r.iter().all(|(k, rv)| {
            l.get(k).map(|lv| json_contains(lv, rv)).unwrap_or(false)
        }),
        (Value::Array(l), Value::Array(r)) => r.iter().all(|rv| l.iter().any(|lv| json_contains(lv, rv))),
        // For an array on the left and a scalar on the right, containment means
        // the scalar is one of the array elements.
        (Value::Array(l), scalar) => l.iter().any(|lv| lv == scalar),
        (lv, rv) => lv == rv,
    }
}

/// Parse a JSON string, returning `None` when it is not valid JSON.
pub(crate) fn parse_json(s: &str) -> Option<Value> {
    serde_json::from_str::<Value>(s).ok()
}

/// A JSONB document operator predicate used to filter rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JsonbPredicate {
    /// `col ->> 'key' = 'value'`
    PathEqText { key: String, value: String },
    /// `col @> '{...}'`
    Contains { doc: String },
    /// `col ? 'key'` — top-level key existence.
    HasKey { key: String },
}

/// Evaluate a JSONB predicate against a raw JSON string from a row column.
pub(crate) fn eval_predicate(json_str: &str, pred: &JsonbPredicate) -> bool {
    let Some(doc) = parse_json(json_str) else {
        return false;
    };
    match pred {
        JsonbPredicate::PathEqText { key, value } => {
            json_get_text(&doc, key).map(|t| &t == value).unwrap_or(false)
        }
        JsonbPredicate::Contains { doc: rhs } => {
            parse_json(rhs).map(|r| json_contains(&doc, &r)).unwrap_or(false)
        }
        JsonbPredicate::HasKey { key } => match &doc {
            Value::Object(map) => map.contains_key(key),
            _ => false,
        },
    }
}

/// GIN-like inverted index mapping each top-level JSON key to the set of row
/// keys whose document contains that key. Accelerates key-existence and
/// path-predicate lookups without a full scan.
#[derive(Debug, Default)]
pub(crate) struct JsonbKeyIndex {
    by_key: HashMap<String, HashSet<String>>,
}

impl JsonbKeyIndex {
    pub(crate) fn new() -> Self {
        Self { by_key: HashMap::new() }
    }

    /// Index a document under `row_key`, registering all of its top-level keys.
    pub(crate) fn index_document(&mut self, row_key: &str, json_str: &str) {
        if let Some(Value::Object(map)) = parse_json(json_str) {
            for k in map.keys() {
                self.by_key.entry(k.clone()).or_default().insert(row_key.to_string());
            }
        }
    }

    /// Row keys whose document has the given top-level key.
    pub(crate) fn rows_with_key(&self, key: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .by_key
            .get(key)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        out.sort();
        out
    }

    /// Number of distinct indexed top-level keys.
    pub(crate) fn key_count(&self) -> usize {
        self.by_key.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn path_get_and_text() {
        let doc = json!({ "name": "ada", "age": 36, "tags": ["a", "b"] });
        assert_eq!(json_get(&doc, "name"), Some(json!("ada")));
        assert_eq!(json_get_text(&doc, "name").as_deref(), Some("ada"));
        assert_eq!(json_get_text(&doc, "age").as_deref(), Some("36"));
        assert_eq!(json_get(&doc, "missing"), None);
        // Array index path.
        let arr = json_get(&doc, "tags").unwrap();
        assert_eq!(json_get_text(&arr, "0").as_deref(), Some("a"));
    }

    #[test]
    fn containment_objects_and_arrays() {
        let left = json!({ "a": 1, "b": { "c": 2 }, "tags": ["x", "y", "z"] });
        assert!(json_contains(&left, &json!({ "a": 1 })));
        assert!(json_contains(&left, &json!({ "b": { "c": 2 } })));
        assert!(json_contains(&left, &json!({ "tags": ["x", "z"] })));
        assert!(!json_contains(&left, &json!({ "a": 2 })));
        assert!(!json_contains(&left, &json!({ "missing": 1 })));
    }

    #[test]
    fn predicate_evaluation() {
        let row = r#"{"status":"active","level":3}"#;
        assert!(eval_predicate(row, &JsonbPredicate::PathEqText { key: "status".into(), value: "active".into() }));
        assert!(!eval_predicate(row, &JsonbPredicate::PathEqText { key: "status".into(), value: "inactive".into() }));
        assert!(eval_predicate(row, &JsonbPredicate::Contains { doc: r#"{"level":3}"#.into() }));
        assert!(eval_predicate(row, &JsonbPredicate::HasKey { key: "level".into() }));
        assert!(!eval_predicate(row, &JsonbPredicate::HasKey { key: "missing".into() }));
        assert!(!eval_predicate("not json", &JsonbPredicate::HasKey { key: "x".into() }));
    }

    #[test]
    fn key_index_build_and_lookup() {
        let mut idx = JsonbKeyIndex::new();
        idx.index_document("doc:1", r#"{"name":"a","email":"x@y"}"#);
        idx.index_document("doc:2", r#"{"name":"b"}"#);
        assert_eq!(idx.key_count(), 2);
        assert_eq!(idx.rows_with_key("name"), vec!["doc:1", "doc:2"]);
        assert_eq!(idx.rows_with_key("email"), vec!["doc:1"]);
        assert!(idx.rows_with_key("missing").is_empty());
    }
}
