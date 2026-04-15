//! MCP integration adapters for VoltNueronGrid

use crate::auth::McpAuthContext;
use crate::tools::{BenchmarkToolResponse, HealthToolResponse, QueryToolResponse, SchemaToolResponse};
use crate::{process_request, McpRequest, McpRequestHeaders, McpServerCapabilities};
use serde_json::{json, Value};

pub struct McpSqlExecutor;

impl McpSqlExecutor {
    pub async fn execute_query(
        query: &str,
        headers: &McpRequestHeaders,
    ) -> Result<QueryToolResponse, String> {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: "integration-query".to_string(),
            method: "tools/query".to_string(),
            params: json!({ "sql_query": query }),
            headers: headers.clone(),
        };
        let resp = process_request(req, &McpServerCapabilities::default()).await;
        if let Some(err) = resp.error {
            return Err(err.message);
        }
        let value = resp.result.ok_or_else(|| "missing result".to_string())?;
        serde_json::from_value(value).map_err(|e| format!("query response decode failed: {}", e))
    }
}

pub struct McpSchemaProvider;

impl McpSchemaProvider {
    pub async fn get_schema(headers: &McpRequestHeaders) -> Result<SchemaToolResponse, String> {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: "integration-schema".to_string(),
            method: "tools/schema".to_string(),
            params: json!({}),
            headers: headers.clone(),
        };
        let resp = process_request(req, &McpServerCapabilities::default()).await;
        if let Some(err) = resp.error {
            return Err(err.message);
        }
        let value = resp.result.ok_or_else(|| "missing result".to_string())?;
        serde_json::from_value(value).map_err(|e| format!("schema response decode failed: {}", e))
    }
}

pub struct McpHealthMonitor;

impl McpHealthMonitor {
    pub async fn get_health(detailed: bool, headers: &McpRequestHeaders) -> Result<HealthToolResponse, String> {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: "integration-health".to_string(),
            method: "tools/health".to_string(),
            params: json!({ "detailed": detailed }),
            headers: headers.clone(),
        };
        let resp = process_request(req, &McpServerCapabilities::default()).await;
        if let Some(err) = resp.error {
            return Err(err.message);
        }
        let value = resp.result.ok_or_else(|| "missing result".to_string())?;
        serde_json::from_value(value).map_err(|e| format!("health response decode failed: {}", e))
    }
}

pub struct McpBenchmarkRunner;

impl McpBenchmarkRunner {
    pub async fn run_benchmark(
        name: &str,
        params: &Value,
        _auth: &McpAuthContext,
        headers: &McpRequestHeaders,
    ) -> Result<BenchmarkToolResponse, String> {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: "integration-benchmark".to_string(),
            method: "tools/benchmark".to_string(),
            params: json!({
                "benchmark_name": name,
                "params": params,
            }),
            headers: headers.clone(),
        };
        let resp = process_request(req, &McpServerCapabilities::default()).await;
        if let Some(err) = resp.error {
            return Err(err.message);
        }
        let value = resp.result.ok_or_else(|| "missing result".to_string())?;
        serde_json::from_value(value).map_err(|e| format!("benchmark response decode failed: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_integration_adapters_require_runtime_proxy() {
        unsafe {
            std::env::set_var("VNG_MCP_RUNTIME_PROXY", "false");
        }
        let headers = McpRequestHeaders {
            x_vng_admin_key: Some("k".to_string()),
            x_vng_operator_id: Some("op-1".to_string()),
            x_vng_tenant_id: None,
            x_vng_user_id: None,
        };

        let result = McpSqlExecutor::execute_query("SELECT 1", &headers).await;
        assert!(result.is_err());
    }
}
