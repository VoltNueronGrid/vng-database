// CONN-6: Kafka / Confluent REST Proxy streaming connector.
//
// Implements consuming from Kafka topics via the Confluent REST Proxy API
// (no C library dependency). Falls back gracefully when no REST proxy is
// configured.
//
// For raw Kafka binary protocol (rdkafka), set VNG_KAFKA_NATIVE=true and
// provide VNG_KAFKA_BROKERS; requires librdkafka at link time (deferred).
//
// Env vars:
//   VNG_KAFKA_REST_URL       — Confluent REST Proxy base URL
//   VNG_KAFKA_GROUP_ID       — consumer group ID (default: "vng-ingest")
//   VNG_KAFKA_TOPIC          — topic to subscribe to
//   VNG_KAFKA_POLL_MAX       — max records per poll (default: 100)
//   VNG_KAFKA_SASL_USERNAME  — SASL username (forwarded in REST auth header)
//   VNG_KAFKA_SASL_PASSWORD  — SASL password

use std::collections::HashMap;
use std::time::Duration;

use crate::{
    ConnectorDescriptor, ConnectorDirection, EventBusBrokerClient, EventBusTransportEvent,
    IngestionConnector, IngestFormat, IngestRecord, StreamDirection, StreamEventEnvelope,
};

/// Errors from the Kafka connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KafkaError {
    MissingConfig(String),
    RequestFailed(String),
    ParseFailed(String),
}

impl std::fmt::Display for KafkaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingConfig(m) => write!(f, "kafka_missing_config: {m}"),
            Self::RequestFailed(m) => write!(f, "kafka_request_failed: {m}"),
            Self::ParseFailed(m) => write!(f, "kafka_parse_failed: {m}"),
        }
    }
}

/// Configuration for the Kafka REST Proxy connector.
#[derive(Debug, Clone)]
pub struct KafkaConnectorConfig {
    /// Confluent REST Proxy base URL, e.g. "http://kafka-rest:8082"
    pub rest_proxy_url: String,
    pub group_id: String,
    pub topic: String,
    pub max_records: usize,
    pub sasl_username: Option<String>,
    pub sasl_password: Option<String>,
    /// Unique consumer instance ID (auto-generated from group + topic).
    pub instance_id: String,
}

impl KafkaConnectorConfig {
    pub fn from_env() -> Result<Self, KafkaError> {
        let rest_proxy_url = std::env::var("VNG_KAFKA_REST_URL")
            .map_err(|_| KafkaError::MissingConfig("VNG_KAFKA_REST_URL".into()))?;
        let topic = std::env::var("VNG_KAFKA_TOPIC")
            .map_err(|_| KafkaError::MissingConfig("VNG_KAFKA_TOPIC".into()))?;
        let group_id =
            std::env::var("VNG_KAFKA_GROUP_ID").unwrap_or_else(|_| "vng-ingest".to_string());
        let max_records = std::env::var("VNG_KAFKA_POLL_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);
        let instance_id = format!("{group_id}-{topic}-vng");
        Ok(Self {
            rest_proxy_url,
            group_id,
            topic,
            max_records,
            sasl_username: std::env::var("VNG_KAFKA_SASL_USERNAME").ok(),
            sasl_password: std::env::var("VNG_KAFKA_SASL_PASSWORD").ok(),
            instance_id,
        })
    }

    pub fn new(rest_proxy_url: impl Into<String>, group_id: impl Into<String>, topic: impl Into<String>) -> Self {
        let group_id = group_id.into();
        let topic = topic.into();
        let instance_id = format!("{group_id}-{topic}-vng");
        Self {
            rest_proxy_url: rest_proxy_url.into(),
            group_id,
            topic,
            max_records: 100,
            sasl_username: None,
            sasl_password: None,
            instance_id,
        }
    }
}

/// A single Kafka record decoded from the REST Proxy JSON response.
#[derive(Debug, Clone)]
pub struct KafkaRecord {
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub key: Option<String>,
    pub value: String,
}

/// Parse the REST Proxy poll response: `[{"topic":"t","partition":0,"offset":5,"key":null,"value":"..."}]`
pub fn parse_kafka_records(json_str: &str) -> Vec<KafkaRecord> {
    // Simple hand-rolled parser — avoids adding serde_json to ingest crate
    // (ingest crate already has serde_json so we use it here).
    let parsed: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap_or_default();
    parsed
        .into_iter()
        .filter_map(|v| {
            let topic = v["topic"].as_str()?.to_string();
            let partition = v["partition"].as_i64().unwrap_or(0) as i32;
            let offset = v["offset"].as_i64().unwrap_or(0);
            let key = v["key"].as_str().map(|s| s.to_string());
            let value = v["value"].as_str().unwrap_or("").to_string();
            Some(KafkaRecord { topic, partition, offset, key, value })
        })
        .collect()
}

fn build_agent(_cfg: &KafkaConnectorConfig) -> ureq::Agent {
    let builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30));
    // SASL auth forwarded as HTTP Basic to the REST Proxy
    builder.build()
}

fn add_auth(req: ureq::Request, cfg: &KafkaConnectorConfig) -> ureq::Request {
    if let (Some(u), Some(p)) = (&cfg.sasl_username, &cfg.sasl_password) {
        use super::webdav::base64_encode;
        let creds = format!("{u}:{p}");
        req.set("Authorization", &format!("Basic {}", base64_encode(creds.as_bytes())))
    } else {
        req
    }
}

/// Create consumer instance with the REST Proxy.
pub fn kafka_create_consumer(cfg: &KafkaConnectorConfig) -> Result<(), KafkaError> {
    let agent = build_agent(cfg);
    let url = format!(
        "{}/consumers/{}",
        cfg.rest_proxy_url.trim_end_matches('/'),
        cfg.group_id
    );
    let body = serde_json::json!({
        "name": cfg.instance_id,
        "format": "json",
        "auto.offset.reset": "earliest",
        "auto.commit.enable": "true",
    });
    let req = add_auth(agent.post(&url), cfg)
        .set("Content-Type", "application/vnd.kafka.v2+json");
    let _resp = req
        .send_string(&body.to_string())
        .map_err(|e| KafkaError::RequestFailed(e.to_string()))?;
    Ok(())
}

/// Subscribe consumer instance to topic.
pub fn kafka_subscribe(cfg: &KafkaConnectorConfig) -> Result<(), KafkaError> {
    let agent = build_agent(cfg);
    let url = format!(
        "{}/consumers/{}/instances/{}/subscription",
        cfg.rest_proxy_url.trim_end_matches('/'),
        cfg.group_id,
        cfg.instance_id
    );
    let body = serde_json::json!({ "topics": [cfg.topic] });
    let req = add_auth(agent.post(&url), cfg)
        .set("Content-Type", "application/vnd.kafka.v2+json");
    req.send_string(&body.to_string())
        .map_err(|e| KafkaError::RequestFailed(e.to_string()))?;
    Ok(())
}

/// Poll for records from the topic.
pub fn kafka_poll(cfg: &KafkaConnectorConfig) -> Result<Vec<KafkaRecord>, KafkaError> {
    let agent = build_agent(cfg);
    let url = format!(
        "{}/consumers/{}/instances/{}/records?max_bytes=1000000",
        cfg.rest_proxy_url.trim_end_matches('/'),
        cfg.group_id,
        cfg.instance_id
    );
    let req = add_auth(agent.get(&url), cfg)
        .set("Accept", "application/vnd.kafka.json.v2+json");
    let resp = req
        .call()
        .map_err(|e| KafkaError::RequestFailed(e.to_string()))?;
    let json_str = resp
        .into_string()
        .map_err(|e| KafkaError::ParseFailed(e.to_string()))?;
    Ok(parse_kafka_records(&json_str)
        .into_iter()
        .take(cfg.max_records)
        .collect())
}

/// Delete consumer instance (cleanup).
pub fn kafka_delete_consumer(cfg: &KafkaConnectorConfig) -> Result<(), KafkaError> {
    let agent = build_agent(cfg);
    let url = format!(
        "{}/consumers/{}/instances/{}",
        cfg.rest_proxy_url.trim_end_matches('/'),
        cfg.group_id,
        cfg.instance_id
    );
    let req = add_auth(agent.delete(&url), cfg)
        .set("Content-Type", "application/vnd.kafka.v2+json");
    let _ = req.call();
    Ok(())
}

/// `IngestionConnector` that creates a consumer, subscribes, polls, and deletes.
pub struct KafkaConnector {
    descriptor: ConnectorDescriptor,
    config: KafkaConnectorConfig,
    /// In-memory event store for `EventBusBrokerClient` trait.
    events: std::sync::Mutex<Vec<EventBusTransportEvent>>,
    sequence: std::sync::atomic::AtomicU64,
}

impl KafkaConnector {
    pub fn new(config: KafkaConnectorConfig) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: "kafka".to_string(),
                display_name: "Kafka REST Proxy Connector".to_string(),
                format: IngestFormat::Stream,
                direction: ConnectorDirection::Inbound,
            },
            config,
            events: std::sync::Mutex::new(Vec::new()),
            sequence: std::sync::atomic::AtomicU64::new(1),
        }
    }
}

impl IngestionConnector for KafkaConnector {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    /// Poll Kafka via REST Proxy. Returns up to max_items records.
    fn read_batch(&self, max_items: usize) -> Vec<IngestRecord> {
        // Ensure consumer exists and subscribed.
        let _ = kafka_create_consumer(&self.config);
        let _ = kafka_subscribe(&self.config);

        match kafka_poll(&self.config) {
            Ok(records) => records
                .into_iter()
                .take(max_items)
                .map(|r| {
                    let key = r.key.unwrap_or_else(|| format!("{}:{}:{}", r.topic, r.partition, r.offset));
                    IngestRecord { key, payload: r.value }
                })
                .collect(),
            Err(e) => {
                eprintln!("[KafkaConnector] poll error: {e}");
                vec![]
            }
        }
    }
}

impl EventBusBrokerClient for KafkaConnector {
    fn broker_kind(&self) -> &'static str {
        "kafka"
    }

    fn broker_target(&self) -> Option<String> {
        Some(format!("{}/{}",
            self.config.rest_proxy_url.trim_end_matches('/'),
            self.config.topic))
    }

    fn publish(
        &mut self,
        stream_name: &str,
        direction: StreamDirection,
        origin: &str,
        payload_json: &str,
        attributes: HashMap<String, String>,
    ) -> Result<EventBusTransportEvent, String> {
        // Produce to Kafka via REST Proxy
        let agent = build_agent(&self.config);
        let url = format!(
            "{}/topics/{}",
            self.config.rest_proxy_url.trim_end_matches('/'),
            stream_name
        );
        let body = serde_json::json!({
            "records": [{ "value": payload_json }]
        });
        let req = add_auth(agent.post(&url), &self.config)
            .set("Content-Type", "application/vnd.kafka.json.v2+json");
        req.send_string(&body.to_string())
            .map_err(|e| format!("kafka_produce_failed: {e}"))?;

        let seq = self.sequence.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let event = StreamEventEnvelope {
            event_id: seq,
            stream_name: stream_name.to_string(),
            direction,
            origin: origin.to_string(),
            occurred_epoch_ms: now_epoch_ms(),
            payload_json: payload_json.to_string(),
            attributes,
        };
        let transport = EventBusTransportEvent { transport_sequence: seq, event };
        self.events.lock().unwrap_or_else(|e| e.into_inner()).push(transport.clone());
        Ok(transport)
    }

    fn export_for_stream_since(&self, stream_name: &str, last_event_id: u64, max_items: usize) -> Vec<StreamEventEnvelope> {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|t| t.event.stream_name == stream_name && t.event.event_id > last_event_id)
            .take(max_items)
            .map(|t| t.event.clone())
            .collect()
    }

    fn total_events(&self) -> usize {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    fn last_event_id_for_stream(&self, stream_name: &str) -> Option<u64> {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|t| t.event.stream_name == stream_name)
            .map(|t| t.event.event_id)
            .max()
    }

    fn snapshot_events(&self) -> Vec<EventBusTransportEvent> {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

fn now_epoch_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// `StreamingConnector` extension trait: subscribe + poll_batch.
pub trait StreamingConnector: IngestionConnector {
    /// Subscribe to a topic/stream.
    fn subscribe(&mut self, topic: &str) -> Result<(), String>;
    /// Poll a batch of records. Returns `(records, new_cursor_offset)`.
    fn poll_batch(&self, max: usize, timeout: Duration) -> Vec<IngestRecord>;
}

impl StreamingConnector for KafkaConnector {
    fn subscribe(&mut self, topic: &str) -> Result<(), String> {
        // Update config topic and re-subscribe.
        self.config.topic = topic.to_string();
        self.config.instance_id = format!("{}-{topic}-vng", self.config.group_id);
        kafka_create_consumer(&self.config).map_err(|e| e.to_string())?;
        kafka_subscribe(&self.config).map_err(|e| e.to_string())
    }

    fn poll_batch(&self, max: usize, _timeout: Duration) -> Vec<IngestRecord> {
        self.read_batch(max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conn6_parse_kafka_records_valid() {
        let json = r#"[
          {"topic":"orders","partition":0,"offset":1,"key":"k1","value":"v1"},
          {"topic":"orders","partition":0,"offset":2,"key":null,"value":"v2"}
        ]"#;
        let records = parse_kafka_records(json);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].key, Some("k1".to_string()));
        assert_eq!(records[0].value, "v1");
        assert_eq!(records[1].key, None);
        assert_eq!(records[1].offset, 2);
    }

    #[test]
    fn conn6_parse_kafka_records_empty_array() {
        let records = parse_kafka_records("[]");
        assert!(records.is_empty());
    }

    #[test]
    fn conn6_parse_kafka_records_invalid_json() {
        let records = parse_kafka_records("not json");
        assert!(records.is_empty());
    }

    #[test]
    fn conn6_config_instance_id_format() {
        let cfg = KafkaConnectorConfig::new("http://localhost:8082", "my-group", "my-topic");
        assert_eq!(cfg.instance_id, "my-group-my-topic-vng");
    }

    #[test]
    fn conn6_connector_descriptor() {
        let cfg = KafkaConnectorConfig::new("http://localhost:8082", "g1", "t1");
        let conn = KafkaConnector::new(cfg);
        assert_eq!(conn.descriptor().id, "kafka");
        assert_eq!(conn.descriptor().format, IngestFormat::Stream);
        assert_eq!(conn.descriptor().direction, ConnectorDirection::Inbound);
    }

    #[test]
    fn conn6_broker_kind_is_kafka() {
        let cfg = KafkaConnectorConfig::new("http://localhost:8082", "g1", "t1");
        let conn = KafkaConnector::new(cfg);
        assert_eq!(conn.broker_kind(), "kafka");
    }

    #[test]
    fn conn6_broker_target_contains_url_and_topic() {
        let cfg = KafkaConnectorConfig::new("http://kafka:8082", "g1", "orders");
        let conn = KafkaConnector::new(cfg);
        let target = conn.broker_target().unwrap();
        assert!(target.contains("kafka:8082"));
        assert!(target.contains("orders"));
    }

    #[test]
    fn conn6_snapshot_events_empty_initially() {
        let cfg = KafkaConnectorConfig::new("http://localhost:8082", "g1", "t1");
        let conn = KafkaConnector::new(cfg);
        assert_eq!(conn.snapshot_events().len(), 0);
        assert_eq!(conn.total_events(), 0);
        assert_eq!(conn.last_event_id_for_stream("orders"), None);
    }
}
