//! VoltNueronGrid MCP (Model Context Protocol) Server
//!
//! Provides tool-based read-only access to database query, schema, health, and benchmark capabilities.
//! Enforces authentication and permission boundaries according to VNG security model.

use reqwest::{Client, Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use std::sync::OnceLock;

static RUNTIME_CLIENT: OnceLock<Client> = OnceLock::new();

fn runtime_client() -> &'static Client {
    RUNTIME_CLIENT.get_or_init(|| {
        Client::builder()
            .build()
            .expect("Failed to initialize runtime proxy HTTP client")
    })
}

pub mod auth;
pub mod tools;
pub mod guardrails;
pub mod integration;

pub use auth::{McpAuthContext, McpAuthError, AuthenticationLevel};
pub use tools::{
    McpTool,
    ToolHandler,
    ToolRequest,
    ToolResponse,
    QueryToolRequest,
    SchemaToolRequest,
    HealthToolRequest,
    BenchmarkToolRequest,
    DdlCreateToolRequest,
    DdlDropToolRequest,
    ErdToolRequest,
    DataTransferToolRequest,
    ClusterTopologyToolRequest,
    TransactionAdminToolRequest,
    LockAdminToolRequest,
    ClusterNodeManageToolRequest,
};
pub use guardrails::{QueryGuardrails, GuardrailError};

/// MCP Server capability version
pub const MCP_VERSION: &str = "0.1.0";

/// MCP Server identification
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpServerCapabilities {
    pub version: String,
    pub tools: Vec<McpToolCapability>,
    pub resources: Vec<McpResourceCapability>,
    pub max_request_size_bytes: usize,
    pub max_result_size_bytes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpToolCapability {
    pub name: String,
    pub description: String,
    pub auth_level: AuthenticationLevel,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpResourceCapability {
    pub name: String,
    pub description: String,
    pub readonly: bool,
}

impl McpServerCapabilities {
    pub fn default() -> Self {
        Self {
            version: MCP_VERSION.to_string(),
            tools: vec![
                McpToolCapability {
                    name: "query".to_string(),
                    description: "Execute SQL queries (read-only)".to_string(),
                    auth_level: AuthenticationLevel::Operator,
                },
                McpToolCapability {
                    name: "schema".to_string(),
                    description: "Introspect database schema (tables, columns, indexes)".to_string(),
                    auth_level: AuthenticationLevel::Operator,
                },
                McpToolCapability {
                    name: "health".to_string(),
                    description: "Get server health status and replication info".to_string(),
                    auth_level: AuthenticationLevel::Operator,
                },
                McpToolCapability {
                    name: "benchmark".to_string(),
                    description: "Run performance benchmarks (admin only)".to_string(),
                    auth_level: AuthenticationLevel::Admin,
                },
                McpToolCapability {
                    name: "ddl_create".to_string(),
                    description: "Create DB objects (table/view/function/etc) with additional DDL key (admin only)".to_string(),
                    auth_level: AuthenticationLevel::Admin,
                },
                McpToolCapability {
                    name: "ddl_drop".to_string(),
                    description: "Drop DB objects with additional DDL key (admin only)".to_string(),
                    auth_level: AuthenticationLevel::Admin,
                },
                McpToolCapability {
                    name: "erd".to_string(),
                    description: "Generate ERD for tables/schema".to_string(),
                    auth_level: AuthenticationLevel::Operator,
                },
                McpToolCapability {
                    name: "data_transfer".to_string(),
                    description: "Import/export data (CSV, Parquet, Blob, WebDAV, FTP) with additional transfer key (admin only)".to_string(),
                    auth_level: AuthenticationLevel::Admin,
                },
                McpToolCapability {
                    name: "cluster_topology".to_string(),
                    description: "Inspect cluster nodes, sessions, transactions, locks, and runtime capacity (admin only)".to_string(),
                    auth_level: AuthenticationLevel::Admin,
                },
                McpToolCapability {
                    name: "transaction_admin".to_string(),
                    description: "Commit or rollback runtime transactions (admin only)".to_string(),
                    auth_level: AuthenticationLevel::Admin,
                },
                McpToolCapability {
                    name: "lock_admin".to_string(),
                    description: "List or kill locks / deadlock victims (admin only)".to_string(),
                    auth_level: AuthenticationLevel::Admin,
                },
                McpToolCapability {
                    name: "cluster_node_manage".to_string(),
                    description: "Add or remove cluster nodes with transaction/session migration (admin only)".to_string(),
                    auth_level: AuthenticationLevel::Admin,
                },
            ],
            resources: vec![
                McpResourceCapability {
                    name: "metrics".to_string(),
                    description: "Server metrics and observability data".to_string(),
                    readonly: true,
                },
            ],
            max_request_size_bytes: 64 * 1024, // 64 KB
            max_result_size_bytes: 10 * 1024, // 10 KB
        }
    }
}

/// MCP Request envelope
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: Value,
    pub headers: McpRequestHeaders,
}

/// HTTP headers passed through MPC request
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpRequestHeaders {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_vng_admin_key: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_vng_operator_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_vng_tenant_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_vng_user_id: Option<String>,
}

/// MCP Response envelope
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: String,
    pub result: Option<Value>,
    pub error: Option<McpErrorResponse>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpErrorResponse {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Debug, Error)]
pub enum McpServerError {
    #[error("Authentication failed: {0}")]
    AuthError(#[from] McpAuthError),

    #[error("Guardrail violation: {0}")]
    GuardrailViolation(#[from] GuardrailError),

    #[error("Tool error: {0}")]
    ToolError(String),

    #[error("Request validation failed: {0}")]
    InvalidRequest(String),

    #[error("Internal server error: {0}")]
    InternalError(String),
}

fn runtime_proxy_enabled() -> bool {
    std::env::var("VNG_MCP_RUNTIME_PROXY")
        .ok()
        .map(|value| value.eq_ignore_ascii_case("true") || value == "1")
        .unwrap_or(false)
}

fn require_runtime_proxy() -> Result<(), McpServerError> {
    if runtime_proxy_enabled() {
        Ok(())
    } else {
        Err(McpServerError::ToolError(
            "Runtime proxy is disabled. Set VNG_MCP_RUNTIME_PROXY=true and configure VNG_RUNTIME_BASE_URL"
                .to_string(),
        ))
    }
}

fn runtime_base_url() -> String {
    if let Ok(url) = std::env::var("VNG_RUNTIME_BASE_URL") {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let bind = std::env::var("VNG_HTTP_BIND")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());
    format!("http://{}", bind)
}

fn parse_runtime_base_url(url: &str) -> Result<Url, McpServerError> {
    let parsed = Url::parse(url.trim())
        .map_err(|e| McpServerError::ToolError(format!("Invalid runtime proxy URL: {}", e)))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(McpServerError::ToolError(
            "Only http:// and https:// runtime base URLs are supported for MCP runtime proxy"
                .to_string(),
        ));
    }
    Ok(parsed)
}

fn normalize_forward_path(base_path: &str, endpoint_path: &str) -> String {
    let mut path = String::new();
    if !base_path.is_empty() {
        if base_path.starts_with('/') {
            path.push_str(base_path.trim_end_matches('/'));
        } else {
            path.push('/');
            path.push_str(base_path.trim_end_matches('/'));
        }
    }
    if !endpoint_path.starts_with('/') {
        path.push('/');
    }
    path.push_str(endpoint_path);
    if path.is_empty() {
        "/".to_string()
    } else {
        path
    }
}

fn runtime_forward_headers(headers: &McpRequestHeaders) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(value) = &headers.x_vng_admin_key {
        if !value.trim().is_empty() {
            out.push(("x-vng-admin-key".to_string(), value.clone()));
        }
    }
    if let Some(value) = &headers.x_vng_operator_id {
        if !value.trim().is_empty() {
            out.push(("x-vng-operator-id".to_string(), value.clone()));
        }
    }
    if let Some(value) = &headers.x_vng_tenant_id {
        if !value.trim().is_empty() {
            out.push(("x-vng-tenant-id".to_string(), value.clone()));
        }
    }
    if let Some(value) = &headers.x_vng_user_id {
        if !value.trim().is_empty() {
            out.push(("x-vng-user-id".to_string(), value.clone()));
        }
    }
    out
}

async fn forward_to_runtime(
    http_method: &str,
    endpoint_path: &str,
    body: Option<&Value>,
    headers: &McpRequestHeaders,
) -> Result<Value, McpServerError> {
    require_runtime_proxy()?;

    let base = parse_runtime_base_url(&runtime_base_url())?;
    let path = normalize_forward_path(base.path(), endpoint_path);
    let url = base
        .join(path.trim_start_matches('/'))
        .map_err(|e| McpServerError::ToolError(format!("Invalid runtime endpoint path: {}", e)))?;

    let method = Method::from_bytes(http_method.as_bytes())
        .map_err(|e| McpServerError::ToolError(format!("Unsupported HTTP method {}: {}", http_method, e)))?;

    let mut request = runtime_client().request(method, url.clone()).header("Accept", "application/json");

    for (key, value) in runtime_forward_headers(headers) {
        request = request.header(key, value);
    }

    if let Some(payload) = body {
        request = request.json(payload);
    }

    let response = request.send().await.map_err(|e| {
        McpServerError::ToolError(format!("Runtime proxy request failed for {}: {}", url, e))
    })?;

    let status = response.status();
    let body_text = response.text().await.map_err(|e| {
        McpServerError::ToolError(format!("Runtime proxy response read failed for {}: {}", url, e))
    })?;

    if !status.is_success() {
        return Err(McpServerError::ToolError(format!(
            "Runtime endpoint {} returned HTTP {}: {}",
            endpoint_path,
            status.as_u16(),
            body_text
        )));
    }

    if body_text.trim().is_empty() {
        return Ok(json!({"status":"ok"}));
    }

    serde_json::from_str::<Value>(&body_text).map_err(|e| {
        McpServerError::ToolError(format!(
            "Runtime endpoint {} returned non-JSON body: {}",
            endpoint_path,
            e
        ))
    })
}

impl McpServerError {
    pub fn error_code(&self) -> i32 {
        match self {
            McpServerError::AuthError(auth_err) => match auth_err {
                McpAuthError::MissingCredentials => 401,
                McpAuthError::InvalidApiKey => 401,
                McpAuthError::InsufficientPrivilege => 403,
                McpAuthError::MissingOperatorId => 401,
                McpAuthError::TenantMismatch => 403,
            },
            McpServerError::GuardrailViolation(_) => 403,
            McpServerError::ToolError(_) => 400,
            McpServerError::InvalidRequest(_) => 400,
            McpServerError::InternalError(_) => 500,
        }
    }
}

/// Process a single MCP request
pub async fn process_request(
    request: McpRequest,
    capabilities: &McpServerCapabilities,
) -> McpResponse {
    match validate_and_route_request(&request, capabilities).await {
        Ok(result) => McpResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: Some(result),
            error: None,
        },
        Err(err) => McpResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(McpErrorResponse {
                code: err.error_code(),
                message: err.to_string(),
                data: None,
            }),
        },
    }
}

async fn validate_and_route_request(
    request: &McpRequest,
    _capabilities: &McpServerCapabilities,
) -> Result<Value, McpServerError> {
    // Validate basic request structure
    if request.jsonrpc != "2.0" {
        return Err(McpServerError::InvalidRequest(
            "jsonrpc must be 2.0".to_string(),
        ));
    }

    // Parse auth context from headers
    let auth = McpAuthContext::from_headers(&request.headers)
        .map_err(McpServerError::AuthError)?;

    // Route to appropriate tool handler
    match request.method.as_str() {
        "tools/query" => handle_query_tool(&request.params, &request.headers, &auth).await,
        "tools/schema" => handle_schema_tool(&request.params, &request.headers, &auth).await,
        "tools/health" => handle_health_tool(&request.params, &request.headers, &auth).await,
        "tools/benchmark" => handle_benchmark_tool(&request.params, &request.headers, &auth).await,
        "tools/ddl_create" => handle_ddl_create_tool(&request.params, &request.headers, &auth).await,
        "tools/ddl_drop" => handle_ddl_drop_tool(&request.params, &request.headers, &auth).await,
        "tools/erd" => handle_erd_tool(&request.params, &request.headers, &auth).await,
        "tools/data_transfer" => handle_data_transfer_tool(&request.params, &request.headers, &auth).await,
        "tools/cluster_topology" => handle_cluster_topology_tool(&request.params, &request.headers, &auth).await,
        "tools/transaction_admin" => handle_transaction_admin_tool(&request.params, &request.headers, &auth).await,
        "tools/lock_admin" => handle_lock_admin_tool(&request.params, &request.headers, &auth).await,
        "tools/cluster_node_manage" => handle_cluster_node_manage_tool(&request.params, &request.headers, &auth).await,
        _ => Err(McpServerError::InvalidRequest(format!(
            "Unknown method: {}",
            request.method
        ))),
    }
}

fn require_additional_key(env_name: &str, provided: &str) -> Result<(), McpServerError> {
    if provided.trim().is_empty() {
        return Err(McpServerError::AuthError(McpAuthError::MissingCredentials));
    }

    match std::env::var(env_name) {
        Ok(expected) => {
            if expected == provided {
                Ok(())
            } else {
                Err(McpServerError::AuthError(McpAuthError::InvalidApiKey))
            }
        }
        Err(_) => {
            // Dev fallback: environment key is not set, but still require a non-empty explicit key.
            Ok(())
        }
    }
}

async fn handle_query_tool(
    params: &Value,
    headers: &McpRequestHeaders,
    auth: &McpAuthContext,
) -> Result<Value, McpServerError> {
    auth.require_operator().map_err(McpServerError::AuthError)?;

    let req: QueryToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid query params: {}", e)))?;

    QueryGuardrails::validate(&req).map_err(McpServerError::GuardrailViolation)?;

    let runtime_req = json!({
        "sql_batch": req.sql_query,
        "max_rows": req.max_rows,
    });
    let runtime_resp = forward_to_runtime("POST", "/api/v1/sql/execute", Some(&runtime_req), headers).await?;

    Ok(json!({
        "columns": runtime_resp.get("columns").cloned().unwrap_or_else(|| json!([])),
        "rows": runtime_resp.get("rows").cloned().unwrap_or_else(|| json!([])),
        "execution_time_ms": runtime_resp.get("execution_time_ms").cloned().unwrap_or_else(|| json!(0)),
        "rowcount": runtime_resp.get("rowcount").cloned().unwrap_or_else(|| json!(0)),
        "route_path": runtime_resp.get("route_path").cloned().unwrap_or_else(|| json!("unknown")),
        "reason": runtime_resp.get("reason").cloned().unwrap_or_else(|| json!("")),
    }))
}

async fn handle_schema_tool(
    params: &Value,
    headers: &McpRequestHeaders,
    auth: &McpAuthContext,
) -> Result<Value, McpServerError> {
    auth.require_operator().map_err(McpServerError::AuthError)?;

    let req: SchemaToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid schema params: {}", e)))?;

    let runtime_resp = forward_to_runtime("GET", "/api/v1/catalog/schemas", None, headers).await?;
    let entries = runtime_resp
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let tables: Vec<Value> = entries
        .into_iter()
        .filter(|entry| {
            if let Some(filter) = &req.table_filter {
                return entry
                    .get("object_name")
                    .and_then(|name| name.as_str())
                    .map(|name| name.contains(filter))
                    .unwrap_or(false);
            }
            true
        })
        .map(|entry| {
            json!({
                "name": entry.get("object_name").cloned().unwrap_or_else(|| json!("")),
                "columns": [],
                "indexes": []
            })
        })
        .collect();

    Ok(json!({
        "tables": tables,
        "total_tables": tables.len(),
        "active_count": runtime_resp.get("active_count").cloned().unwrap_or_else(|| json!(0)),
        "total_count": runtime_resp.get("total_count").cloned().unwrap_or_else(|| json!(0)),
    }))
}

async fn handle_health_tool(
    params: &Value,
    headers: &McpRequestHeaders,
    auth: &McpAuthContext,
) -> Result<Value, McpServerError> {
    auth.require_operator().map_err(McpServerError::AuthError)?;

    let req: HealthToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid health params: {}", e)))?;

    // Always fetch basic status + reliability data (uptime/node_count not in /health response)
    let (basic, reliability) = tokio::try_join!(
        forward_to_runtime("GET", "/health", None, headers),
        forward_to_runtime("GET", "/api/v1/sre/reliability/status", None, headers),
    )?;

    if req.detailed {
        return Ok(json!({
            "status": basic.get("status").cloned().unwrap_or_else(|| json!("unknown")),
            "version": MCP_VERSION,
            "uptime_ms": reliability.get("uptime_ms").cloned().unwrap_or_else(|| json!(0)),
            "node_count": reliability.get("node_count").cloned().unwrap_or_else(|| json!(1)),
            "replication_lag_ms": reliability.get("replication_lag_ms").cloned().unwrap_or_else(|| json!(0)),
            "detailed_metrics": reliability,
            "node_id": basic.get("node_id").cloned().unwrap_or_else(|| json!("")),
            "cluster_mode": basic.get("cluster_mode").cloned().unwrap_or_else(|| json!("unknown")),
        }));
    }

    Ok(json!({
        "status": basic.get("status").cloned().unwrap_or_else(|| json!("unknown")),
        "version": MCP_VERSION,
        "uptime_ms": reliability.get("uptime_ms").cloned().unwrap_or_else(|| json!(0)),
        "node_count": reliability.get("node_count").cloned().unwrap_or_else(|| json!(1)),
        "replication_lag_ms": reliability.get("replication_lag_ms").cloned().unwrap_or_else(|| json!(0)),
        "node_id": basic.get("node_id").cloned().unwrap_or_else(|| json!("")),
        "cluster_mode": basic.get("cluster_mode").cloned().unwrap_or_else(|| json!("unknown")),
    }))
}

async fn handle_benchmark_tool(
    params: &Value,
    headers: &McpRequestHeaders,
    auth: &McpAuthContext,
) -> Result<Value, McpServerError> {
    auth.require_admin().map_err(McpServerError::AuthError)?;

    let req: BenchmarkToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid benchmark params: {}", e)))?;

    let benchmark_name = req.benchmark_name.to_ascii_lowercase();
    if benchmark_name.contains("ingest") {
        let runtime_req = json!({
            "record_count": req.params.get("record_count").cloned().or_else(|| req.iterations.map(|v| json!(v))),
            "chunk_target_rows": req.params.get("chunk_target_rows").cloned(),
        });
        let runtime_resp = forward_to_runtime("POST", "/api/v1/benchmark/ingest", Some(&runtime_req), headers).await?;
        return Ok(json!({
            "name": req.benchmark_name,
            "duration_ms": runtime_resp.get("wall_time_ms").cloned().unwrap_or_else(|| json!(0)),
            "ops_per_sec": runtime_resp.get("records_per_second").cloned().unwrap_or_else(|| json!(0.0)),
            "latency_p50_ms": runtime_resp.get("latency_p50_ms").cloned(),
            "latency_p99_ms": runtime_resp.get("latency_p99_ms").cloned(),
            "latency_p999_ms": runtime_resp.get("latency_p999_ms").cloned(),
            "throughput_bytes_per_sec": runtime_resp.get("throughput_bytes_per_sec").cloned(),
            "status": runtime_resp.get("status").cloned().unwrap_or_else(|| json!("ok")),
        }));
    }

    let runtime_req = json!({
        "op_count": req.params.get("op_count").cloned().or_else(|| req.iterations.map(|v| json!(v))),
    });
    let runtime_resp = forward_to_runtime("POST", "/api/v1/benchmark/query", Some(&runtime_req), headers).await?;

    Ok(json!({
        "name": req.benchmark_name,
        "duration_ms": runtime_resp.get("wall_time_ms").cloned().unwrap_or_else(|| json!(0)),
        "ops_per_sec": runtime_resp.get("ops_per_second").cloned().unwrap_or_else(|| json!(0.0)),
        "latency_p50_ms": 0.0,
        "latency_p99_ms": 0.0,
        "latency_p999_ms": 0.0,
        "throughput_bytes_per_sec": 0,
        "status": runtime_resp.get("status").cloned().unwrap_or_else(|| json!("ok")),
    }))
}

async fn handle_ddl_create_tool(
    params: &Value,
    headers: &McpRequestHeaders,
    auth: &McpAuthContext,
) -> Result<Value, McpServerError> {
    auth.require_admin().map_err(McpServerError::AuthError)?;

    let req: DdlCreateToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid ddl_create params: {}", e)))?;

    require_additional_key("VNG_MCP_DDL_KEY", &req.ddl_admin_key)?;

    let runtime_req = json!({
        "sql_batch": req.create_sql,
        "max_rows": 0,
    });
    let runtime_resp = forward_to_runtime("POST", "/api/v1/sql/execute", Some(&runtime_req), headers).await?;

    Ok(json!({
        "status": runtime_resp.get("status").cloned().unwrap_or_else(|| json!("ok")),
        "object_type": req.object_type,
        "object_name": req.object_name,
        "message": "DDL create executed through runtime sql/execute",
        "route_path": runtime_resp.get("route_path").cloned().unwrap_or_else(|| json!("")),
    }))
}

async fn handle_ddl_drop_tool(
    params: &Value,
    headers: &McpRequestHeaders,
    auth: &McpAuthContext,
) -> Result<Value, McpServerError> {
    auth.require_admin().map_err(McpServerError::AuthError)?;

    let req: DdlDropToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid ddl_drop params: {}", e)))?;

    require_additional_key("VNG_MCP_DDL_KEY", &req.ddl_admin_key)?;

    let runtime_req = json!({
        "sql_batch": req.drop_sql,
        "max_rows": 0,
    });
    let runtime_resp = forward_to_runtime("POST", "/api/v1/sql/execute", Some(&runtime_req), headers).await?;

    Ok(json!({
        "status": runtime_resp.get("status").cloned().unwrap_or_else(|| json!("ok")),
        "object_type": req.object_type,
        "object_name": req.object_name,
        "message": "DDL drop executed through runtime sql/execute",
        "route_path": runtime_resp.get("route_path").cloned().unwrap_or_else(|| json!("")),
    }))
}

async fn handle_erd_tool(
    params: &Value,
    headers: &McpRequestHeaders,
    auth: &McpAuthContext,
) -> Result<Value, McpServerError> {
    auth.require_operator().map_err(McpServerError::AuthError)?;

    let req: ErdToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid erd params: {}", e)))?;

    let format = req.output_format.unwrap_or_else(|| "mermaid".to_string());
    let schema_resp = forward_to_runtime("GET", "/api/v1/catalog/schemas", None, headers).await?;
    let entries = schema_resp
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut table_names: Vec<String> = entries
        .iter()
        .filter_map(|entry| entry.get("object_name").and_then(|name| name.as_str()).map(|value| value.to_string()))
        .filter(|name| {
            if !req.table_names.is_empty() {
                return req.table_names.iter().any(|wanted| wanted.eq_ignore_ascii_case(name));
            }
            true
        })
        .collect();
    table_names.sort();
    table_names.dedup();

    let diagram = if format.eq_ignore_ascii_case("dot") {
        if table_names.is_empty() {
            "digraph ERD {\n}\n".to_string()
        } else {
            let mut dot = String::from("digraph ERD {\n");
            for table in &table_names {
                dot.push_str(&format!("  \"{}\";\n", table));
            }
            dot.push('}');
            dot.push('\n');
            dot
        }
    } else {
        let mut mermaid = String::from("erDiagram\n");
        if table_names.is_empty() {
            mermaid.push_str("    EMPTY ||--|| EMPTY : none\n");
        } else {
            for table in &table_names {
                mermaid.push_str(&format!("    {} {{\n    }}\n", table.to_ascii_uppercase()));
            }
        }
        mermaid
    };

    Ok(json!({
        "format": format,
        "diagram": diagram,
        "table_count": table_names.len(),
    }))
}

async fn handle_data_transfer_tool(
    params: &Value,
    headers: &McpRequestHeaders,
    auth: &McpAuthContext,
) -> Result<Value, McpServerError> {
    auth.require_admin().map_err(McpServerError::AuthError)?;

    let req: DataTransferToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid data_transfer params: {}", e)))?;

    require_additional_key("VNG_MCP_TRANSFER_KEY", &req.transfer_admin_key)?;

    let result = match req.direction {
        tools::DataTransferDirection::Export => {
            let since_sequence = req.options.get("since_sequence").cloned().unwrap_or_else(|| json!(0));
            let max_items = req.options.get("max_items").cloned().unwrap_or_else(|| json!(500));
            let runtime_req = json!({
                "since_sequence": since_sequence,
                "max_items": max_items,
            });
            let runtime_resp = forward_to_runtime("POST", "/api/v1/store/htap/export", Some(&runtime_req), headers).await?;
            json!({
                "status": runtime_resp.get("status").cloned().unwrap_or_else(|| json!("ok")),
                "direction": req.direction,
                "format": req.format,
                "endpoint": req.endpoint,
                "rows_affected": runtime_resp.get("mutation_count").cloned().unwrap_or_else(|| json!(0)),
                "message": format!("Export completed from runtime for table {}", req.table_name),
                "runtime": runtime_resp,
            })
        }
        tools::DataTransferDirection::Import => {
            let connector_id = req
                .options
                .get("connector_id")
                .and_then(|value| value.as_str())
                .unwrap_or("mcp-transfer");
            let runtime_req = match req.format {
                tools::DataFormat::Csv => {
                    let csv_data = req
                        .options
                        .get("csv_data")
                        .and_then(|value| value.as_str())
                        .ok_or_else(|| {
                            McpServerError::InvalidRequest(
                                "data_transfer import csv requires options.csv_data".to_string(),
                            )
                        })?;
                    json!({
                        "connector_id": connector_id,
                        "csv_data": csv_data,
                    })
                }
                tools::DataFormat::Parquet => {
                    let parquet_data_base64 = req
                        .options
                        .get("parquet_data_base64")
                        .and_then(|value| value.as_str())
                        .ok_or_else(|| {
                            McpServerError::InvalidRequest(
                                "data_transfer import parquet requires options.parquet_data_base64".to_string(),
                            )
                        })?;
                    json!({
                        "connector_id": connector_id,
                        "parquet_data_base64": parquet_data_base64,
                    })
                }
            };

            let endpoint = match req.format {
                tools::DataFormat::Csv => "/api/v1/ingest/csv",
                tools::DataFormat::Parquet => "/api/v1/ingest/parquet",
            };
            let runtime_resp = forward_to_runtime("POST", endpoint, Some(&runtime_req), headers).await?;
            json!({
                "status": runtime_resp.get("status").cloned().unwrap_or_else(|| json!("ok")),
                "direction": req.direction,
                "format": req.format,
                "endpoint": req.endpoint,
                "rows_affected": runtime_resp.get("records_parsed").cloned().unwrap_or_else(|| json!(0)),
                "message": format!("Import completed through runtime endpoint {}", endpoint),
                "runtime": runtime_resp,
            })
        }
    };

    Ok(result)
}

async fn handle_cluster_topology_tool(
    params: &Value,
    headers: &McpRequestHeaders,
    auth: &McpAuthContext,
) -> Result<Value, McpServerError> {
    auth.require_admin().map_err(McpServerError::AuthError)?;
    let _req: ClusterTopologyToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid cluster_topology params: {}", e)))?;

    forward_to_runtime(
        "GET",
        "/api/v1/admin/cluster/topology",
        None,
        headers,
    )
    .await
}

async fn handle_transaction_admin_tool(
    params: &Value,
    headers: &McpRequestHeaders,
    auth: &McpAuthContext,
) -> Result<Value, McpServerError> {
    auth.require_admin().map_err(McpServerError::AuthError)?;
    let req: TransactionAdminToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid transaction_admin params: {}", e)))?;

    forward_to_runtime(
        "POST",
        "/api/v1/admin/sql/transactions/control",
        Some(&json!({
            "action": req.action,
            "transaction_id": req.transaction_id,
            "reason": req.reason,
        })),
        headers,
    )
    .await
}

async fn handle_lock_admin_tool(
    params: &Value,
    headers: &McpRequestHeaders,
    auth: &McpAuthContext,
) -> Result<Value, McpServerError> {
    auth.require_admin().map_err(McpServerError::AuthError)?;
    let req: LockAdminToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid lock_admin params: {}", e)))?;

    forward_to_runtime(
        "POST",
        "/api/v1/admin/sql/locks/control",
        Some(&json!({
            "action": req.action,
            "lock_id": req.lock_id,
            "transaction_id": req.transaction_id,
            "reason": req.reason,
        })),
        headers,
    )
    .await
}

async fn handle_cluster_node_manage_tool(
    params: &Value,
    headers: &McpRequestHeaders,
    auth: &McpAuthContext,
) -> Result<Value, McpServerError> {
    auth.require_admin().map_err(McpServerError::AuthError)?;
    let req: ClusterNodeManageToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid cluster_node_manage params: {}", e)))?;

    forward_to_runtime(
        "POST",
        "/api/v1/admin/cluster/nodes/manage",
        Some(&json!({
            "action": req.action,
            "node_id": req.node_id,
            "role": req.role,
            "desired_status": req.desired_status,
            "total_cpu_cores": req.total_cpu_cores,
            "total_ram_mb": req.total_ram_mb,
            "target_node_id": req.target_node_id,
            "reason": req.reason,
        })),
        headers,
    )
    .await
}

/// Blocking wrapper for stdio MCP server
/// 
/// Converts async process_request to sync for use in blocking contexts (stdio server).
/// Uses tokio::runtime::Handle to run async code in existing runtime, or creates one if needed.
pub fn process_mcp_request_blocking(request: McpRequest) -> McpResponse {
    let capabilities = McpServerCapabilities::default();
    
    // Try to use existing tokio runtime, or create a new one
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.block_on(process_request(request, &capabilities))
        }
        Err(_) => {
            // No existing runtime, create new one
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(process_request(request, &capabilities))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities_default() {
        let cap = McpServerCapabilities::default();
        assert_eq!(cap.version, MCP_VERSION);
        assert_eq!(cap.tools.len(), 12);
    }

    #[tokio::test]
    async fn test_invalid_jsonrpc_version() {
        let req = McpRequest {
            jsonrpc: "1.0".to_string(),
            id: "1".to_string(),
            method: "tools/health".to_string(),
            params: json!({}),
            headers: McpRequestHeaders {
                x_vng_admin_key: Some("key".to_string()),
                x_vng_operator_id: None,
                x_vng_tenant_id: None,
                x_vng_user_id: None,
            },
        };
        let resp = process_request(req, &McpServerCapabilities::default()).await;
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn test_missing_auth_headers() {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: "1".to_string(),
            method: "tools/query".to_string(),
            params: json!({"sql_query": "SELECT 1"}),
            headers: McpRequestHeaders {
                x_vng_admin_key: None,
                x_vng_operator_id: None,
                x_vng_tenant_id: None,
                x_vng_user_id: None,
            },
        };
        let resp = process_request(req, &McpServerCapabilities::default()).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.as_ref().unwrap().code, 401);
    }

    #[test]
    fn test_parse_runtime_base_url_with_explicit_port() {
        let parsed = parse_runtime_base_url("http://127.0.0.1:8080").unwrap();
        assert_eq!(parsed.host_str(), Some("127.0.0.1"));
        assert_eq!(parsed.port(), Some(8080));
        assert_eq!(parsed.path(), "/");
    }

    #[test]
    fn test_parse_runtime_base_url_with_base_path() {
        let parsed = parse_runtime_base_url("http://localhost:8080/base").unwrap();
        assert_eq!(parsed.host_str(), Some("localhost"));
        assert_eq!(parsed.port(), Some(8080));
        assert_eq!(parsed.path(), "/base");
    }

    #[test]
    fn test_parse_runtime_base_url_supports_https() {
        let parsed = parse_runtime_base_url("https://example.com/api").unwrap();
        assert_eq!(parsed.scheme(), "https");
        assert_eq!(parsed.path(), "/api");
    }

    #[test]
    fn test_parse_runtime_base_url_rejects_non_http_schemes() {
        let err = parse_runtime_base_url("ftp://example.com").unwrap_err();
        assert!(err.to_string().contains("http:// and https://"));
    }

    #[test]
    fn test_normalize_forward_path() {
        let path = normalize_forward_path("/base", "/api/v1/admin/cluster/topology");
        assert_eq!(path, "/base/api/v1/admin/cluster/topology");
    }

    #[test]
    fn test_runtime_proxy_enabled_env_values() {
        unsafe {
            std::env::set_var("VNG_MCP_RUNTIME_PROXY", "true");
        }
        assert!(runtime_proxy_enabled());
        unsafe {
            std::env::set_var("VNG_MCP_RUNTIME_PROXY", "1");
        }
        assert!(runtime_proxy_enabled());
        unsafe {
            std::env::set_var("VNG_MCP_RUNTIME_PROXY", "false");
        }
        assert!(!runtime_proxy_enabled());
    }

    #[test]
    fn test_runtime_base_url_prefers_runtime_base_var() {
        unsafe {
            std::env::set_var("VNG_RUNTIME_BASE_URL", "https://runtime.example.test/base");
            std::env::set_var("VNG_HTTP_BIND", "127.0.0.1:9999");
        }
        assert_eq!(runtime_base_url(), "https://runtime.example.test/base");
        unsafe {
            std::env::remove_var("VNG_RUNTIME_BASE_URL");
            std::env::remove_var("VNG_HTTP_BIND");
        }
    }
}
