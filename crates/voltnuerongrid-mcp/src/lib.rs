//! VoltNueronGrid MCP (Model Context Protocol) Server
//!
//! Provides tool-based read-only access to database query, schema, health, and benchmark capabilities.
//! Enforces authentication and permission boundaries according to VNG security model.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

pub mod auth;
pub mod tools;
pub mod guardrails;
pub mod integration;

pub use auth::{McpAuthContext, McpAuthError, AuthenticationLevel};
pub use tools::{McpTool, ToolHandler, ToolRequest, ToolResponse, QueryToolRequest, SchemaToolRequest, HealthToolRequest, BenchmarkToolRequest};
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
        "tools/query" => handle_query_tool(&request.params, &auth).await,
        "tools/schema" => handle_schema_tool(&request.params, &auth).await,
        "tools/health" => handle_health_tool(&request.params, &auth).await,
        "tools/benchmark" => handle_benchmark_tool(&request.params, &auth).await,
        _ => Err(McpServerError::InvalidRequest(format!(
            "Unknown method: {}",
            request.method
        ))),
    }
}

async fn handle_query_tool(params: &Value, auth: &McpAuthContext) -> Result<Value, McpServerError> {
    // Verify operator access
    auth.require_operator()
        .map_err(McpServerError::AuthError)?;

    // Parse query request
    let req: QueryToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid query params: {}", e)))?;

    // Apply guardrails
    QueryGuardrails::validate(&req)
        .map_err(McpServerError::GuardrailViolation)?;

    // Execute query (placeholder implementation)
    Ok(json!({
        "columns": ["id", "name"],
        "rows": [],
        "execution_time_ms": 0,
        "rowcount": 0,
    }))
}

async fn handle_schema_tool(params: &Value, auth: &McpAuthContext) -> Result<Value, McpServerError> {
    // Verify operator access
    auth.require_operator()
        .map_err(McpServerError::AuthError)?;

    // Parse schema request
    let _req: SchemaToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid schema params: {}", e)))?;

    // Return schema (placeholder)
    Ok(json!({
        "tables": []
    }))
}

async fn handle_health_tool(params: &Value, auth: &McpAuthContext) -> Result<Value, McpServerError> {
    // Verify operator access
    auth.require_operator()
        .map_err(McpServerError::AuthError)?;

    let _req: HealthToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid health params: {}", e)))?;

    Ok(json!({
        "status": "healthy",
        "version": MCP_VERSION,
        "uptime_ms": 0,
        "node_count": 1,
        "replication_lag_ms": 0
    }))
}

async fn handle_benchmark_tool(params: &Value, auth: &McpAuthContext) -> Result<Value, McpServerError> {
    // Verify admin access only
    auth.require_admin()
        .map_err(McpServerError::AuthError)?;

    let _req: BenchmarkToolRequest = serde_json::from_value(params.clone())
        .map_err(|e| McpServerError::InvalidRequest(format!("Invalid benchmark params: {}", e)))?;

    Ok(json!({
        "name": "benchmark_1",
        "duration_ms": 0,
        "ops_per_sec": 0.0,
        "latency_p50_ms": 0.0,
        "latency_p99_ms": 0.0
    }))
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
        assert_eq!(cap.tools.len(), 4);
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
}
