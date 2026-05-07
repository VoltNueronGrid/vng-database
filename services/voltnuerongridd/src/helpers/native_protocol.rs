//! VNG native wire protocol helpers (TLS, frame types, auth).


pub(crate) fn load_native_tls_acceptor(
    cert_path: &str,
    key_path: &str,
    client_ca_path: Option<&str>,
) -> Result<Arc<tokio_rustls::TlsAcceptor>, String> {
    use rustls::RootCertStore;
    use rustls::ServerConfig;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls::server::WebPkiClientVerifier;
    use rustls_pemfile::{certs, private_key};
    use std::fs::File;
    use std::io::BufReader;

    let mut cert_r = BufReader::new(
        File::open(cert_path).map_err(|e| format!("open cert {cert_path}: {e}"))?,
    );
    let cert_chain: Vec<CertificateDer<'static>> = certs(&mut cert_r)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("parse cert PEM: {e}"))?;
    if cert_chain.is_empty() {
        return Err("no certificates found in PEM file".to_string());
    }

    let mut key_r = BufReader::new(
        File::open(key_path).map_err(|e| format!("open key {key_path}: {e}"))?,
    );
    let key: PrivateKeyDer<'static> = private_key(&mut key_r)
        .map_err(|e| format!("parse key PEM: {e}"))?
        .ok_or_else(|| "no private keys in PEM file".to_string())?;

    let cfg = if let Some(ca_path) = client_ca_path {
        let mut ca_r = BufReader::new(
            File::open(ca_path).map_err(|e| format!("open client CA {ca_path}: {e}"))?,
        );
        let ca_certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut ca_r)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("parse client CA PEM: {e}"))?;
        if ca_certs.is_empty() {
            return Err("no certificates in client CA PEM file".to_string());
        }
        let mut root_store = RootCertStore::empty();
        let (added, _ignored) = root_store.add_parsable_certificates(ca_certs);
        if added == 0 {
            return Err("no valid client CA trust anchors parsed".to_string());
        }
        let verifier = WebPkiClientVerifier::builder(Arc::new(root_store))
            .build()
            .map_err(|e| format!("client cert verifier: {e}"))?;
        ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(cert_chain, key)
            .map_err(|e| format!("rustls server config: {e}"))?
    } else {
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain, key)
            .map_err(|e| format!("rustls server config: {e}"))?
    };

    Ok(Arc::new(tokio_rustls::TlsAcceptor::from(Arc::new(cfg))))
}


/// Driver-compatible JSON wire shape (`native-protocol-v1` / Rust driver codec).
pub(crate) fn internal_native_frame_to_driver_wire_json(
    frame: &NativeFrame,
    protocol_version: &str,
) -> serde_json::Value {
    let ft = native_frame_type_wire_name(frame.frame_type);
    json!({
        "frame_type": ft,
        "protocol_version": protocol_version,
        "request_id": frame.request_id,
        "session_id": frame.session_id,
        "payload": frame.payload_json.clone().unwrap_or(json!({})),
    })
}


pub(crate) fn native_frame_type_wire_name(t: NativeFrameType) -> &'static str {
    match t {
        NativeFrameType::Hello => "Hello",
        NativeFrameType::HelloAck => "HelloAck",
        NativeFrameType::Auth => "Auth",
        NativeFrameType::AuthAck => "AuthAck",
        NativeFrameType::Command => "Command",
        NativeFrameType::Result => "Result",
        NativeFrameType::Error => "Error",
        NativeFrameType::Ping => "Ping",
        NativeFrameType::Pong => "Pong",
        NativeFrameType::StreamChunk => "StreamChunk",
        NativeFrameType::StreamEnd => "StreamEnd",
        NativeFrameType::Cancel => "Cancel",
        NativeFrameType::Goodbye => "Goodbye",
    }
}


pub(crate) fn wire_protocol_error_frame(request_id: &str, message: &str) -> serde_json::Value {
    json!({
        "frame_type": "Error",
        "protocol_version": "v1",
        "request_id": request_id,
        "session_id": null,
        "payload": { "kind": "protocol", "message": message }
    })
}


pub(crate) fn strip_command_field(payload: &serde_json::Value) -> serde_json::Value {
    let mut obj = payload
        .as_object()
        .cloned()
        .unwrap_or_else(|| serde_json::Map::new());
    obj.remove("command");
    serde_json::Value::Object(obj)
}


pub(crate) fn wire_json_to_native_dispatch_frame(body: &serde_json::Value) -> Result<NativeFrame, String> {
    let frame_type = body
        .get("frame_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing frame_type".to_string())?;
    if frame_type != "Command" {
        return Err(format!("expected Command frame for dispatch, got {frame_type}"));
    }
    let request_id = body
        .get("request_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let session_id = body
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let payload = body.get("payload").cloned().unwrap_or(json!({}));
    let cmd_str = payload
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing payload.command".to_string())?;
    let command = match cmd_str {
        "health" => NativeCommandKind::Health,
        "sql.analyze" => NativeCommandKind::SqlAnalyze,
        "sql.route" => NativeCommandKind::SqlRoute,
        "sql.execute" => NativeCommandKind::SqlExecute,
        "sql.transaction" => NativeCommandKind::SqlTransaction,
        "ingest.schema.registry" => NativeCommandKind::IngestSchemaRegistry,
        _ => NativeCommandKind::Unknown,
    };
    let payload_json = match command {
        NativeCommandKind::Health | NativeCommandKind::IngestSchemaRegistry => None,
        _ => Some(strip_command_field(&payload)),
    };
    Ok(NativeFrame {
        frame_type: NativeFrameType::Command,
        request_id,
        session_id,
        command: Some(command),
        payload_json,
    })
}


pub(crate) fn native_wire_hello_ack(
    request_id: &str,
    session_from_hello: Option<&serde_json::Value>,
) -> serde_json::Value {
    json!({
        "frame_type": "HelloAck",
        "protocol_version": "v1",
        "request_id": request_id,
        "session_id": session_from_hello.cloned(),
        "payload": {
            "accepted": true,
            "version": "v1",
        }
    })
}


pub(crate) fn native_wire_auth_ack(request_id: &str, session_id: Option<&str>, accepted: bool) -> serde_json::Value {
    json!({
        "frame_type": "AuthAck",
        "protocol_version": "v1",
        "request_id": request_id,
        "session_id": session_id,
        "payload": { "accepted": accepted }
    })
}


/// NT-S6-001: check an Auth frame payload against configured admin_api_key and/or bearer_token.
///
/// Auth succeeds when no credentials are configured (open listener), or when at least one of the
/// supplied fields matches a configured credential.  Both fields are optional in the frame.
pub(crate) fn native_auth_payload_matches_runtime(
    state: &AppState,
    config: &NativeListenerConfig,
    auth_payload: &serde_json::Value,
) -> bool {
    let has_admin_key_cfg = state.admin_api_key.is_some();
    let has_bearer_token_cfg = config.bearer_token.is_some();

    // No credentials configured → open listener, always accept.
    if !has_admin_key_cfg && !has_bearer_token_cfg {
        return true;
    }

    // Check admin_api_key field
    if let Some(expected) = &state.admin_api_key {
        if let Some(key) = auth_payload.get("admin_api_key").and_then(|v| v.as_str()) {
            if key == expected.as_str() {
                return true;
            }
        }
    }

    // NT-S6-001: check bearer_token field
    if let Some(expected_token) = &config.bearer_token {
        if let Some(token) = auth_payload.get("bearer_token").and_then(|v| v.as_str()) {
            if token == expected_token.as_str() {
                return true;
            }
        }
    }

    false
}


/// One JSON object per line on stderr (`component` = `vng_native_listener`) for log aggregation.
pub(crate) fn vng_native_listener_log(event: &str, detail: serde_json::Value) {
    let mut m = serde_json::Map::new();
    m.insert("component".to_string(), json!("vng_native_listener"));
    m.insert("event".to_string(), json!(event));
    if let Some(delta) = detail.as_object() {
        for (k, v) in delta {
            m.insert(k.clone(), v.clone());
        }
    }
    eprintln!("{}", serde_json::Value::Object(m));
}

