use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::io::AsyncWriteExt;
use std::env;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use voltnuerongrid_audit::AuditEventKind;
use voltnuerongrid_auth::PrivilegeAction;
use voltnuerongrid_mcp::{McpRequest, McpServerCapabilities, process_request};
use voltnuerongrid_sql::{I18nCatalog, SupportedLocale};
use voltnuerongrid_store::htap_sync::MutationOp;
use crate::{AppState, AuthErrorResponse, now_unix_ms_u64, now_epoch_ms_chaos};
use crate::{NativeCommandKind, NativeFrameType};
use crate::{CommandDispatcher, CanonicalCommandName, CanonicalError, TransportKind};
use crate::{RedisCacheCommandRequest, RedisCacheCommandResponse};
use crate::{CanonicalCommandEnvelope, CanonicalSuccess, ConnectorHealthEntry, ConnectorHealthResponse};
use crate::{FulltextSearchRequest, FulltextSearchResponse, FulltextNotEnabledError};
use crate::{OlapQueryRequest, OlapQueryResponse};
use crate::{DumpExportQuery, DumpExportResponse, ObjectHistoryQuery, ObjectHistoryResponse};
use crate::auth::{require_operator_auth, require_operator_privilege, require_cluster_failover_privilege};
use crate::audit_helpers::append_audit_event;
use crate::{execute_olap_query, record_transport_mutation, rotate_leader};
use crate::{build_failover_handoff_report, dequeue_dr_hook_task, execute_dr_hook};
use crate::{load_native_tls_acceptor, vng_native_listener_log, run_native_connection};
use crate::observability;

// ─── Misc DTOs ──────────────────────────────────────────────────────────


#[derive(Serialize)]
pub(crate) struct HealthResponse {
    pub(crate) status: &'static str,
    pub(crate) node_id: String,
    pub(crate) cluster_mode: String,
}


#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct NativeFrame {
    pub(crate) frame_type: NativeFrameType,
    pub(crate) request_id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) command: Option<NativeCommandKind>,
    pub(crate) payload_json: Option<serde_json::Value>,
}


#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct NativeAdapter;

impl NativeAdapter {
    #[allow(dead_code)]
    pub(crate) fn from_command_frame<TPayload>(
        frame: &NativeFrame,
        command: CanonicalCommandName,
        payload: TPayload,
    ) -> Result<CanonicalCommandEnvelope<TPayload>, CanonicalError> {
        if frame.frame_type != NativeFrameType::Command {
            return Err(CanonicalError {
                request_id: frame.request_id.clone(),
                transport: TransportKind::Native,
                kind: "protocol",
                message: "expected COMMAND frame for canonical dispatch".to_string(),
            });
        }
        let mut transport_metadata = std::collections::HashMap::new();
        transport_metadata.insert("protocol".to_string(), "native".to_string());
        transport_metadata.insert("frame_type".to_string(), "COMMAND".to_string());
        if let Some(cmd) = frame.command {
            transport_metadata.insert("native_command".to_string(), format!("{cmd:?}"));
        }
        Ok(CanonicalCommandEnvelope {
            request_id: frame.request_id.clone(),
            transport: TransportKind::Native,
            command,
            session_context: frame.session_id.clone(),
            transport_metadata,
            payload,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn success_to_result_frame<TPayload: Serialize>(
        success: &CanonicalSuccess<TPayload>,
    ) -> NativeFrame {
        let payload_json = serde_json::to_value(&success.payload).ok();
        NativeFrame {
            frame_type: NativeFrameType::Result,
            request_id: success.request_id.clone(),
            session_id: None,
            command: None,
            payload_json,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn error_to_error_frame(error: &CanonicalError) -> NativeFrame {
        NativeFrame {
            frame_type: NativeFrameType::Error,
            request_id: error.request_id.clone(),
            session_id: None,
            command: None,
            payload_json: Some(json!({
                "kind": error.kind,
                "message": error.message,
            })),
        }
    }
}


// ─── S7-WS6-04: Chaos injection types ────────────────────────────────────────

/// A single chaos fault event injected into the cluster simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChaosEvent {
    /// The type of fault, e.g. `"network_partition"`, `"node_crash"`, `"packet_loss"`.
    pub(crate) fault_type: String,
    /// Optional target node identifier.
    pub(crate) target_node: Option<String>,
    /// Arbitrary key–value parameters for the fault (e.g. `{ "loss_pct": "30" }`).
    pub(crate) parameters: HashMap<String, String>,
    /// Epoch-millisecond timestamp when the fault was injected.
    pub(crate) injected_at_ms: u64,
    /// Epoch-millisecond timestamp when the fault was cleared, if any.
    pub(crate) cleared_at_ms: Option<u64>,
}


/// Mutable chaos state (active faults + history).
#[derive(Debug, Default)]
pub(crate) struct ChaosState {
    pub(crate) active_faults: Vec<ChaosEvent>,
    pub(crate) event_history: Vec<ChaosEvent>,
}


/// Request body for `POST /api/v1/cluster/chaos/inject`.
#[derive(Deserialize)]
pub(crate) struct ChaosInjectRequest {
    pub(crate) fault_type: String,
    pub(crate) target_node: Option<String>,
    #[serde(default)]
    pub(crate) parameters: HashMap<String, String>,
}


#[derive(Serialize)]
pub(crate) struct ChaosStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) active_fault_count: usize,
    pub(crate) total_injected: usize,
    pub(crate) active_faults: Vec<ChaosEvent>,
}


/// Response for `GET /api/v1/cluster/chaos/health`.
#[derive(Serialize)]
pub(crate) struct ChaosHealthResponse {
    pub(crate) status: &'static str,
    pub(crate) cluster_healthy: bool,
    pub(crate) active_fault_count: usize,
    pub(crate) history_len: usize,
}


/// Response for `GET /api/v1/cluster/chaos/history`.
#[derive(Serialize)]
pub(crate) struct ChaosHistoryResponse {
    pub(crate) status: &'static str,
    pub(crate) history_len: usize,
    pub(crate) events: Vec<ChaosEvent>,
}




// ─── S7-WS6-04: Chaos fire-drill structs ────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct ChaosFireDrillRequest {
    pub(crate) drill_type: String,
    pub(crate) target_node: Option<String>,
}


#[derive(Debug, Serialize)]
pub(crate) struct ChaosFireDrillResponse {
    pub(crate) status: &'static str,
    pub(crate) drill_type: String,
    pub(crate) faults_injected: usize,
    pub(crate) target_node: String,
}



#[derive(Serialize)]
pub(crate) struct FailoverStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) cluster_mode: String,
    pub(crate) leader_node_id: String,
    pub(crate) unresolved_critical_count: usize,
    pub(crate) rto_seconds_target: u32,
    pub(crate) rpo_data_loss_rows_target: u32,
}


#[derive(Deserialize)]
pub(crate) struct FailoverSimulateRequest {
    pub(crate) new_leader_node_id: String,
    pub(crate) reason: Option<String>,
    pub(crate) requested_by: Option<String>,
}


#[derive(Serialize)]
pub(crate) struct FailoverSimulateResponse {
    pub(crate) status: &'static str,
    pub(crate) previous_leader_node_id: String,
    pub(crate) new_leader_node_id: String,
    pub(crate) reason: String,
    pub(crate) requested_by: String,
    pub(crate) handoff_report: FailoverHandoffReportResponse,
}


#[derive(Serialize)]
pub(crate) struct FailoverHandoffGapResponse {
    pub(crate) expected: u64,
    pub(crate) actual: u64,
}


#[derive(Serialize)]
pub(crate) struct FailoverHandoffReportResponse {
    pub(crate) handoff_state: &'static str,
    pub(crate) source_node_id: String,
    pub(crate) target_node_id: String,
    pub(crate) last_applied_sequence_before: u64,
    pub(crate) last_applied_sequence_after: u64,
    pub(crate) replay_batch_size: usize,
    pub(crate) applied_count: usize,
    pub(crate) gap_count: usize,
    pub(crate) gaps: Vec<FailoverHandoffGapResponse>,
}


#[derive(Deserialize)]
pub(crate) struct I18nMessagesQuery {
    pub(crate) locale: Option<String>,
}


#[derive(Serialize)]
pub(crate) struct I18nMessagesResponse {
    pub(crate) status: &'static str,
    pub(crate) locale: String,
    pub(crate) messages: std::collections::BTreeMap<String, String>,
}



#[derive(Debug, Clone)]
pub(crate) struct NativeListenerConfig {
    pub(crate) enabled: bool,
    pub(crate) bind: String,
    pub(crate) tls_enabled: bool,
    pub(crate) tls_cert_path: Option<String>,
    pub(crate) tls_key_path: Option<String>,
    pub(crate) tls_client_ca_path: Option<String>,
    pub(crate) max_connections: usize,
    pub(crate) idle_timeout_ms: u64,
    pub(crate) handshake_timeout_ms: u64,
    pub(crate) heartbeat_interval_ms: u64,
    pub(crate) max_frame_bytes: usize,
    pub(crate) compression_enabled: bool,
    pub(crate) compression_threshold_bytes: usize,
    pub(crate) bearer_token: Option<String>,
}



// â”€â”€ WS4 Ingest handlers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€


// â”€â”€ REQ-02: DDL catalog schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

// â”€â”€ REQ-23: ACID active transactions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

// ─── Phase 1.3 — Database CRUD ─────────────────────────────────────────────
//
// HTTP API for first-class database lifecycle management. Sits alongside
// (and will eventually replace) the implicit `database_name` string used as
// a key prefix in `DdlCatalog`. See `gaps-may26-1.md` §3.2 for context.

// REQ-10/19: benchmark endpoint types and handlers
#[derive(Deserialize)]
pub(crate) struct BenchmarkIngestRequest {
    /// Number of synthetic records to generate (default: 10_000)
    pub(crate) record_count: Option<usize>,
    /// Target chunk size (default: 256)
    pub(crate) chunk_target_rows: Option<usize>,
}


#[derive(Serialize)]
pub(crate) struct BenchmarkIngestResponse {
    pub(crate) status: &'static str,
    pub(crate) record_count: usize,
    pub(crate) chunk_count: usize,
    pub(crate) wall_time_ms: u128,
    pub(crate) records_per_second: f64,
}


#[derive(Deserialize)]
pub(crate) struct BenchmarkQueryRequest {
    /// Number of SQL classification ops to run (default: 10_000)
    pub(crate) op_count: Option<usize>,
}


#[derive(Serialize)]
pub(crate) struct BenchmarkQueryResponse {
    pub(crate) status: &'static str,
    pub(crate) op_count: usize,
    pub(crate) wall_time_ms: u128,
    pub(crate) ops_per_second: f64,
}


// ─── S6-003: Import SQL execution pipeline ────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct ImportSqlRequest {
    pub(crate) sql_script: String,
    pub(crate) dry_run: Option<bool>,
    pub(crate) stop_on_error: Option<bool>,
}


#[derive(Debug, Serialize)]
pub(crate) struct ImportSqlResponse {
    pub(crate) statements_executed: usize,
    pub(crate) errors: Vec<String>,
}


// ─── Misc handlers ───────────────────────────────────────────────────────


pub(crate) async fn run_native_listener(config: NativeListenerConfig, state: AppState) {
    let tls_acceptor: Option<Arc<tokio_rustls::TlsAcceptor>> = if config.tls_enabled {
        match (&config.tls_cert_path, &config.tls_key_path) {
            (Some(cert_path), Some(key_path)) => match load_native_tls_acceptor(
                cert_path,
                key_path,
                config.tls_client_ca_path.as_deref(),
            ) {
                Ok(a) => Some(a),
                Err(e) => {
                    vng_native_listener_log(
                        "tls_cert_load_failed",
                        json!({ "message": e.to_string() }),
                    );
                    return;
                }
            },
            _ => {
                vng_native_listener_log(
                    "tls_config_invalid",
                    json!({ "message": "VNG_NATIVE_TLS_ENABLED=true requires VNG_NATIVE_TLS_CERT_PATH and VNG_NATIVE_TLS_KEY_PATH" }),
                );
                return;
            }
        }
    } else {
        None
    };

    let bind_addr: SocketAddr = match config.bind.parse() {
        Ok(addr) => addr,
        Err(err) => {
            vng_native_listener_log(
                "bind_parse_failed",
                json!({ "bind": config.bind, "message": err.to_string() }),
            );
            return;
        }
    };

    let listener = match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            vng_native_listener_log(
                "bind_failed",
                json!({ "bind_addr": bind_addr.to_string(), "message": err.to_string() }),
            );
            return;
        }
    };

    let sem = Arc::new(Semaphore::new(config.max_connections));
    let handshake = Duration::from_millis(config.handshake_timeout_ms.max(1000));

    loop {
        match listener.accept().await {
            Ok((mut socket, peer_addr)) => {
                let permit = match sem.clone().try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        vng_native_listener_log(
                            "connection_rejected",
                            json!({
                                "reason": "max_connections",
                                "max_connections": config.max_connections,
                                "peer": peer_addr.to_string(),
                            }),
                        );
                        let _ = socket.shutdown().await;
                        continue;
                    }
                };
                vng_native_listener_log(
                    "accepted",
                    json!({ "peer": peer_addr.to_string(), "tls": tls_acceptor.is_some() }),
                );
                let st = state.clone();
                let cfg = config.clone();
                if let Some(ref acc) = tls_acceptor {
                    let acc = acc.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        let tls_stream = match tokio::time::timeout(handshake, acc.accept(socket)).await {
                            Ok(Ok(s)) => s,
                            Ok(Err(e)) => {
                                vng_native_listener_log(
                                    "tls_handshake_failed",
                                    json!({ "peer": peer_addr.to_string(), "message": e.to_string() }),
                                );
                                return;
                            }
                            Err(_) => {
                                vng_native_listener_log(
                                    "tls_handshake_timeout",
                                    json!({ "peer": peer_addr.to_string(), "handshake_timeout_ms": config.handshake_timeout_ms }),
                                );
                                return;
                            }
                        };
                        run_native_connection(tls_stream, st, cfg).await;
                    });
                } else {
                    tokio::spawn(async move {
                        let _permit = permit;
                        run_native_connection(socket, st, cfg).await;
                    });
                }
            }
            Err(err) => {
                vng_native_listener_log("accept_failed", json!({ "message": err.to_string() }));
                break;
            }
        }
    }
}


pub(crate) async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let dispatcher = CommandDispatcher::new();
    Json(dispatcher.dispatch_health(&state))
}


/// Prometheus scrape endpoint.
///
/// Returns the current metrics as Prometheus text-format. Returns an empty
/// string (not an error) when metrics are disabled — Prometheus tolerates
/// that and stops scraping after a few zero-byte responses.
pub(crate) async fn metrics_handler() -> (axum::http::StatusCode, [(axum::http::HeaderName, &'static str); 1], String) {
    let body = observability::render_metrics();
    (
        axum::http::StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}


pub(crate) async fn olap_query(
    State(state): State<AppState>,
    Json(req): Json<OlapQueryRequest>,
) -> Json<OlapQueryResponse> {
    let rs = state.row_store.lock().expect("row_store lock olap_query");
    Json(execute_olap_query(req.query, req.max_rows, &rs))
}


pub(crate) async fn failover_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<FailoverStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let _operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Read)?;
    let leader = state
        .leader_node_id
        .lock()
        .map(|value| value.clone())
        .unwrap_or_else(|_| state.node_id.clone());
    let unresolved_critical_count = state
        .cluster_failure_signals
        .lock()
        .map(|signals| {
            signals
                .iter()
                .filter(|signal| signal.severity.eq_ignore_ascii_case("critical") && !signal.resolved)
                .count()
        })
        .unwrap_or(usize::MAX);
    Ok(Json(FailoverStatusResponse {
        status: if unresolved_critical_count > 0 {
            "degraded"
        } else {
            "healthy"
        },
        cluster_mode: state.cluster_mode,
        leader_node_id: leader,
        unresolved_critical_count,
        rto_seconds_target: 30,
        rpo_data_loss_rows_target: 0,
    }))
}


pub(crate) async fn failover_simulate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FailoverSimulateRequest>,
) -> Result<Json<FailoverSimulateResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let operator = require_cluster_failover_privilege(&headers, &state, PrivilegeAction::Execute)?;
    let (previous_leader_node_id, new_leader_node_id) =
        rotate_leader(&state.leader_node_id, &req.new_leader_node_id, &state.node_id);
    record_transport_mutation(
        &state,
        &previous_leader_node_id,
        &new_leader_node_id,
        "failover_control_plane",
        "cluster_failover",
        &format!("{}->{}:prepare", previous_leader_node_id, new_leader_node_id),
        MutationOp::Insert,
        json!({
            "event": "leader_handoff_prepare",
            "source_node_id": previous_leader_node_id,
            "target_node_id": new_leader_node_id,
            "requested_by": operator.operator_id.as_str(),
            "operator_role": operator.role.as_str(),
            "reason": req
                .reason
                .clone()
                .unwrap_or_else(|| "manual_failover_simulation".to_string()),
            "transport": "control_plane"
        }),
    );
    record_transport_mutation(
        &state,
        &previous_leader_node_id,
        &new_leader_node_id,
        "failover_control_plane",
        "cluster_failover",
        &format!("{}->{}:commit", previous_leader_node_id, new_leader_node_id),
        MutationOp::Update,
        json!({
            "event": "leader_handoff_commit",
            "source_node_id": previous_leader_node_id,
            "target_node_id": new_leader_node_id,
            "requested_by": operator.operator_id.as_str(),
            "operator_role": operator.role.as_str(),
            "reason": req
                .reason
                .clone()
                .unwrap_or_else(|| "manual_failover_simulation".to_string()),
            "transport": "control_plane"
        }),
    );
    let handoff_report = build_failover_handoff_report(
        &state,
        &previous_leader_node_id,
        &new_leader_node_id,
    );
    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "failover_simulate",
        "ok",
        &json!({
            "previous_leader_node_id": previous_leader_node_id.clone(),
            "new_leader_node_id": new_leader_node_id.clone(),
            "operator_role": operator.role.as_str(),
            "reason": req.reason.clone().unwrap_or_else(|| "manual_failover_simulation".to_string()),
            "handoff_state": handoff_report.handoff_state,
            "replay_batch_size": handoff_report.replay_batch_size,
            "applied_count": handoff_report.applied_count,
            "gap_count": handoff_report.gap_count
        })
        .to_string(),
    );
    Ok(Json(FailoverSimulateResponse {
        status: "ok",
        previous_leader_node_id,
        new_leader_node_id,
        reason: req
            .reason
            .unwrap_or_else(|| "manual_failover_simulation".to_string()),
        requested_by: req.requested_by.unwrap_or_else(|| "unknown".to_string()),
        handoff_report,
    }))
}


// REQ-27: Redis-compat cache command handler -----------------------------------
pub(crate) async fn cache_redis_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RedisCacheCommandRequest>,
) -> Result<Json<RedisCacheCommandResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "cluster.sre",
        "sre/cache",
        PrivilegeAction::Execute,
    )?;

    let cmd = req.cmd.trim().to_ascii_uppercase();
    let partition_id = req.partition_id.as_deref().unwrap_or("default");
    let now_ms = now_unix_ms_u64();

    let response = match cmd.as_str() {
        "PING" => RedisCacheCommandResponse {
            status: "ok",
            cmd: cmd.clone(),
            value: Some(serde_json::json!("PONG")),
            exists: None,
            removed: None,
            flushed_count: None,
            keys: None,
            error: None,
        },
        "GET" => {
            let key = req.key.as_deref().unwrap_or("");
            let result = state.distributed_cache.lock().expect("cache manager lock")
                .get(partition_id, key, now_ms);
            match result {
                Ok(value) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: Some(e.to_string()),
                },
            }
        }
        "SET" => {
            let key = req.key.clone().unwrap_or_default();
            let value = req.value.clone().unwrap_or(serde_json::Value::Null);
            let result = state.distributed_cache.lock().expect("cache manager lock")
                .set(partition_id, key, value, req.ttl_ms, now_ms);
            RedisCacheCommandResponse {
                status: if result.is_ok() { "ok" } else { "error" },
                cmd: cmd.clone(),
                value: None,
                exists: None,
                removed: None,
                flushed_count: None,
                keys: None,
                error: result.err().map(|e| e.to_string()),
            }
        }
        "DEL" => {
            let key = req.key.as_deref().unwrap_or("");
            let result = state.distributed_cache.lock().expect("cache manager lock")
                .invalidate(partition_id, key);
            match result {
                Ok(removed) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: Some(removed),
                    flushed_count: None,
                    keys: None,
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: Some(false),
                    flushed_count: None,
                    keys: None,
                    error: Some(e.to_string()),
                },
            }
        }
        "EXISTS" => {
            let key = req.key.as_deref().unwrap_or("");
            let result = state.distributed_cache.lock().expect("cache manager lock")
                .get(partition_id, key, now_ms);
            let exists = result.as_ref().map(|v| v.is_some()).unwrap_or(false);
            RedisCacheCommandResponse {
                status: "ok",
                cmd: cmd.clone(),
                value: None,
                exists: Some(exists),
                removed: None,
                flushed_count: None,
                keys: None,
                error: result.err().map(|e| e.to_string()),
            }
        }
        "KEYS" => {
            let result = state.distributed_cache.lock().expect("cache manager lock")
                .keys_in_partition(partition_id, now_ms);
            match result {
                Ok(keys) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: Some(keys),
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: Some(vec![]),
                    error: Some(e.to_string()),
                },
            }
        }
        "FLUSH" => {
            let result = state.distributed_cache.lock().expect("cache manager lock")
                .invalidate_partition(partition_id);
            match result {
                Ok(flushed) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: Some(flushed),
                    keys: None,
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: Some(0),
                    keys: None,
                    error: Some(e.to_string()),
                },
            }
        }
        // REQ-27: EXPIRE â€” update TTL on existing key
        "EXPIRE" => {
            let key = req.key.as_deref().unwrap_or("");
            let ttl = req.expire_ms.or(req.ttl_ms).unwrap_or(60_000);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            match cache.expire_key(partition_id, key, ttl, now_ms) {
                Ok(updated) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value: Some(serde_json::json!(updated)),
                    exists: Some(updated),
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: Some(e.to_string()),
                },
            }
        }
        // REQ-27: INCR / INCRBY â€” atomically increment numeric value
        "INCR" | "INCRBY" => {
            let key = req.key.as_deref().unwrap_or("");
            let delta = if cmd.as_str() == "INCR" { 1.0 } else { req.delta.unwrap_or(1.0) };
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            match cache.increment_key(partition_id, key, delta, req.ttl_ms, now_ms) {
                Ok(new_val) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value: Some(serde_json::json!(new_val)),
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: Some(e.to_string()),
                },
            }
        }
        // REQ-27: DECR / DECRBY â€” atomically decrement numeric value
        "DECR" | "DECRBY" => {
            let key = req.key.as_deref().unwrap_or("");
            let delta = if cmd.as_str() == "DECR" { -1.0 } else { -(req.delta.unwrap_or(1.0)) };
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            match cache.increment_key(partition_id, key, delta, req.ttl_ms, now_ms) {
                Ok(new_val) => RedisCacheCommandResponse {
                    status: "ok",
                    cmd: cmd.clone(),
                    value: Some(serde_json::json!(new_val)),
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: None,
                },
                Err(e) => RedisCacheCommandResponse {
                    status: "error",
                    cmd: cmd.clone(),
                    value: None,
                    exists: None,
                    removed: None,
                    flushed_count: None,
                    keys: None,
                    error: Some(e.to_string()),
                },
            }
        }
        // REQ-27: MGET â€” return values for multiple keys as a JSON array
        "MGET" => {
            let keys = req.keys.clone().unwrap_or_default();
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let mut results = Vec::new();
            for k in &keys {
                let v = cache.get(partition_id, k, now_ms).ok().flatten();
                results.push(v.unwrap_or(serde_json::Value::Null));
            }
            RedisCacheCommandResponse {
                status: "ok",
                cmd: cmd.clone(),
                value: Some(serde_json::Value::Array(results)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: MSET â€” set multiple keys from a JSON object { key: value, ... }
        "MSET" => {
            let obj = match req.value.as_ref().and_then(|v| v.as_object()) {
                Some(m) => m.clone(),
                None => return Ok(Json(RedisCacheCommandResponse {
                    status: "error", cmd: cmd.clone(), value: None, exists: None,
                    removed: None, flushed_count: None, keys: None,
                    error: Some("MSET requires value to be a JSON object".to_string()),
                })),
            };
            let count = obj.len();
            {
                let mut cache = state.distributed_cache.lock().expect("cache lock");
                for (k, v) in obj {
                    let _ = cache.set(partition_id, k, v, req.ttl_ms, now_ms);
                }
            }
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(count)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: GETSET â€” atomically get old value then set new value
        "GETSET" => {
            let key = req.key.as_deref().unwrap_or("");
            let new_val = req.value.clone().unwrap_or(serde_json::Value::Null);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let old_val = cache.get(partition_id, key, now_ms).ok().flatten();
            let _ = cache.set(partition_id, key.to_string(), new_val, req.ttl_ms, now_ms);
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: old_val,
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: LPUSH â€” prepend a value to a list (stored as JSON array)
        "LPUSH" => {
            let key = req.key.as_deref().unwrap_or("");
            let push_val = req.value.clone().unwrap_or(serde_json::Value::Null);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let mut list: Vec<serde_json::Value> = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr,
                _ => Vec::new(),
            };
            list.insert(0, push_val);
            let new_len = list.len();
            let _ = cache.set(partition_id, key.to_string(), serde_json::Value::Array(list), req.ttl_ms, now_ms);
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(new_len)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: RPUSH â€” append a value to a list (stored as JSON array)
        "RPUSH" => {
            let key = req.key.as_deref().unwrap_or("");
            let push_val = req.value.clone().unwrap_or(serde_json::Value::Null);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let mut list: Vec<serde_json::Value> = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr,
                _ => Vec::new(),
            };
            list.push(push_val);
            let new_len = list.len();
            let _ = cache.set(partition_id, key.to_string(), serde_json::Value::Array(list), req.ttl_ms, now_ms);
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(new_len)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: LLEN â€” return the length of a list
        "LLEN" => {
            let key = req.key.as_deref().unwrap_or("");
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let len = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr.len(),
                _ => 0,
            };
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(len)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: LRANGE â€” return a sub-range of a list (Redis semantics, inclusive stop,
        // negative indices count from tail; -1 = last element)
        "LRANGE" => {
            let key = req.key.as_deref().unwrap_or("");
            let start = req.start.unwrap_or(0);
            let stop = req.stop.unwrap_or(-1);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let list: Vec<serde_json::Value> = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr,
                _ => Vec::new(),
            };
            let len = list.len() as i64;
            let resolve = |i: i64| -> usize {
                if i < 0 { (len + i).max(0) as usize } else { i.min(len) as usize }
            };
            let s = resolve(start);
            let e = (resolve(stop) + 1).min(len as usize);
            let slice = if s < e { list[s..e].to_vec() } else { Vec::new() };
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::Value::Array(slice)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: HSET — set a field in a hash (stored as JSON object in cache)
        "HSET" => {
            let key = req.key.as_deref().unwrap_or("");
            let field = req.field.as_deref().unwrap_or("");
            let val = req.value.clone().unwrap_or(serde_json::Value::Null);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let mut hash: serde_json::Map<String, serde_json::Value> =
                match cache.get(partition_id, key, now_ms).ok().flatten() {
                    Some(serde_json::Value::Object(m)) => m,
                    _ => serde_json::Map::new(),
                };
            hash.insert(field.to_string(), val);
            let _ = cache.set(partition_id, key.to_string(), serde_json::Value::Object(hash), req.ttl_ms, now_ms);
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(1)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: HGET — get a single field from a hash
        "HGET" => {
            let key = req.key.as_deref().unwrap_or("");
            let field = req.field.as_deref().unwrap_or("");
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let field_val = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Object(m)) => m.get(field).cloned(),
                _ => None,
            };
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: field_val,
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: HDEL — delete a field from a hash
        "HDEL" => {
            let key = req.key.as_deref().unwrap_or("");
            let field = req.field.as_deref().unwrap_or("");
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let mut removed = false;
            match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Object(mut m)) => {
                    removed = m.remove(field).is_some();
                    let _ = cache.set(partition_id, key.to_string(), serde_json::Value::Object(m), req.ttl_ms, now_ms);
                }
                _ => {}
            }
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(if removed { 1 } else { 0 })),
                exists: None, removed: Some(removed), flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: HGETALL — return full hash as JSON object
        "HGETALL" => {
            let key = req.key.as_deref().unwrap_or("");
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let hash_val = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(v @ serde_json::Value::Object(_)) => v,
                _ => serde_json::Value::Object(serde_json::Map::new()),
            };
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(hash_val),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: SADD — add a member to a set (stored as JSON array, deduplicated)
        "SADD" => {
            let key = req.key.as_deref().unwrap_or("");
            let member = req.value.clone().unwrap_or(serde_json::Value::Null);
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let mut set: Vec<serde_json::Value> = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr,
                _ => Vec::new(),
            };
            let added = if !set.contains(&member) {
                set.push(member);
                1usize
            } else {
                0usize
            };
            let _ = cache.set(partition_id, key.to_string(), serde_json::Value::Array(set), req.ttl_ms, now_ms);
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(added)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: SMEMBERS — return all members of a set
        "SMEMBERS" => {
            let key = req.key.as_deref().unwrap_or("");
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let members = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr,
                _ => Vec::new(),
            };
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::Value::Array(members)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        // REQ-27: SCARD — return the cardinality (number of members) of a set
        "SCARD" => {
            let key = req.key.as_deref().unwrap_or("");
            let mut cache = state.distributed_cache.lock().expect("cache lock");
            let card = match cache.get(partition_id, key, now_ms).ok().flatten() {
                Some(serde_json::Value::Array(arr)) => arr.len(),
                _ => 0,
            };
            RedisCacheCommandResponse {
                status: "ok", cmd: cmd.clone(), value: Some(serde_json::json!(card)),
                exists: None, removed: None, flushed_count: None, keys: None, error: None,
            }
        }
        unsupported_cmd => RedisCacheCommandResponse {
            status: "error",
            cmd: cmd.clone(),
            value: None,
            exists: None,
            removed: None,
            flushed_count: None,
            keys: None,
            error: Some(format!("unsupported Redis-compat command: {unsupported_cmd}")),
        },
    };
    append_audit_event(
        &state,
        AuditEventKind::Failover,
        &operator.operator_id,
        "cache_redis_command",
        response.status,
        &json!({
            "cmd": response.cmd,
            "partition_id": partition_id,
            "key": req.key,
        })
        .to_string(),
    );

    Ok(Json(response))
}


pub(crate) async fn i18n_messages(Query(query): Query<I18nMessagesQuery>) -> Json<I18nMessagesResponse> {
    let locale = SupportedLocale::parse(query.locale.as_deref().unwrap_or("en-US"));
    let keys = ["unauthorized", "missing_or_invalid_admin_key", "health_ok"];
    let mut messages = std::collections::BTreeMap::new();
    for key in keys {
        let localized = I18nCatalog::message(locale, key);
        messages.insert(key.to_string(), localized.message.to_string());
    }
    Json(I18nMessagesResponse {
        status: "ok",
        locale: locale.as_str().to_string(),
        messages,
    })
}


/// S7-WS6-04: inject a chaos/game-day fault event.
pub(crate) async fn chaos_inject(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Json(req): axum::extract::Json<ChaosInjectRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let event = ChaosEvent {
        fault_type: req.fault_type,
        target_node: req.target_node,
        parameters: req.parameters,
        injected_at_ms: now_epoch_ms_chaos(),
        cleared_at_ms: None,
    };
    let mut cs = state.chaos_state.lock().expect("chaos_state lock");
    cs.active_faults.push(event);
    let count = cs.active_faults.len();
    drop(cs);
    Ok((StatusCode::OK, Json(serde_json::json!({ "status": "injected", "active_fault_count": count }))))
}


/// S7-WS6-04: clear all active faults; move them to history.
pub(crate) async fn chaos_clear(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let cleared_at = now_epoch_ms_chaos();
    let mut cs = state.chaos_state.lock().expect("chaos_state lock");
    let mut cleared: Vec<ChaosEvent> = cs.active_faults.drain(..).map(|mut e| {
        e.cleared_at_ms = Some(cleared_at);
        e
    }).collect();
    cs.event_history.append(&mut cleared);
    let history_len = cs.event_history.len();
    drop(cs);
    Ok((StatusCode::OK, Json(serde_json::json!({ "status": "cleared", "history_len": history_len }))))
}


/// S7-WS6-04: return current chaos state summary.
pub(crate) async fn chaos_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ChaosStatusResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let cs = state.chaos_state.lock().expect("chaos_state lock");
    let active_fault_count = cs.active_faults.len();
    let total_injected = cs.active_faults.len() + cs.event_history.len();
    let active_faults = cs.active_faults.clone();
    drop(cs);
    Ok((StatusCode::OK, Json(ChaosStatusResponse {
        status: "ok",
        active_fault_count,
        total_injected,
        active_faults,
    })))
}


/// S7-WS6-04: return cluster health based on active chaos faults.
pub(crate) async fn chaos_health(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ChaosHealthResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let cs = state.chaos_state.lock().expect("chaos_state lock");
    let active_fault_count = cs.active_faults.len();
    let history_len = cs.event_history.len();
    drop(cs);
    let cluster_healthy = active_fault_count == 0;
    Ok((StatusCode::OK, Json(ChaosHealthResponse {
        status: "ok",
        cluster_healthy,
        active_fault_count,
        history_len,
    })))
}


/// S7-WS6-04: Return the full fault event history (cleared faults).
pub(crate) async fn chaos_history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ChaosHistoryResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let cs = state.chaos_state.lock().expect("chaos_state lock history");
    let events = cs.event_history.clone();
    let history_len = events.len();
    drop(cs);
    Ok((StatusCode::OK, Json(ChaosHistoryResponse {
        status: "ok",
        history_len,
        events,
    })))
}


// ─── S7-WS6-04: Chaos fire-drill handler ────────────────────────────────────

/// S7-WS6-04: Execute a scheduled chaos fire drill — injects a fault and marks it as
/// a drill (not a real failure); clears immediately after injection.
pub(crate) async fn chaos_fire_drill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ChaosFireDrillRequest>,
) -> Result<(StatusCode, Json<ChaosFireDrillResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let target_node = req.target_node.unwrap_or_else(|| "local".to_string());
    let now_ms = now_unix_ms_u64();
    let drill_event = ChaosEvent {
        fault_type: format!("fire_drill:{}", req.drill_type),
        target_node: Some(target_node.clone()),
        parameters: std::collections::HashMap::new(),
        injected_at_ms: now_ms,
        cleared_at_ms: Some(now_ms),
    };
    {
        let mut cs = state.chaos_state.lock().expect("chaos_state fire_drill lock");
        cs.event_history.push(drill_event);
    }
    Ok((StatusCode::OK, Json(ChaosFireDrillResponse {
        status: "ok",
        drill_type: req.drill_type,
        faults_injected: 1,
        target_node,
    })))
}


// ─── S11-WS1-12: Connector health check endpoint ────────────────────────────

/// S11-WS1-12: Return health status for all registered connectors.
pub(crate) async fn connectors_health(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ConnectorHealthResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let registry = state.connector_registry.lock().expect("connector_registry lock connectors_health");
    let entries: Vec<ConnectorHealthEntry> = registry.iter().map(|c| ConnectorHealthEntry {
        connector_id: c.connector_id.clone(),
        connector_type: c.connector_type.clone(),
        version: c.version.clone(),
        signed: c.signed,
        // Scaffold: signed connectors are considered healthy; unsigned ones are degraded.
        healthy: c.signed,
    }).collect();
    let total = entries.len();
    let healthy = entries.iter().filter(|e| e.healthy).count();
    drop(registry);
    Ok((StatusCode::OK, Json(ConnectorHealthResponse {
        status: "ok",
        total,
        healthy,
        entries,
    })))
}


pub(crate) async fn run_dr_hook_scheduler(state: AppState) {
    loop {
        if let Some(task) = dequeue_dr_hook_task(&state) {
            let execution = execute_dr_hook(&state, &task.hook, Some(&task.scope), task.dry_run);
            append_audit_event(
                &state,
                AuditEventKind::Failover,
                &task.requested_by,
                "sre_dr_hook_scheduler_execute",
                execution.status,
                &json!({
                    "task_id": task.task_id,
                    "hook": task.hook,
                    "scope": task.scope,
                    "reason": task.reason,
                    "execution_id": execution.execution_id,
                    "policy_decision": execution.policy_decision,
                })
                .to_string(),
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}


pub(crate) async fn benchmark_ingest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<BenchmarkIngestRequest>,
) -> Result<(StatusCode, Json<BenchmarkIngestResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    use voltnuerongrid_ingest::chunked_loader::ChunkedLoader;
    use voltnuerongrid_ingest::batch_config::IngestParallelConfig;
    use voltnuerongrid_ingest::IngestRecord;

    let record_count = req.record_count.unwrap_or(10_000).min(1_000_000);
    let chunk_size = req.chunk_target_rows.unwrap_or(256);

    let records: Vec<IngestRecord> = (0..record_count)
        .map(|i| IngestRecord {
            key: format!("bmark-{i}"),
            payload: format!("{{\"id\":{i},\"value\":{i}}}"),
        })
        .collect();

    let cfg = IngestParallelConfig { chunk_target_rows: chunk_size, max_in_flight_tasks: 4 };
    let start = std::time::Instant::now();
    let mut loader = ChunkedLoader::new(cfg);
    loader.push_chunk(records);
    let stats = loader.finalize();
    let elapsed = start.elapsed();
    let wall_ms = elapsed.as_millis();
    let rps = if wall_ms == 0 { record_count as f64 * 1000.0 } else { record_count as f64 / (wall_ms as f64 / 1000.0) };

    Ok((StatusCode::OK, Json(BenchmarkIngestResponse {
        status: "ok",
        record_count,
        chunk_count: stats.chunk_count,
        wall_time_ms: wall_ms,
        records_per_second: rps,
    })))
}


pub(crate) async fn benchmark_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<BenchmarkQueryRequest>,
) -> Result<(StatusCode, Json<BenchmarkQueryResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;

    let op_count = req.op_count.unwrap_or(10_000).min(1_000_000);
    let samples = [
        "SELECT * FROM orders WHERE id = 1",
        "INSERT INTO events VALUES (1, 'start')",
        "UPDATE accounts SET balance = 0 WHERE id = 99",
        "DELETE FROM staging WHERE ts < 1000",
        "BEGIN",
        "COMMIT",
    ];

    let start = std::time::Instant::now();
    for i in 0..op_count {
        let sql = samples[i % samples.len()];
        let _ = voltnuerongrid_sql::SqlAnalyzer::classify_statement(sql);
    }
    let elapsed = start.elapsed();
    let wall_ms = elapsed.as_millis();
    let ops = if wall_ms == 0 { op_count as f64 * 1000.0 } else { op_count as f64 / (wall_ms as f64 / 1000.0) };

    Ok((StatusCode::OK, Json(BenchmarkQueryResponse {
        status: "ok",
        op_count,
        wall_time_ms: wall_ms,
        ops_per_second: ops,
    })))
}


pub(crate) async fn history_object(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ObjectHistoryQuery>,
) -> Result<(StatusCode, Json<ObjectHistoryResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let limit = params.limit.unwrap_or(50).min(500);
    // Stub: runtime will populate entries from WAL/audit log in a future sprint.
    let _ = (params.table, params.schema, params.database, limit);
    Ok((
        StatusCode::OK,
        Json(ObjectHistoryResponse {
            entries: vec![],
            total: 0,
        }),
    ))
}


pub(crate) async fn export_dump(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<DumpExportQuery>,
) -> Result<(StatusCode, Json<DumpExportResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let format = params.format.unwrap_or_else(|| "sql".to_string());
    let _table = params.table.unwrap_or_default();
    let _limit = params.limit.unwrap_or(0);
    Ok((
        StatusCode::OK,
        Json(DumpExportResponse {
            format,
            content: "-- dump placeholder".to_string(),
            rows_exported: 0,
            warning: "streaming export not yet implemented".to_string(),
        }),
    ))
}


pub(crate) async fn import_sql(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ImportSqlRequest>,
) -> Result<(StatusCode, Json<ImportSqlResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;

    // Guard: max 10 MB body (content length check via script byte length).
    const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;
    if req.sql_script.len() > MAX_BODY_BYTES {
        return Ok((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ImportSqlResponse {
                statements_executed: 0,
                errors: vec!["sql_script exceeds 10 MB limit".to_string()],
            }),
        ));
    }

    let dry_run = req.dry_run.unwrap_or(false);
    let _stop_on_error = req.stop_on_error.unwrap_or(true);

    let statements = voltnuerongrid_sql::SqlAnalyzer::parse_batch(&req.sql_script);
    let count = statements.len();

    if dry_run {
        return Ok((
            StatusCode::OK,
            Json(ImportSqlResponse {
                statements_executed: 0,
                errors: vec![],
            }),
        ));
    }

    // Stub: route through existing sql_execute logic in a future sprint.
    Ok((
        StatusCode::OK,
        Json(ImportSqlResponse {
            statements_executed: count,
            errors: vec![],
        }),
    ))
}


pub(crate) async fn search_fulltext(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FulltextSearchRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;

    let enabled = env::var("VNG_FTS_ENABLED")
        .unwrap_or_default()
        .trim()
        .eq_ignore_ascii_case("true");

    if !enabled {
        let body = serde_json::to_value(FulltextNotEnabledError {
            error: "full-text search not enabled".to_string(),
            enable_with: "VNG_FTS_ENABLED=true".to_string(),
        })
        .unwrap_or(json!({"error": "full-text search not enabled"}));
        return Ok((StatusCode::NOT_IMPLEMENTED, Json(body)));
    }

    let _limit = req.limit.unwrap_or(10).min(100);
    let _table = req.table.unwrap_or_default();
    let _query = req.query;

    // Stub: real FTS will be implemented in a future sprint.
    let body = serde_json::to_value(FulltextSearchResponse {
        hits: vec![],
        total: 0,
    })
    .unwrap_or(json!({}));

    Ok((StatusCode::OK, Json(body)))
}


// ─── MCP endpoints ────────────────────────────────────────────────────────────

pub(crate) async fn mcp_capabilities() -> Json<McpServerCapabilities> {
    Json(McpServerCapabilities::default())
}


pub(crate) async fn mcp_invoke(
    Json(req): Json<McpRequest>,
) -> Json<voltnuerongrid_mcp::McpResponse> {
    let capabilities = McpServerCapabilities::default();
    Json(process_request(req, &capabilities).await)
}

