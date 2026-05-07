use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use crate::{AppState, AuthErrorResponse, ConnectorPlugin, DriverSession, PoolStatsResponse};
use crate::{DRIVER_SESSION_COUNTER, now_unix_ms_u64, now_epoch_ms_chaos, pool_stats_response, release_sql_data_plane_connection};
use crate::auth::require_operator_auth;

// ─── Driver protocol DTOs ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct DriverProtocolInfo {
    pub(crate) protocol_version: &'static str,
    pub(crate) encoding: &'static str,
    pub(crate) auth_modes: Vec<String>,
    pub(crate) supported_statements: Vec<String>,
    pub(crate) max_batch_size: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DriverConnectRequest {
    pub(crate) driver_name: String,
    pub(crate) driver_version: String,
    pub(crate) requested_capabilities: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DriverConnectResponse {
    pub(crate) status: &'static str,
    pub(crate) session_token: String,
    pub(crate) negotiated_capabilities: Vec<String>,
    pub(crate) max_batch_size: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DriverDisconnectRequest {
    pub(crate) session_token: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DriverDisconnectResponse {
    pub(crate) status: &'static str,
    pub(crate) session_token: String,
    pub(crate) disconnected: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct DriverSessionInfo {
    pub(crate) session_token: String,
    pub(crate) driver_name: String,
    pub(crate) driver_version: String,
    pub(crate) connected_at_ms: u64,
    pub(crate) assigned_node_id: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DriverSessionListResponse {
    pub(crate) status: &'static str,
    pub(crate) session_count: usize,
    pub(crate) sessions: Vec<DriverSessionInfo>,
}

// ─── Connector plugin DTOs ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct ConnectorRegisterRequest {
    pub(crate) connector_id: String,
    pub(crate) connector_type: String,
    pub(crate) version: String,
    pub(crate) signed: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct ConnectorRegisterResponse {
    pub(crate) status: &'static str,
    pub(crate) connector_id: String,
    pub(crate) registered_at_ms: u64,
}

#[derive(Serialize)]
pub(crate) struct ConnectorListResponse {
    pub(crate) status: &'static str,
    pub(crate) connector_count: usize,
    pub(crate) connectors: Vec<ConnectorPlugin>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ConnectorGetQuery {
    pub(crate) id: String,
}

#[derive(Serialize)]
pub(crate) struct ConnectorGetResponse {
    pub(crate) status: &'static str,
    pub(crate) found: bool,
    pub(crate) connector: Option<ConnectorPlugin>,
}

#[derive(Deserialize)]
pub(crate) struct ConnectorDeregisterRequest {
    pub(crate) connector_id: String,
}

#[derive(Serialize)]
pub(crate) struct ConnectorDeregisterResponse {
    pub(crate) status: &'static str,
    pub(crate) connector_id: String,
    pub(crate) removed: bool,
}

#[derive(Deserialize)]
pub(crate) struct ConnectorUpdateRequest {
    pub(crate) connector_id: String,
    pub(crate) version: Option<String>,
    pub(crate) signed: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct ConnectorUpdateResponse {
    pub(crate) status: &'static str,
    pub(crate) connector_id: String,
    pub(crate) updated: bool,
}

// ─── Driver health/query/ping DTOs ────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct DriverHealthResponse {
    pub(crate) status: &'static str,
    pub(crate) active_sessions: usize,
    pub(crate) pool_circuit_breaker: String,
    pub(crate) pool_active_connections: usize,
    pub(crate) pool_total_acquired: u64,
    pub(crate) healthy: bool,
}

#[derive(Deserialize)]
pub(crate) struct DriverQueryRequest {
    pub(crate) session_token: String,
    pub(crate) sql: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DriverQueryResponse {
    pub(crate) status: &'static str,
    pub(crate) session_token: String,
    pub(crate) sql: String,
    pub(crate) rows_returned: usize,
    pub(crate) executed_at_ms: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DriverPingRequest {
    pub(crate) session_token: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DriverPingResponse {
    pub(crate) status: &'static str,
    pub(crate) session_token: String,
    pub(crate) pinged_at_ms: u64,
}

// ─── Driver handlers ──────────────────────────────────────────────────────────

/// S8-WS10-02: Return the current wire protocol capabilities.
pub(crate) async fn driver_protocol_info() -> (StatusCode, Json<DriverProtocolInfo>) {
    (StatusCode::OK, Json(DriverProtocolInfo {
        protocol_version: "1.0",
        encoding: "json",
        auth_modes: vec![
            "admin_key".to_string(),
            "operator_id".to_string(),
            "tenant".to_string(),
        ],
        supported_statements: vec![
            "SELECT".to_string(), "INSERT".to_string(),
            "UPDATE".to_string(), "DELETE".to_string(),
            "BEGIN".to_string(), "COMMIT".to_string(), "ROLLBACK".to_string(),
        ],
        max_batch_size: 500,
    }))
}

/// S8-WS10-02: Negotiate a driver connection session and return a session token.
pub(crate) async fn driver_connect(
    State(state): State<AppState>,
    Json(req): Json<DriverConnectRequest>,
) -> (StatusCode, Json<DriverConnectResponse>) {
    let sid = DRIVER_SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let session_token = format!("drv-sess-{sid}");
    let pooled_connection_id = {
        let now_ms = now_unix_ms_u64();
        state
            .driver_pool
            .lock()
            .expect("driver pool lock")
            .acquire(now_ms)
            .ok()
    };
    let mut sessions = state.driver_sessions.lock().expect("driver_sessions lock");
    sessions.insert(session_token.clone(), DriverSession {
        driver_name: req.driver_name,
        driver_version: req.driver_version,
        connected_at_ms: now_epoch_ms_chaos(),
        assigned_node_id: state.node_id.clone(),
        pooled_connection_id,
    });
    let negotiated: Vec<String> = req.requested_capabilities
        .unwrap_or_default()
        .into_iter()
        .filter(|c| matches!(c.as_str(), "batch_execute" | "streaming" | "prepared_statements"))
        .collect();
    (StatusCode::OK, Json(DriverConnectResponse {
        status: "connected",
        session_token,
        negotiated_capabilities: negotiated,
        max_batch_size: 500,
    }))
}

pub(crate) async fn driver_disconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DriverDisconnectRequest>,
) -> Result<(StatusCode, Json<DriverDisconnectResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut sessions = state.driver_sessions.lock().expect("driver_sessions lock");
    let removed = sessions.remove(&req.session_token);
    drop(sessions);

    if let Some(session) = removed.as_ref() {
        if let Some(connection_id) = session.pooled_connection_id.as_ref() {
            release_sql_data_plane_connection(&state, connection_id);
        }
    }

    let disconnected = removed.is_some();
    Ok((StatusCode::OK, Json(DriverDisconnectResponse {
        status: "ok",
        session_token: req.session_token,
        disconnected,
    })))
}

// ─── Connector handlers ───────────────────────────────────────────────────────

/// Register a connector plugin manifest at runtime.
pub(crate) async fn connector_register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ConnectorRegisterRequest>,
) -> Result<(StatusCode, Json<ConnectorRegisterResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let registered_at_ms = now_epoch_ms_chaos();
    let plugin = ConnectorPlugin {
        connector_id: req.connector_id.clone(),
        connector_type: req.connector_type.clone(),
        version: req.version.clone(),
        signed: req.signed.unwrap_or(false),
        registered_at_ms,
    };
    state.connector_registry.lock().expect("connector_registry lock").push(plugin);
    Ok((
        StatusCode::OK,
        Json(ConnectorRegisterResponse {
            status: "ok",
            connector_id: req.connector_id,
            registered_at_ms,
        }),
    ))
}

/// List all registered connector plugins.
pub(crate) async fn connector_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ConnectorListResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let connectors = state.connector_registry.lock().expect("connector_registry lock").clone();
    let connector_count = connectors.len();
    Ok((StatusCode::OK, Json(ConnectorListResponse { status: "ok", connector_count, connectors })))
}

/// S5-E4A-01: Deregister a connector plugin by ID.
pub(crate) async fn connector_deregister(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ConnectorDeregisterRequest>,
) -> Result<(StatusCode, Json<ConnectorDeregisterResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut registry = state.connector_registry.lock().expect("connector_registry lock");
    let before_len = registry.len();
    registry.retain(|c| c.connector_id != req.connector_id);
    let removed = registry.len() < before_len;
    drop(registry);
    Ok((
        StatusCode::OK,
        Json(ConnectorDeregisterResponse {
            status: "ok",
            connector_id: req.connector_id,
            removed,
        }),
    ))
}

/// S5-E4A-01: Return a single connector from the registry by connector_id.
pub(crate) async fn connector_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ConnectorGetQuery>,
) -> Result<(StatusCode, Json<ConnectorGetResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let registry = state.connector_registry.lock().expect("connector_registry lock");
    let connector = registry.iter().find(|c| c.connector_id == params.id).cloned();
    let found = connector.is_some();
    Ok((StatusCode::OK, Json(ConnectorGetResponse {
        status: "ok",
        found,
        connector,
    })))
}

/// S5-E4A-01: Update the version or signed flag of an existing registered connector.
pub(crate) async fn connector_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ConnectorUpdateRequest>,
) -> Result<(StatusCode, Json<ConnectorUpdateResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut registry = state.connector_registry.lock().expect("connector_registry lock update");
    let entry = registry.iter_mut().find(|c| c.connector_id == req.connector_id);
    let updated = if let Some(plugin) = entry {
        if let Some(v) = req.version {
            plugin.version = v;
        }
        if let Some(s) = req.signed {
            plugin.signed = s;
        }
        true
    } else {
        false
    };
    drop(registry);
    Ok((StatusCode::OK, Json(ConnectorUpdateResponse {
        status: "ok",
        connector_id: req.connector_id,
        updated,
    })))
}

// ─── Driver session handlers ──────────────────────────────────────────────────

/// S8-WS10-02: List all active driver sessions.
pub(crate) async fn driver_session_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<DriverSessionListResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let sessions = state.driver_sessions.lock().expect("driver_sessions lock list");
    let list: Vec<DriverSessionInfo> = sessions
        .iter()
        .map(|(token, sess)| DriverSessionInfo {
            session_token: token.clone(),
            driver_name: sess.driver_name.clone(),
            driver_version: sess.driver_version.clone(),
            connected_at_ms: sess.connected_at_ms,
            assigned_node_id: sess.assigned_node_id.clone(),
        })
        .collect();
    let session_count = list.len();
    drop(sessions);
    Ok((StatusCode::OK, Json(DriverSessionListResponse {
        status: "ok",
        session_count,
        sessions: list,
    })))
}

/// S8-WS10-02: Execute a simple query through a driver session (scaffold).
pub(crate) async fn driver_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DriverQueryRequest>,
) -> Result<(StatusCode, Json<DriverQueryResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let sessions = state.driver_sessions.lock().expect("driver_sessions lock");
    let session_exists = sessions.contains_key(&req.session_token);
    drop(sessions);
    if !session_exists {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse { status: "error", reason: "invalid_session_token".to_string(), locale: "en".to_string(), localized_message: "Invalid or expired session token".to_string() }),
        ));
    }
    let executed_at_ms = now_unix_ms_u64();
    Ok((StatusCode::OK, Json(DriverQueryResponse {
        status: "ok",
        session_token: req.session_token,
        sql: req.sql,
        rows_returned: 0,
        executed_at_ms,
    })))
}

/// S8-WS10-02: Ping/keepalive for an existing driver session.
pub(crate) async fn driver_ping(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DriverPingRequest>,
) -> Result<(StatusCode, Json<DriverPingResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let sessions = state.driver_sessions.lock().expect("driver_sessions lock");
    let session_exists = sessions.contains_key(&req.session_token);
    drop(sessions);
    if !session_exists {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                status: "error",
                reason: "invalid_session_token".to_string(),
                locale: "en".to_string(),
                localized_message: "Invalid or expired session token".to_string(),
            }),
        ));
    }
    let pinged_at_ms = now_unix_ms_u64();
    Ok((StatusCode::OK, Json(DriverPingResponse {
        status: "pong",
        session_token: req.session_token,
        pinged_at_ms,
    })))
}

/// S8-WS10-02: Return driver connection pool statistics.
pub(crate) async fn driver_pool_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<PoolStatsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let now_ms = now_unix_ms_u64();
    let stats = state.driver_pool.lock().expect("driver_pool stats lock").pool_stats(now_ms);
    Ok((StatusCode::OK, Json(pool_stats_response(&stats))))
}

pub(crate) async fn driver_health(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<DriverHealthResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let active_sessions = state.driver_sessions.lock().expect("driver_sessions health lock").len();
    let now_ms = now_unix_ms_u64();
    let pool_stats = state.driver_pool.lock().expect("driver_pool health lock").pool_stats(now_ms);
    let healthy = pool_stats.circuit_breaker_state == "closed" && active_sessions < 1_000;
    Ok((StatusCode::OK, Json(DriverHealthResponse {
        status: "ok",
        active_sessions,
        pool_circuit_breaker: pool_stats.circuit_breaker_state.clone(),
        pool_active_connections: pool_stats.active_connections,
        pool_total_acquired: pool_stats.total_acquired,
        healthy,
    })))
}
