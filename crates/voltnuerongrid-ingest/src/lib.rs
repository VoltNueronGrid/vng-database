#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-ingest";

use std::collections::HashMap;
use voltnuerongrid_store::wal_adapter::WalAdapter;
use voltnuerongrid_store::WalRecord;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamDirection {
    Inbound,
    Outbound,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamEventEnvelope {
    pub event_id: u64,
    pub stream_name: String,
    pub direction: StreamDirection,
    pub origin: String,
    pub occurred_epoch_ms: u128,
    pub payload_json: String,
    pub attributes: HashMap<String, String>,
}

impl StreamEventEnvelope {
    pub fn replay_key(&self) -> String {
        format!("{}:{}", self.stream_name, self.event_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayCursor {
    pub from_event_id: u64,
    pub max_items: usize,
}

pub trait StreamSource: Send {
    fn next_event(&mut self) -> Option<StreamEventEnvelope>;
}

pub trait StreamSink: Send {
    fn push_event(&mut self, event: &StreamEventEnvelope);
}

#[derive(Default)]
pub struct InMemoryEventLog {
    next_event_id: u64,
    events: Vec<StreamEventEnvelope>,
}

impl InMemoryEventLog {
    pub fn new() -> Self {
        Self {
            next_event_id: 1,
            events: Vec::new(),
        }
    }

    pub fn append_event(
        &mut self,
        stream_name: &str,
        direction: StreamDirection,
        origin: &str,
        payload_json: &str,
        attributes: HashMap<String, String>,
    ) -> StreamEventEnvelope {
        let event = StreamEventEnvelope {
            event_id: self.next_event_id,
            stream_name: stream_name.to_string(),
            direction,
            origin: origin.to_string(),
            occurred_epoch_ms: now_epoch_millis(),
            payload_json: payload_json.to_string(),
            attributes,
        };
        self.next_event_id += 1;
        self.events.push(event.clone());
        event
    }

    pub fn replay(&self, cursor: ReplayCursor) -> Vec<StreamEventEnvelope> {
        self.events
            .iter()
            .filter(|event| event.event_id >= cursor.from_event_id)
            .take(cursor.max_items)
            .cloned()
            .collect()
    }

    pub fn publish_to_sink<S: StreamSink>(&self, cursor: ReplayCursor, sink: &mut S) -> usize {
        let replayed = self.replay(cursor);
        let count = replayed.len();
        for event in replayed {
            sink.push_event(&event);
        }
        count
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn replay_from_store<S: ReplayCursorStore>(
        &self,
        stream_name: &str,
        store: &S,
        max_items: usize,
    ) -> Vec<StreamEventEnvelope> {
        let from_event_id = store
            .load(stream_name)
            .map(|checkpoint| checkpoint + 1)
            .unwrap_or(1);
        self.events
            .iter()
            .filter(|event| event.stream_name == stream_name && event.event_id >= from_event_id)
            .take(max_items)
            .cloned()
            .collect()
    }

    pub fn acknowledge_replay<S: ReplayCursorStore>(
        &self,
        stream_name: &str,
        delivered: &[StreamEventEnvelope],
        store: &mut S,
    ) -> Result<(), String> {
        if let Some(last) = delivered.last() {
            store.save(stream_name, last.event_id)?;
        }
        Ok(())
    }
}

pub trait ReplayCursorStore {
    fn load(&self, stream_name: &str) -> Option<u64>;
    fn save(&mut self, stream_name: &str, last_replayed_event_id: u64) -> Result<(), String>;
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryReplayCursorStore {
    checkpoints: HashMap<String, u64>,
}

impl ReplayCursorStore for InMemoryReplayCursorStore {
    fn load(&self, stream_name: &str) -> Option<u64> {
        self.checkpoints.get(stream_name).copied()
    }

    fn save(&mut self, stream_name: &str, last_replayed_event_id: u64) -> Result<(), String> {
        self.checkpoints
            .insert(stream_name.to_string(), last_replayed_event_id);
        Ok(())
    }
}

pub struct WalBackedReplayCursorStore<A: WalAdapter> {
    adapter: A,
    sequence: u64,
}

impl<A: WalAdapter> WalBackedReplayCursorStore<A> {
    pub fn new(adapter: A) -> Self {
        Self {
            adapter,
            sequence: 1,
        }
    }
}

impl<A: WalAdapter> ReplayCursorStore for WalBackedReplayCursorStore<A> {
    fn load(&self, stream_name: &str) -> Option<u64> {
        let key = format!("replay_cursor::{stream_name}");
        let records = self.adapter.read_all().ok()?;
        records
            .iter()
            .rev()
            .find(|record| record.key == key)
            .and_then(|record| record.value.parse::<u64>().ok())
    }

    fn save(&mut self, stream_name: &str, last_replayed_event_id: u64) -> Result<(), String> {
        let key = format!("replay_cursor::{stream_name}");
        let record = WalRecord {
            sequence: self.sequence,
            timestamp_epoch_ms: now_epoch_millis(),
            key,
            value: last_replayed_event_id.to_string(),
        };
        self.sequence += 1;
        self.adapter
            .append(&record)
            .map_err(|err| format!("wal append failed: {err:?}"))
    }
}

#[derive(Debug, Clone)]
pub struct StaticStreamSource {
    cursor: usize,
    events: Vec<StreamEventEnvelope>,
}

impl StaticStreamSource {
    pub fn new(events: Vec<StreamEventEnvelope>) -> Self {
        Self { cursor: 0, events }
    }
}

impl StreamSource for StaticStreamSource {
    fn next_event(&mut self) -> Option<StreamEventEnvelope> {
        if self.cursor >= self.events.len() {
            return None;
        }
        let event = self.events[self.cursor].clone();
        self.cursor += 1;
        Some(event)
    }
}

#[derive(Debug, Default, Clone)]
pub struct CapturingStreamSink {
    pub delivered: Vec<StreamEventEnvelope>,
}

impl StreamSink for CapturingStreamSink {
    fn push_event(&mut self, event: &StreamEventEnvelope) {
        self.delivered.push(event.clone());
    }
}

fn now_epoch_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before epoch")
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use voltnuerongrid_store::wal_adapter::FileWalAdapter;

    fn unique_wal_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "vng-ws4a-cursor-test-{}-{}.log",
            std::process::id(),
            nanos
        ))
    }

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

    #[test]
    fn appends_replayable_event_envelopes() {
        let mut log = InMemoryEventLog::new();
        let mut attrs = HashMap::new();
        attrs.insert("tenant".to_string(), "acme".to_string());
        let first = log.append_event(
            "ingest.orders",
            StreamDirection::Inbound,
            "ftp-connector",
            "{\"id\":1}",
            attrs,
        );
        let second = log.append_event(
            "ingest.orders",
            StreamDirection::Inbound,
            "ftp-connector",
            "{\"id\":2}",
            HashMap::new(),
        );
        assert_eq!(first.event_id, 1);
        assert_eq!(second.event_id, 2);
        assert_eq!(first.replay_key(), "ingest.orders:1");
        assert_eq!(log.len(), 2);

        let replayed = log.replay(ReplayCursor {
            from_event_id: 2,
            max_items: 10,
        });
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].payload_json, "{\"id\":2}");
    }

    #[test]
    fn streams_from_source_and_replays_to_sink() {
        let source_events = vec![
            StreamEventEnvelope {
                event_id: 101,
                stream_name: "outbound.events".to_string(),
                direction: StreamDirection::Outbound,
                origin: "query-engine".to_string(),
                occurred_epoch_ms: 1000,
                payload_json: "{\"type\":\"query\"}".to_string(),
                attributes: HashMap::new(),
            },
            StreamEventEnvelope {
                event_id: 102,
                stream_name: "outbound.events".to_string(),
                direction: StreamDirection::Outbound,
                origin: "query-engine".to_string(),
                occurred_epoch_ms: 1001,
                payload_json: "{\"type\":\"txn\"}".to_string(),
                attributes: HashMap::new(),
            },
        ];

        let mut source = StaticStreamSource::new(source_events);
        let mut sink = CapturingStreamSink::default();
        while let Some(event) = source.next_event() {
            sink.push_event(&event);
        }

        assert_eq!(sink.delivered.len(), 2);
        assert_eq!(sink.delivered[0].event_id, 101);

        let mut replay_log = InMemoryEventLog::new();
        for delivered in &sink.delivered {
            replay_log.append_event(
                &delivered.stream_name,
                delivered.direction,
                &delivered.origin,
                &delivered.payload_json,
                delivered.attributes.clone(),
            );
        }

        let mut replay_sink = CapturingStreamSink::default();
        let replay_count = replay_log.publish_to_sink(
            ReplayCursor {
                from_event_id: 1,
                max_items: 5,
            },
            &mut replay_sink,
        );
        assert_eq!(replay_count, 2);
        assert_eq!(replay_sink.delivered[1].payload_json, "{\"type\":\"txn\"}");
    }

    #[test]
    fn persists_and_loads_cursor_in_memory() {
        let mut store = InMemoryReplayCursorStore::default();
        assert_eq!(store.load("ingest.orders"), None);
        store.save("ingest.orders", 42).expect("save");
        assert_eq!(store.load("ingest.orders"), Some(42));
    }

    #[test]
    fn persists_and_loads_cursor_in_wal_adapter() {
        let wal_path = unique_wal_path();
        let adapter = FileWalAdapter::new(&wal_path).expect("adapter");
        let mut store = WalBackedReplayCursorStore::new(adapter.clone());

        store.save("ingest.orders", 5).expect("save first");
        store.save("ingest.orders", 8).expect("save second");
        assert_eq!(store.load("ingest.orders"), Some(8));

        let _ = fs::remove_file(adapter.wal_path());
    }

    #[test]
    fn replays_from_persisted_cursor_checkpoint() {
        let mut log = InMemoryEventLog::new();
        log.append_event(
            "ingest.orders",
            StreamDirection::Inbound,
            "ftp",
            "{\"id\":1}",
            HashMap::new(),
        );
        log.append_event(
            "ingest.orders",
            StreamDirection::Inbound,
            "ftp",
            "{\"id\":2}",
            HashMap::new(),
        );
        log.append_event(
            "ingest.orders",
            StreamDirection::Inbound,
            "ftp",
            "{\"id\":3}",
            HashMap::new(),
        );

        let mut store = InMemoryReplayCursorStore::default();
        store.save("ingest.orders", 1).expect("checkpoint");

        let replay = log.replay_from_store("ingest.orders", &store, 10);
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0].event_id, 2);
        assert_eq!(replay[1].event_id, 3);

        log.acknowledge_replay("ingest.orders", &replay, &mut store)
            .expect("ack");
        assert_eq!(store.load("ingest.orders"), Some(3));
    }
}
