#![forbid(unsafe_code)]

use crate::{
    ConnectorDescriptor, ConnectorDirection, IngestFormat, IngestRecord, IngestionConnector,
};

/// A connector that parses newline-delimited JSON (NDJSON) into `IngestRecord`s.
///
/// Each line must be a JSON object. The connector extracts a designated key field
/// and stores the full line as the payload.
#[derive(Debug, Clone)]
pub struct JsonConnector {
    descriptor: ConnectorDescriptor,
    key_field: String,
    records: Vec<IngestRecord>,
}

impl JsonConnector {
    pub fn new(id: &str, display_name: &str, key_field: &str) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: id.to_string(),
                display_name: display_name.to_string(),
                format: IngestFormat::Json,
                direction: ConnectorDirection::Inbound,
            },
            key_field: key_field.to_string(),
            records: Vec::new(),
        }
    }

    /// Parse NDJSON text. Returns the number of records successfully parsed.
    ///
    /// Lines that are not valid JSON objects or lack the key field are skipped.
    pub fn load_ndjson(&mut self, ndjson_text: &str) -> usize {
        self.records.clear();
        for line in ndjson_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(key) = extract_string_field(trimmed, &self.key_field) {
                self.records.push(IngestRecord {
                    key,
                    payload: trimmed.to_string(),
                });
            }
        }
        self.records.len()
    }
}

impl IngestionConnector for JsonConnector {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    fn read_batch(&self, max_items: usize) -> Vec<IngestRecord> {
        self.records.iter().take(max_items).cloned().collect()
    }
}

/// Lightweight JSON string-field extractor that avoids a full JSON parser dependency.
///
/// Looks for `"field":"value"` or `"field": "value"` patterns.
/// Only handles simple string values (no nested objects/arrays or escaped quotes in values).
fn extract_string_field(json_line: &str, field: &str) -> Option<String> {
    let pattern = format!("\"{}\"", field);
    let field_start = json_line.find(&pattern)?;
    let after_key = &json_line[field_start + pattern.len()..];
    // Skip optional whitespace + colon + optional whitespace
    let after_colon = after_key.trim_start();
    let after_colon = after_colon.strip_prefix(':')?;
    let after_colon = after_colon.trim_start();

    if let Some(rest) = after_colon.strip_prefix('"') {
        // String value — find the closing quote
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else {
        // Numeric or boolean — take until comma, closing brace, or whitespace
        let end = after_colon
            .find(|c: char| c == ',' || c == '}' || c == ']' || c.is_whitespace())
            .unwrap_or(after_colon.len());
        let val = after_colon[..end].trim();
        if val.is_empty() {
            None
        } else {
            Some(val.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ndjson_with_string_keys() {
        let ndjson = r#"{"id":"1","name":"Alice","region":"us-east"}
{"id":"2","name":"Bob","region":"eu-west"}
"#;
        let mut conn = JsonConnector::new("json-test", "JSON Test", "id");
        let count = conn.load_ndjson(ndjson);
        assert_eq!(count, 2);
        let batch = conn.read_batch(10);
        assert_eq!(batch[0].key, "1");
        assert!(batch[0].payload.contains("Alice"));
        assert_eq!(batch[1].key, "2");
    }

    #[test]
    fn parses_numeric_key_values() {
        let ndjson = r#"{"order_id": 100, "item": "widget"}
{"order_id": 200, "item": "gadget"}
"#;
        let mut conn = JsonConnector::new("json-num", "JSON Num", "order_id");
        let count = conn.load_ndjson(ndjson);
        assert_eq!(count, 2);
        let batch = conn.read_batch(10);
        assert_eq!(batch[0].key, "100");
        assert_eq!(batch[1].key, "200");
    }

    #[test]
    fn skips_lines_missing_key_field() {
        let ndjson = r#"{"id":"1","v":"a"}
{"no_id":"2","v":"b"}
{"id":"3","v":"c"}
"#;
        let mut conn = JsonConnector::new("json-skip", "JSON Skip", "id");
        let count = conn.load_ndjson(ndjson);
        assert_eq!(count, 2);
    }

    #[test]
    fn handles_empty_input() {
        let mut conn = JsonConnector::new("json-empty", "JSON Empty", "id");
        assert_eq!(conn.load_ndjson(""), 0);
        assert_eq!(conn.load_ndjson("\n\n"), 0);
    }

    #[test]
    fn read_batch_respects_max() {
        let ndjson = r#"{"id":"1"}
{"id":"2"}
{"id":"3"}
"#;
        let mut conn = JsonConnector::new("json-max", "JSON Max", "id");
        conn.load_ndjson(ndjson);
        let batch = conn.read_batch(2);
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn descriptor_reflects_json_format() {
        let conn = JsonConnector::new("json-desc", "JSON Desc", "id");
        assert_eq!(conn.descriptor().format, IngestFormat::Json);
    }
}
