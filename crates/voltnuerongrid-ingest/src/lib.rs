#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-ingest";

pub mod csv;
pub mod json;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use voltnuerongrid_store::wal_adapter::{FileWalAdapter, WalAdapter};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventBusTransportEvent {
    pub transport_sequence: u64,
    pub event: StreamEventEnvelope,
}

pub trait EventBusBrokerClient: Send {
    fn broker_kind(&self) -> &'static str;

    fn broker_target(&self) -> Option<String> {
        None
    }

    fn publish(
        &mut self,
        stream_name: &str,
        direction: StreamDirection,
        origin: &str,
        payload_json: &str,
        attributes: HashMap<String, String>,
    ) -> Result<EventBusTransportEvent, String>;

    fn export_for_stream_since(
        &self,
        stream_name: &str,
        last_event_id: u64,
        max_items: usize,
    ) -> Vec<StreamEventEnvelope>;

    fn total_events(&self) -> usize;

    fn last_event_id_for_stream(&self, stream_name: &str) -> Option<u64>;

    fn snapshot_events(&self) -> Vec<EventBusTransportEvent>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalBrokerKind {
    Kafka,
    Nats,
    EventHubs,
}

impl ExternalBrokerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Kafka => "kafka",
            Self::Nats => "nats",
            Self::EventHubs => "event_hubs",
        }
    }

    pub fn from_broker_mode(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "kafka" => Some(Self::Kafka),
            "nats" => Some(Self::Nats),
            "event_hubs" | "eventhubs" | "event-hubs" | "azure_event_hubs" => {
                Some(Self::EventHubs)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalBrokerDispatch {
    pub broker_kind: ExternalBrokerKind,
    pub broker_target: String,
    pub subject: String,
    pub key: String,
    pub payload_json: String,
    pub headers: HashMap<String, String>,
}

pub trait ExternalBrokerPublisher: Send {
    fn broker_kind(&self) -> ExternalBrokerKind;

    fn broker_target(&self) -> &str;

    fn publish(&mut self, dispatch: &ExternalBrokerDispatch) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredExternalBrokerPublisher {
    broker_kind: ExternalBrokerKind,
    broker_target: String,
    published: Vec<ExternalBrokerDispatch>,
}

impl ConfiguredExternalBrokerPublisher {
    pub fn new(broker_kind: ExternalBrokerKind, broker_target: &str) -> Result<Self, String> {
        let broker_target = broker_target.trim();
        if broker_target.is_empty() {
            return Err(format!(
                "external broker target is required for {}",
                broker_kind.as_str()
            ));
        }

        Ok(Self {
            broker_kind,
            broker_target: broker_target.to_string(),
            published: Vec::new(),
        })
    }

    pub fn published(&self) -> &[ExternalBrokerDispatch] {
        &self.published
    }
}

impl ExternalBrokerPublisher for ConfiguredExternalBrokerPublisher {
    fn broker_kind(&self) -> ExternalBrokerKind {
        self.broker_kind
    }

    fn broker_target(&self) -> &str {
        &self.broker_target
    }

    fn publish(&mut self, dispatch: &ExternalBrokerDispatch) -> Result<(), String> {
        self.published.push(dispatch.clone());
        Ok(())
    }
}

pub struct ExternalBrokerBackedEventBusTransport {
    ledger: FileBackedEventBusTransport,
    publisher: Box<dyn ExternalBrokerPublisher>,
    subject_prefix: String,
}

impl ExternalBrokerBackedEventBusTransport {
    pub fn new<P: AsRef<Path>>(
        broker_kind: ExternalBrokerKind,
        wal_path: P,
        broker_target: &str,
        subject_prefix: Option<&str>,
    ) -> Result<Self, String> {
        let publisher = ConfiguredExternalBrokerPublisher::new(broker_kind, broker_target)?;
        Self::with_publisher(wal_path, Box::new(publisher), subject_prefix)
    }

    pub fn with_publisher<P: AsRef<Path>>(
        wal_path: P,
        publisher: Box<dyn ExternalBrokerPublisher>,
        subject_prefix: Option<&str>,
    ) -> Result<Self, String> {
        Ok(Self {
            ledger: FileBackedEventBusTransport::new(wal_path)?,
            publisher,
            subject_prefix: subject_prefix.unwrap_or("").trim().to_string(),
        })
    }

    fn subject_for_stream(&self, stream_name: &str) -> String {
        if self.subject_prefix.is_empty() {
            stream_name.to_string()
        } else {
            format!("{}.{}", self.subject_prefix.trim_end_matches('.'), stream_name)
        }
    }

    pub fn broker_target(&self) -> &str {
        self.publisher.broker_target()
    }
}

impl EventBusBrokerClient for ExternalBrokerBackedEventBusTransport {
    fn broker_kind(&self) -> &'static str {
        self.publisher.broker_kind().as_str()
    }

    fn broker_target(&self) -> Option<String> {
        Some(self.publisher.broker_target().to_string())
    }

    fn publish(
        &mut self,
        stream_name: &str,
        direction: StreamDirection,
        origin: &str,
        payload_json: &str,
        attributes: HashMap<String, String>,
    ) -> Result<EventBusTransportEvent, String> {
        let event = self
            .ledger
            .publish(stream_name, direction, origin, payload_json, attributes.clone())?;
        let mut headers = attributes;
        headers.insert("stream_name".to_string(), event.event.stream_name.clone());
        headers.insert(
            "transport_sequence".to_string(),
            event.transport_sequence.to_string(),
        );
        headers.insert("origin".to_string(), event.event.origin.clone());

        let dispatch = ExternalBrokerDispatch {
            broker_kind: self.publisher.broker_kind(),
            broker_target: self.publisher.broker_target().to_string(),
            subject: self.subject_for_stream(stream_name),
            key: event.event.replay_key(),
            payload_json: event.event.payload_json.clone(),
            headers,
        };
        self.publisher.publish(&dispatch)?;
        Ok(event)
    }

    fn export_for_stream_since(
        &self,
        stream_name: &str,
        last_event_id: u64,
        max_items: usize,
    ) -> Vec<StreamEventEnvelope> {
        self.ledger
            .export_for_stream_since(stream_name, last_event_id, max_items)
    }

    fn total_events(&self) -> usize {
        self.ledger.total_events()
    }

    fn last_event_id_for_stream(&self, stream_name: &str) -> Option<u64> {
        self.ledger.last_event_id_for_stream(stream_name)
    }

    fn snapshot_events(&self) -> Vec<EventBusTransportEvent> {
        self.ledger.events().to_vec()
    }
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryEventBusTransport {
    next_transport_sequence: u64,
    next_event_id: u64,
    events: Vec<EventBusTransportEvent>,
}

impl InMemoryEventBusTransport {
    pub fn new() -> Self {
        Self {
            next_transport_sequence: 1,
            next_event_id: 1,
            events: Vec::new(),
        }
    }

    pub fn publish(
        &mut self,
        stream_name: &str,
        direction: StreamDirection,
        origin: &str,
        payload_json: &str,
        attributes: HashMap<String, String>,
    ) -> EventBusTransportEvent {
        let event = EventBusTransportEvent {
            transport_sequence: self.next_transport_sequence,
            event: StreamEventEnvelope {
                event_id: self.next_event_id,
                stream_name: stream_name.to_string(),
                direction,
                origin: origin.to_string(),
                occurred_epoch_ms: now_epoch_millis(),
                payload_json: payload_json.to_string(),
                attributes,
            },
        };
        self.next_transport_sequence += 1;
        self.next_event_id += 1;
        self.events.push(event.clone());
        event
    }

    pub fn export_for_stream_since(
        &self,
        stream_name: &str,
        last_event_id: u64,
        max_items: usize,
    ) -> Vec<StreamEventEnvelope> {
        self.events
            .iter()
            .filter(|event| event.event.stream_name == stream_name && event.event.event_id > last_event_id)
            .take(max_items)
            .map(|event| event.event.clone())
            .collect()
    }

    pub fn total_events(&self) -> usize {
        self.events.len()
    }

    pub fn last_event_id_for_stream(&self, stream_name: &str) -> Option<u64> {
        self.events
            .iter()
            .rev()
            .find(|event| event.event.stream_name == stream_name)
            .map(|event| event.event.event_id)
    }

    pub fn events(&self) -> &[EventBusTransportEvent] {
        &self.events
    }
}

impl EventBusBrokerClient for InMemoryEventBusTransport {
    fn broker_kind(&self) -> &'static str {
        "in_memory"
    }

    fn publish(
        &mut self,
        stream_name: &str,
        direction: StreamDirection,
        origin: &str,
        payload_json: &str,
        attributes: HashMap<String, String>,
    ) -> Result<EventBusTransportEvent, String> {
        Ok(InMemoryEventBusTransport::publish(
            self,
            stream_name,
            direction,
            origin,
            payload_json,
            attributes,
        ))
    }

    fn export_for_stream_since(
        &self,
        stream_name: &str,
        last_event_id: u64,
        max_items: usize,
    ) -> Vec<StreamEventEnvelope> {
        InMemoryEventBusTransport::export_for_stream_since(self, stream_name, last_event_id, max_items)
    }

    fn total_events(&self) -> usize {
        InMemoryEventBusTransport::total_events(self)
    }

    fn last_event_id_for_stream(&self, stream_name: &str) -> Option<u64> {
        InMemoryEventBusTransport::last_event_id_for_stream(self, stream_name)
    }

    fn snapshot_events(&self) -> Vec<EventBusTransportEvent> {
        self.events().to_vec()
    }
}

#[derive(Debug, Clone)]
pub struct FileBackedEventBusTransport {
    wal_path: PathBuf,
    adapter: FileWalAdapter,
    next_transport_sequence: u64,
    next_event_id: u64,
    events: Vec<EventBusTransportEvent>,
}

impl FileBackedEventBusTransport {
    pub fn new<P: AsRef<Path>>(wal_path: P) -> Result<Self, String> {
        let wal_path = wal_path.as_ref().to_path_buf();
        let adapter = FileWalAdapter::new(&wal_path)
            .map_err(|err| format!("event bus adapter init failed: {err:?}"))?;
        let records = adapter
            .read_all()
            .map_err(|err| format!("event bus adapter read failed: {err:?}"))?;
        let mut events = Vec::with_capacity(records.len());
        let mut next_transport_sequence = 1;
        let mut next_event_id = 1;

        for record in records {
            let decoded = decode_event_bus_record(&record)?;
            next_transport_sequence = next_transport_sequence.max(decoded.transport_sequence + 1);
            next_event_id = next_event_id.max(decoded.event.event_id + 1);
            events.push(decoded);
        }

        Ok(Self {
            wal_path,
            adapter,
            next_transport_sequence,
            next_event_id,
            events,
        })
    }

    pub fn wal_path(&self) -> &Path {
        &self.wal_path
    }

    pub fn publish(
        &mut self,
        stream_name: &str,
        direction: StreamDirection,
        origin: &str,
        payload_json: &str,
        attributes: HashMap<String, String>,
    ) -> Result<EventBusTransportEvent, String> {
        let event = EventBusTransportEvent {
            transport_sequence: self.next_transport_sequence,
            event: StreamEventEnvelope {
                event_id: self.next_event_id,
                stream_name: stream_name.to_string(),
                direction,
                origin: origin.to_string(),
                occurred_epoch_ms: now_epoch_millis(),
                payload_json: payload_json.to_string(),
                attributes,
            },
        };
        let record = encode_event_bus_record(&event);
        self.adapter
            .append(&record)
            .map_err(|err| format!("event bus append failed: {err:?}"))?;
        self.next_transport_sequence += 1;
        self.next_event_id += 1;
        self.events.push(event.clone());
        Ok(event)
    }

    pub fn export_for_stream_since(
        &self,
        stream_name: &str,
        last_event_id: u64,
        max_items: usize,
    ) -> Vec<StreamEventEnvelope> {
        self.events
            .iter()
            .filter(|event| event.event.stream_name == stream_name && event.event.event_id > last_event_id)
            .take(max_items)
            .map(|event| event.event.clone())
            .collect()
    }

    pub fn total_events(&self) -> usize {
        self.events.len()
    }

    pub fn last_event_id_for_stream(&self, stream_name: &str) -> Option<u64> {
        self.events
            .iter()
            .rev()
            .find(|event| event.event.stream_name == stream_name)
            .map(|event| event.event.event_id)
    }

    pub fn events(&self) -> &[EventBusTransportEvent] {
        &self.events
    }
}

impl EventBusBrokerClient for FileBackedEventBusTransport {
    fn broker_kind(&self) -> &'static str {
        "file_wal"
    }

    fn publish(
        &mut self,
        stream_name: &str,
        direction: StreamDirection,
        origin: &str,
        payload_json: &str,
        attributes: HashMap<String, String>,
    ) -> Result<EventBusTransportEvent, String> {
        FileBackedEventBusTransport::publish(
            self,
            stream_name,
            direction,
            origin,
            payload_json,
            attributes,
        )
    }

    fn export_for_stream_since(
        &self,
        stream_name: &str,
        last_event_id: u64,
        max_items: usize,
    ) -> Vec<StreamEventEnvelope> {
        FileBackedEventBusTransport::export_for_stream_since(self, stream_name, last_event_id, max_items)
    }

    fn total_events(&self) -> usize {
        FileBackedEventBusTransport::total_events(self)
    }

    fn last_event_id_for_stream(&self, stream_name: &str) -> Option<u64> {
        FileBackedEventBusTransport::last_event_id_for_stream(self, stream_name)
    }

    fn snapshot_events(&self) -> Vec<EventBusTransportEvent> {
        self.events().to_vec()
    }
}

pub struct ManagedEventBusTransport {
    broker: Box<dyn EventBusBrokerClient>,
}

impl ManagedEventBusTransport {
    pub fn in_memory() -> Self {
        Self {
            broker: Box::new(InMemoryEventBusTransport::new()),
        }
    }

    pub fn file_backed<P: AsRef<Path>>(wal_path: P) -> Result<Self, String> {
        Ok(Self {
            broker: Box::new(FileBackedEventBusTransport::new(wal_path)?),
        })
    }

    pub fn external_broker<P: AsRef<Path>>(
        broker_kind: ExternalBrokerKind,
        wal_path: P,
        broker_target: &str,
        subject_prefix: Option<&str>,
    ) -> Result<Self, String> {
        Ok(Self {
            broker: Box::new(ExternalBrokerBackedEventBusTransport::new(
                broker_kind,
                wal_path,
                broker_target,
                subject_prefix,
            )?),
        })
    }

    pub fn from_broker_mode<P: AsRef<Path>>(broker_mode: &str, wal_path: P) -> Result<Self, String> {
        Self::from_broker_mode_with_target(broker_mode, wal_path, None, None)
    }

    pub fn from_broker_mode_with_target<P: AsRef<Path>>(
        broker_mode: &str,
        wal_path: P,
        broker_target: Option<&str>,
        subject_prefix: Option<&str>,
    ) -> Result<Self, String> {
        match broker_mode.trim().to_ascii_lowercase().as_str() {
            "in_memory" | "memory" => Ok(Self::in_memory()),
            "file" | "file_wal" | "wal" => Self::file_backed(wal_path),
            other if ExternalBrokerKind::from_broker_mode(other).is_some() => {
                let broker_kind = ExternalBrokerKind::from_broker_mode(other)
                    .expect("external broker mode already checked");
                let broker_target = broker_target
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        format!(
                            "external broker target is required for {}; set VNG_INGEST_EXTERNAL_BROKER_TARGET",
                            broker_kind.as_str()
                        )
                    })?;
                Self::external_broker(broker_kind, wal_path, broker_target, subject_prefix)
            }
            other => Err(format!(
                "unsupported ingest outbox broker mode: {other}; expected in_memory, file_wal, kafka, nats, or event_hubs"
            )),
        }
    }

    pub fn broker_kind(&self) -> &'static str {
        self.broker.broker_kind()
    }

    pub fn broker_target(&self) -> Option<String> {
        self.broker.broker_target()
    }

    pub fn publish(
        &mut self,
        stream_name: &str,
        direction: StreamDirection,
        origin: &str,
        payload_json: &str,
        attributes: HashMap<String, String>,
    ) -> Result<EventBusTransportEvent, String> {
        self.broker
            .publish(stream_name, direction, origin, payload_json, attributes)
    }

    pub fn export_for_stream_since(
        &self,
        stream_name: &str,
        last_event_id: u64,
        max_items: usize,
    ) -> Vec<StreamEventEnvelope> {
        self.broker
            .export_for_stream_since(stream_name, last_event_id, max_items)
    }

    pub fn total_events(&self) -> usize {
        self.broker.total_events()
    }

    pub fn last_event_id_for_stream(&self, stream_name: &str) -> Option<u64> {
        self.broker.last_event_id_for_stream(stream_name)
    }

    pub fn events(&self) -> Vec<EventBusTransportEvent> {
        self.broker.snapshot_events()
    }
}

fn encode_event_bus_record(event: &EventBusTransportEvent) -> WalRecord {
    let mut components = vec![
        event.transport_sequence.to_string(),
        event.event.event_id.to_string(),
        event.event.stream_name.clone(),
        stream_direction_as_str(event.event.direction).to_string(),
        event.event.origin.clone(),
        event.event.occurred_epoch_ms.to_string(),
        event.event.payload_json.clone(),
    ];

    let mut attributes = event
        .event
        .attributes
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    attributes.sort_by(|left, right| left.0.cmp(&right.0));
    components.push(attributes.len().to_string());
    for (key, value) in attributes {
        components.push(key);
        components.push(value);
    }

    WalRecord {
        sequence: event.transport_sequence,
        timestamp_epoch_ms: event.event.occurred_epoch_ms,
        key: format!("eventbus::{}", event.transport_sequence),
        value: components
            .into_iter()
            .map(|component| escape_component(&component))
            .collect::<Vec<_>>()
            .join("\t"),
    }
}

fn decode_event_bus_record(record: &WalRecord) -> Result<EventBusTransportEvent, String> {
    let parts = record
        .value
        .split('\t')
        .map(unescape_component)
        .collect::<Vec<_>>();
    if parts.len() < 8 {
        return Err(format!(
            "event bus record missing required fields for sequence {}",
            record.sequence
        ));
    }

    let transport_sequence = parts[0]
        .parse::<u64>()
        .map_err(|_| format!("invalid transport sequence in record {}", record.sequence))?;
    let event_id = parts[1]
        .parse::<u64>()
        .map_err(|_| format!("invalid event id in record {}", record.sequence))?;
    let direction = parse_stream_direction(&parts[3])?;
    let occurred_epoch_ms = parts[5]
        .parse::<u128>()
        .map_err(|_| format!("invalid timestamp in record {}", record.sequence))?;
    let attribute_count = parts[7]
        .parse::<usize>()
        .map_err(|_| format!("invalid attribute count in record {}", record.sequence))?;
    let expected_len = 8 + (attribute_count * 2);
    if parts.len() != expected_len {
        return Err(format!(
            "event bus record {} expected {} fields, found {}",
            record.sequence,
            expected_len,
            parts.len()
        ));
    }

    let mut attributes = HashMap::new();
    for pair in parts[8..].chunks(2) {
        attributes.insert(pair[0].clone(), pair[1].clone());
    }

    Ok(EventBusTransportEvent {
        transport_sequence,
        event: StreamEventEnvelope {
            event_id,
            stream_name: parts[2].clone(),
            direction,
            origin: parts[4].clone(),
            occurred_epoch_ms,
            payload_json: parts[6].clone(),
            attributes,
        },
    })
}

fn stream_direction_as_str(direction: StreamDirection) -> &'static str {
    match direction {
        StreamDirection::Inbound => "inbound",
        StreamDirection::Outbound => "outbound",
        StreamDirection::Internal => "internal",
    }
}

fn parse_stream_direction(value: &str) -> Result<StreamDirection, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "inbound" => Ok(StreamDirection::Inbound),
        "outbound" => Ok(StreamDirection::Outbound),
        "internal" => Ok(StreamDirection::Internal),
        other => Err(format!("unsupported stream direction: {other}")),
    }
}

fn escape_component(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn unescape_component(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
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

    pub fn events(&self) -> &[StreamEventEnvelope] {
        &self.events
    }

    pub fn last_event_id(&self) -> Option<u64> {
        self.events.last().map(|event| event.event_id)
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
        let sequence = adapter
            .read_all()
            .ok()
            .and_then(|records| records.iter().map(|record| record.sequence).max())
            .map(|value| value + 1)
            .unwrap_or(1);
        Self {
            adapter,
            sequence,
        }
    }
}

pub enum ManagedReplayCursorStore {
    InMemory(InMemoryReplayCursorStore),
    WalBacked(WalBackedReplayCursorStore<FileWalAdapter>),
}

impl ManagedReplayCursorStore {
    pub fn in_memory() -> Self {
        Self::InMemory(InMemoryReplayCursorStore::default())
    }

    pub fn wal_backed<P: AsRef<Path>>(wal_path: P) -> Result<Self, String> {
        let adapter = FileWalAdapter::new(wal_path)
            .map_err(|err| format!("cursor store adapter init failed: {err:?}"))?;
        Ok(Self::WalBacked(WalBackedReplayCursorStore::new(adapter)))
    }
}

impl ReplayCursorStore for ManagedReplayCursorStore {
    fn load(&self, stream_name: &str) -> Option<u64> {
        match self {
            Self::InMemory(store) => store.load(stream_name),
            Self::WalBacked(store) => store.load(stream_name),
        }
    }

    fn save(&mut self, stream_name: &str, last_replayed_event_id: u64) -> Result<(), String> {
        match self {
            Self::InMemory(store) => store.save(stream_name, last_replayed_event_id),
            Self::WalBacked(store) => store.save(stream_name, last_replayed_event_id),
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
    fn managed_wal_backed_cursor_store_recovers_saved_checkpoint() {
        let wal_path = unique_wal_path();
        let mut store = ManagedReplayCursorStore::wal_backed(&wal_path).expect("managed wal store");
        store.save("ingest.orders", 9).expect("save checkpoint");

        let recovered = ManagedReplayCursorStore::wal_backed(&wal_path).expect("recover managed wal store");
        assert_eq!(recovered.load("ingest.orders"), Some(9));

        let _ = fs::remove_file(&wal_path);
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

    #[test]
    fn event_bus_transport_exports_only_unacknowledged_events() {
        let mut bus = InMemoryEventBusTransport::new();
        bus.publish(
            "ingest.outbox.orders",
            StreamDirection::Internal,
            "node-1",
            "{\"id\":1}",
            HashMap::new(),
        );
        bus.publish(
            "ingest.outbox.orders",
            StreamDirection::Internal,
            "node-1",
            "{\"id\":2}",
            HashMap::new(),
        );

        let replay = bus.export_for_stream_since("ingest.outbox.orders", 1, 10);
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].event_id, 2);
        assert_eq!(bus.last_event_id_for_stream("ingest.outbox.orders"), Some(2));
    }

    #[test]
    fn file_backed_event_bus_persists_and_recovers_events() {
        let wal_path = unique_wal_path();
        let mut bus = FileBackedEventBusTransport::new(&wal_path).expect("file backed event bus");
        bus.publish(
            "ingest.outbox.orders",
            StreamDirection::Internal,
            "node-1",
            "{\"id\":1}",
            HashMap::from([("tenant".to_string(), "acme".to_string())]),
        )
        .expect("publish first");
        bus.publish(
            "ingest.outbox.orders",
            StreamDirection::Internal,
            "node-1",
            "{\"id\":2}",
            HashMap::new(),
        )
        .expect("publish second");

        let recovered = FileBackedEventBusTransport::new(&wal_path).expect("recover event bus");
        let replay = recovered.export_for_stream_since("ingest.outbox.orders", 0, 10);
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0].event_id, 1);
        assert_eq!(replay[0].attributes.get("tenant").map(String::as_str), Some("acme"));
        assert_eq!(recovered.last_event_id_for_stream("ingest.outbox.orders"), Some(2));

        let _ = fs::remove_file(recovered.wal_path());
    }

    #[test]
    fn managed_event_bus_selects_requested_broker_mode() {
        let wal_path = unique_wal_path();

        let in_memory = ManagedEventBusTransport::from_broker_mode("in_memory", &wal_path)
            .expect("in-memory broker");
        assert_eq!(in_memory.broker_kind(), "in_memory");

        let file_backed = ManagedEventBusTransport::from_broker_mode("file_wal", &wal_path)
            .expect("file broker");
        assert_eq!(file_backed.broker_kind(), "file_wal");

        let kafka = ManagedEventBusTransport::from_broker_mode_with_target(
            "kafka",
            &wal_path,
            Some("broker-a:9092"),
            Some("vng.outbox"),
        )
        .expect("kafka broker");
        assert_eq!(kafka.broker_kind(), "kafka");
        assert_eq!(kafka.broker_target().as_deref(), Some("broker-a:9092"));

        let _ = fs::remove_file(&wal_path);
    }

    #[test]
    fn external_broker_transport_preserves_ledger_replay_contract() {
        let wal_path = unique_wal_path();
        let mut bus = ManagedEventBusTransport::from_broker_mode_with_target(
            "event_hubs",
            &wal_path,
            Some("sb://namespace.servicebus.windows.net/orders"),
            Some("vng.outbox"),
        )
        .expect("event hubs broker");

        bus.publish(
            "ingest.outbox.orders",
            StreamDirection::Internal,
            "node-1",
            "{\"id\":1}",
            HashMap::from([("tenant".to_string(), "acme".to_string())]),
        )
        .expect("publish first");

        let replay = bus.export_for_stream_since("ingest.outbox.orders", 0, 10);
        assert_eq!(bus.broker_kind(), "event_hubs");
        assert_eq!(bus.broker_target().as_deref(), Some("sb://namespace.servicebus.windows.net/orders"));
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].event_id, 1);

        let _ = fs::remove_file(&wal_path);
    }

    #[test]
    fn external_broker_mode_requires_target() {
        let wal_path = unique_wal_path();
        let error = ManagedEventBusTransport::from_broker_mode_with_target(
            "nats",
            &wal_path,
            None,
            Some("vng.outbox"),
        )
        .expect_err("missing target should fail");
        assert!(error.contains("VNG_INGEST_EXTERNAL_BROKER_TARGET"));
    }
}
