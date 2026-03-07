#![forbid(unsafe_code)]

use crate::{
    ConnectorDescriptor, ConnectorDirection, IngestFormat, IngestRecord, IngestionConnector,
};

/// A connector that parses CSV text into `IngestRecord`s.
///
/// Expected format: first line is a header row, subsequent lines are data rows.
/// The first column is used as the record key; remaining columns are joined
/// back into a comma-separated payload string.
#[derive(Debug, Clone)]
pub struct CsvConnector {
    descriptor: ConnectorDescriptor,
    records: Vec<IngestRecord>,
}

impl CsvConnector {
    pub fn new(id: &str, display_name: &str) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: id.to_string(),
                display_name: display_name.to_string(),
                format: IngestFormat::Csv,
                direction: ConnectorDirection::Inbound,
            },
            records: Vec::new(),
        }
    }

    /// Parse raw CSV text. Returns the number of data rows parsed.
    pub fn load_csv(&mut self, csv_text: &str) -> usize {
        self.records.clear();
        let mut lines = csv_text.lines();
        // Skip the header row
        let _header = match lines.next() {
            Some(h) => h,
            None => return 0,
        };

        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut cols = trimmed.splitn(2, ',');
            let key = cols.next().unwrap_or("").trim().to_string();
            let payload = cols.next().unwrap_or("").trim().to_string();
            if !key.is_empty() {
                self.records.push(IngestRecord { key, payload });
            }
        }
        self.records.len()
    }
}

impl IngestionConnector for CsvConnector {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    fn read_batch(&self, max_items: usize) -> Vec<IngestRecord> {
        self.records.iter().take(max_items).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_csv() {
        let csv = "id,name,region\n1,Alice,us-east\n2,Bob,eu-west\n";
        let mut conn = CsvConnector::new("csv-test", "CSV Test");
        let count = conn.load_csv(csv);
        assert_eq!(count, 2);
        let batch = conn.read_batch(10);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].key, "1");
        assert_eq!(batch[0].payload, "Alice,us-east");
        assert_eq!(batch[1].key, "2");
        assert_eq!(batch[1].payload, "Bob,eu-west");
    }

    #[test]
    fn skips_empty_lines() {
        let csv = "id,value\n\n1,foo\n\n2,bar\n\n";
        let mut conn = CsvConnector::new("csv-skip", "CSV Skip");
        let count = conn.load_csv(csv);
        assert_eq!(count, 2);
    }

    #[test]
    fn handles_empty_input() {
        let mut conn = CsvConnector::new("csv-empty", "CSV Empty");
        assert_eq!(conn.load_csv(""), 0);
        assert_eq!(conn.load_csv("header_only"), 0);
    }

    #[test]
    fn read_batch_respects_max() {
        let csv = "id,v\n1,a\n2,b\n3,c\n";
        let mut conn = CsvConnector::new("csv-max", "CSV Max");
        conn.load_csv(csv);
        let batch = conn.read_batch(2);
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn descriptor_reflects_csv_format() {
        let conn = CsvConnector::new("csv-desc", "CSV Desc");
        assert_eq!(conn.descriptor().format, IngestFormat::Csv);
        assert_eq!(conn.descriptor().direction, ConnectorDirection::Inbound);
    }
}
