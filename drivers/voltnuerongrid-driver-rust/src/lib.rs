#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-driver-rust";

use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriverErrorKind {
    Validation,
    Transport,
    HttpStatus,
    Serialization,
    Timeout,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverError {
    pub kind: DriverErrorKind,
    pub message: String,
    pub status_code: Option<u16>,
    pub request_id: Option<String>,
}

impl DriverError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: DriverErrorKind::Validation,
            message: message.into(),
            status_code: None,
            request_id: None,
        }
    }

    pub fn serialization(message: impl Into<String>) -> Self {
        Self {
            kind: DriverErrorKind::Serialization,
            message: message.into(),
            status_code: None,
            request_id: None,
        }
    }

    pub fn transport(message: impl Into<String>) -> Self {
        Self {
            kind: DriverErrorKind::Transport,
            message: message.into(),
            status_code: None,
            request_id: None,
        }
    }

    pub fn http_status(
        status_code: u16,
        message: impl Into<String>,
        request_id: Option<String>,
    ) -> Self {
        Self {
            kind: DriverErrorKind::HttpStatus,
            message: message.into(),
            status_code: Some(status_code),
            request_id,
        }
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            kind: DriverErrorKind::Timeout,
            message: message.into(),
            status_code: None,
            request_id: None,
        }
    }

    pub fn cancelled(message: impl Into<String>) -> Self {
        Self {
            kind: DriverErrorKind::Cancelled,
            message: message.into(),
            status_code: None,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for DriverError {}

pub type DriverResult<T> = Result<T, DriverError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverConfig {
    pub base_url: String,
    /// When `base_url` is `vng://...`, set this to the HTTP runtime base (e.g. `http://127.0.0.1:8080`) so REST
    /// request builders and `DriverTransportMode::Auto` can fall back to HTTP.
    pub http_fallback_url: Option<String>,
    pub session_id: String,
    pub tenant_id: Option<String>,
    pub user_id: Option<String>,
    pub admin_api_key: Option<String>,
    pub operator_id: Option<String>,
    pub route_hint: Option<String>,
}

impl DriverConfig {
    pub fn validate(&self) -> DriverResult<()> {
        if self.base_url.trim().is_empty() {
            return Err(DriverError::validation("base_url must not be empty"));
        }
        if self.session_id.trim().is_empty() {
            return Err(DriverError::validation("session_id must not be empty"));
        }
        if self.tenant_id.is_some() != self.user_id.is_some() {
            return Err(DriverError::validation("tenant_id and user_id must be set together"));
        }
        if let Some(ref h) = self.http_fallback_url {
            let h = h.trim();
            if h.is_empty() {
                return Err(DriverError::validation("http_fallback_url must not be empty when set"));
            }
            let hl = h.to_ascii_lowercase();
            if !hl.starts_with("http://") && !hl.starts_with("https://") {
                return Err(DriverError::validation(
                    "http_fallback_url must start with http:// or https://",
                ));
            }
            if !self.base_url.trim().to_ascii_lowercase().starts_with("vng://") {
                return Err(DriverError::validation(
                    "http_fallback_url is only valid when base_url uses the vng:// scheme",
                ));
            }
        }
        Ok(())
    }

    /// Base URL for REST (`/api/...`) request building. When `base_url` is native (`vng://`), uses `http_fallback_url`.
    pub fn http_rest_base_url(&self) -> DriverResult<&str> {
        let base = self.base_url.trim();
        if base.to_ascii_lowercase().starts_with("vng://") {
            self.http_fallback_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    DriverError::validation(
                        "http_fallback_url is required when base_url uses vng:// (REST APIs need an http(s) endpoint)",
                    )
                })
        } else {
            Ok(base)
        }
    }
}

/// Injected availability for native vs HTTP when resolving [`DriverTransportMode::Auto`] (dual-endpoint or conformance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportCapabilities {
    pub native_available: bool,
    pub http_available: bool,
}

/// Result of dual-endpoint / capability-aware auto resolution (NT-S3-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoTransportResolution {
    pub active: DriverTransportMode,
    pub fallback_triggered: bool,
    pub fallback_reason: Option<String>,
    pub notes: Option<String>,
}

/// Resolves `Auto` using [`TransportCapabilities`]. Supports dual-endpoint (`vng` + `http_fallback_url`) with
/// native-first order per `transport-mode-cases.json` defaults.
pub fn resolve_auto_transport(
    config: &DriverConfig,
    caps: TransportCapabilities,
) -> DriverResult<AutoTransportResolution> {
    let dual = config
        .http_fallback_url
        .as_ref()
        .map(|s| !s.trim().is_empty())
        == Some(true);

    if dual {
        if caps.native_available {
            return Ok(AutoTransportResolution {
                active: DriverTransportMode::Native,
                fallback_triggered: false,
                fallback_reason: None,
                notes: Some("auto: dual-endpoint; native available (native-first)".to_string()),
            });
        }
        if caps.http_available {
            return Ok(AutoTransportResolution {
                active: DriverTransportMode::Http,
                fallback_triggered: true,
                fallback_reason: Some("native_unavailable".to_string()),
                notes: Some("auto: dual-endpoint; fell back to http_fallback_url".to_string()),
            });
        }
        return Err(DriverError::transport(
            "no available transport: native and http are unavailable",
        ));
    }

    let base = config.base_url.trim();
    let is_vng = base.to_ascii_lowercase().starts_with("vng://");

    if is_vng {
        if caps.native_available {
            return Ok(AutoTransportResolution {
                active: DriverTransportMode::Native,
                fallback_triggered: false,
                fallback_reason: None,
                notes: Some("auto: single vng URL; native available".to_string()),
            });
        }
        if caps.http_available {
            return Err(DriverError::transport(
                "native unavailable and no http_fallback_url is configured for HTTP fallback",
            ));
        }
        return Err(DriverError::transport(
            "no available transport: native and http are unavailable",
        ));
    }

    if caps.http_available {
        return Ok(AutoTransportResolution {
            active: DriverTransportMode::Http,
            fallback_triggered: false,
            fallback_reason: None,
            notes: Some("auto: single http(s) URL".to_string()),
        });
    }

    Err(DriverError::transport(
        "no available transport: native and http are unavailable",
    ))
}

fn http_origin_host_port_for_probe(url: &str) -> Option<String> {
    let u = url.trim();
    let rest = u
        .strip_prefix("http://")
        .or_else(|| u.strip_prefix("https://"))?;
    let hostport = rest.split('/').next()?.split('?').next()?.trim();
    if hostport.is_empty() {
        return None;
    }
    Some(hostport.to_string())
}

/// Best-effort TCP connect probe for `host:port` (e.g. `127.0.0.1:7542`).
pub fn probe_tcp_connect(host_port: &str, timeout_ms: u64) -> bool {
    let host_port = host_port.trim();
    if host_port.is_empty() || timeout_ms == 0 {
        return false;
    }
    let mut addrs = match host_port.to_socket_addrs() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let Some(addr) = addrs.next() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms)).is_ok()
}

/// Derives [`TransportCapabilities`] from live TCP reachability (native `vng` host:port + HTTP origin).
pub fn infer_transport_capabilities_tcp(
    config: &DriverConfig,
    native_connect_timeout_ms: u64,
    http_connect_timeout_ms: u64,
) -> TransportCapabilities {
    let mut native_available = false;
    let base = config.base_url.trim();
    if base.to_ascii_lowercase().starts_with("vng://") {
        if let Some(hp) = base.strip_prefix("vng://").map(str::trim) {
            if !hp.is_empty() {
                native_available = probe_tcp_connect(hp, native_connect_timeout_ms);
            }
        }
    }

    let mut http_available = false;
    if let Ok(http_base) = config.http_rest_base_url() {
        if let Some(hp) = http_origin_host_port_for_probe(http_base) {
            http_available = probe_tcp_connect(&hp, http_connect_timeout_ms);
        }
    }

    TransportCapabilities {
        native_available,
        http_available,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverRoutingConfigContract {
    pub base_url: String,
    pub session_header_name: String,
    pub tenant_header_name: String,
    pub user_header_name: String,
    pub route_hint_header_name: String,
    pub admin_header_name: String,
    pub operator_header_name: String,
    pub pool_min_connections: u32,
    pub pool_max_connections: u32,
    pub request_timeout_ms: u64,
}

impl DriverRoutingConfigContract {
    pub fn validate(&self) -> DriverResult<()> {
        if self.base_url.trim().is_empty() {
            return Err(DriverError::validation("base_url must not be empty"));
        }
        for header in [
            &self.session_header_name,
            &self.tenant_header_name,
            &self.user_header_name,
            &self.route_hint_header_name,
            &self.admin_header_name,
            &self.operator_header_name,
        ] {
            if header.trim().is_empty() || !header.starts_with("x-") {
                return Err(DriverError::validation("all header names must start with 'x-'"));
            }
        }
        if self.pool_min_connections == 0 {
            return Err(DriverError::validation("pool_min_connections must be >= 1"));
        }
        if self.pool_max_connections < self.pool_min_connections {
            return Err(DriverError::validation("pool_max_connections must be >= pool_min_connections"));
        }
        if self.request_timeout_ms < 100 {
            return Err(DriverError::validation("request_timeout_ms must be >= 100"));
        }
        Ok(())
    }

    pub fn from_json_str(input: &str) -> DriverResult<Self> {
        let contract =
            serde_json::from_str::<Self>(input).map_err(|e| DriverError::serialization(format!("json parse failed: {e}")))?;
        contract.validate()?;
        Ok(contract)
    }

    pub fn from_yaml_str(input: &str) -> DriverResult<Self> {
        let contract =
            serde_yaml::from_str::<Self>(input).map_err(|e| DriverError::serialization(format!("yaml parse failed: {e}")))?;
        contract.validate()?;
        Ok(contract)
    }

    pub fn from_properties_str(input: &str) -> DriverResult<Self> {
        let mut map = BTreeMap::<String, String>::new();
        for line in input.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }

        let parse_u32 = |key: &str| -> DriverResult<u32> {
            map.get(key)
                .ok_or_else(|| DriverError::validation(format!("missing {key}")))?
                .parse::<u32>()
                .map_err(|_| DriverError::validation(format!("{key} must be integer")))
        };
        let parse_u64 = |key: &str| -> DriverResult<u64> {
            map.get(key)
                .ok_or_else(|| DriverError::validation(format!("missing {key}")))?
                .parse::<u64>()
                .map_err(|_| DriverError::validation(format!("{key} must be integer")))
        };

        let contract = Self {
            base_url: map
                .get("driver.baseUrl")
                .ok_or_else(|| DriverError::validation("missing driver.baseUrl"))?
                .to_string(),
            session_header_name: map
                .get("driver.sessionHeaderName")
                .ok_or_else(|| DriverError::validation("missing driver.sessionHeaderName"))?
                .to_string(),
            tenant_header_name: map
                .get("driver.tenantHeaderName")
                .ok_or_else(|| DriverError::validation("missing driver.tenantHeaderName"))?
                .to_string(),
            user_header_name: map
                .get("driver.userHeaderName")
                .ok_or_else(|| DriverError::validation("missing driver.userHeaderName"))?
                .to_string(),
            route_hint_header_name: map
                .get("driver.routeHintHeaderName")
                .ok_or_else(|| DriverError::validation("missing driver.routeHintHeaderName"))?
                .to_string(),
            admin_header_name: map
                .get("driver.adminHeaderName")
                .ok_or_else(|| DriverError::validation("missing driver.adminHeaderName"))?
                .to_string(),
            operator_header_name: map
                .get("driver.operatorHeaderName")
                .ok_or_else(|| DriverError::validation("missing driver.operatorHeaderName"))?
                .to_string(),
            pool_min_connections: parse_u32("driver.pool.minConnections")?,
            pool_max_connections: parse_u32("driver.pool.maxConnections")?,
            request_timeout_ms: parse_u64("driver.requestTimeoutMs")?,
        };
        contract.validate()?;
        Ok(contract)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body_json: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeFrameType {
    Hello,
    HelloAck,
    Auth,
    AuthAck,
    Command,
    Result,
    Error,
    Ping,
    Pong,
    StreamChunk,
    StreamEnd,
    Cancel,
    Goodbye,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeAuthMode {
    Admin,
    Operator,
    Tenant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeFrame {
    pub frame_type: NativeFrameType,
    pub protocol_version: String,
    pub request_id: String,
    pub session_id: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeHandshakeState {
    pub session_id: String,
    pub protocol_version: String,
    pub mode: NativeAuthMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriverTransportMode {
    Http,
    Native,
    Auto,
}

/// Result of resolving [`DriverTransportMode::Auto`] (NT-S3-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportResolution {
    pub active: DriverTransportMode,
    pub used_auto_resolution: bool,
    pub notes: Option<String>,
}

/// Picks native vs HTTP from `base_url` scheme (`vng://` → native). Used by [`DriverTransportMode::Auto`].
pub fn select_transport_from_base_url(base_url: &str) -> DriverTransportMode {
    let b = base_url.trim().to_ascii_lowercase();
    if b.starts_with("vng://") {
        DriverTransportMode::Native
    } else {
        DriverTransportMode::Http
    }
}

pub trait NativeTransport {
    fn send_frame(&self, frame: &NativeFrame) -> DriverResult<NativeFrame>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketNativeTransport {
    endpoint: String,
    connect_timeout_ms: u64,
}

impl SocketNativeTransport {
    pub fn new(endpoint: impl Into<String>, connect_timeout_ms: u64) -> DriverResult<Self> {
        let endpoint = endpoint.into();
        if endpoint.trim().is_empty() {
            return Err(DriverError::validation(
                "native socket endpoint must not be empty",
            ));
        }
        if connect_timeout_ms == 0 {
            return Err(DriverError::validation(
                "native socket connect_timeout_ms must be >= 1",
            ));
        }
        Ok(Self {
            endpoint,
            connect_timeout_ms,
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn connect_timeout_ms(&self) -> u64 {
        self.connect_timeout_ms
    }

    pub(crate) fn map_socket_error(context: &str, err: std::io::Error) -> DriverError {
        match err.kind() {
            ErrorKind::TimedOut => {
                DriverError::timeout(format!("{context}: socket timeout ({err})"))
            }
            ErrorKind::Interrupted => {
                DriverError::cancelled(format!("{context}: socket operation cancelled/interrupted ({err})"))
            }
            ErrorKind::ConnectionRefused => {
                DriverError::transport(format!("{context}: connection refused ({err}) [retryable=true]"))
            }
            ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::BrokenPipe => {
                DriverError::transport(format!("{context}: connection reset/aborted ({err}) [retryable=true]"))
            }
            _ => DriverError::transport(format!("{context}: socket error ({err})")),
        }
    }

    pub(crate) fn write_framed(stream: &mut TcpStream, payload: &[u8]) -> DriverResult<()> {
        let len_u32 = u32::try_from(payload.len())
            .map_err(|_| DriverError::serialization("native frame payload exceeds u32 length prefix"))?;
        stream
            .write_all(&len_u32.to_be_bytes())
            .map_err(|err| Self::map_socket_error("failed to write native frame length", err))?;
        stream
            .write_all(payload)
            .map_err(|err| Self::map_socket_error("failed to write native frame payload", err))?;
        Ok(())
    }

    pub(crate) fn read_framed(stream: &mut TcpStream) -> DriverResult<Vec<u8>> {
        let mut len_buf = [0u8; 4];
        stream
            .read_exact(&mut len_buf)
            .map_err(|err| Self::map_socket_error("failed to read native frame length", err))?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut payload = vec![0u8; len];
        stream
            .read_exact(&mut payload)
            .map_err(|err| Self::map_socket_error("failed to read native frame payload", err))?;
        Ok(payload)
    }
}

impl NativeTransport for SocketNativeTransport {
    fn send_frame(&self, frame: &NativeFrame) -> DriverResult<NativeFrame> {
        let encoded = NativeFrameCodec::encode(frame)?;

        let mut resolved = self
            .endpoint
            .to_socket_addrs()
            .map_err(|err| Self::map_socket_error(
                &format!("failed to resolve native endpoint {}", self.endpoint),
                err,
            ))?;
        let addr = resolved.next().ok_or_else(|| {
            DriverError::transport(format!("no socket addresses resolved for {}", self.endpoint))
        })?;

        let timeout = Duration::from_millis(self.connect_timeout_ms);
        let mut stream = TcpStream::connect_timeout(&addr, timeout)
            .map_err(|err| Self::map_socket_error(
                &format!("failed to connect native endpoint {}", self.endpoint),
                err,
            ))?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|err| Self::map_socket_error("failed to set read timeout", err))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|err| Self::map_socket_error("failed to set write timeout", err))?;

        Self::write_framed(&mut stream, &encoded)?;
        let response_bytes = Self::read_framed(&mut stream)?;
        NativeFrameCodec::decode(&response_bytes)
    }
}

#[derive(Debug)]
pub struct PersistentNativeSession {
    stream: TcpStream,
    pub session_id: String,
    pub protocol_version: String,
    pub mode: NativeAuthMode,
}

impl PersistentNativeSession {
    fn connect_with_handshake(
        driver: &VoltNueronGridDriver,
        transport: &SocketNativeTransport,
        hello_request_id: &str,
        auth_request_id: &str,
    ) -> DriverResult<Self> {
        let mut resolved = transport
            .endpoint
            .to_socket_addrs()
            .map_err(|err| SocketNativeTransport::map_socket_error(
                &format!("failed to resolve native endpoint {}", transport.endpoint),
                err,
            ))?;
        let addr = resolved.next().ok_or_else(|| {
            DriverError::transport(format!("no socket addresses resolved for {}", transport.endpoint))
        })?;

        let timeout = Duration::from_millis(transport.connect_timeout_ms);
        let mut stream = TcpStream::connect_timeout(&addr, timeout)
            .map_err(|err| SocketNativeTransport::map_socket_error(
                &format!("failed to connect native endpoint {}", transport.endpoint),
                err,
            ))?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|err| SocketNativeTransport::map_socket_error("failed to set read timeout", err))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|err| SocketNativeTransport::map_socket_error("failed to set write timeout", err))?;

        let hello = driver.build_native_hello_frame(hello_request_id)?;
        let hello_bytes = NativeFrameCodec::encode(&hello)?;
        SocketNativeTransport::write_framed(&mut stream, &hello_bytes)?;
        let hello_ack_bytes = SocketNativeTransport::read_framed(&mut stream)?;
        let hello_ack = NativeFrameCodec::decode(&hello_ack_bytes)?;

        let auth = driver.build_native_auth_frame(auth_request_id)?;
        let auth_bytes = NativeFrameCodec::encode(&auth)?;
        SocketNativeTransport::write_framed(&mut stream, &auth_bytes)?;
        let auth_ack_bytes = SocketNativeTransport::read_framed(&mut stream)?;
        let auth_ack = NativeFrameCodec::decode(&auth_ack_bytes)?;

        let state = driver.complete_native_handshake(&hello_ack, &auth_ack)?;
        Ok(Self {
            stream,
            session_id: state.session_id,
            protocol_version: state.protocol_version,
            mode: state.mode,
        })
    }

    pub fn send_command_frame(&mut self, frame: &NativeFrame) -> DriverResult<NativeFrame> {
        let encoded = NativeFrameCodec::encode(frame)?;
        SocketNativeTransport::write_framed(&mut self.stream, &encoded)?;
        let response_bytes = SocketNativeTransport::read_framed(&mut self.stream)?;
        NativeFrameCodec::decode(&response_bytes)
    }
}

pub trait NativeFrameResponder {
    fn handle_frame(&self, frame: &NativeFrame) -> DriverResult<NativeFrame>;
}

#[derive(Debug, Clone)]
pub struct DefaultNativeLoopbackResponder;

impl NativeFrameResponder for DefaultNativeLoopbackResponder {
    fn handle_frame(&self, frame: &NativeFrame) -> DriverResult<NativeFrame> {
        if frame.frame_type != NativeFrameType::Command {
            return Err(DriverError::transport(format!(
                "loopback responder expected COMMAND frame, got {:?}",
                frame.frame_type
            )));
        }
        let command = frame
            .payload
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DriverError::serialization("missing command field in payload"))?;
        match command {
            "health" => Ok(NativeFrame {
                frame_type: NativeFrameType::Result,
                protocol_version: frame.protocol_version.clone(),
                request_id: frame.request_id.clone(),
                session_id: frame.session_id.clone(),
                payload: serde_json::json!({
                    "status": "ok",
                    "node_id": "loopback-node-1",
                    "cluster_mode": "single",
                }),
            }),
            _ => Ok(NativeFrame {
                frame_type: NativeFrameType::Error,
                protocol_version: frame.protocol_version.clone(),
                request_id: frame.request_id.clone(),
                session_id: frame.session_id.clone(),
                payload: serde_json::json!({
                    "kind": "protocol",
                    "message": format!("unsupported loopback command: {command}"),
                }),
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoopbackNativeTransport<R: NativeFrameResponder> {
    responder: R,
}

impl<R: NativeFrameResponder> LoopbackNativeTransport<R> {
    pub fn new(responder: R) -> Self {
        Self { responder }
    }
}

impl<R: NativeFrameResponder> NativeTransport for LoopbackNativeTransport<R> {
    fn send_frame(&self, frame: &NativeFrame) -> DriverResult<NativeFrame> {
        // Simulate real transport boundary by enforcing encode/decode on both request and response.
        let encoded_request = NativeFrameCodec::encode(frame)?;
        let decoded_request = NativeFrameCodec::decode(&encoded_request)?;
        let response = self.responder.handle_frame(&decoded_request)?;
        let encoded_response = NativeFrameCodec::encode(&response)?;
        NativeFrameCodec::decode(&encoded_response)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NativeFrameCodec;

impl NativeFrameCodec {
    pub fn encode(frame: &NativeFrame) -> DriverResult<Vec<u8>> {
        serde_json::to_vec(frame)
            .map_err(|err| DriverError::serialization(format!("native frame encode failed: {err}")))
    }

    pub fn decode(bytes: &[u8]) -> DriverResult<NativeFrame> {
        serde_json::from_slice(bytes)
            .map_err(|err| DriverError::serialization(format!("native frame decode failed: {err}")))
    }
}

#[derive(Debug, Clone)]
pub struct VoltNueronGridDriver {
    config: DriverConfig,
}

impl VoltNueronGridDriver {
    pub const NATIVE_PROTOCOL_ID: &'static str = "vng-native";
    pub const NATIVE_PROTOCOL_VERSION: &'static str = "v1";

    pub fn new(config: DriverConfig) -> DriverResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Resolves the effective transport for API calls. `Auto` uses `base_url` scheme (`vng://` → native, else HTTP).
    /// For capability-aware auto (dual-endpoint, conformance), use [`Self::resolve_auto`].
    pub fn resolve_transport_mode(&self, mode: DriverTransportMode) -> TransportResolution {
        match mode {
            DriverTransportMode::Http | DriverTransportMode::Native => TransportResolution {
                active: mode,
                used_auto_resolution: false,
                notes: None,
            },
            DriverTransportMode::Auto => {
                let active = select_transport_from_base_url(&self.config.base_url);
                TransportResolution {
                    active,
                    used_auto_resolution: true,
                    notes: Some(format!(
                        "auto: selected {active:?} from base_url scheme"
                    )),
                }
            }
        }
    }

    /// Capability-aware `Auto` resolution (native-first when `http_fallback_url` is set). See `resolve_auto_transport`.
    pub fn resolve_auto(&self, caps: TransportCapabilities) -> DriverResult<AutoTransportResolution> {
        resolve_auto_transport(&self.config, caps)
    }

    /// TCP probes `vng://` host:port from `base_url` and the HTTP origin from [`DriverConfig::http_rest_base_url`]
    /// to populate [`TransportCapabilities`] for [`Self::resolve_auto`].
    pub fn probe_transport_capabilities_tcp(
        &self,
        native_connect_timeout_ms: u64,
        http_connect_timeout_ms: u64,
    ) -> TransportCapabilities {
        infer_transport_capabilities_tcp(
            &self.config,
            native_connect_timeout_ms,
            http_connect_timeout_ms,
        )
    }

    pub fn build_sql_execute_request(
        &self,
        sql_batch: &str,
        max_rows: Option<usize>,
    ) -> DriverResult<DriverRequest> {
        if sql_batch.trim().is_empty() {
            return Err(DriverError::validation("sql_batch must not be empty"));
        }
        let body = serde_json::json!({
            "sql_batch": sql_batch,
            "max_rows": max_rows,
        });
        self.build_json_post("/api/v1/sql/execute", body)
    }

    pub fn build_sql_analyze_request(&self, sql_batch: &str) -> DriverResult<DriverRequest> {
        if sql_batch.trim().is_empty() {
            return Err(DriverError::validation("sql_batch must not be empty"));
        }
        let body = serde_json::json!({
            "sql_batch": sql_batch,
        });
        self.build_json_post("/api/v1/sql/analyze", body)
    }

    pub fn build_sql_route_request(&self, sql_batch: &str) -> DriverResult<DriverRequest> {
        if sql_batch.trim().is_empty() {
            return Err(DriverError::validation("sql_batch must not be empty"));
        }
        let body = serde_json::json!({
            "sql_batch": sql_batch,
        });
        self.build_json_post("/api/v1/sql/route", body)
    }

    pub fn build_sql_transaction_request(&self, statements: &[&str]) -> DriverResult<DriverRequest> {
        if statements.is_empty() {
            return Err(DriverError::validation("statements must not be empty"));
        }
        let body = serde_json::json!({
            "statements": statements,
        });
        self.build_json_post("/api/v1/sql/transaction", body)
    }

    pub fn build_authorize_action_request(
        &self,
        action: &str,
        scope: Option<&str>,
    ) -> DriverResult<DriverRequest> {
        if action.trim().is_empty() {
            return Err(DriverError::validation("action must not be empty"));
        }
        let body = serde_json::json!({
            "action": action,
            "scope": scope,
        });
        self.build_json_post("/api/v1/autonomous/actions/authorize", body)
    }

    fn build_json_post(&self, path: &str, body: serde_json::Value) -> DriverResult<DriverRequest> {
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("x-vng-session-id".to_string(), self.config.session_id.clone());
        if let Some(tenant_id) = &self.config.tenant_id {
            headers.insert("x-vng-tenant-id".to_string(), tenant_id.clone());
        }
        if let Some(user_id) = &self.config.user_id {
            headers.insert("x-vng-user-id".to_string(), user_id.clone());
        }
        if let Some(route_hint) = &self.config.route_hint {
            headers.insert("x-vng-route-hint".to_string(), route_hint.clone());
        }
        if let Some(admin_key) = &self.config.admin_api_key {
            headers.insert("x-vng-admin-key".to_string(), admin_key.clone());
        }
        if let Some(operator_id) = &self.config.operator_id {
            headers.insert("x-vng-operator-id".to_string(), operator_id.clone());
        }

        let base = self.config.http_rest_base_url()?;
        Ok(DriverRequest {
            method: "POST".to_string(),
            url: format!(
                "{}{}",
                base.trim_end_matches('/'),
                path
            ),
            headers,
            body_json: body.to_string(),
        })
    }

    fn native_auth_mode(&self) -> DriverResult<NativeAuthMode> {
        if self.config.tenant_id.is_some() && self.config.user_id.is_some() {
            return Ok(NativeAuthMode::Tenant);
        }
        if self.config.operator_id.is_some() {
            return Ok(NativeAuthMode::Operator);
        }
        Ok(NativeAuthMode::Admin)
    }

    pub fn build_native_hello_frame(&self, request_id: &str) -> DriverResult<NativeFrame> {
        if request_id.trim().is_empty() {
            return Err(DriverError::validation(
                "request_id must not be empty for native HELLO frame",
            ));
        }
        Ok(NativeFrame {
            frame_type: NativeFrameType::Hello,
            protocol_version: Self::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: request_id.to_string(),
            session_id: None,
            payload: serde_json::json!({
                "protocol": Self::NATIVE_PROTOCOL_ID,
                "version": Self::NATIVE_PROTOCOL_VERSION,
                "capabilities": ["json_payload", "streaming", "cancel"],
                "session_id": self.config.session_id,
            }),
        })
    }

    pub fn build_native_auth_frame(&self, request_id: &str) -> DriverResult<NativeFrame> {
        if request_id.trim().is_empty() {
            return Err(DriverError::validation(
                "request_id must not be empty for native AUTH frame",
            ));
        }
        let mode = self.native_auth_mode()?;
        let admin_api_key = self.config.admin_api_key.clone().ok_or_else(|| {
            DriverError::validation("admin_api_key is required for native auth in admin/operator mode")
        })?;

        let payload = match mode {
            NativeAuthMode::Admin => serde_json::json!({
                "mode": "admin",
                "admin_api_key": admin_api_key,
            }),
            NativeAuthMode::Operator => serde_json::json!({
                "mode": "operator",
                "admin_api_key": admin_api_key,
                "operator_id": self.config.operator_id,
            }),
            NativeAuthMode::Tenant => serde_json::json!({
                "mode": "tenant",
                "admin_api_key": admin_api_key,
                "tenant_id": self.config.tenant_id,
                "user_id": self.config.user_id,
            }),
        };

        Ok(NativeFrame {
            frame_type: NativeFrameType::Auth,
            protocol_version: Self::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: request_id.to_string(),
            session_id: Some(self.config.session_id.clone()),
            payload,
        })
    }

    pub fn complete_native_handshake(
        &self,
        hello_ack: &NativeFrame,
        auth_ack: &NativeFrame,
    ) -> DriverResult<NativeHandshakeState> {
        if hello_ack.frame_type != NativeFrameType::HelloAck {
            return Err(DriverError::transport(format!(
                "expected HELLO_ACK, got {:?}",
                hello_ack.frame_type
            )));
        }
        if auth_ack.frame_type != NativeFrameType::AuthAck {
            return Err(DriverError::transport(format!(
                "expected AUTH_ACK, got {:?}",
                auth_ack.frame_type
            )));
        }
        let accepted = auth_ack
            .payload
            .get("accepted")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !accepted {
            return Err(DriverError::transport(
                "native auth rejected by server AUTH_ACK",
            ));
        }
        let negotiated_version = hello_ack
            .payload
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or(Self::NATIVE_PROTOCOL_VERSION);

        Ok(NativeHandshakeState {
            session_id: self.config.session_id.clone(),
            protocol_version: negotiated_version.to_string(),
            mode: self.native_auth_mode()?,
        })
    }

    pub fn build_native_health_command_frame(&self, request_id: &str) -> DriverResult<NativeFrame> {
        if request_id.trim().is_empty() {
            return Err(DriverError::validation(
                "request_id must not be empty for native health COMMAND frame",
            ));
        }
        Ok(NativeFrame {
            frame_type: NativeFrameType::Command,
            protocol_version: Self::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: request_id.to_string(),
            session_id: Some(self.config.session_id.clone()),
            payload: serde_json::json!({
                "command": "health"
            }),
        })
    }

    pub fn build_native_sql_execute_command_frame(
        &self,
        request_id: &str,
        sql_batch: &str,
        max_rows: Option<usize>,
    ) -> DriverResult<NativeFrame> {
        if request_id.trim().is_empty() {
            return Err(DriverError::validation(
                "request_id must not be empty for native sql.execute COMMAND frame",
            ));
        }
        if sql_batch.trim().is_empty() {
            return Err(DriverError::validation(
                "sql_batch must not be empty for native sql.execute COMMAND frame",
            ));
        }
        Ok(NativeFrame {
            frame_type: NativeFrameType::Command,
            protocol_version: Self::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: request_id.to_string(),
            session_id: Some(self.config.session_id.clone()),
            payload: serde_json::json!({
                "command": "sql.execute",
                "sql_batch": sql_batch,
                "max_rows": max_rows,
            }),
        })
    }

    pub fn build_native_sql_analyze_command_frame(
        &self,
        request_id: &str,
        sql_batch: &str,
    ) -> DriverResult<NativeFrame> {
        if request_id.trim().is_empty() {
            return Err(DriverError::validation(
                "request_id must not be empty for native sql.analyze COMMAND frame",
            ));
        }
        if sql_batch.trim().is_empty() {
            return Err(DriverError::validation(
                "sql_batch must not be empty for native sql.analyze COMMAND frame",
            ));
        }
        Ok(NativeFrame {
            frame_type: NativeFrameType::Command,
            protocol_version: Self::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: request_id.to_string(),
            session_id: Some(self.config.session_id.clone()),
            payload: serde_json::json!({
                "command": "sql.analyze",
                "sql_batch": sql_batch,
            }),
        })
    }

    pub fn build_native_sql_route_command_frame(
        &self,
        request_id: &str,
        sql_batch: &str,
    ) -> DriverResult<NativeFrame> {
        if request_id.trim().is_empty() {
            return Err(DriverError::validation(
                "request_id must not be empty for native sql.route COMMAND frame",
            ));
        }
        if sql_batch.trim().is_empty() {
            return Err(DriverError::validation(
                "sql_batch must not be empty for native sql.route COMMAND frame",
            ));
        }
        Ok(NativeFrame {
            frame_type: NativeFrameType::Command,
            protocol_version: Self::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: request_id.to_string(),
            session_id: Some(self.config.session_id.clone()),
            payload: serde_json::json!({
                "command": "sql.route",
                "sql_batch": sql_batch,
            }),
        })
    }

    fn execute_native_command_roundtrip<T: NativeTransport>(
        &self,
        transport_mode: DriverTransportMode,
        transport: &T,
        command_frame: NativeFrame,
        mode_validation_error: &str,
        expected_result_error_prefix: &str,
    ) -> DriverResult<NativeFrame> {
        if transport_mode != DriverTransportMode::Native {
            return Err(DriverError::validation(mode_validation_error));
        }
        let response = transport.send_frame(&command_frame)?;
        if response.frame_type != NativeFrameType::Result {
            return Err(DriverError::transport(format!(
                "{expected_result_error_prefix}, got {:?}",
                response.frame_type
            )));
        }
        Ok(response)
    }

    pub fn execute_native_health_roundtrip<T: NativeTransport>(
        &self,
        transport_mode: DriverTransportMode,
        transport: &T,
        request_id: &str,
    ) -> DriverResult<NativeFrame> {
        let command = self.build_native_health_command_frame(request_id)?;
        self.execute_native_command_roundtrip(
            transport_mode,
            transport,
            command,
            "native health roundtrip requires explicit transportMode=native opt-in",
            "native health roundtrip expected RESULT frame",
        )
    }

    pub fn execute_native_sql_execute_roundtrip<T: NativeTransport>(
        &self,
        transport_mode: DriverTransportMode,
        transport: &T,
        request_id: &str,
        sql_batch: &str,
        max_rows: Option<usize>,
    ) -> DriverResult<NativeFrame> {
        let command = self.build_native_sql_execute_command_frame(
            request_id,
            sql_batch,
            max_rows,
        )?;
        self.execute_native_command_roundtrip(
            transport_mode,
            transport,
            command,
            "native sql.execute roundtrip requires explicit transportMode=native opt-in",
            "native sql.execute roundtrip expected RESULT frame",
        )
    }

    pub fn execute_native_sql_analyze_roundtrip<T: NativeTransport>(
        &self,
        transport_mode: DriverTransportMode,
        transport: &T,
        request_id: &str,
        sql_batch: &str,
    ) -> DriverResult<NativeFrame> {
        let command = self.build_native_sql_analyze_command_frame(
            request_id,
            sql_batch,
        )?;
        self.execute_native_command_roundtrip(
            transport_mode,
            transport,
            command,
            "native sql.analyze roundtrip requires explicit transportMode=native opt-in",
            "native sql.analyze roundtrip expected RESULT frame",
        )
    }

    pub fn execute_native_sql_route_roundtrip<T: NativeTransport>(
        &self,
        transport_mode: DriverTransportMode,
        transport: &T,
        request_id: &str,
        sql_batch: &str,
    ) -> DriverResult<NativeFrame> {
        let command = self.build_native_sql_route_command_frame(
            request_id,
            sql_batch,
        )?;
        self.execute_native_command_roundtrip(
            transport_mode,
            transport,
            command,
            "native sql.route roundtrip requires explicit transportMode=native opt-in",
            "native sql.route roundtrip expected RESULT frame",
        )
    }

    pub fn derive_native_socket_endpoint(&self) -> DriverResult<String> {
        let url = self.config.base_url.trim();
        let without_scheme = url.strip_prefix("vng://").ok_or_else(|| {
            DriverError::validation(
                "native transport requires base_url with vng:// scheme (e.g. vng://127.0.0.1:7542)",
            )
        })?;
        if without_scheme.trim().is_empty() {
            return Err(DriverError::validation(
                "native transport base_url host:port segment must not be empty",
            ));
        }
        Ok(without_scheme.to_string())
    }

    pub fn build_socket_native_transport(&self, connect_timeout_ms: u64) -> DriverResult<SocketNativeTransport> {
        let endpoint = self.derive_native_socket_endpoint()?;
        SocketNativeTransport::new(endpoint, connect_timeout_ms)
    }

    pub fn execute_native_health_roundtrip_socket(
        &self,
        transport_mode: DriverTransportMode,
        connect_timeout_ms: u64,
        request_id: &str,
    ) -> DriverResult<NativeFrame> {
        let transport = self.build_socket_native_transport(connect_timeout_ms)?;
        self.execute_native_health_roundtrip(transport_mode, &transport, request_id)
    }

    pub fn execute_native_health_roundtrip_with_optional_session(
        &self,
        transport_mode: DriverTransportMode,
        connect_timeout_ms: u64,
        session: Option<&mut PersistentNativeSession>,
        request_id: &str,
    ) -> DriverResult<NativeFrame> {
        if let Some(active_session) = session {
            self.execute_native_health_roundtrip_in_session(active_session, request_id)
        } else {
            self.execute_native_health_roundtrip_socket(
                transport_mode,
                connect_timeout_ms,
                request_id,
            )
        }
    }

    pub fn execute_native_sql_execute_roundtrip_socket(
        &self,
        transport_mode: DriverTransportMode,
        connect_timeout_ms: u64,
        request_id: &str,
        sql_batch: &str,
        max_rows: Option<usize>,
    ) -> DriverResult<NativeFrame> {
        let transport = self.build_socket_native_transport(connect_timeout_ms)?;
        self.execute_native_sql_execute_roundtrip(
            transport_mode,
            &transport,
            request_id,
            sql_batch,
            max_rows,
        )
    }

    pub fn execute_native_sql_execute_roundtrip_with_optional_session(
        &self,
        transport_mode: DriverTransportMode,
        connect_timeout_ms: u64,
        session: Option<&mut PersistentNativeSession>,
        request_id: &str,
        sql_batch: &str,
        max_rows: Option<usize>,
    ) -> DriverResult<NativeFrame> {
        if let Some(active_session) = session {
            self.execute_native_sql_execute_roundtrip_in_session(
                active_session,
                request_id,
                sql_batch,
                max_rows,
            )
        } else {
            self.execute_native_sql_execute_roundtrip_socket(
                transport_mode,
                connect_timeout_ms,
                request_id,
                sql_batch,
                max_rows,
            )
        }
    }

    pub fn execute_native_sql_analyze_roundtrip_socket(
        &self,
        transport_mode: DriverTransportMode,
        connect_timeout_ms: u64,
        request_id: &str,
        sql_batch: &str,
    ) -> DriverResult<NativeFrame> {
        let transport = self.build_socket_native_transport(connect_timeout_ms)?;
        self.execute_native_sql_analyze_roundtrip(
            transport_mode,
            &transport,
            request_id,
            sql_batch,
        )
    }

    pub fn execute_native_sql_analyze_roundtrip_with_optional_session(
        &self,
        transport_mode: DriverTransportMode,
        connect_timeout_ms: u64,
        session: Option<&mut PersistentNativeSession>,
        request_id: &str,
        sql_batch: &str,
    ) -> DriverResult<NativeFrame> {
        if let Some(active_session) = session {
            self.execute_native_sql_analyze_roundtrip_in_session(
                active_session,
                request_id,
                sql_batch,
            )
        } else {
            self.execute_native_sql_analyze_roundtrip_socket(
                transport_mode,
                connect_timeout_ms,
                request_id,
                sql_batch,
            )
        }
    }

    pub fn execute_native_sql_route_roundtrip_socket(
        &self,
        transport_mode: DriverTransportMode,
        connect_timeout_ms: u64,
        request_id: &str,
        sql_batch: &str,
    ) -> DriverResult<NativeFrame> {
        let transport = self.build_socket_native_transport(connect_timeout_ms)?;
        self.execute_native_sql_route_roundtrip(
            transport_mode,
            &transport,
            request_id,
            sql_batch,
        )
    }

    pub fn execute_native_sql_route_roundtrip_with_optional_session(
        &self,
        transport_mode: DriverTransportMode,
        connect_timeout_ms: u64,
        session: Option<&mut PersistentNativeSession>,
        request_id: &str,
        sql_batch: &str,
    ) -> DriverResult<NativeFrame> {
        if let Some(active_session) = session {
            self.execute_native_sql_route_roundtrip_in_session(
                active_session,
                request_id,
                sql_batch,
            )
        } else {
            self.execute_native_sql_route_roundtrip_socket(
                transport_mode,
                connect_timeout_ms,
                request_id,
                sql_batch,
            )
        }
    }

    pub fn open_persistent_native_session(
        &self,
        transport_mode: DriverTransportMode,
        connect_timeout_ms: u64,
        hello_request_id: &str,
        auth_request_id: &str,
    ) -> DriverResult<PersistentNativeSession> {
        if transport_mode != DriverTransportMode::Native {
            return Err(DriverError::validation(
                "persistent native session requires explicit transportMode=native opt-in",
            ));
        }
        let transport = self.build_socket_native_transport(connect_timeout_ms)?;
        PersistentNativeSession::connect_with_handshake(
            self,
            &transport,
            hello_request_id,
            auth_request_id,
        )
    }

    fn execute_native_command_in_session(
        &self,
        session: &mut PersistentNativeSession,
        command_frame: NativeFrame,
        expected_result_error_prefix: &str,
    ) -> DriverResult<NativeFrame> {
        let response = session.send_command_frame(&command_frame)?;
        if response.frame_type != NativeFrameType::Result {
            return Err(DriverError::transport(format!(
                "{expected_result_error_prefix}, got {:?}",
                response.frame_type
            )));
        }
        Ok(response)
    }

    pub fn execute_native_health_roundtrip_in_session(
        &self,
        session: &mut PersistentNativeSession,
        request_id: &str,
    ) -> DriverResult<NativeFrame> {
        let command = self.build_native_health_command_frame(request_id)?;
        self.execute_native_command_in_session(
            session,
            command,
            "native health roundtrip expected RESULT frame",
        )
    }

    pub fn execute_native_sql_execute_roundtrip_in_session(
        &self,
        session: &mut PersistentNativeSession,
        request_id: &str,
        sql_batch: &str,
        max_rows: Option<usize>,
    ) -> DriverResult<NativeFrame> {
        let command = self.build_native_sql_execute_command_frame(
            request_id,
            sql_batch,
            max_rows,
        )?;
        self.execute_native_command_in_session(
            session,
            command,
            "native sql.execute roundtrip expected RESULT frame",
        )
    }

    pub fn execute_native_sql_analyze_roundtrip_in_session(
        &self,
        session: &mut PersistentNativeSession,
        request_id: &str,
        sql_batch: &str,
    ) -> DriverResult<NativeFrame> {
        let command = self.build_native_sql_analyze_command_frame(
            request_id,
            sql_batch,
        )?;
        self.execute_native_command_in_session(
            session,
            command,
            "native sql.analyze roundtrip expected RESULT frame",
        )
    }

    pub fn execute_native_sql_route_roundtrip_in_session(
        &self,
        session: &mut PersistentNativeSession,
        request_id: &str,
        sql_batch: &str,
    ) -> DriverResult<NativeFrame> {
        let command = self.build_native_sql_route_command_frame(
            request_id,
            sql_batch,
        )?;
        self.execute_native_command_in_session(
            session,
            command,
            "native sql.route roundtrip expected RESULT frame",
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolConnectionState {
    Idle,
    Active,
    HealthChecking,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PooledConnection {
    pub connection_id: String,
    pub state: PoolConnectionState,
    pub created_at_ms: u64,
    pub last_used_ms: u64,
    pub use_count: u64,
    pub last_error: Option<String>,
}

impl PooledConnection {
    pub fn new(id: String, now_ms: u64) -> Self {
        Self {
            connection_id: id,
            state: PoolConnectionState::Idle,
            created_at_ms: now_ms,
            last_used_ms: now_ms,
            use_count: 0,
            last_error: None,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.state == PoolConnectionState::Idle
    }

    pub fn is_healthy(&self, now_ms: u64, idle_timeout_ms: u64) -> bool {
        self.state != PoolConnectionState::Failed
            && now_ms.saturating_sub(self.last_used_ms) < idle_timeout_ms
    }

    pub fn mark_active(&mut self, now_ms: u64) {
        self.state = PoolConnectionState::Active;
        self.last_used_ms = now_ms;
        self.use_count = self.use_count.saturating_add(1);
    }

    pub fn mark_idle(&mut self, now_ms: u64) {
        self.state = PoolConnectionState::Idle;
        self.last_used_ms = now_ms;
    }

    pub fn mark_failed(&mut self, error: String) {
        self.state = PoolConnectionState::Failed;
        self.last_error = Some(error);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolCircuitBreakerState {
    Closed,
    Open { opened_at_ms: u64, failure_count: u32 },
    HalfOpen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolBackpressurePolicy {
    pub max_pool_size: usize,
    pub min_pool_size: usize,
    pub acquire_timeout_ms: u64,
    pub idle_timeout_ms: u64,
    pub max_queue_depth: usize,
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_reset_ms: u64,
    pub storm_detection_window_ms: u64,
    pub storm_detection_rps_threshold: u64,
}

impl Default for PoolBackpressurePolicy {
    fn default() -> Self {
        Self {
            max_pool_size: 50,
            min_pool_size: 5,
            acquire_timeout_ms: 5000,
            idle_timeout_ms: 60000,
            max_queue_depth: 200,
            circuit_breaker_threshold: 10,
            circuit_breaker_reset_ms: 30000,
            storm_detection_window_ms: 1000,
            storm_detection_rps_threshold: 500,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StormDetector {
    pub policy: PoolBackpressurePolicy,
    request_timestamps_ms: Vec<u64>,
    pub storm_active: bool,
    pub storm_started_at_ms: Option<u64>,
    pub total_requests: u64,
    pub rejected_requests: u64,
    pub storm_events: u64,
}

impl StormDetector {
    pub fn new(policy: PoolBackpressurePolicy) -> Self {
        Self {
            policy,
            request_timestamps_ms: Vec::new(),
            storm_active: false,
            storm_started_at_ms: None,
            total_requests: 0,
            rejected_requests: 0,
            storm_events: 0,
        }
    }

    pub fn record_request(&mut self, now_ms: u64) -> StormCheckResult {
        self.total_requests = self.total_requests.saturating_add(1);
        self.request_timestamps_ms.push(now_ms);

        let window_ms = self.policy.storm_detection_window_ms.max(1);
        self.request_timestamps_ms
            .retain(|ts| now_ms.saturating_sub(*ts) <= window_ms);

        let window_seconds = (window_ms / 1000).max(1);
        let current_rps = (self.request_timestamps_ms.len() as u64) / window_seconds;
        let threshold = self.policy.storm_detection_rps_threshold;

        if current_rps >= threshold && !self.storm_active {
            self.storm_active = true;
            self.storm_events = self.storm_events.saturating_add(1);
            self.storm_started_at_ms = Some(now_ms);
        } else if current_rps < (threshold / 2) && self.storm_active {
            self.storm_active = false;
            self.storm_started_at_ms = None;
        }

        StormCheckResult {
            storm_active: self.storm_active,
            current_rps,
            threshold_rps: threshold,
        }
    }

    pub fn current_rps(&self, now_ms: u64) -> u64 {
        let window_ms = self.policy.storm_detection_window_ms.max(1);
        let window_seconds = (window_ms / 1000).max(1);
        let requests_in_window = self
            .request_timestamps_ms
            .iter()
            .filter(|ts| now_ms.saturating_sub(**ts) <= window_ms)
            .count() as u64;
        requests_in_window / window_seconds
    }

    pub fn storm_stats(&self) -> StormStats {
        StormStats {
            storm_active: self.storm_active,
            storm_events: self.storm_events,
            total_requests: self.total_requests,
            rejected_requests: self.rejected_requests,
            current_rps: self.current_rps(*self.request_timestamps_ms.last().unwrap_or(&0u64)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StormCheckResult {
    pub storm_active: bool,
    pub current_rps: u64,
    pub threshold_rps: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StormStats {
    pub storm_active: bool,
    pub storm_events: u64,
    pub total_requests: u64,
    pub rejected_requests: u64,
    pub current_rps: u64,
}

#[derive(Debug, Clone)]
pub struct ConnectionPoolManager {
    connections: Vec<PooledConnection>,
    policy: PoolBackpressurePolicy,
    circuit_breaker: PoolCircuitBreakerState,
    storm_detector: StormDetector,
    circuit_failure_count: u32,
    pub connection_counter: u64,
    pub total_acquired: u64,
    pub total_released: u64,
    pub total_rejected: u64,
    pub total_circuit_opens: u64,
}

impl ConnectionPoolManager {
    pub fn new(policy: PoolBackpressurePolicy) -> Self {
        let mut connections = Vec::with_capacity(policy.max_pool_size);
        for i in 0..policy.min_pool_size {
            connections.push(PooledConnection::new(format!("conn-{}", i + 1), 0u64));
        }
        Self {
            connection_counter: policy.min_pool_size as u64,
            connections,
            circuit_breaker: PoolCircuitBreakerState::Closed,
            storm_detector: StormDetector::new(policy.clone()),
            policy,
            circuit_failure_count: 0,
            total_acquired: 0,
            total_released: 0,
            total_rejected: 0,
            total_circuit_opens: 0,
        }
    }

    pub fn with_default_policy() -> Self {
        Self::new(PoolBackpressurePolicy::default())
    }

    pub fn acquire(&mut self, now_ms: u64) -> Result<String, PoolAcquireError> {
        let storm = self.storm_detector.record_request(now_ms);
        let active_count = self
            .connections
            .iter()
            .filter(|c| c.state == PoolConnectionState::Active)
            .count();

        let queue_pressure_limit = self.policy.max_queue_depth.min(self.policy.max_pool_size);
        if storm.storm_active && active_count >= queue_pressure_limit {
            self.total_rejected = self.total_rejected.saturating_add(1);
            self.storm_detector.rejected_requests =
                self.storm_detector.rejected_requests.saturating_add(1);
            return Err(PoolAcquireError::StormRejection {
                current_rps: storm.current_rps,
                threshold: storm.threshold_rps,
            });
        }

        if let PoolCircuitBreakerState::Open {
            opened_at_ms,
            failure_count,
        } = self.circuit_breaker
        {
            let elapsed = now_ms.saturating_sub(opened_at_ms);
            if elapsed < self.policy.circuit_breaker_reset_ms {
                self.total_rejected = self.total_rejected.saturating_add(1);
                return Err(PoolAcquireError::CircuitOpen {
                    failure_count,
                    reset_in_ms: self.policy.circuit_breaker_reset_ms.saturating_sub(elapsed),
                });
            }
            self.circuit_breaker = PoolCircuitBreakerState::HalfOpen;
            self.circuit_failure_count = 0;
        }

        if let Some(connection) = self
            .connections
            .iter_mut()
            .find(|c| c.is_idle() && c.is_healthy(now_ms, self.policy.idle_timeout_ms))
        {
            connection.mark_active(now_ms);
            self.total_acquired = self.total_acquired.saturating_add(1);
            if self.circuit_breaker == PoolCircuitBreakerState::HalfOpen {
                self.circuit_breaker = PoolCircuitBreakerState::Closed;
                self.circuit_failure_count = 0;
            }
            return Ok(connection.connection_id.clone());
        }

        if self.connections.len() < self.policy.max_pool_size {
            self.connection_counter = self.connection_counter.saturating_add(1);
            let conn_id = format!("conn-{}", self.connection_counter);
            let mut connection = PooledConnection::new(conn_id.clone(), now_ms);
            connection.mark_active(now_ms);
            self.connections.push(connection);
            self.total_acquired = self.total_acquired.saturating_add(1);
            if self.circuit_breaker == PoolCircuitBreakerState::HalfOpen {
                self.circuit_breaker = PoolCircuitBreakerState::Closed;
                self.circuit_failure_count = 0;
            }
            return Ok(conn_id);
        }

        self.total_rejected = self.total_rejected.saturating_add(1);
        Err(PoolAcquireError::PoolExhausted {
            active_count,
            max: self.policy.max_pool_size,
        })
    }

    pub fn release(&mut self, connection_id: &str, now_ms: u64) -> bool {
        if let Some(connection) = self
            .connections
            .iter_mut()
            .find(|c| c.connection_id == connection_id)
        {
            connection.mark_idle(now_ms);
            self.total_released = self.total_released.saturating_add(1);
            return true;
        }
        false
    }

    pub fn mark_failed(&mut self, connection_id: &str, error: String, now_ms: u64) {
        if let Some(connection) = self
            .connections
            .iter_mut()
            .find(|c| c.connection_id == connection_id)
        {
            connection.mark_failed(error);
            self.record_circuit_failure(now_ms);
        }
    }

    pub fn record_circuit_failure(&mut self, now_ms: u64) {
        self.circuit_failure_count = self.circuit_failure_count.saturating_add(1);
        if self.circuit_failure_count >= self.policy.circuit_breaker_threshold {
            self.circuit_breaker = PoolCircuitBreakerState::Open {
                opened_at_ms: now_ms,
                failure_count: self.circuit_failure_count,
            };
            self.total_circuit_opens = self.total_circuit_opens.saturating_add(1);
        }
    }

    pub fn check_circuit_recovery(&mut self, now_ms: u64) -> bool {
        if let PoolCircuitBreakerState::Open {
            opened_at_ms,
            failure_count: _,
        } = self.circuit_breaker
        {
            if now_ms.saturating_sub(opened_at_ms) >= self.policy.circuit_breaker_reset_ms {
                self.circuit_breaker = PoolCircuitBreakerState::HalfOpen;
                self.circuit_failure_count = 0;
                return true;
            }
        }
        false
    }

    pub fn prune_unhealthy(&mut self, now_ms: u64) -> usize {
        let original_len = self.connections.len();
        self.connections
            .retain(|c| c.is_healthy(now_ms, self.policy.idle_timeout_ms));
        original_len.saturating_sub(self.connections.len())
    }

    pub fn pool_stats(&self, now_ms: u64) -> PoolStats {
        let total_connections = self.connections.len();
        let idle_connections = self
            .connections
            .iter()
            .filter(|c| c.state == PoolConnectionState::Idle)
            .count();
        let active_connections = self
            .connections
            .iter()
            .filter(|c| c.state == PoolConnectionState::Active)
            .count();
        let failed_connections = self
            .connections
            .iter()
            .filter(|c| c.state == PoolConnectionState::Failed)
            .count();

        PoolStats {
            total_connections,
            idle_connections,
            active_connections,
            failed_connections,
            circuit_breaker_state: self.circuit_breaker_state_str().to_string(),
            storm_active: self.storm_detector.storm_active,
            current_rps: self.storm_detector.current_rps(now_ms),
            total_acquired: self.total_acquired,
            total_released: self.total_released,
            total_rejected: self.total_rejected,
            total_circuit_opens: self.total_circuit_opens,
        }
    }

    pub fn circuit_breaker_state_str(&self) -> &'static str {
        match self.circuit_breaker {
            PoolCircuitBreakerState::Closed => "closed",
            PoolCircuitBreakerState::Open { .. } => "open",
            PoolCircuitBreakerState::HalfOpen => "half_open",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolAcquireError {
    PoolExhausted { active_count: usize, max: usize },
    CircuitOpen { failure_count: u32, reset_in_ms: u64 },
    StormRejection { current_rps: u64, threshold: u64 },
    AcquireTimeout { waited_ms: u64 },
}

impl std::fmt::Display for PoolAcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoolAcquireError::PoolExhausted { active_count, max } => {
                write!(f, "pool exhausted: active={} max={}", active_count, max)
            }
            PoolAcquireError::CircuitOpen {
                failure_count,
                reset_in_ms,
            } => write!(
                f,
                "circuit open: failures={} reset_in_ms={}",
                failure_count, reset_in_ms
            ),
            PoolAcquireError::StormRejection {
                current_rps,
                threshold,
            } => write!(
                f,
                "storm rejection: current_rps={} threshold={}",
                current_rps, threshold
            ),
            PoolAcquireError::AcquireTimeout { waited_ms } => {
                write!(f, "acquire timeout: waited_ms={}", waited_ms)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolStats {
    pub total_connections: usize,
    pub idle_connections: usize,
    pub active_connections: usize,
    pub failed_connections: usize,
    pub circuit_breaker_state: String,
    pub storm_active: bool,
    pub current_rps: u64,
    pub total_acquired: u64,
    pub total_released: u64,
    pub total_rejected: u64,
    pub total_circuit_opens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn config() -> DriverConfig {
        DriverConfig {
            base_url: "http://127.0.0.1:8080".to_string(),
            http_fallback_url: None,
            session_id: "sess-1".to_string(),
            tenant_id: Some("acme".to_string()),
            user_id: Some("analyst-acme".to_string()),
            admin_api_key: Some("secret".to_string()),
            operator_id: Some("operator-a".to_string()),
            route_hint: Some("oltp".to_string()),
        }
    }

    #[test]
    fn builds_sql_execute_request_with_session_headers() {
        let driver = VoltNueronGridDriver::new(config()).expect("driver");
        let request = driver
            .build_sql_execute_request("SELECT 1", Some(100))
            .expect("request");
        assert_eq!(request.method, "POST");
        assert!(request.url.ends_with("/api/v1/sql/execute"));
        assert_eq!(
            request.headers.get("x-vng-session-id").map(String::as_str),
            Some("sess-1")
        );
        assert_eq!(
            request.headers.get("x-vng-tenant-id").map(String::as_str),
            Some("acme")
        );
        assert_eq!(
            request.headers.get("x-vng-user-id").map(String::as_str),
            Some("analyst-acme")
        );
        assert_eq!(
            request.headers.get("x-vng-route-hint").map(String::as_str),
            Some("oltp")
        );
        assert!(request.body_json.contains("SELECT 1"));
    }

    #[test]
    fn builds_authorize_request_with_admin_and_operator_headers() {
        let driver = VoltNueronGridDriver::new(config()).expect("driver");
        let request = driver
            .build_authorize_action_request("schema_change", Some("database"))
            .expect("request");
        assert!(request.url.ends_with("/api/v1/autonomous/actions/authorize"));
        assert_eq!(
            request.headers.get("x-vng-admin-key").map(String::as_str),
            Some("secret")
        );
        assert_eq!(
            request.headers.get("x-vng-operator-id").map(String::as_str),
            Some("operator-a")
        );
        assert!(request.body_json.contains("schema_change"));
    }

    #[test]
    fn builds_sql_route_and_transaction_requests_with_user_headers() {
        let driver = VoltNueronGridDriver::new(config()).expect("driver");
        let route = driver.build_sql_route_request("SELECT 1").expect("route request");
        let tx = driver
            .build_sql_transaction_request(&["BEGIN", "COMMIT"])
            .expect("transaction request");
        assert!(route.url.ends_with("/api/v1/sql/route"));
        assert!(tx.url.ends_with("/api/v1/sql/transaction"));
        assert_eq!(route.headers.get("x-vng-user-id").map(String::as_str), Some("analyst-acme"));
        assert_eq!(tx.headers.get("x-vng-tenant-id").map(String::as_str), Some("acme"));
    }

    #[test]
    fn rejects_partial_tenant_user_config() {
        let err = VoltNueronGridDriver::new(DriverConfig {
            tenant_id: Some("acme".to_string()),
            user_id: None,
            ..config()
        })
        .expect_err("partial tenant user config should fail");
        assert!(err.message.contains("tenant_id and user_id"));
    }

    #[test]
    fn validates_driver_contract_from_json() {
        let json = r#"{
          "base_url":"http://127.0.0.1:8080",
          "session_header_name":"x-vng-session-id",
                    "tenant_header_name":"x-vng-tenant-id",
                    "user_header_name":"x-vng-user-id",
          "route_hint_header_name":"x-vng-route-hint",
          "admin_header_name":"x-vng-admin-key",
          "operator_header_name":"x-vng-operator-id",
          "pool_min_connections":2,
          "pool_max_connections":16,
          "request_timeout_ms":2500
        }"#;
        let contract = DriverRoutingConfigContract::from_json_str(json).expect("valid");
        assert_eq!(contract.pool_max_connections, 16);
    }

    #[test]
    fn validates_driver_contract_from_properties() {
        let properties = r#"
driver.baseUrl=http://127.0.0.1:8080
driver.sessionHeaderName=x-vng-session-id
driver.tenantHeaderName=x-vng-tenant-id
driver.userHeaderName=x-vng-user-id
driver.routeHintHeaderName=x-vng-route-hint
driver.adminHeaderName=x-vng-admin-key
driver.operatorHeaderName=x-vng-operator-id
driver.pool.minConnections=2
driver.pool.maxConnections=16
driver.requestTimeoutMs=2500
"#;
        let contract =
            DriverRoutingConfigContract::from_properties_str(properties).expect("valid");
        assert_eq!(contract.request_timeout_ms, 2500);
    }

    #[test]
    fn validates_driver_contract_from_yaml() {
        let yaml = r#"
base_url: http://127.0.0.1:8080
session_header_name: x-vng-session-id
tenant_header_name: x-vng-tenant-id
user_header_name: x-vng-user-id
route_hint_header_name: x-vng-route-hint
admin_header_name: x-vng-admin-key
operator_header_name: x-vng-operator-id
pool_min_connections: 2
pool_max_connections: 16
request_timeout_ms: 2500
"#;
        let contract = DriverRoutingConfigContract::from_yaml_str(yaml).expect("valid");
        assert_eq!(contract.pool_min_connections, 2);
    }

    #[test]
    fn test_pool_acquire_and_release() {
        let policy = PoolBackpressurePolicy {
            min_pool_size: 1,
            max_pool_size: 2,
            ..PoolBackpressurePolicy::default()
        };
        let mut pool = ConnectionPoolManager::new(policy);
        let conn_id = pool.acquire(0u64).expect("acquire should succeed");
        assert!(pool.release(&conn_id, 100u64));
        let stats = pool.pool_stats(100u64);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn test_pool_exhaustion_error() {
        let policy = PoolBackpressurePolicy {
            min_pool_size: 0,
            max_pool_size: 1,
            ..PoolBackpressurePolicy::default()
        };
        let mut pool = ConnectionPoolManager::new(policy);
        let _ = pool.acquire(0u64).expect("first acquire should pass");
        let err = pool.acquire(1u64).expect_err("pool should be exhausted");
        assert!(matches!(err, PoolAcquireError::PoolExhausted { .. }));
    }

    #[test]
    fn test_pool_circuit_breaker_opens() {
        let policy = PoolBackpressurePolicy {
            min_pool_size: 1,
            max_pool_size: 1,
            circuit_breaker_threshold: 2,
            ..PoolBackpressurePolicy::default()
        };
        let mut pool = ConnectionPoolManager::new(policy);
        let conn_id = pool.acquire(0u64).expect("acquire should succeed");
        pool.mark_failed(&conn_id, "io failure".to_string(), 10u64);
        pool.mark_failed(&conn_id, "io failure".to_string(), 20u64);

        let err = pool.acquire(21u64).expect_err("circuit should be open");
        assert!(matches!(err, PoolAcquireError::CircuitOpen { .. }));
    }

    #[test]
    fn test_pool_circuit_breaker_half_open() {
        let policy = PoolBackpressurePolicy {
            min_pool_size: 1,
            max_pool_size: 1,
            circuit_breaker_threshold: 1,
            circuit_breaker_reset_ms: 1000,
            ..PoolBackpressurePolicy::default()
        };
        let mut pool = ConnectionPoolManager::new(policy);
        let conn_id = pool.acquire(0u64).expect("acquire should succeed");
        pool.mark_failed(&conn_id, "downstream failure".to_string(), 5u64);

        assert!(pool.check_circuit_recovery(1005u64));
        assert_eq!(pool.circuit_breaker_state_str(), "half_open");
    }

    #[test]
    fn test_storm_detector_activates() {
        let policy = PoolBackpressurePolicy {
            storm_detection_window_ms: 1000,
            storm_detection_rps_threshold: 10,
            ..PoolBackpressurePolicy::default()
        };
        let mut detector = StormDetector::new(policy);

        for i in 0u64..10u64 {
            let _ = detector.record_request(i);
        }

        assert!(detector.storm_active);
    }

    #[test]
    fn test_storm_detector_deactivates() {
        let policy = PoolBackpressurePolicy {
            storm_detection_window_ms: 1000,
            storm_detection_rps_threshold: 10,
            ..PoolBackpressurePolicy::default()
        };
        let mut detector = StormDetector::new(policy);

        for i in 0u64..10u64 {
            let _ = detector.record_request(i);
        }
        assert!(detector.storm_active);

        let _ = detector.record_request(3000u64);
        assert!(!detector.storm_active);
    }

    #[test]
    fn test_pool_prune_unhealthy() {
        let policy = PoolBackpressurePolicy {
            min_pool_size: 1,
            max_pool_size: 1,
            ..PoolBackpressurePolicy::default()
        };
        let mut pool = ConnectionPoolManager::new(policy);
        let conn_id = pool.acquire(0u64).expect("acquire should succeed");
        pool.mark_failed(&conn_id, "failed health check".to_string(), 10u64);

        let pruned = pool.prune_unhealthy(11u64);
        assert_eq!(pruned, 1);
        assert_eq!(pool.pool_stats(11u64).total_connections, 0);
    }

    #[test]
    fn driver_error_helpers_map_contract_kinds() {
        let transport = DriverError::transport("socket closed");
        assert_eq!(transport.kind, DriverErrorKind::Transport);
        assert_eq!(transport.status_code, None);

        let http = DriverError::http_status(503, "service unavailable", Some("req-503".to_string()));
        assert_eq!(http.kind, DriverErrorKind::HttpStatus);
        assert_eq!(http.status_code, Some(503));
        assert_eq!(http.request_id.as_deref(), Some("req-503"));

        let timeout = DriverError::timeout("deadline exceeded");
        assert_eq!(timeout.kind, DriverErrorKind::Timeout);

        let cancelled = DriverError::cancelled("request cancelled");
        assert_eq!(cancelled.kind, DriverErrorKind::Cancelled);
    }

    #[derive(Debug, Deserialize)]
    struct ConformanceConfigValidationFixtures {
        cases: Vec<ConformanceValidationCase>,
    }

    #[derive(Debug, Deserialize)]
    struct ConformanceValidationCase {
        name: String,
        #[serde(rename = "config")]
        config: ConformanceConfigInput,
        #[serde(rename = "expectError")]
        expect_error: String,
    }

    #[derive(Debug, Deserialize)]
    struct ConformanceConfigInput {
        #[serde(rename = "baseUrl")]
        base_url: String,
        #[serde(rename = "sessionId")]
        session_id: String,
        mode: String,
        #[serde(rename = "adminApiKey")]
        admin_api_key: Option<String>,
        #[serde(rename = "operatorId")]
        operator_id: Option<String>,
        #[serde(rename = "tenantId")]
        tenant_id: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct ConformanceRequestFixture {
        #[serde(rename = "operatorExecuteCase")]
        operator_execute_case: OperatorExecuteCase,
    }

    #[derive(Debug, Deserialize)]
    struct OperatorExecuteCase {
        config: OperatorCaseConfig,
        #[serde(rename = "sqlBatch")]
        sql_batch: String,
        #[serde(rename = "maxRows")]
        max_rows: usize,
        expect: OperatorCaseExpect,
    }

    #[derive(Debug, Deserialize)]
    struct OperatorCaseConfig {
        #[serde(rename = "baseUrl")]
        base_url: String,
        #[serde(rename = "sessionId")]
        session_id: String,
        mode: String,
        #[serde(rename = "adminApiKey")]
        admin_api_key: String,
        #[serde(rename = "operatorId")]
        operator_id: String,
    }

    #[derive(Debug, Deserialize)]
    struct OperatorCaseExpect {
        method: String,
        url: String,
        headers: BTreeMap<String, String>,
    }

    #[derive(Debug, Deserialize)]
    struct TransportModeFixture {
        defaults: TransportModeDefaults,
        cases: Vec<TransportModeCase>,
    }

    #[derive(Debug, Deserialize)]
    struct TransportModeDefaults {
        #[serde(rename = "fallbackPolicy")]
        fallback_policy: String,
        #[serde(rename = "transportAutoOrder")]
        transport_auto_order: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct TransportModeCase {
        id: String,
        #[serde(rename = "transportMode")]
        transport_mode: String,
        operation: String,
        config: TransportModeConfig,
        #[serde(rename = "runtimeCapabilities")]
        runtime_capabilities: Option<RuntimeCapabilities>,
        expect: Option<TransportModeExpect>,
        #[serde(rename = "expectError")]
        expect_error: Option<TransportModeExpectError>,
    }

    #[derive(Debug, Deserialize)]
    struct TransportModeConfig {
        #[serde(rename = "baseUrl")]
        base_url: String,
        #[serde(rename = "sessionId")]
        session_id: String,
        mode: String,
        #[serde(rename = "adminApiKey")]
        admin_api_key: Option<String>,
        #[serde(rename = "operatorId")]
        operator_id: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct RuntimeCapabilities {
        #[serde(rename = "nativeAvailable")]
        native_available: bool,
        #[serde(rename = "httpAvailable")]
        http_available: bool,
    }

    #[derive(Debug, Deserialize)]
    struct TransportModeExpect {
        #[serde(rename = "activeTransport")]
        active_transport: String,
        #[serde(rename = "fallbackTriggered")]
        fallback_triggered: bool,
    }

    #[derive(Debug, Deserialize)]
    struct TransportModeExpectError {
        kind: String,
        message: String,
    }

    #[test]
    fn conformance_config_validation_fixture_is_enforced() {
        let fixture_raw =
            include_str!("../../conformance/fixtures/config-validation-cases.json");
        let fixture: ConformanceConfigValidationFixtures =
            serde_json::from_str(fixture_raw).expect("valid conformance config fixture");

        for case in fixture.cases {
            let tenant_id = case.config.tenant_id.clone();
            let config = DriverConfig {
                base_url: case.config.base_url,
                session_id: case.config.session_id,
                tenant_id: tenant_id.clone(),
                // Keep Rust contract symmetry for tenant/user pairing check.
                user_id: tenant_id.as_ref().map(|_| "fixture-user".to_string()),
                admin_api_key: case.config.admin_api_key,
                operator_id: case.config.operator_id,
                http_fallback_url: None,
                route_hint: None,
            };

            let validation_error = if case.config.mode == "admin" && config.admin_api_key.is_none() {
                DriverError::validation("admin mode requires adminApiKey")
            } else if case.config.mode == "operator"
                && (config.admin_api_key.is_none() || config.operator_id.is_none())
            {
                DriverError::validation("operator mode requires adminApiKey and operatorId")
            } else if case.config.mode == "tenant" && config.tenant_id.is_none() {
                DriverError::validation("tenant mode requires tenantId")
            } else {
                config
                    .validate()
                    .expect("fixture should fail via explicit mode checks before config.validate");
                DriverError::validation("unexpected")
            };

            assert_eq!(
                validation_error.message, case.expect_error,
                "fixture case failed: {}",
                case.name
            );
        }
    }

    #[test]
    fn conformance_request_building_fixture_is_enforced() {
        let fixture_raw =
            include_str!("../../conformance/fixtures/request-building-cases.json");
        let fixture: ConformanceRequestFixture =
            serde_json::from_str(fixture_raw).expect("valid conformance request fixture");

        let use_case = fixture.operator_execute_case;
        assert_eq!(use_case.config.mode, "operator");

        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: use_case.config.base_url,
            session_id: use_case.config.session_id,
            tenant_id: None,
            user_id: None,
            admin_api_key: Some(use_case.config.admin_api_key),
            operator_id: Some(use_case.config.operator_id),
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let request = driver
            .build_sql_execute_request(&use_case.sql_batch, Some(use_case.max_rows))
            .expect("request");

        assert_eq!(request.method, use_case.expect.method);
        assert_eq!(request.url, use_case.expect.url);
        for (header_key, header_value) in use_case.expect.headers {
            assert_eq!(
                request.headers.get(&header_key).map(String::as_str),
                Some(header_value.as_str()),
                "missing/mismatched expected header: {header_key}"
            );
        }
    }

    #[test]
    fn conformance_transport_mode_fixture_is_enforced() {
        let fixture_raw =
            include_str!("../../conformance/fixtures/transport-mode-cases.json");
        let fixture: TransportModeFixture =
            serde_json::from_str(fixture_raw).expect("valid transport mode fixture");

        assert_eq!(fixture.defaults.fallback_policy, "native_primary_http_fallback");
        assert_eq!(fixture.defaults.transport_auto_order, vec!["native", "http"]);
        assert!(fixture.cases.len() >= 5);

        let http_case = fixture
            .cases
            .iter()
            .find(|entry| entry.id == "tm-http-execute-operator")
            .expect("http transport case exists");
        assert_eq!(http_case.transport_mode, "http");
        assert_eq!(http_case.operation, "sql.execute");
        assert_eq!(
            http_case.expect.as_ref().map(|e| e.active_transport.as_str()),
            Some("http")
        );

        let http_driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: http_case.config.base_url.clone(),
            session_id: http_case.config.session_id.clone(),
            tenant_id: None,
            user_id: None,
            admin_api_key: http_case.config.admin_api_key.clone(),
            operator_id: http_case.config.operator_id.clone(),
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("http case driver");
        let http_request = http_driver
            .build_sql_execute_request("SELECT 1;", Some(100))
            .expect("http execute request");
        assert_eq!(http_request.method, "POST");
        assert!(http_request.url.contains("/api/v1/sql/execute"));

        let auto_fallback_case = fixture
            .cases
            .iter()
            .find(|entry| entry.id == "tm-auto-fallback-http")
            .expect("auto fallback case exists");
        assert_eq!(auto_fallback_case.transport_mode, "auto");
        assert_eq!(
            auto_fallback_case
                .runtime_capabilities
                .as_ref()
                .map(|caps| caps.native_available),
            Some(false)
        );
        assert_eq!(
            auto_fallback_case
                .runtime_capabilities
                .as_ref()
                .map(|caps| caps.http_available),
            Some(true)
        );
        assert_eq!(
            auto_fallback_case
                .expect
                .as_ref()
                .map(|expect| expect.active_transport.as_str()),
            Some("http")
        );
        assert_eq!(
            auto_fallback_case
                .expect
                .as_ref()
                .map(|expect| expect.fallback_triggered),
            Some(true)
        );

        let no_transport_case = fixture
            .cases
            .iter()
            .find(|entry| entry.id == "tm-auto-no-transports")
            .expect("no transport case exists");
        assert_eq!(
            no_transport_case
                .expect_error
                .as_ref()
                .map(|err| err.kind.as_str()),
            Some("transport")
        );
        assert_eq!(
            no_transport_case
                .expect_error
                .as_ref()
                .map(|err| err.message.as_str()),
            Some("no available transport: native and http are unavailable")
        );
    }

    #[test]
    fn native_frame_codec_roundtrip_preserves_core_fields() {
        let frame = NativeFrame {
            frame_type: NativeFrameType::Hello,
            protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: "native-hello-1".to_string(),
            session_id: None,
            payload: serde_json::json!({
                "protocol": VoltNueronGridDriver::NATIVE_PROTOCOL_ID,
                "version": VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION
            }),
        };

        let encoded = NativeFrameCodec::encode(&frame).expect("encode");
        let decoded = NativeFrameCodec::decode(&encoded).expect("decode");
        assert_eq!(decoded.frame_type, NativeFrameType::Hello);
        assert_eq!(decoded.request_id, "native-hello-1");
        assert_eq!(
            decoded.payload.get("protocol").and_then(|v| v.as_str()),
            Some(VoltNueronGridDriver::NATIVE_PROTOCOL_ID)
        );
    }

    #[test]
    fn native_handshake_scaffold_builds_hello_and_auth_frames() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "sess-native-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: Some("ops-1".to_string()),
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let hello = driver
            .build_native_hello_frame("hello-1")
            .expect("hello frame");
        assert_eq!(hello.frame_type, NativeFrameType::Hello);
        assert_eq!(
            hello.payload.get("protocol").and_then(|v| v.as_str()),
            Some(VoltNueronGridDriver::NATIVE_PROTOCOL_ID)
        );

        let auth = driver
            .build_native_auth_frame("auth-1")
            .expect("auth frame");
        assert_eq!(auth.frame_type, NativeFrameType::Auth);
        assert_eq!(
            auth.payload.get("mode").and_then(|v| v.as_str()),
            Some("operator")
        );
        assert_eq!(
            auth.payload.get("operator_id").and_then(|v| v.as_str()),
            Some("ops-1")
        );
    }

    #[test]
    fn native_handshake_scaffold_accepts_hello_ack_and_auth_ack() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "sess-native-2".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let hello_ack = NativeFrame {
            frame_type: NativeFrameType::HelloAck,
            protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: "hello-ack-1".to_string(),
            session_id: Some("sess-native-2".to_string()),
            payload: serde_json::json!({
                "accepted": true,
                "version": VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION
            }),
        };
        let auth_ack = NativeFrame {
            frame_type: NativeFrameType::AuthAck,
            protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: "auth-ack-1".to_string(),
            session_id: Some("sess-native-2".to_string()),
            payload: serde_json::json!({
                "accepted": true
            }),
        };

        let state = driver
            .complete_native_handshake(&hello_ack, &auth_ack)
            .expect("handshake state");
        assert_eq!(state.session_id, "sess-native-2");
        assert_eq!(
            state.protocol_version,
            VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION
        );
        assert_eq!(state.mode, NativeAuthMode::Admin);
    }

    #[test]
    fn native_handshake_scaffold_rejects_invalid_ack_types() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "sess-native-3".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let bad_hello_ack = NativeFrame {
            frame_type: NativeFrameType::Result,
            protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: "bad-hello".to_string(),
            session_id: Some("sess-native-3".to_string()),
            payload: serde_json::json!({}),
        };
        let auth_ack = NativeFrame {
            frame_type: NativeFrameType::AuthAck,
            protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: "auth-ack-2".to_string(),
            session_id: Some("sess-native-3".to_string()),
            payload: serde_json::json!({
                "accepted": true
            }),
        };

        let err = driver
            .complete_native_handshake(&bad_hello_ack, &auth_ack)
            .expect_err("invalid hello ack type should fail");
        assert_eq!(err.kind, DriverErrorKind::Transport);
        assert!(err.message.contains("expected HELLO_ACK"));
    }

    #[derive(Debug, Clone)]
    struct MockNativeHealthTransport {
        force_error_frame: bool,
    }

    impl NativeTransport for MockNativeHealthTransport {
        fn send_frame(&self, frame: &NativeFrame) -> DriverResult<NativeFrame> {
            if frame.frame_type != NativeFrameType::Command {
                return Err(DriverError::transport("expected COMMAND frame input"));
            }
            if self.force_error_frame {
                return Ok(NativeFrame {
                    frame_type: NativeFrameType::Error,
                    protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                    request_id: frame.request_id.clone(),
                    session_id: frame.session_id.clone(),
                    payload: serde_json::json!({
                        "kind": "transport",
                        "message": "simulated native failure"
                    }),
                });
            }
            Ok(NativeFrame {
                frame_type: NativeFrameType::Result,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: frame.request_id.clone(),
                session_id: frame.session_id.clone(),
                payload: serde_json::json!({
                    "status": "ok",
                    "node_id": "mock-node-1",
                    "cluster_mode": "single"
                }),
            })
        }
    }

    #[test]
    fn native_health_roundtrip_mock_transport_returns_result_frame() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "sess-native-health-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");
        let transport = MockNativeHealthTransport {
            force_error_frame: false,
        };

        let result = driver
            .execute_native_health_roundtrip(
                DriverTransportMode::Native,
                &transport,
                "native-health-roundtrip-1",
            )
            .expect("native health roundtrip should succeed");
        assert_eq!(result.frame_type, NativeFrameType::Result);
        assert_eq!(
            result.payload.get("status").and_then(|v| v.as_str()),
            Some("ok")
        );
    }

    #[test]
    fn native_health_roundtrip_requires_explicit_native_opt_in() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "http://127.0.0.1:8080".to_string(),
            session_id: "sess-http-health-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");
        let transport = MockNativeHealthTransport {
            force_error_frame: false,
        };

        let err = driver
            .execute_native_health_roundtrip(
                DriverTransportMode::Http,
                &transport,
                "native-health-roundtrip-http-1",
            )
            .expect_err("non-native mode should be rejected");
        assert_eq!(err.kind, DriverErrorKind::Validation);
        assert!(err.message.contains("transportMode=native"));
    }

    #[test]
    fn native_health_roundtrip_rejects_non_result_frame_from_transport() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "sess-native-health-2".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");
        let transport = MockNativeHealthTransport {
            force_error_frame: true,
        };

        let err = driver
            .execute_native_health_roundtrip(
                DriverTransportMode::Native,
                &transport,
                "native-health-roundtrip-err-1",
            )
            .expect_err("non-result frame should be rejected");
        assert_eq!(err.kind, DriverErrorKind::Transport);
        assert!(err.message.contains("expected RESULT frame"));
    }

    #[derive(Debug, Clone)]
    struct AlwaysFailResponder;

    impl NativeFrameResponder for AlwaysFailResponder {
        fn handle_frame(&self, _frame: &NativeFrame) -> DriverResult<NativeFrame> {
            Err(DriverError::transport(
                "synthetic loopback transport failure from responder",
            ))
        }
    }

    #[test]
    fn loopback_native_transport_default_responder_health_roundtrip() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "sess-loopback-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let transport = LoopbackNativeTransport::new(DefaultNativeLoopbackResponder);
        let result = driver
            .execute_native_health_roundtrip(
                DriverTransportMode::Native,
                &transport,
                "native-loopback-health-1",
            )
            .expect("loopback health roundtrip should succeed");

        assert_eq!(result.frame_type, NativeFrameType::Result);
        assert_eq!(
            result.payload.get("status").and_then(|v| v.as_str()),
            Some("ok")
        );
        assert_eq!(
            result.payload.get("node_id").and_then(|v| v.as_str()),
            Some("loopback-node-1")
        );
    }

    #[test]
    fn loopback_native_transport_propagates_responder_failures() {
        let transport = LoopbackNativeTransport::new(AlwaysFailResponder);
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: "native-loopback-fail-1".to_string(),
            session_id: Some("sess-loopback-fail-1".to_string()),
            payload: serde_json::json!({
                "command": "health"
            }),
        };

        let err = transport
            .send_frame(&frame)
            .expect_err("responder failure should bubble up");
        assert_eq!(err.kind, DriverErrorKind::Transport);
        assert!(err.message.contains("synthetic loopback transport failure"));
    }

    #[test]
    fn socket_native_transport_builder_parses_vng_endpoint() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "sess-socket-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let endpoint = driver
            .derive_native_socket_endpoint()
            .expect("endpoint");
        assert_eq!(endpoint, "127.0.0.1:7542");

        let transport = driver
            .build_socket_native_transport(5000)
            .expect("socket transport");
        assert_eq!(transport.endpoint(), "127.0.0.1:7542");
        assert_eq!(transport.connect_timeout_ms(), 5000);
    }

    #[test]
    fn socket_native_transport_builder_rejects_non_native_url() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "http://127.0.0.1:8080".to_string(),
            session_id: "sess-socket-2".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let err = driver
            .derive_native_socket_endpoint()
            .expect_err("http URL should be rejected for native socket endpoint");
        assert_eq!(err.kind, DriverErrorKind::Validation);
        assert!(err.message.contains("vng://"));
    }

    #[test]
    fn socket_native_transport_send_frame_returns_typed_stub_error() {
        let transport = SocketNativeTransport::new("bad-endpoint", 5000).expect("transport");
        let frame = NativeFrame {
            frame_type: NativeFrameType::Command,
            protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: "native-socket-stub-1".to_string(),
            session_id: Some("sess-socket-stub-1".to_string()),
            payload: serde_json::json!({
                "command": "health"
            }),
        };

        let err = transport
            .send_frame(&frame)
            .expect_err("invalid endpoint should return typed transport error");
        assert_eq!(err.kind, DriverErrorKind::Transport);
        assert!(err.message.contains("failed to resolve native endpoint"));
    }

    #[test]
    fn socket_native_transport_roundtrip_with_local_tcp_server() {
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback test listener");
        let addr = listener.local_addr().expect("listener addr");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept client");

            let mut len_buf = [0u8; 4];
            socket.read_exact(&mut len_buf).expect("read request length");
            let req_len = u32::from_be_bytes(len_buf) as usize;
            let mut req_payload = vec![0u8; req_len];
            socket
                .read_exact(&mut req_payload)
                .expect("read request payload");
            let request = NativeFrameCodec::decode(&req_payload).expect("decode request");
            assert_eq!(request.frame_type, NativeFrameType::Command);
            assert_eq!(
                request.payload.get("command").and_then(|v| v.as_str()),
                Some("health")
            );

            let response = NativeFrame {
                frame_type: NativeFrameType::Result,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: request.request_id,
                session_id: request.session_id,
                payload: serde_json::json!({
                    "status": "ok",
                    "node_id": "tcp-loopback-node",
                    "cluster_mode": "single"
                }),
            };
            let encoded = NativeFrameCodec::encode(&response).expect("encode response");
            socket
                .write_all(&(encoded.len() as u32).to_be_bytes())
                .expect("write response length");
            socket
                .write_all(&encoded)
                .expect("write response payload");
        });

        let transport = SocketNativeTransport::new(addr.to_string(), 5000).expect("transport");
        let request = NativeFrame {
            frame_type: NativeFrameType::Command,
            protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
            request_id: "native-socket-live-1".to_string(),
            session_id: Some("sess-socket-live-1".to_string()),
            payload: serde_json::json!({
                "command": "health"
            }),
        };

        let response = transport
            .send_frame(&request)
            .expect("socket roundtrip should succeed");
        assert_eq!(response.frame_type, NativeFrameType::Result);
        assert_eq!(
            response.payload.get("status").and_then(|v| v.as_str()),
            Some("ok")
        );
        assert_eq!(
            response.payload.get("node_id").and_then(|v| v.as_str()),
            Some("tcp-loopback-node")
        );

        server.join().expect("server thread join");
    }

    #[test]
    fn native_health_roundtrip_socket_requires_explicit_native_opt_in() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "sess-socket-optin-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let err = driver
            .execute_native_health_roundtrip_socket(
                DriverTransportMode::Http,
                5000,
                "native-socket-optin-http-1",
            )
            .expect_err("socket helper should enforce native opt-in");
        assert_eq!(err.kind, DriverErrorKind::Validation);
        assert!(err.message.contains("transportMode=native"));
    }

    #[test]
    fn socket_native_transport_error_mapping_timeout_refused_reset() {
        let timeout_err = SocketNativeTransport::map_socket_error(
            "connect",
            std::io::Error::new(ErrorKind::TimedOut, "timed out"),
        );
        assert_eq!(timeout_err.kind, DriverErrorKind::Timeout);
        assert!(timeout_err.message.contains("timeout"));

        let refused_err = SocketNativeTransport::map_socket_error(
            "connect",
            std::io::Error::new(ErrorKind::ConnectionRefused, "refused"),
        );
        assert_eq!(refused_err.kind, DriverErrorKind::Transport);
        assert!(refused_err.message.contains("connection refused"));
        assert!(refused_err.message.contains("retryable=true"));

        let reset_err = SocketNativeTransport::map_socket_error(
            "read",
            std::io::Error::new(ErrorKind::ConnectionReset, "reset"),
        );
        assert_eq!(reset_err.kind, DriverErrorKind::Transport);
        assert!(reset_err.message.contains("connection reset/aborted"));
        assert!(reset_err.message.contains("retryable=true"));
    }

    #[test]
    fn socket_native_transport_error_mapping_interrupted_is_cancelled() {
        let cancelled = SocketNativeTransport::map_socket_error(
            "write",
            std::io::Error::new(ErrorKind::Interrupted, "interrupted"),
        );
        assert_eq!(cancelled.kind, DriverErrorKind::Cancelled);
        assert!(cancelled.message.contains("cancelled/interrupted"));
    }

    #[test]
    fn native_sql_execute_roundtrip_socket_with_local_tcp_server() {
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback sql.execute listener");
        let addr = listener.local_addr().expect("listener addr");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept client");

            let mut len_buf = [0u8; 4];
            socket.read_exact(&mut len_buf).expect("read request length");
            let req_len = u32::from_be_bytes(len_buf) as usize;
            let mut req_payload = vec![0u8; req_len];
            socket
                .read_exact(&mut req_payload)
                .expect("read request payload");
            let request = NativeFrameCodec::decode(&req_payload).expect("decode request");
            assert_eq!(request.frame_type, NativeFrameType::Command);
            assert_eq!(
                request.payload.get("command").and_then(|v| v.as_str()),
                Some("sql.execute")
            );
            assert_eq!(
                request.payload.get("sql_batch").and_then(|v| v.as_str()),
                Some("SELECT 1;")
            );
            assert_eq!(
                request.payload.get("max_rows").and_then(|v| v.as_u64()),
                Some(10)
            );

            let response = NativeFrame {
                frame_type: NativeFrameType::Result,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: request.request_id,
                session_id: request.session_id,
                payload: serde_json::json!({
                    "status": "ok",
                    "rows": [{"value": 1}],
                    "row_count": 1
                }),
            };
            let encoded = NativeFrameCodec::encode(&response).expect("encode response");
            socket
                .write_all(&(encoded.len() as u32).to_be_bytes())
                .expect("write response length");
            socket
                .write_all(&encoded)
                .expect("write response payload");
        });

        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: format!("vng://{}", addr),
            session_id: "sess-socket-execute-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: Some("ops-1".to_string()),
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let response = driver
            .execute_native_sql_execute_roundtrip_socket(
                DriverTransportMode::Native,
                5000,
                "native-socket-execute-1",
                "SELECT 1;",
                Some(10),
            )
            .expect("native sql.execute socket roundtrip should succeed");
        assert_eq!(response.frame_type, NativeFrameType::Result);
        assert_eq!(
            response.payload.get("status").and_then(|v| v.as_str()),
            Some("ok")
        );
        assert_eq!(
            response.payload.get("row_count").and_then(|v| v.as_u64()),
            Some(1)
        );

        server.join().expect("server thread join");
    }

    #[test]
    fn native_sql_execute_roundtrip_requires_explicit_native_opt_in() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "sess-socket-execute-optin-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: Some("ops-1".to_string()),
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let err = driver
            .execute_native_sql_execute_roundtrip_socket(
                DriverTransportMode::Http,
                5000,
                "native-socket-execute-http-1",
                "SELECT 1;",
                Some(10),
            )
            .expect_err("non-native mode should be rejected");
        assert_eq!(err.kind, DriverErrorKind::Validation);
        assert!(err.message.contains("transportMode=native"));
    }

    #[test]
    fn native_sql_analyze_roundtrip_socket_with_local_tcp_server() {
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback sql.analyze listener");
        let addr = listener.local_addr().expect("listener addr");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept client");

            let mut len_buf = [0u8; 4];
            socket.read_exact(&mut len_buf).expect("read request length");
            let req_len = u32::from_be_bytes(len_buf) as usize;
            let mut req_payload = vec![0u8; req_len];
            socket
                .read_exact(&mut req_payload)
                .expect("read request payload");
            let request = NativeFrameCodec::decode(&req_payload).expect("decode request");
            assert_eq!(request.frame_type, NativeFrameType::Command);
            assert_eq!(
                request.payload.get("command").and_then(|v| v.as_str()),
                Some("sql.analyze")
            );
            assert_eq!(
                request.payload.get("sql_batch").and_then(|v| v.as_str()),
                Some("SELECT 1;")
            );

            let response = NativeFrame {
                frame_type: NativeFrameType::Result,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: request.request_id,
                session_id: request.session_id,
                payload: serde_json::json!({
                    "status": "ok",
                    "total_statements": 1,
                    "rejected_statements": 0
                }),
            };
            let encoded = NativeFrameCodec::encode(&response).expect("encode response");
            socket
                .write_all(&(encoded.len() as u32).to_be_bytes())
                .expect("write response length");
            socket
                .write_all(&encoded)
                .expect("write response payload");
        });

        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: format!("vng://{}", addr),
            session_id: "sess-socket-analyze-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: Some("ops-1".to_string()),
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let response = driver
            .execute_native_sql_analyze_roundtrip_socket(
                DriverTransportMode::Native,
                5000,
                "native-socket-analyze-1",
                "SELECT 1;",
            )
            .expect("native sql.analyze socket roundtrip should succeed");
        assert_eq!(response.frame_type, NativeFrameType::Result);
        assert_eq!(
            response.payload.get("status").and_then(|v| v.as_str()),
            Some("ok")
        );
        assert_eq!(
            response
                .payload
                .get("total_statements")
                .and_then(|v| v.as_u64()),
            Some(1)
        );

        server.join().expect("server thread join");
    }

    #[test]
    fn native_sql_analyze_roundtrip_requires_explicit_native_opt_in() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "sess-socket-analyze-optin-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: Some("ops-1".to_string()),
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let err = driver
            .execute_native_sql_analyze_roundtrip_socket(
                DriverTransportMode::Http,
                5000,
                "native-socket-analyze-http-1",
                "SELECT 1;",
            )
            .expect_err("non-native mode should be rejected");
        assert_eq!(err.kind, DriverErrorKind::Validation);
        assert!(err.message.contains("transportMode=native"));
    }

    #[test]
    fn native_sql_route_roundtrip_socket_with_local_tcp_server() {
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback sql.route listener");
        let addr = listener.local_addr().expect("listener addr");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept client");

            let mut len_buf = [0u8; 4];
            socket.read_exact(&mut len_buf).expect("read request length");
            let req_len = u32::from_be_bytes(len_buf) as usize;
            let mut req_payload = vec![0u8; req_len];
            socket
                .read_exact(&mut req_payload)
                .expect("read request payload");
            let request = NativeFrameCodec::decode(&req_payload).expect("decode request");
            assert_eq!(request.frame_type, NativeFrameType::Command);
            assert_eq!(
                request.payload.get("command").and_then(|v| v.as_str()),
                Some("sql.route")
            );
            assert_eq!(
                request.payload.get("sql_batch").and_then(|v| v.as_str()),
                Some("SELECT * FROM orders;")
            );

            let response = NativeFrame {
                frame_type: NativeFrameType::Result,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: request.request_id,
                session_id: request.session_id,
                payload: serde_json::json!({
                    "status": "ok",
                    "route_path": "olap",
                    "reason": "analytical query"
                }),
            };
            let encoded = NativeFrameCodec::encode(&response).expect("encode response");
            socket
                .write_all(&(encoded.len() as u32).to_be_bytes())
                .expect("write response length");
            socket
                .write_all(&encoded)
                .expect("write response payload");
        });

        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: format!("vng://{}", addr),
            session_id: "sess-socket-route-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: Some("ops-1".to_string()),
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let response = driver
            .execute_native_sql_route_roundtrip_socket(
                DriverTransportMode::Native,
                5000,
                "native-socket-route-1",
                "SELECT * FROM orders;",
            )
            .expect("native sql.route socket roundtrip should succeed");
        assert_eq!(response.frame_type, NativeFrameType::Result);
        assert_eq!(
            response.payload.get("status").and_then(|v| v.as_str()),
            Some("ok")
        );
        assert_eq!(
            response.payload.get("route_path").and_then(|v| v.as_str()),
            Some("olap")
        );

        server.join().expect("server thread join");
    }

    #[test]
    fn native_sql_route_roundtrip_requires_explicit_native_opt_in() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "sess-socket-route-optin-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: Some("ops-1".to_string()),
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let err = driver
            .execute_native_sql_route_roundtrip_socket(
                DriverTransportMode::Http,
                5000,
                "native-socket-route-http-1",
                "SELECT * FROM orders;",
            )
            .expect_err("non-native mode should be rejected");
        assert_eq!(err.kind, DriverErrorKind::Validation);
        assert!(err.message.contains("transportMode=native"));
    }

    #[test]
    fn persistent_native_session_handshake_and_multi_command_reuse_single_connection() {
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind persistent session listener");
        let addr = listener.local_addr().expect("listener addr");

        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept client");

            // HELLO
            let mut len_buf = [0u8; 4];
            socket.read_exact(&mut len_buf).expect("read hello len");
            let hello_len = u32::from_be_bytes(len_buf) as usize;
            let mut hello_payload = vec![0u8; hello_len];
            socket.read_exact(&mut hello_payload).expect("read hello payload");
            let hello = NativeFrameCodec::decode(&hello_payload).expect("decode hello");
            assert_eq!(hello.frame_type, NativeFrameType::Hello);

            let hello_ack = NativeFrame {
                frame_type: NativeFrameType::HelloAck,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: hello.request_id,
                session_id: hello.payload.get("session_id").and_then(|v| v.as_str()).map(str::to_string),
                payload: serde_json::json!({
                    "accepted": true,
                    "version": VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION
                }),
            };
            let hello_ack_bytes = NativeFrameCodec::encode(&hello_ack).expect("encode hello ack");
            socket
                .write_all(&(hello_ack_bytes.len() as u32).to_be_bytes())
                .expect("write hello ack len");
            socket
                .write_all(&hello_ack_bytes)
                .expect("write hello ack payload");

            // AUTH
            socket.read_exact(&mut len_buf).expect("read auth len");
            let auth_len = u32::from_be_bytes(len_buf) as usize;
            let mut auth_payload = vec![0u8; auth_len];
            socket.read_exact(&mut auth_payload).expect("read auth payload");
            let auth = NativeFrameCodec::decode(&auth_payload).expect("decode auth");
            assert_eq!(auth.frame_type, NativeFrameType::Auth);

            let auth_ack = NativeFrame {
                frame_type: NativeFrameType::AuthAck,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: auth.request_id,
                session_id: auth.session_id.clone(),
                payload: serde_json::json!({
                    "accepted": true
                }),
            };
            let auth_ack_bytes = NativeFrameCodec::encode(&auth_ack).expect("encode auth ack");
            socket
                .write_all(&(auth_ack_bytes.len() as u32).to_be_bytes())
                .expect("write auth ack len");
            socket
                .write_all(&auth_ack_bytes)
                .expect("write auth ack payload");

            // COMMAND #1 health
            socket.read_exact(&mut len_buf).expect("read health len");
            let health_len = u32::from_be_bytes(len_buf) as usize;
            let mut health_payload = vec![0u8; health_len];
            socket
                .read_exact(&mut health_payload)
                .expect("read health payload");
            let health = NativeFrameCodec::decode(&health_payload).expect("decode health cmd");
            assert_eq!(
                health.payload.get("command").and_then(|v| v.as_str()),
                Some("health")
            );
            let health_res = NativeFrame {
                frame_type: NativeFrameType::Result,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: health.request_id,
                session_id: health.session_id.clone(),
                payload: serde_json::json!({
                    "status": "ok",
                    "node_id": "persistent-node"
                }),
            };
            let health_res_bytes = NativeFrameCodec::encode(&health_res).expect("encode health result");
            socket
                .write_all(&(health_res_bytes.len() as u32).to_be_bytes())
                .expect("write health result len");
            socket
                .write_all(&health_res_bytes)
                .expect("write health result payload");

            // COMMAND #2 sql.execute
            socket.read_exact(&mut len_buf).expect("read execute len");
            let execute_len = u32::from_be_bytes(len_buf) as usize;
            let mut execute_payload = vec![0u8; execute_len];
            socket
                .read_exact(&mut execute_payload)
                .expect("read execute payload");
            let execute = NativeFrameCodec::decode(&execute_payload).expect("decode execute cmd");
            assert_eq!(
                execute.payload.get("command").and_then(|v| v.as_str()),
                Some("sql.execute")
            );
            let execute_res = NativeFrame {
                frame_type: NativeFrameType::Result,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: execute.request_id,
                session_id: execute.session_id.clone(),
                payload: serde_json::json!({
                    "status": "ok",
                    "row_count": 1
                }),
            };
            let execute_res_bytes =
                NativeFrameCodec::encode(&execute_res).expect("encode execute result");
            socket
                .write_all(&(execute_res_bytes.len() as u32).to_be_bytes())
                .expect("write execute result len");
            socket
                .write_all(&execute_res_bytes)
                .expect("write execute result payload");
        });

        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: format!("vng://{}", addr),
            session_id: "sess-persistent-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: Some("ops-1".to_string()),
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let mut session = driver
            .open_persistent_native_session(
                DriverTransportMode::Native,
                5000,
                "hello-persistent-1",
                "auth-persistent-1",
            )
            .expect("open persistent session");
        assert_eq!(session.session_id, "sess-persistent-1");
        assert_eq!(
            session.protocol_version,
            VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION
        );

        let health = driver
            .execute_native_health_roundtrip_in_session(
                &mut session,
                "health-persistent-1",
            )
            .expect("health roundtrip in session");
        assert_eq!(health.frame_type, NativeFrameType::Result);
        assert_eq!(
            health.payload.get("status").and_then(|v| v.as_str()),
            Some("ok")
        );

        let execute = driver
            .execute_native_sql_execute_roundtrip_in_session(
                &mut session,
                "execute-persistent-1",
                "SELECT 1;",
                Some(10),
            )
            .expect("execute roundtrip in session");
        assert_eq!(execute.frame_type, NativeFrameType::Result);
        assert_eq!(
            execute.payload.get("row_count").and_then(|v| v.as_u64()),
            Some(1)
        );

        server.join().expect("server thread join");
    }

    #[test]
    fn persistent_native_session_requires_explicit_native_opt_in() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "sess-persistent-optin-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let err = driver
            .open_persistent_native_session(
                DriverTransportMode::Http,
                5000,
                "hello-persistent-http-1",
                "auth-persistent-http-1",
            )
            .expect_err("non-native mode should be rejected");
        assert_eq!(err.kind, DriverErrorKind::Validation);
        assert!(err.message.contains("transportMode=native"));
    }

    #[test]
    fn optional_session_helpers_fallback_to_socket_when_session_not_provided() {
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind optional-session fallback listener");
        let addr = listener.local_addr().expect("listener addr");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept client");
            let mut len_buf = [0u8; 4];
            socket.read_exact(&mut len_buf).expect("read request len");
            let req_len = u32::from_be_bytes(len_buf) as usize;
            let mut req_payload = vec![0u8; req_len];
            socket.read_exact(&mut req_payload).expect("read request payload");
            let request = NativeFrameCodec::decode(&req_payload).expect("decode request");
            assert_eq!(
                request.payload.get("command").and_then(|v| v.as_str()),
                Some("health")
            );
            let response = NativeFrame {
                frame_type: NativeFrameType::Result,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: request.request_id,
                session_id: request.session_id,
                payload: serde_json::json!({ "status": "ok" }),
            };
            let encoded = NativeFrameCodec::encode(&response).expect("encode response");
            socket
                .write_all(&(encoded.len() as u32).to_be_bytes())
                .expect("write len");
            socket.write_all(&encoded).expect("write payload");
        });

        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: format!("vng://{}", addr),
            session_id: "sess-optional-fallback-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let response = driver
            .execute_native_health_roundtrip_with_optional_session(
                DriverTransportMode::Native,
                5000,
                None,
                "optional-fallback-health-1",
            )
            .expect("health via socket fallback");
        assert_eq!(response.frame_type, NativeFrameType::Result);
        assert_eq!(
            response.payload.get("status").and_then(|v| v.as_str()),
            Some("ok")
        );

        server.join().expect("server join");
    }

    #[test]
    fn optional_session_helpers_reuse_provided_persistent_session() {
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind optional-session reuse listener");
        let addr = listener.local_addr().expect("listener addr");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept client");

            let mut len_buf = [0u8; 4];

            // HELLO
            socket.read_exact(&mut len_buf).expect("read hello len");
            let hello_len = u32::from_be_bytes(len_buf) as usize;
            let mut hello_payload = vec![0u8; hello_len];
            socket.read_exact(&mut hello_payload).expect("read hello payload");
            let hello = NativeFrameCodec::decode(&hello_payload).expect("decode hello");
            let hello_ack = NativeFrame {
                frame_type: NativeFrameType::HelloAck,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: hello.request_id,
                session_id: hello.payload.get("session_id").and_then(|v| v.as_str()).map(str::to_string),
                payload: serde_json::json!({ "accepted": true, "version": VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION }),
            };
            let hello_ack_bytes = NativeFrameCodec::encode(&hello_ack).expect("encode hello ack");
            socket.write_all(&(hello_ack_bytes.len() as u32).to_be_bytes()).expect("write hello ack len");
            socket.write_all(&hello_ack_bytes).expect("write hello ack payload");

            // AUTH
            socket.read_exact(&mut len_buf).expect("read auth len");
            let auth_len = u32::from_be_bytes(len_buf) as usize;
            let mut auth_payload = vec![0u8; auth_len];
            socket.read_exact(&mut auth_payload).expect("read auth payload");
            let auth = NativeFrameCodec::decode(&auth_payload).expect("decode auth");
            let auth_ack = NativeFrame {
                frame_type: NativeFrameType::AuthAck,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: auth.request_id,
                session_id: auth.session_id.clone(),
                payload: serde_json::json!({ "accepted": true }),
            };
            let auth_ack_bytes = NativeFrameCodec::encode(&auth_ack).expect("encode auth ack");
            socket.write_all(&(auth_ack_bytes.len() as u32).to_be_bytes()).expect("write auth ack len");
            socket.write_all(&auth_ack_bytes).expect("write auth ack payload");

            // COMMAND #1 sql.analyze
            socket.read_exact(&mut len_buf).expect("read analyze len");
            let analyze_len = u32::from_be_bytes(len_buf) as usize;
            let mut analyze_payload = vec![0u8; analyze_len];
            socket.read_exact(&mut analyze_payload).expect("read analyze payload");
            let analyze = NativeFrameCodec::decode(&analyze_payload).expect("decode analyze");
            assert_eq!(analyze.payload.get("command").and_then(|v| v.as_str()), Some("sql.analyze"));
            let analyze_res = NativeFrame {
                frame_type: NativeFrameType::Result,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: analyze.request_id,
                session_id: analyze.session_id.clone(),
                payload: serde_json::json!({ "status": "ok", "total_statements": 1 }),
            };
            let analyze_res_bytes = NativeFrameCodec::encode(&analyze_res).expect("encode analyze res");
            socket.write_all(&(analyze_res_bytes.len() as u32).to_be_bytes()).expect("write analyze len");
            socket.write_all(&analyze_res_bytes).expect("write analyze payload");

            // COMMAND #2 sql.route
            socket.read_exact(&mut len_buf).expect("read route len");
            let route_len = u32::from_be_bytes(len_buf) as usize;
            let mut route_payload = vec![0u8; route_len];
            socket.read_exact(&mut route_payload).expect("read route payload");
            let route = NativeFrameCodec::decode(&route_payload).expect("decode route");
            assert_eq!(route.payload.get("command").and_then(|v| v.as_str()), Some("sql.route"));
            let route_res = NativeFrame {
                frame_type: NativeFrameType::Result,
                protocol_version: VoltNueronGridDriver::NATIVE_PROTOCOL_VERSION.to_string(),
                request_id: route.request_id,
                session_id: route.session_id.clone(),
                payload: serde_json::json!({ "status": "ok", "route_path": "olap" }),
            };
            let route_res_bytes = NativeFrameCodec::encode(&route_res).expect("encode route res");
            socket.write_all(&(route_res_bytes.len() as u32).to_be_bytes()).expect("write route len");
            socket.write_all(&route_res_bytes).expect("write route payload");
        });

        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: format!("vng://{}", addr),
            session_id: "sess-optional-reuse-1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: Some("ops-1".to_string()),
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");

        let mut session = driver
            .open_persistent_native_session(
                DriverTransportMode::Native,
                5000,
                "hello-optional-reuse-1",
                "auth-optional-reuse-1",
            )
            .expect("open session");

        let analyze = driver
            .execute_native_sql_analyze_roundtrip_with_optional_session(
                DriverTransportMode::Native,
                5000,
                Some(&mut session),
                "analyze-optional-reuse-1",
                "SELECT 1;",
            )
            .expect("analyze via reused session");
        assert_eq!(analyze.frame_type, NativeFrameType::Result);
        assert_eq!(
            analyze.payload.get("status").and_then(|v| v.as_str()),
            Some("ok")
        );

        let route = driver
            .execute_native_sql_route_roundtrip_with_optional_session(
                DriverTransportMode::Native,
                5000,
                Some(&mut session),
                "route-optional-reuse-1",
                "SELECT * FROM orders;",
            )
            .expect("route via reused session");
        assert_eq!(route.frame_type, NativeFrameType::Result);
        assert_eq!(
            route.payload.get("route_path").and_then(|v| v.as_str()),
            Some("olap")
        );

        server.join().expect("server join");
    }

    #[test]
    fn resolve_transport_auto_selects_native_for_vng_scheme() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            session_id: "s1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("k".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");
        let r = driver.resolve_transport_mode(DriverTransportMode::Auto);
        assert!(r.used_auto_resolution);
        assert_eq!(r.active, DriverTransportMode::Native);
        assert!(r.notes.is_some());
    }

    #[test]
    fn resolve_transport_auto_selects_http_for_http_scheme() {
        let driver = VoltNueronGridDriver::new(DriverConfig {
            base_url: "http://127.0.0.1:8080".to_string(),
            session_id: "s1".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("k".to_string()),
            operator_id: None,
            http_fallback_url: None,
            route_hint: None,
        })
        .expect("driver");
        let r = driver.resolve_transport_mode(DriverTransportMode::Auto);
        assert_eq!(r.active, DriverTransportMode::Http);
    }

    #[test]
    fn http_rest_base_url_requires_fallback_when_base_is_vng() {
        let cfg = DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            http_fallback_url: None,
            session_id: "s".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("k".to_string()),
            operator_id: None,
            route_hint: None,
        };
        assert!(cfg.http_rest_base_url().is_err());
        let cfg_ok = DriverConfig {
            http_fallback_url: Some("http://127.0.0.1:8080".to_string()),
            ..cfg
        };
        assert_eq!(
            cfg_ok.http_rest_base_url().expect("rest base"),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn resolve_auto_dual_endpoint_matches_transport_mode_fixture() {
        let dual = DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            http_fallback_url: Some("http://127.0.0.1:8080".to_string()),
            session_id: "tm-s-auto-dual".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: None,
            route_hint: None,
        };
        dual.validate().expect("valid dual config");

        let r = resolve_auto_transport(
            &dual,
            TransportCapabilities {
                native_available: true,
                http_available: true,
            },
        )
        .expect("native available");
        assert_eq!(r.active, DriverTransportMode::Native);
        assert!(!r.fallback_triggered);

        let r2 = resolve_auto_transport(
            &dual,
            TransportCapabilities {
                native_available: false,
                http_available: true,
            },
        )
        .expect("fallback http");
        assert_eq!(r2.active, DriverTransportMode::Http);
        assert!(r2.fallback_triggered);
        assert_eq!(r2.fallback_reason.as_deref(), Some("native_unavailable"));

        let err = resolve_auto_transport(
            &dual,
            TransportCapabilities {
                native_available: false,
                http_available: false,
            },
        )
        .expect_err("both dead");
        assert_eq!(err.kind, DriverErrorKind::Transport);
        assert!(err.message.contains("no available transport"));
    }

    #[test]
    fn resolve_auto_single_vng_no_transport_matches_fixture_error() {
        let single = DriverConfig {
            base_url: "vng://127.0.0.1:7542".to_string(),
            http_fallback_url: None,
            session_id: "tm-s-auto-3".to_string(),
            tenant_id: None,
            user_id: None,
            admin_api_key: Some("secret".to_string()),
            operator_id: Some("ops-1".to_string()),
            route_hint: None,
        };
        let err = resolve_auto_transport(
            &single,
            TransportCapabilities {
                native_available: false,
                http_available: false,
            },
        )
        .expect_err("fixture tm-auto-no-transports");
        assert_eq!(err.kind, DriverErrorKind::Transport);
        assert!(err.message.contains("no available transport"));
    }
}
