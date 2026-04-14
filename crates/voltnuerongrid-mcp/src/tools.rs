//! MCP Tool definitions and handlers

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Generic tool trait for extensibility
pub trait McpTool {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
}

/// Generic tool handler trait
#[allow(async_fn_in_trait)]
pub trait ToolHandler {
    async fn handle(&self, request: &ToolRequest) -> Result<ToolResponse, String>;
}

/// Generic tool request
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolRequest {
    pub tool_name: String,
    pub params: Value,
}

/// Generic tool response
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResponse {
    pub tool_name: String,
    pub result: Value,
    pub execution_time_ms: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// Query Tool
// ═══════════════════════════════════════════════════════════════════════════

/// Request to execute a SQL query
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryToolRequest {
    /// SQL query string (SELECT only)
    pub sql_query: String,

    /// Query timeout in milliseconds
    #[serde(default)]
    pub timeout_ms: Option<u64>,

    /// Optional tenant ID for scoped queries
    #[serde(default)]
    pub tenant_id: Option<String>,

    /// Maximum rows to return
    #[serde(default)]
    pub max_rows: Option<usize>,
}

/// Response from query execution
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryToolResponse {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub execution_time_ms: u64,
    pub rowcount: usize,
}

// ═══════════════════════════════════════════════════════════════════════════
// Schema Tool
// ═══════════════════════════════════════════════════════════════════════════

/// Request to introspect database schema
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchemaToolRequest {
    /// Optional schema/database filter
    #[serde(default)]
    pub schema_filter: Option<String>,

    /// Optional table name filter
    #[serde(default)]
    pub table_filter: Option<String>,
}

/// Column information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
}

/// Table information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableInfo {
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    pub indexes: Vec<IndexInfo>,
}

/// Index information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

/// Response with schema information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchemaToolResponse {
    pub tables: Vec<TableInfo>,
    pub total_tables: usize,
}

// ═══════════════════════════════════════════════════════════════════════════
// Health Tool
// ═══════════════════════════════════════════════════════════════════════════

/// Request to get server health
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthToolRequest {
    /// Include detailed metrics
    #[serde(default)]
    pub detailed: bool,
}

/// Health status response
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthToolResponse {
    pub status: String, // "healthy", "degraded", "unhealthy"
    pub version: String,
    pub uptime_ms: u64,
    pub node_count: usize,
    pub replication_lag_ms: u64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub detailed_metrics: Option<DetailedMetrics>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetailedMetrics {
    pub active_connections: usize,
    pub memory_usage_mb: usize,
    pub disk_usage_mb: usize,
    pub query_cache_hit_rate: f64,
    pub transactions_per_sec: f64,
}

// ═══════════════════════════════════════════════════════════════════════════
// Benchmark Tool
// ═══════════════════════════════════════════════════════════════════════════

/// Request to run a benchmark
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkToolRequest {
    /// Name of the benchmark to run
    pub benchmark_name: String,

    /// Parameters for the benchmark
    #[serde(default)]
    pub params: Value,

    /// Number of iterations (admin-restricted)
    #[serde(default)]
    pub iterations: Option<usize>,
}

/// Benchmark result
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkToolResponse {
    pub name: String,
    pub duration_ms: u64,
    pub ops_per_sec: f64,
    pub latency_p50_ms: f64,
    pub latency_p99_ms: f64,
    pub latency_p999_ms: f64,
    pub throughput_bytes_per_sec: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_request_serialization() {
        let req = QueryToolRequest {
            sql_query: "SELECT * FROM users".to_string(),
            timeout_ms: Some(5000),
            tenant_id: Some("tenant-123".to_string()),
            max_rows: Some(100),
        };

        let json = serde_json::to_value(&req).unwrap();
        let deserialized: QueryToolRequest = serde_json::from_value(json).unwrap();

        assert_eq!(deserialized.sql_query, "SELECT * FROM users");
        assert_eq!(deserialized.timeout_ms, Some(5000));
        assert_eq!(deserialized.tenant_id, Some("tenant-123".to_string()));
        assert_eq!(deserialized.max_rows, Some(100));
    }

    #[test]
    fn test_schema_info_serialization() {
        let column = ColumnInfo {
            name: "id".to_string(),
            data_type: "INT".to_string(),
            nullable: false,
            primary_key: true,
        };

        let json = serde_json::to_value(&column).unwrap();
        let deserialized: ColumnInfo = serde_json::from_value(json).unwrap();

        assert_eq!(deserialized.name, "id");
        assert_eq!(deserialized.data_type, "INT");
        assert!(!deserialized.nullable);
        assert!(deserialized.primary_key);
    }

    #[test]
    fn test_health_response() {
        let health = HealthToolResponse {
            status: "healthy".to_string(),
            version: "0.1.0".to_string(),
            uptime_ms: 3600000,
            node_count: 3,
            replication_lag_ms: 10,
            detailed_metrics: Some(DetailedMetrics {
                active_connections: 42,
                memory_usage_mb: 512,
                disk_usage_mb: 2048,
                query_cache_hit_rate: 0.95,
                transactions_per_sec: 1000.0,
            }),
        };

        let json = serde_json::to_value(&health).unwrap();
        assert!(json.get("detailed_metrics").is_some());
        assert_eq!(
            json.get("status").and_then(|v| v.as_str()),
            Some("healthy")
        );
    }

    #[test]
    fn test_benchmark_response() {
        let bench = BenchmarkToolResponse {
            name: "query_select_all".to_string(),
            duration_ms: 1000,
            ops_per_sec: 1000.0,
            latency_p50_ms: 0.5,
            latency_p99_ms: 5.0,
            latency_p999_ms: 50.0,
            throughput_bytes_per_sec: 1024 * 1024,
        };

        let json = serde_json::to_value(&bench).unwrap();
        assert_eq!(json.get("name").and_then(|v| v.as_str()), Some("query_select_all"));
        assert_eq!(json.get("ops_per_sec").and_then(|v| v.as_f64()), Some(1000.0));
    }
}
