use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use voltnuerongrid_audit::AuditEventKind;
use voltnuerongrid_auth::PrivilegeAction;
use voltnuerongrid_ingest::{IngestionConnector, ReplayCursorStore, StreamDirection};
use crate::{AppState, AuthErrorResponse, RuntimeAccessPrincipal};
use crate::auth::{require_operator_auth, require_ingest_runtime_privilege, bad_request_error};
use crate::audit_helpers::append_runtime_audit_event;

// ─── Broker adapter DTOs ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct BrokerAdapterInfo {
    pub(crate) broker_type: String,
    pub(crate) enabled: bool,
    pub(crate) flush_count: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct BrokerAdapterStatus {
    pub(crate) status: &'static str,
    pub(crate) adapters: Vec<BrokerAdapterInfo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BrokerFlushRequest {
    pub(crate) broker_type: String,
    pub(crate) max_events: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BrokerFlushResponse {
    pub(crate) status: &'static str,
    pub(crate) broker_type: String,
    pub(crate) events_flushed: usize,
    pub(crate) total_flush_count: u64,
}

#[derive(Serialize)]
pub(crate) struct BrokerHealthEntry {
    pub(crate) broker_type: &'static str,
    pub(crate) flush_count: u64,
    pub(crate) wal_len: usize,
    pub(crate) healthy: bool,
}

#[derive(Serialize)]
pub(crate) struct BrokerHealthResponse {
    pub(crate) status: &'static str,
    pub(crate) broker_count: usize,
    pub(crate) brokers: Vec<BrokerHealthEntry>,
}

// ─── Ingest schema fields DTOs ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct IngestSchemaFieldsQuery {
    pub(crate) schema_id: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SchemaFieldEntry {
    pub(crate) field_name: String,
    pub(crate) field_type: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct IngestSchemaFieldsResponse {
    pub(crate) status: &'static str,
    pub(crate) schema_id: String,
    pub(crate) field_count: usize,
    pub(crate) fields: Vec<SchemaFieldEntry>,
}

// ─── Ingest format DTOs ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct IngestCsvRequest {
    pub(crate) connector_id: String,
    pub(crate) csv_data: String,
}

#[derive(Serialize)]
pub(crate) struct IngestCsvResponse {
    pub(crate) status: &'static str,
    pub(crate) connector_id: String,
    pub(crate) records_parsed: usize,
}

#[derive(Deserialize)]
pub(crate) struct IngestJsonRequest {
    pub(crate) connector_id: String,
    pub(crate) key_field: String,
    pub(crate) ndjson_data: String,
}

#[derive(Serialize)]
pub(crate) struct IngestJsonResponse {
    pub(crate) status: &'static str,
    pub(crate) connector_id: String,
    pub(crate) records_parsed: usize,
}

#[derive(Deserialize)]
pub(crate) struct IngestParquetRequest {
    pub(crate) connector_id: String,
    /// Standard base64 (RFC 4648) encoded Parquet file bytes.
    pub(crate) parquet_data_base64: String,
}

#[derive(Serialize)]
pub(crate) struct IngestParquetResponse {
    pub(crate) status: &'static str,
    pub(crate) connector_id: String,
    pub(crate) records_parsed: usize,
}

#[derive(Deserialize)]
pub(crate) struct IngestExcelRequest {
    pub(crate) connector_id: String,
    /// Standard base64 (RFC 4648) encoded `.xlsx` workbook bytes.
    pub(crate) xlsx_data_base64: String,
}

#[derive(Serialize)]
pub(crate) struct IngestExcelResponse {
    pub(crate) status: &'static str,
    pub(crate) connector_id: String,
    pub(crate) records_parsed: usize,
}

#[derive(Deserialize)]
pub(crate) struct IngestChunkedRequest {
    pub(crate) connector_id: String,
    /// JSON-serialized record payloads – one per element
    pub(crate) records: Vec<String>,
    pub(crate) chunk_target_rows: Option<usize>,
    pub(crate) max_in_flight_tasks: Option<usize>,
}

#[derive(Serialize)]
pub(crate) struct IngestChunkedResponse {
    pub(crate) status: &'static str,
    pub(crate) connector_id: String,
    pub(crate) total_records: usize,
    pub(crate) chunk_count: usize,
    pub(crate) tasks_dispatched: usize,
    pub(crate) chunks_succeeded: usize,
    pub(crate) chunks_failed: usize,
}

#[derive(Serialize)]
pub(crate) struct IngestStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) csv_connectors: usize,
    pub(crate) json_connectors: usize,
    pub(crate) parquet_connectors: usize,
    pub(crate) excel_connectors: usize,
    pub(crate) total_records_loaded: usize,
}

// ─── Schema registry DTOs ─────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct IngestSchemaColumn {
    pub(crate) name: String,
    pub(crate) inferred_type: &'static str,
}

#[derive(Serialize)]
pub(crate) struct IngestSchemaEntry {
    pub(crate) connector_id: String,
    pub(crate) format: String,
    pub(crate) row_count: usize,
    pub(crate) columns: Vec<IngestSchemaColumn>,
}

#[derive(Serialize)]
pub(crate) struct IngestSchemaRegistryResponse {
    pub(crate) status: &'static str,
    pub(crate) connector_count: usize,
    pub(crate) entries: Vec<IngestSchemaEntry>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct IngestSchemaListQuery {
    pub(crate) format: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct IngestSchemaListResponse {
    pub(crate) status: &'static str,
    pub(crate) format_filter: Option<String>,
    pub(crate) connector_count: usize,
    pub(crate) entries: Vec<IngestSchemaEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct IngestFormatDetectRequest {
    pub(crate) sample_data: String,
}

#[derive(Serialize)]
pub(crate) struct IngestFormatDetectResponse {
    pub(crate) status: &'static str,
    pub(crate) detected_format: String,
    pub(crate) confidence: f64,
    pub(crate) field_count: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct IngestConnectorValidateRequest {
    pub(crate) connector_id: String,
    pub(crate) format: String,
    pub(crate) config_json: String,
}

#[derive(Serialize)]
pub(crate) struct IngestConnectorValidateResponse {
    pub(crate) status: &'static str,
    pub(crate) valid: bool,
    pub(crate) issues: Vec<String>,
}

// ─── Outbox DTOs ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct IngestOutboxStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) broker_mode: String,
    pub(crate) broker_target: Option<String>,
    pub(crate) stream_count: usize,
    pub(crate) total_events: usize,
    pub(crate) last_event_id: Option<u64>,
    pub(crate) streams: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct IngestOutboxReplayRequest {
    pub(crate) connector_id: String,
    pub(crate) consumer_id: Option<String>,
    pub(crate) max_items: Option<usize>,
    pub(crate) acknowledge: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct IngestOutboxReplayEventResponse {
    pub(crate) replay_key: String,
    pub(crate) event_id: u64,
    pub(crate) stream_name: String,
    pub(crate) origin: String,
    pub(crate) payload_json: String,
}

#[derive(Serialize)]
pub(crate) struct IngestOutboxReplayResponse {
    pub(crate) status: &'static str,
    pub(crate) delivery_state: &'static str,
    pub(crate) stream_name: String,
    pub(crate) consumer_id: String,
    pub(crate) delivered_count: usize,
    pub(crate) cursor_before_ack: Option<u64>,
    pub(crate) cursor_after_ack: Option<u64>,
    pub(crate) acknowledged: bool,
    pub(crate) events: Vec<IngestOutboxReplayEventResponse>,
}

// ─── Private helpers ──────────────────────────────────────────────────────────

fn ingest_scope_for_connector(connector_id: &str, format: &str) -> String {
    format!("ingest/connectors/{connector_id}/{format}")
}

pub(crate) fn ingest_status_scope() -> &'static str {
    "ingest/status"
}

fn ingest_outbox_scope(connector_id: Option<&str>) -> String {
    match connector_id {
        Some(connector_id) => format!("ingest/outbox/{connector_id}"),
        None => "ingest/outbox".to_string(),
    }
}

fn ingest_outbox_stream_name(storage_key: &str) -> String {
    format!(
        "ingest.outbox.{}",
        storage_key
            .replace('/', ".")
            .replace(':', ".")
            .replace(' ', "_")
    )
}

fn ingest_storage_key(principal: &RuntimeAccessPrincipal, connector_id: &str) -> String {
    match principal {
        RuntimeAccessPrincipal::Operator(_) => connector_id.to_string(),
        RuntimeAccessPrincipal::TenantUser(user) => {
            format!("tenant/{}/{}", user.tenant_id, connector_id)
        }
    }
}

fn count_tenant_ingest_records<T>(records: &HashMap<String, Vec<T>>, tenant_id: &str) -> (usize, usize) {
    let prefix = format!("tenant/{tenant_id}/");
    let connectors = records.keys().filter(|key| key.starts_with(&prefix)).count();
    let total_records = records
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .map(|(_, value)| value.len())
        .sum();
    (connectors, total_records)
}

fn ingest_infer_columns(
    records: &[voltnuerongrid_ingest::IngestRecord],
) -> Vec<IngestSchemaColumn> {
    if records.is_empty() {
        return vec![IngestSchemaColumn { name: "payload".to_string(), inferred_type: "utf8" }];
    }
    vec![
        IngestSchemaColumn { name: "key".to_string(), inferred_type: "utf8" },
        IngestSchemaColumn { name: "payload".to_string(), inferred_type: "utf8" },
    ]
}

pub(crate) fn collect_ingest_schema_registry_response(state: &AppState) -> IngestSchemaRegistryResponse {
    let csv_map = state.ingest_csv_records.lock().expect("csv schema lock");
    let json_map = state.ingest_json_records.lock().expect("json schema lock");
    let mut entries: Vec<IngestSchemaEntry> = Vec::new();
    for (connector_id, records) in csv_map.iter() {
        let columns = ingest_infer_columns(records);
        entries.push(IngestSchemaEntry {
            connector_id: connector_id.clone(),
            format: "csv".to_string(),
            row_count: records.len(),
            columns,
        });
    }
    for (connector_id, records) in json_map.iter() {
        let columns = ingest_infer_columns(records);
        entries.push(IngestSchemaEntry {
            connector_id: connector_id.clone(),
            format: "json".to_string(),
            row_count: records.len(),
            columns,
        });
    }
    let connector_count = entries.len();
    drop(csv_map);
    drop(json_map);
    IngestSchemaRegistryResponse {
        status: "ok",
        connector_count,
        entries,
    }
}

fn append_ingest_outbox_events(
    state: &AppState,
    principal: &RuntimeAccessPrincipal,
    connector_id: &str,
    format: &str,
    records: &[voltnuerongrid_ingest::IngestRecord],
) -> usize {
    let storage_key = ingest_storage_key(principal, connector_id);
    let stream_name = ingest_outbox_stream_name(&storage_key);

    if let Ok(mut stream_map) = state.ingest_outbox_streams.lock() {
        stream_map.insert(storage_key.clone(), stream_name.clone());
    }

    let mut event_bus = match state.ingest_event_bus.lock() {
        Ok(guard) => guard,
        Err(_) => return 0,
    };

    let mut appended = 0usize;
    for record in records {
        let mut attributes = HashMap::new();
        attributes.insert("connector_id".to_string(), connector_id.to_string());
        attributes.insert("format".to_string(), format.to_string());
        attributes.insert("storage_key".to_string(), storage_key.clone());
        attributes.insert("record_key".to_string(), record.key.clone());
        if let RuntimeAccessPrincipal::TenantUser(user) = principal {
            attributes.insert("tenant_id".to_string(), user.tenant_id.clone());
        }

        if event_bus
            .publish(
            &stream_name,
            StreamDirection::Internal,
            &state.node_id,
            &json!({
                "connector_id": connector_id,
                "format": format,
                "storage_key": storage_key,
                "record_key": record.key,
                "payload": record.payload,
            })
            .to_string(),
            attributes,
        )
            .is_ok()
        {
            appended += 1;
        }
    }

    appended
}

// ─── Outbox broker handlers ───────────────────────────────────────────────────

pub(crate) async fn outbox_broker_status(
    State(state): State<AppState>,
) -> (StatusCode, Json<BrokerAdapterStatus>) {
    let counts = state.broker_flush_counts.lock().expect("broker_flush_counts lock");
    let adapters: Vec<BrokerAdapterInfo> = ["kafka", "nats", "event_hubs"]
        .iter()
        .map(|b| BrokerAdapterInfo {
            broker_type: b.to_string(),
            enabled: false,
            flush_count: *counts.get(*b).unwrap_or(&0),
        })
        .collect();
    drop(counts);
    (StatusCode::OK, Json(BrokerAdapterStatus {
        status: "ok",
        adapters,
    }))
}

/// S5-WS4A-02: Flush pending outbox events to the specified broker adapter (scaffold).
pub(crate) async fn outbox_broker_flush(
    State(state): State<AppState>,
    Json(req): Json<BrokerFlushRequest>,
) -> (StatusCode, Json<BrokerFlushResponse>) {
    if !["kafka", "nats", "event_hubs"].contains(&req.broker_type.as_str()) {
        return (StatusCode::BAD_REQUEST, Json(BrokerFlushResponse {
            status: "error",
            broker_type: req.broker_type,
            events_flushed: 0,
            total_flush_count: 0,
        }));
    }
    let max_events = req.max_events.unwrap_or(100).min(10_000);
    let wal = state.wal_engine.lock().expect("wal_engine lock broker_flush");
    let events_available = wal.wal_records().len();
    drop(wal);
    let events_flushed = events_available.min(max_events);
    let mut counts = state.broker_flush_counts.lock().expect("broker_flush_counts lock flush");
    let cnt = counts.entry(req.broker_type.clone()).or_insert(0);
    *cnt += 1;
    let total_flush_count = *cnt;
    drop(counts);
    (StatusCode::OK, Json(BrokerFlushResponse {
        status: "ok",
        broker_type: req.broker_type,
        events_flushed,
        total_flush_count,
    }))
}

/// S5-WS4A-02: Return per-broker health: flush count vs WAL length.
pub(crate) async fn outbox_broker_health(
    State(state): State<AppState>,
) -> (StatusCode, Json<BrokerHealthResponse>) {
    let wal_len = state.wal_engine.lock().expect("wal_engine lock health").wal_records().len();
    let counts = state.broker_flush_counts.lock().expect("broker_flush_counts lock health");
    let brokers: Vec<BrokerHealthEntry> = ["kafka", "nats", "event_hubs"].iter().map(|bt| {
        let flush_count = counts.get(*bt).copied().unwrap_or(0);
        BrokerHealthEntry {
            broker_type: bt,
            flush_count,
            wal_len,
            healthy: flush_count > 0 || wal_len == 0,
        }
    }).collect();
    let broker_count = brokers.len();
    (StatusCode::OK, Json(BrokerHealthResponse {
        status: "ok",
        broker_count,
        brokers,
    }))
}

// ─── Schema registry handlers ─────────────────────────────────────────────────

pub(crate) async fn ingest_schema_fields(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<IngestSchemaFieldsQuery>,
) -> Result<(StatusCode, Json<IngestSchemaFieldsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let csv_map = state.ingest_csv_records.lock().expect("csv_records lock ingest_schema_fields");
    let json_map = state.ingest_json_records.lock().expect("json_records lock ingest_schema_fields");
    let columns = if let Some(records) = csv_map.get(params.schema_id.as_str()) {
        ingest_infer_columns(records)
    } else if let Some(records) = json_map.get(params.schema_id.as_str()) {
        ingest_infer_columns(records)
    } else {
        vec![]
    };
    drop(csv_map);
    drop(json_map);
    let fields: Vec<SchemaFieldEntry> = columns.iter().map(|c| SchemaFieldEntry {
        field_name: c.name.clone(),
        field_type: c.inferred_type.to_string(),
    }).collect();
    let field_count = fields.len();
    Ok((StatusCode::OK, Json(IngestSchemaFieldsResponse {
        status: "ok",
        schema_id: params.schema_id,
        field_count,
        fields,
    })))
}

pub(crate) async fn ingest_schema_registry(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<IngestSchemaRegistryResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_ingest_runtime_privilege(&headers, &state, PrivilegeAction::Read, "ingest/schema")?;
    Ok((StatusCode::OK, Json(collect_ingest_schema_registry_response(&state))))
}

pub(crate) async fn ingest_schema_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<IngestSchemaListQuery>,
) -> Result<(StatusCode, Json<IngestSchemaListResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_ingest_runtime_privilege(&headers, &state, PrivilegeAction::Read, "ingest/schema")?;
    let csv_map = state.ingest_csv_records.lock().expect("csv schema list lock");
    let json_map = state.ingest_json_records.lock().expect("json schema list lock");
    let mut entries: Vec<IngestSchemaEntry> = Vec::new();
    let fmt = params.format.as_deref();
    if fmt.is_none() || fmt == Some("csv") {
        for (connector_id, records) in csv_map.iter() {
            entries.push(IngestSchemaEntry {
                connector_id: connector_id.clone(),
                format: "csv".to_string(),
                row_count: records.len(),
                columns: ingest_infer_columns(records),
            });
        }
    }
    if fmt.is_none() || fmt == Some("json") {
        for (connector_id, records) in json_map.iter() {
            entries.push(IngestSchemaEntry {
                connector_id: connector_id.clone(),
                format: "json".to_string(),
                row_count: records.len(),
                columns: ingest_infer_columns(records),
            });
        }
    }
    let connector_count = entries.len();
    drop(csv_map);
    drop(json_map);
    Ok((StatusCode::OK, Json(IngestSchemaListResponse {
        status: "ok",
        format_filter: params.format,
        connector_count,
        entries,
    })))
}

pub(crate) async fn ingest_format_detect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestFormatDetectRequest>,
) -> Result<(StatusCode, Json<IngestFormatDetectResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let sample = req.sample_data.trim();
    let (detected_format, confidence, field_count) =
        if sample.starts_with('[') || sample.starts_with('{') {
            let fc = if let Ok(v) = serde_json::from_str::<serde_json::Value>(sample) {
                if let Some(obj) = v.as_object() {
                    obj.len()
                } else if let Some(arr) = v.as_array() {
                    arr.first().and_then(|x| x.as_object()).map(|o| o.len()).unwrap_or(0)
                } else { 0 }
            } else { 0 };
            ("json".to_string(), 0.95f64, fc)
        } else if sample.lines().next().map(|l| l.contains(',')).unwrap_or(false) {
            let fc = sample.lines().next().map(|l| l.split(',').count()).unwrap_or(0);
            ("csv".to_string(), 0.85f64, fc)
        } else {
            ("unknown".to_string(), 0.0f64, 0usize)
        };
    Ok((StatusCode::OK, Json(IngestFormatDetectResponse {
        status: "ok",
        detected_format,
        confidence,
        field_count,
    })))
}

pub(crate) async fn ingest_connector_validate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestConnectorValidateRequest>,
) -> Result<(StatusCode, Json<IngestConnectorValidateResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut issues: Vec<String> = Vec::new();
    if req.connector_id.trim().is_empty() {
        issues.push("connector_id cannot be empty".to_string());
    }
    match req.format.as_str() {
        "json" | "csv" | "parquet" | "excel" => {}
        _ => issues.push(format!("unsupported format '{}'; expected: json, csv, parquet, excel", req.format)),
    }
    if let Err(e) = serde_json::from_str::<serde_json::Value>(&req.config_json) {
        issues.push(format!("config_json is not valid JSON: {e}"));
    }
    let valid = issues.is_empty();
    Ok((StatusCode::OK, Json(IngestConnectorValidateResponse {
        status: "ok",
        valid,
        issues,
    })))
}

// ─── Ingest format handlers ───────────────────────────────────────────────────

pub(crate) async fn ingest_csv(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestCsvRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Write,
        &ingest_scope_for_connector(&req.connector_id, "csv"),
    )?;
    use voltnuerongrid_ingest::csv::CsvConnector;
    let mut conn = CsvConnector::new(&req.connector_id, &req.connector_id);
    let count = conn.load_csv(&req.csv_data);
    let records = conn.read_batch(usize::MAX);
    {
        let mut rs = state.row_store.lock().expect("row_store lock");
        let xid = rs.begin_xid();
        for record in &records {
            let mut data = std::collections::HashMap::new();
            data.insert("payload".to_string(), record.payload.clone());
            data.insert("source".to_string(), format!("csv:{}", req.connector_id));
            rs.insert(xid, &record.key, data);
        }
    }
    let storage_key = ingest_storage_key(&principal, &req.connector_id);
    state
        .ingest_csv_records
        .lock()
        .expect("csv lock")
        .insert(storage_key, records);
    let outbox_events_written = append_ingest_outbox_events(
        &state,
        &principal,
        &req.connector_id,
        "csv",
        state
            .ingest_csv_records
            .lock()
            .expect("csv lock")
            .get(&ingest_storage_key(&principal, &req.connector_id))
            .cloned()
            .unwrap_or_default()
            .as_slice(),
    );
    let response = IngestCsvResponse {
        status: "ok",
        connector_id: req.connector_id,
        records_parsed: count,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_csv",
        "ok",
        json!({
            "route_scope": "ingest/connectors/csv",
            "connector_id": response.connector_id,
            "records_parsed": response.records_parsed,
            "outbox_events_written": outbox_events_written,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(response).expect("json")),
    ))
}

pub(crate) async fn ingest_json(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestJsonRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Write,
        &ingest_scope_for_connector(&req.connector_id, "json"),
    )?;
    use voltnuerongrid_ingest::json::JsonConnector;
    let mut conn = JsonConnector::new(&req.connector_id, &req.connector_id, &req.key_field);
    let count = conn.load_ndjson(&req.ndjson_data);
    let records = conn.read_batch(usize::MAX);
    {
        let mut rs = state.row_store.lock().expect("row_store lock");
        let xid = rs.begin_xid();
        for record in &records {
            let mut data = std::collections::HashMap::new();
            data.insert("payload".to_string(), record.payload.clone());
            data.insert("source".to_string(), format!("json:{}", req.connector_id));
            rs.insert(xid, &record.key, data);
        }
    }
    let storage_key = ingest_storage_key(&principal, &req.connector_id);
    state
        .ingest_json_records
        .lock()
        .expect("json lock")
        .insert(storage_key, records);
    let outbox_events_written = append_ingest_outbox_events(
        &state,
        &principal,
        &req.connector_id,
        "json",
        state
            .ingest_json_records
            .lock()
            .expect("json lock")
            .get(&ingest_storage_key(&principal, &req.connector_id))
            .cloned()
            .unwrap_or_default()
            .as_slice(),
    );
    let response = IngestJsonResponse {
        status: "ok",
        connector_id: req.connector_id,
        records_parsed: count,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_json",
        "ok",
        json!({
            "route_scope": "ingest/connectors/json",
            "connector_id": response.connector_id,
            "records_parsed": response.records_parsed,
            "key_field": req.key_field,
            "outbox_events_written": outbox_events_written,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(response).expect("json")),
    ))
}

pub(crate) async fn ingest_parquet(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestParquetRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Write,
        &ingest_scope_for_connector(&req.connector_id, "parquet"),
    )?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(req.parquet_data_base64.trim())
        .map_err(|_| bad_request_error(&headers, "invalid_base64_payload"))?;
    use voltnuerongrid_ingest::parquet::ParquetConnector;
    let mut conn = ParquetConnector::new(&req.connector_id, &req.connector_id);
    let count = conn
        .load_parquet_bytes(&raw)
        .map_err(|_| bad_request_error(&headers, "parquet_parse_failed"))?;
    let records = conn.read_batch(usize::MAX);
    {
        let mut rs = state.row_store.lock().expect("row_store lock");
        let xid = rs.begin_xid();
        for record in &records {
            let mut data = std::collections::HashMap::new();
            data.insert("payload".to_string(), record.payload.clone());
            data.insert("source".to_string(), format!("parquet:{}", req.connector_id));
            rs.insert(xid, &record.key, data);
        }
    }
    let storage_key = ingest_storage_key(&principal, &req.connector_id);
    state
        .ingest_parquet_records
        .lock()
        .expect("parquet lock")
        .insert(storage_key, records);
    let outbox_events_written = append_ingest_outbox_events(
        &state,
        &principal,
        &req.connector_id,
        "parquet",
        state
            .ingest_parquet_records
            .lock()
            .expect("parquet lock")
            .get(&ingest_storage_key(&principal, &req.connector_id))
            .cloned()
            .unwrap_or_default()
            .as_slice(),
    );
    let response = IngestParquetResponse {
        status: "ok",
        connector_id: req.connector_id,
        records_parsed: count,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_parquet",
        "ok",
        json!({
            "route_scope": "ingest/connectors/parquet",
            "connector_id": response.connector_id,
            "records_parsed": response.records_parsed,
            "outbox_events_written": outbox_events_written,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(response).expect("json")),
    ))
}

pub(crate) async fn ingest_excel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestExcelRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Write,
        &ingest_scope_for_connector(&req.connector_id, "excel"),
    )?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(req.xlsx_data_base64.trim())
        .map_err(|_| bad_request_error(&headers, "invalid_base64_payload"))?;
    use voltnuerongrid_ingest::excel::ExcelConnector;
    let mut conn = ExcelConnector::new(&req.connector_id, &req.connector_id);
    let count = conn
        .load_xlsx_bytes(&raw)
        .map_err(|_| bad_request_error(&headers, "excel_parse_failed"))?;
    let records = conn.read_batch(usize::MAX);
    {
        let mut rs = state.row_store.lock().expect("row_store lock");
        let xid = rs.begin_xid();
        for record in &records {
            let mut data = std::collections::HashMap::new();
            data.insert("payload".to_string(), record.payload.clone());
            data.insert("source".to_string(), format!("excel:{}", req.connector_id));
            rs.insert(xid, &record.key, data);
        }
    }
    let storage_key = ingest_storage_key(&principal, &req.connector_id);
    state
        .ingest_excel_records
        .lock()
        .expect("excel lock")
        .insert(storage_key, records);
    let outbox_events_written = append_ingest_outbox_events(
        &state,
        &principal,
        &req.connector_id,
        "excel",
        state
            .ingest_excel_records
            .lock()
            .expect("excel lock")
            .get(&ingest_storage_key(&principal, &req.connector_id))
            .cloned()
            .unwrap_or_default()
            .as_slice(),
    );
    let response = IngestExcelResponse {
        status: "ok",
        connector_id: req.connector_id,
        records_parsed: count,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_excel",
        "ok",
        json!({
            "route_scope": "ingest/connectors/excel",
            "connector_id": response.connector_id,
            "records_parsed": response.records_parsed,
            "outbox_events_written": outbox_events_written,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(response).expect("json")),
    ))
}

pub(crate) async fn ingest_chunked(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestChunkedRequest>,
) -> Result<(StatusCode, Json<IngestChunkedResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Write,
        &ingest_scope_for_connector(&req.connector_id, "chunked"),
    )?;
    use voltnuerongrid_ingest::batch_config::IngestParallelConfig;
    use voltnuerongrid_ingest::IngestRecord;

    let cfg = IngestParallelConfig {
        chunk_target_rows: req.chunk_target_rows.unwrap_or(256),
        max_in_flight_tasks: req.max_in_flight_tasks.unwrap_or(4),
    };
    let records: Vec<IngestRecord> = req
        .records
        .iter()
        .enumerate()
        .map(|(i, payload)| IngestRecord {
            key: format!("{}-{i}", req.connector_id),
            payload: payload.clone(),
        })
        .collect();

    let chunk_target = cfg.chunk_target_rows.max(1);
    let in_flight_cap = cfg.max_in_flight_tasks.max(1);
    let raw_chunks: Vec<Vec<IngestRecord>> = records
        .chunks(chunk_target)
        .map(|c| c.to_vec())
        .collect();
    let chunk_count = raw_chunks.len();
    let mut all_outcomes: Vec<voltnuerongrid_ingest::chunked_loader::ChunkOutcome> = Vec::new();
    for (wave_start, wave) in raw_chunks.chunks(in_flight_cap).enumerate() {
        let base_idx = wave_start * in_flight_cap;
        let handles: Vec<_> = wave
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, chunk)| {
                let chunk_index = base_idx + i;
                tokio::task::spawn_blocking(move || {
                    voltnuerongrid_ingest::chunked_loader::ChunkOutcome {
                        chunk_index,
                        records_in_chunk: chunk.len(),
                    }
                })
            })
            .collect();
        for handle in handles {
            if let Ok(outcome) = handle.await {
                all_outcomes.push(outcome);
            }
        }
    }
    let stats = voltnuerongrid_ingest::chunked_loader::ChunkedIngestStats {
        total_records: records.len(),
        chunk_count,
        chunk_target_rows: chunk_target,
        max_in_flight_tasks: in_flight_cap,
        tasks_dispatched: chunk_count.min(in_flight_cap),
        outcomes: all_outcomes,
    };

    let storage_key = ingest_storage_key(&principal, &req.connector_id);
    state
        .ingest_json_records
        .lock()
        .expect("json lock")
        .insert(storage_key, records);

    let chunks_succeeded = stats.outcomes.len();
    let chunks_failed = stats.chunk_count.saturating_sub(chunks_succeeded);

    let response = IngestChunkedResponse {
        status: "ok",
        connector_id: req.connector_id.clone(),
        total_records: stats.total_records,
        chunk_count: stats.chunk_count,
        tasks_dispatched: stats.tasks_dispatched,
        chunks_succeeded,
        chunks_failed,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_chunked",
        "ok",
        json!({
            "connector_id": response.connector_id,
            "total_records": response.total_records,
            "chunk_count": response.chunk_count,
        }),
    );
    Ok((StatusCode::OK, Json(response)))
}

pub(crate) async fn ingest_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<IngestStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Read,
        ingest_status_scope(),
    )?;
    let csv_map = state.ingest_csv_records.lock().expect("csv lock");
    let json_map = state.ingest_json_records.lock().expect("json lock");
    let parquet_map = state.ingest_parquet_records.lock().expect("parquet lock");
    let excel_map = state.ingest_excel_records.lock().expect("excel lock");
    let (csv_connectors, csv_total) = match &principal {
        RuntimeAccessPrincipal::Operator(_) => (
            csv_map.len(),
            csv_map.values().map(|v| v.len()).sum(),
        ),
        RuntimeAccessPrincipal::TenantUser(user) => {
            count_tenant_ingest_records(&csv_map, &user.tenant_id)
        }
    };
    let (json_connectors, json_total) = match &principal {
        RuntimeAccessPrincipal::Operator(_) => (
            json_map.len(),
            json_map.values().map(|v| v.len()).sum(),
        ),
        RuntimeAccessPrincipal::TenantUser(user) => {
            count_tenant_ingest_records(&json_map, &user.tenant_id)
        }
    };
    let (parquet_connectors, parquet_total) = match &principal {
        RuntimeAccessPrincipal::Operator(_) => (
            parquet_map.len(),
            parquet_map.values().map(|v| v.len()).sum(),
        ),
        RuntimeAccessPrincipal::TenantUser(user) => {
            count_tenant_ingest_records(&parquet_map, &user.tenant_id)
        }
    };
    let (excel_connectors, excel_total) = match &principal {
        RuntimeAccessPrincipal::Operator(_) => (
            excel_map.len(),
            excel_map.values().map(|v| v.len()).sum(),
        ),
        RuntimeAccessPrincipal::TenantUser(user) => {
            count_tenant_ingest_records(&excel_map, &user.tenant_id)
        }
    };
    let response = IngestStatusResponse {
        status: "ok",
        csv_connectors,
        json_connectors,
        parquet_connectors,
        excel_connectors,
        total_records_loaded: csv_total + json_total + parquet_total + excel_total,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_status",
        "ok",
        json!({
            "route_scope": "ingest/status",
            "csv_connectors": response.csv_connectors,
            "json_connectors": response.json_connectors,
            "parquet_connectors": response.parquet_connectors,
            "excel_connectors": response.excel_connectors,
            "total_records_loaded": response.total_records_loaded,
        }),
    );
    Ok(Json(response))
}

pub(crate) async fn ingest_outbox_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<IngestOutboxStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Read,
        &ingest_outbox_scope(None),
    )?;
    let stream_map = state.ingest_outbox_streams.lock().expect("outbox stream map lock");
    let accessible_streams = match &principal {
        RuntimeAccessPrincipal::Operator(_) => stream_map.values().cloned().collect::<Vec<_>>(),
        RuntimeAccessPrincipal::TenantUser(user) => {
            let prefix = format!("tenant/{}/", user.tenant_id);
            stream_map
                .iter()
                .filter(|(storage_key, _)| storage_key.starts_with(&prefix))
                .map(|(_, stream_name)| stream_name.clone())
                .collect::<Vec<_>>()
        }
    };
    drop(stream_map);

    let accessible_set = accessible_streams.iter().cloned().collect::<HashSet<_>>();
    let event_bus = state.ingest_event_bus.lock().expect("event bus lock");
    let broker_mode = event_bus.broker_kind().to_string();
    let broker_target = event_bus.broker_target();
    let visible_events = event_bus
        .events()
        .into_iter()
        .filter(|event| accessible_set.contains(&event.event.stream_name))
        .collect::<Vec<_>>();
    let response = IngestOutboxStatusResponse {
        status: "ok",
        broker_mode,
        broker_target,
        stream_count: accessible_streams.len(),
        total_events: visible_events.len(),
        last_event_id: visible_events.iter().map(|event| event.event.event_id).max(),
        streams: accessible_streams,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_outbox_status",
        "ok",
        json!({
            "route_scope": "ingest/outbox",
            "stream_count": response.stream_count,
            "total_events": response.total_events,
            "last_event_id": response.last_event_id,
        }),
    );
    Ok(Json(response))
}

pub(crate) async fn ingest_outbox_replay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestOutboxReplayRequest>,
) -> Result<Json<IngestOutboxReplayResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_ingest_runtime_privilege(
        &headers,
        &state,
        PrivilegeAction::Read,
        &ingest_outbox_scope(Some(&req.connector_id)),
    )?;
    let storage_key = ingest_storage_key(&principal, &req.connector_id);
    let stream_name = ingest_outbox_stream_name(&storage_key);
    let consumer_id = req
        .consumer_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "default-consumer".to_string());
    let cursor_key = format!("consumer/{consumer_id}/{stream_name}");
    let max_items = req.max_items.unwrap_or(100).min(1_000);
    let acknowledge = req.acknowledge.unwrap_or(true);

    let cursor_before_ack = state
        .ingest_outbox_cursors
        .lock()
        .expect("outbox cursor lock")
        .load(&cursor_key);
    let last_acknowledged_event_id = cursor_before_ack.unwrap_or(0);
    let delivered = state
        .ingest_event_bus
        .lock()
        .expect("event bus lock")
        .export_for_stream_since(&stream_name, last_acknowledged_event_id, max_items)
        .into_iter()
        .collect::<Vec<_>>();

    let mut cursor_after_ack = cursor_before_ack;
    if acknowledge && !delivered.is_empty() {
        let last_event_id = delivered
            .last()
            .map(|event| event.event_id)
            .expect("delivered last event");
        let mut cursor_store = state
            .ingest_outbox_cursors
            .lock()
            .expect("outbox cursor lock");
        let _ = cursor_store.save(&cursor_key, last_event_id);
        cursor_after_ack = cursor_store.load(&cursor_key);
    }

    let delivery_state = if delivered.is_empty() {
        "already_acknowledged"
    } else if acknowledge {
        "delivered_and_acked"
    } else {
        "delivered_pending_ack"
    };
    let response = IngestOutboxReplayResponse {
        status: "ok",
        delivery_state,
        stream_name,
        consumer_id: consumer_id.clone(),
        delivered_count: delivered.len(),
        cursor_before_ack,
        cursor_after_ack,
        acknowledged: acknowledge,
        events: delivered
            .into_iter()
            .map(|event| IngestOutboxReplayEventResponse {
                replay_key: event.replay_key(),
                event_id: event.event_id,
                stream_name: event.stream_name,
                origin: event.origin,
                payload_json: event.payload_json,
            })
            .collect(),
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Ingest,
        &principal,
        "ingest_outbox_replay",
        "ok",
        json!({
            "route_scope": format!("ingest/outbox/{}", req.connector_id),
            "consumer_id": response.consumer_id,
            "delivery_state": response.delivery_state,
            "delivered_count": response.delivered_count,
            "cursor_before_ack": response.cursor_before_ack,
            "cursor_after_ack": response.cursor_after_ack,
            "acknowledged": response.acknowledged,
        }),
    );
    Ok(Json(response))
}
