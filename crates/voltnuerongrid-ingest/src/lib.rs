#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-ingest";

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestFormat {
    Csv,
    Parquet,
    Json,
    Excel,
    Stream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorDirection {
    Inbound,
    Outbound,
    Bidirectional,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestRecord {
    pub key: String,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorDescriptor {
    pub id: String,
    pub display_name: String,
    pub format: IngestFormat,
    pub direction: ConnectorDirection,
}

pub trait IngestionConnector: Send + Sync {
    fn descriptor(&self) -> &ConnectorDescriptor;
    fn read_batch(&self, max_items: usize) -> Vec<IngestRecord>;
}

#[derive(Default)]
pub struct ConnectorRegistry {
    connectors: HashMap<String, Box<dyn IngestionConnector>>,
}

impl ConnectorRegistry {
    pub fn register(&mut self, connector: Box<dyn IngestionConnector>) {
        self.connectors
            .insert(connector.descriptor().id.clone(), connector);
    }

    pub fn has_connector(&self, id: &str) -> bool {
        self.connectors.contains_key(id)
    }

    pub fn read_batch(&self, connector_id: &str, max_items: usize) -> Option<Vec<IngestRecord>> {
        self.connectors
            .get(connector_id)
            .map(|connector| connector.read_batch(max_items))
    }
}

#[derive(Debug, Clone)]
pub struct StaticInMemoryConnector {
    descriptor: ConnectorDescriptor,
    records: Vec<IngestRecord>,
}

impl StaticInMemoryConnector {
    pub fn new(descriptor: ConnectorDescriptor, records: Vec<IngestRecord>) -> Self {
        Self {
            descriptor,
            records,
        }
    }
}

impl IngestionConnector for StaticInMemoryConnector {
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
    fn registers_connector_and_reads_batch() {
        let connector = StaticInMemoryConnector::new(
            ConnectorDescriptor {
                id: "csv-local".to_string(),
                display_name: "CSV Local".to_string(),
                format: IngestFormat::Csv,
                direction: ConnectorDirection::Inbound,
            },
            vec![
                IngestRecord {
                    key: "k1".to_string(),
                    payload: "{\"amount\":10}".to_string(),
                },
                IngestRecord {
                    key: "k2".to_string(),
                    payload: "{\"amount\":20}".to_string(),
                },
            ],
        );

        let mut registry = ConnectorRegistry::default();
        registry.register(Box::new(connector));
        assert!(registry.has_connector("csv-local"));

        let batch = registry
            .read_batch("csv-local", 1)
            .expect("connector should exist");
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].key, "k1");
    }
}
