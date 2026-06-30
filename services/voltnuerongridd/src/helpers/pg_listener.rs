//! D-1: PostgreSQL wire-protocol TCP listener.
//!
//! Drives the `pg_wire` codec over a TCP socket: completes the startup
//! handshake, authenticates against the admin API key (cleartext password) or
//! trusts when no key is configured, then services simple `Query` messages by
//! running them through the existing SQL engine (`sql_execute`) and streaming
//! the columns/rows back as `RowDescription` + `DataRow` + `CommandComplete`.
//!
//! Opt-in via `VNG_PGWIRE_ENABLED=true` (default off). Port from
//! `VNG_PGWIRE_PORT` (default 5433) and bind host from `VNG_PGWIRE_BIND`
//! (default `127.0.0.1`). Live `psql`/BI-tool smoke validation is tracked under
//! E-5; the protocol codec itself is unit-tested in `pg_wire`.

use crate::helpers::pg_wire::*;
use crate::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// PostgreSQL wire-protocol listener configuration (from env).
#[derive(Debug, Clone)]
pub(crate) struct PgWireConfig {
    pub(crate) enabled: bool,
    pub(crate) bind: String,
    pub(crate) port: u16,
}

impl PgWireConfig {
    pub(crate) fn from_env() -> Self {
        Self::from_values(
            std::env::var("VNG_PGWIRE_ENABLED").ok(),
            std::env::var("VNG_PGWIRE_PORT").ok(),
            std::env::var("VNG_PGWIRE_BIND").ok(),
        )
    }

    /// Pure config resolution from raw (optional) env values. Extracted so it can
    /// be unit-tested without mutating process-global environment variables.
    pub(crate) fn from_values(
        enabled_var: Option<String>,
        port_var: Option<String>,
        bind_var: Option<String>,
    ) -> Self {
        let enabled = enabled_var
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        let port = port_var
            .and_then(|v| v.trim().parse::<u16>().ok())
            .filter(|p| *p > 0)
            .unwrap_or(5433);
        let bind = bind_var
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        PgWireConfig { enabled, bind, port }
    }

    pub(crate) fn socket_addr(&self) -> String {
        format!("{}:{}", self.bind, self.port)
    }
}

fn pg_log(event: &str, detail: serde_json::Value) {
    tracing::info!(target: "vng.pgwire", event = event, detail = %detail, "pgwire");
}

/// Background task: bind the Postgres-wire listener and service connections.
/// No-op (returns immediately) when disabled.
pub(crate) async fn run_pg_wire_listener(state: AppState) {
    let config = PgWireConfig::from_env();
    if !config.enabled {
        pg_log("disabled", json!({ "hint": "set VNG_PGWIRE_ENABLED=true to enable" }));
        return;
    }
    let addr = config.socket_addr();
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            pg_log("bind_failed", json!({ "addr": addr, "message": e.to_string() }));
            return;
        }
    };
    pg_log("listening", json!({ "addr": addr }));
    loop {
        match listener.accept().await {
            Ok((socket, peer)) => {
                let st = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_pg_connection(socket, st).await {
                        pg_log("connection_error", json!({ "peer": peer.to_string(), "message": e }));
                    }
                });
            }
            Err(e) => {
                pg_log("accept_failed", json!({ "message": e.to_string() }));
                break;
            }
        }
    }
}

/// Read until `decode_startup` yields a non-Incomplete result.
async fn read_startup<S>(socket: &mut S, buf: &mut Vec<u8>) -> Result<StartupDecode, String>
where
    S: AsyncReadExt + Unpin,
{
    loop {
        match decode_startup(buf)? {
            StartupDecode::Incomplete => {
                let mut chunk = [0u8; 1024];
                let n = socket.read(&mut chunk).await.map_err(|e| e.to_string())?;
                if n == 0 {
                    return Err("connection closed during startup".to_string());
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            other => return Ok(other),
        }
    }
}

/// Service one Postgres-wire connection end-to-end.
async fn handle_pg_connection(mut socket: tokio::net::TcpStream, state: AppState) -> Result<(), String> {
    let mut buf: Vec<u8> = Vec::new();

    // 1. Startup handshake (handle a single SSLRequest by declining TLS with 'N').
    let startup = loop {
        match read_startup(&mut socket, &mut buf).await? {
            StartupDecode::SslRequest => {
                // Decline SSL ('N'); the client will resend a plaintext startup.
                socket.write_all(b"N").await.map_err(|e| e.to_string())?;
                // Consume the 8-byte SSLRequest we just decoded.
                buf.drain(0..8);
            }
            StartupDecode::Startup(msg) => break msg,
            StartupDecode::Incomplete => unreachable!("read_startup loops until complete"),
        }
    };
    // Consume the startup packet bytes.
    let startup_len = i32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    buf.drain(0..startup_len);

    // Reject unsupported protocol versions (only the v3.0 frontend is supported).
    if startup.protocol_version != PROTOCOL_VERSION_3 {
        let _ = socket
            .write_all(&encode_error_response(
                "0A000",
                &format!("unsupported protocol version {}", startup.protocol_version),
            ))
            .await;
        return Ok(());
    }

    let user = startup.get("user").unwrap_or("postgres").to_string();
    let database = startup.get("database").map(str::to_string);

    // 2. Authentication. When an admin key is configured, request a cleartext
    // password and validate it equals the admin key; otherwise accept (trust).
    let admin_key = state.auth.admin_api_key.clone();
    if let Some(expected) = admin_key.clone() {
        socket
            .write_all(&encode_authentication_cleartext_password())
            .await
            .map_err(|e| e.to_string())?;
        // Read the PasswordMessage.
        let password = loop {
            if let Some((msg, consumed)) = decode_frontend_message(&buf)? {
                buf.drain(0..consumed);
                match msg {
                    FrontendMessage::Password(p) => break p,
                    FrontendMessage::Terminate => return Ok(()),
                    _ => continue,
                }
            }
            let mut chunk = [0u8; 1024];
            let n = socket.read(&mut chunk).await.map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("connection closed during auth".to_string());
            }
            buf.extend_from_slice(&chunk[..n]);
        };
        if password != expected {
            socket
                .write_all(&encode_error_response("28P01", "password authentication failed"))
                .await
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
    }

    // 3. Auth OK + parameter status + ready.
    let mut hello = Vec::new();
    hello.extend_from_slice(&encode_authentication_ok());
    hello.extend_from_slice(&encode_parameter_status("server_version", "14.0 (VoltNueronGrid)"));
    hello.extend_from_slice(&encode_parameter_status("client_encoding", "UTF8"));
    hello.extend_from_slice(&encode_parameter_status("DateStyle", "ISO, MDY"));
    hello.extend_from_slice(&encode_backend_key_data(std::process::id() as i32, 0));
    hello.extend_from_slice(&encode_ready_for_query(TransactionStatus::Idle));
    socket.write_all(&hello).await.map_err(|e| e.to_string())?;

    pg_log("authenticated", json!({ "user": user, "database": database }));

    // 4. Simple query loop. Track the session's transaction status so each
    // ReadyForQuery reports Idle / InTransaction / Failed per the protocol.
    let mut tx_status = TransactionStatus::Idle;
    loop {
        // Decode any complete frontend messages already buffered.
        if let Some((msg, consumed)) = decode_frontend_message(&buf)? {
            buf.drain(0..consumed);
            match msg {
                FrontendMessage::Terminate => return Ok(()),
                FrontendMessage::Query(sql) => {
                    let (response, next_status) =
                        run_query_and_encode(&state, &sql, database.as_deref(), admin_key.as_deref(), tx_status).await;
                    tx_status = next_status;
                    socket.write_all(&response).await.map_err(|e| e.to_string())?;
                }
                _ => {
                    // Acknowledge unsupported messages with ReadyForQuery to keep the session alive.
                    socket
                        .write_all(&encode_ready_for_query(tx_status))
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
            continue;
        }
        // Need more bytes.
        let mut chunk = [0u8; 4096];
        let n = socket.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(()); // client disconnected
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Compute the next transaction status after a statement given the prior status
/// and whether the statement succeeded.
fn next_tx_status(current: TransactionStatus, sql: &str, ok: bool) -> TransactionStatus {
    let upper = sql.trim_start().to_ascii_uppercase();
    if !ok {
        // An error inside a transaction block puts it into the failed state.
        return if current == TransactionStatus::Idle {
            TransactionStatus::Idle
        } else {
            TransactionStatus::Failed
        };
    }
    if upper.starts_with("BEGIN") || upper.starts_with("START TRANSACTION") {
        TransactionStatus::InTransaction
    } else if upper.starts_with("COMMIT") || upper.starts_with("ROLLBACK") || upper.starts_with("END") {
        TransactionStatus::Idle
    } else {
        current
    }
}

/// Execute one SQL string through the engine and encode the Postgres response
/// bytes (RowDescription + DataRow* + CommandComplete + ReadyForQuery, or an
/// ErrorResponse + ReadyForQuery on failure).
async fn run_query_and_encode(
    state: &AppState,
    sql: &str,
    database: Option<&str>,
    admin_key: Option<&str>,
    tx_status: TransactionStatus,
) -> (Vec<u8>, TransactionStatus) {
    if sql.trim().is_empty() {
        let mut out = encode_empty_query_response();
        out.extend_from_slice(&encode_ready_for_query(tx_status));
        return (out, tx_status);
    }

    // Build internal headers: authenticate as admin so the SQL runtime principal
    // resolves (the pg-wire password was already validated against the admin key).
    // The operator identity defaults to `admin` (a default Dba binding) and can be
    // overridden with VNG_PGWIRE_OPERATOR_ID.
    let mut headers = HeaderMap::new();
    if let Some(key) = admin_key {
        if let Ok(v) = axum::http::HeaderValue::from_str(key) {
            headers.insert("x-vng-admin-key", v);
        }
        let operator_id = std::env::var("VNG_PGWIRE_OPERATOR_ID")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "admin".to_string());
        if let Ok(v) = axum::http::HeaderValue::from_str(&operator_id) {
            headers.insert("x-vng-operator-id", v);
        }
    }
    if let Some(db) = database {
        if let Ok(v) = axum::http::HeaderValue::from_str(db) {
            headers.insert("x-vng-database", v);
        }
    }

    let req = crate::handlers::sql::SqlExecuteRequest {
        sql_batch: sql.to_string(),
        ..Default::default()
    };
    match crate::handlers::sql::sql_execute(State(state.clone()), headers, Json(req)).await {
        Ok((_code, Json(resp))) => {
            let next = next_tx_status(tx_status, sql, true);
            (encode_success(sql, &resp, next), next)
        }
        Err((code, Json(err))) => {
            let next = next_tx_status(tx_status, sql, false);
            let mut out = encode_error_response("42000", &format!("{}: {}", code.as_u16(), err.reason));
            out.extend_from_slice(&encode_ready_for_query(next));
            (out, next)
        }
    }
}

/// Convert a successful `SqlExecuteResponse` into Postgres wire bytes.
fn encode_success(
    sql: &str,
    resp: &crate::handlers::sql::SqlExecuteResponse,
    tx_status: TransactionStatus,
) -> Vec<u8> {
    let mut out = Vec::new();

    // Extract column names + row cells from the response (UI-facing columns/rows).
    let (fields, data_rows) = extract_columns_and_rows(resp);

    if !fields.is_empty() {
        out.extend_from_slice(&encode_row_description(&fields));
        for row in &data_rows {
            out.extend_from_slice(&encode_data_row(row));
        }
    }
    let row_count = data_rows.len();
    out.extend_from_slice(&encode_command_complete(&command_tag(sql, row_count)));
    out.extend_from_slice(&encode_ready_for_query(tx_status));
    out
}

/// Pull `(fields, rows)` from the SQL response's `columns`/`rows` JSON.
fn extract_columns_and_rows(
    resp: &crate::handlers::sql::SqlExecuteResponse,
) -> (Vec<FieldDescription>, Vec<Vec<Option<String>>>) {
    // Column names: each entry may be a string or an object with a "name" field.
    let mut field_names: Vec<String> = Vec::new();
    if let Some(cols) = &resp.columns {
        for c in cols {
            let name = c
                .as_str()
                .map(str::to_string)
                .or_else(|| c.get("name").and_then(|n| n.as_str()).map(str::to_string))
                .unwrap_or_else(|| "?column?".to_string());
            field_names.push(name);
        }
    }

    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    if let Some(json_rows) = &resp.rows {
        for row in json_rows {
            match row {
                serde_json::Value::Array(arr) => {
                    if field_names.is_empty() {
                        field_names = (0..arr.len()).map(|i| format!("column{i}")).collect();
                    }
                    rows.push(arr.iter().map(json_cell_to_text).collect());
                }
                serde_json::Value::Object(map) => {
                    if field_names.is_empty() {
                        field_names = map.keys().cloned().collect();
                    }
                    rows.push(
                        field_names
                            .iter()
                            .map(|k| json_cell_to_text(map.get(k).unwrap_or(&serde_json::Value::Null)))
                            .collect(),
                    );
                }
                other => {
                    if field_names.is_empty() {
                        field_names.push("value".to_string());
                    }
                    rows.push(vec![json_cell_to_text(other)]);
                }
            }
        }
    }

    let fields = field_names.into_iter().map(FieldDescription::text).collect();
    (fields, rows)
}

/// Render a JSON cell as Postgres text, mapping JSON null → SQL NULL.
fn json_cell_to_text(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// Test-only re-export of the query→encode path so integration tests can drive
/// the wire front-end against the real SQL engine without opening a socket.
#[cfg(test)]
pub(crate) async fn run_query_and_encode_for_test(
    state: &AppState,
    sql: &str,
    database: Option<&str>,
    admin_key: Option<&str>,
) -> Vec<u8> {
    run_query_and_encode(state, sql, database, admin_key, TransactionStatus::Idle)
        .await
        .0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_disabled_on_5433() {
        // Pure resolution — no process env mutation, so safe under parallel tests.
        let cfg = PgWireConfig::from_values(None, None, None);
        assert!(!cfg.enabled);
        assert_eq!(cfg.port, 5433);
        assert_eq!(cfg.bind, "127.0.0.1");
        assert_eq!(cfg.socket_addr(), "127.0.0.1:5433");
    }

    #[test]
    fn config_reads_env_overrides() {
        let cfg = PgWireConfig::from_values(
            Some("true".to_string()),
            Some("6000".to_string()),
            Some("0.0.0.0".to_string()),
        );
        assert!(cfg.enabled);
        assert_eq!(cfg.port, 6000);
        assert_eq!(cfg.bind, "0.0.0.0");
        // Invalid/zero port falls back to the default.
        let bad = PgWireConfig::from_values(Some("yes".to_string()), Some("0".to_string()), None);
        assert!(bad.enabled);
        assert_eq!(bad.port, 5433);
    }

    #[test]
    fn json_cell_null_maps_to_sql_null() {
        assert_eq!(json_cell_to_text(&serde_json::Value::Null), None);
        assert_eq!(json_cell_to_text(&json!("hi")), Some("hi".to_string()));
        assert_eq!(json_cell_to_text(&json!(42)), Some("42".to_string()));
    }

    #[test]
    fn tx_status_transitions_follow_begin_commit_and_errors() {
        // BEGIN enters a transaction block; COMMIT/ROLLBACK returns to idle.
        assert_eq!(
            next_tx_status(TransactionStatus::Idle, "BEGIN", true),
            TransactionStatus::InTransaction
        );
        assert_eq!(
            next_tx_status(TransactionStatus::InTransaction, "COMMIT", true),
            TransactionStatus::Idle
        );
        assert_eq!(
            next_tx_status(TransactionStatus::InTransaction, "ROLLBACK", true),
            TransactionStatus::Idle
        );
        // A statement error inside a transaction block marks it failed.
        assert_eq!(
            next_tx_status(TransactionStatus::InTransaction, "SELECT bad", false),
            TransactionStatus::Failed
        );
        // An error outside a transaction stays idle.
        assert_eq!(
            next_tx_status(TransactionStatus::Idle, "SELECT bad", false),
            TransactionStatus::Idle
        );
        // A plain successful statement preserves the current status.
        assert_eq!(
            next_tx_status(TransactionStatus::InTransaction, "INSERT INTO t VALUES (1)", true),
            TransactionStatus::InTransaction
        );
    }
}
