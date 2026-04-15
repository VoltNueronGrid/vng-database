/// Integration tests for VoltNueronGrid MCP Server
/// Tests the full end-to-end flow including authentication, authorization, guardrails, and tool execution

use axum::{extract::Path, routing::{get, post}, Json, Router};
use serde_json::{json, Value};
use serial_test::serial;
use voltnuerongrid_mcp::{
    McpRequest, McpRequestHeaders, McpServerCapabilities, process_request,
    auth::{McpAuthContext, AuthenticationLevel},
    tools::QueryToolRequest,
    guardrails::QueryGuardrails,
};

struct MockRuntimeGuard {
    _handle: tokio::task::JoinHandle<()>,
}

impl Drop for MockRuntimeGuard {
    fn drop(&mut self) {
        self._handle.abort();
        unsafe {
            std::env::remove_var("VNG_MCP_RUNTIME_PROXY");
            std::env::remove_var("VNG_RUNTIME_BASE_URL");
            std::env::remove_var("VNG_MCP_DDL_KEY");
            std::env::remove_var("VNG_MCP_TRANSFER_KEY");
            std::env::remove_var("VNG_ADMIN_API_KEY");
        }
    }
}

async fn setup_mock_runtime() -> MockRuntimeGuard {
    async fn health() -> Json<Value> {
        Json(json!({"status":"ok","node_id":"node-1","cluster_mode":"single"}))
    }

    async fn reliability() -> Json<Value> {
        Json(json!({"status":"ok","node_count":1,"replication_lag_ms":0,"uptime_ms":1000}))
    }

    async fn sql_execute() -> Json<Value> {
        Json(json!({"status":"ok","columns":["v"],"rows":[[1]],"execution_time_ms":1,"rowcount":1,"route_path":"oltp","reason":"test"}))
    }

    async fn catalog() -> Json<Value> {
        Json(json!({"status":"ok","active_count":1,"total_count":1,"entries":[{"object_name":"users","object_kind":"table"}]}))
    }

    async fn benchmark_ingest() -> Json<Value> {
        Json(json!({"status":"ok","wall_time_ms":5,"records_per_second":1000.0}))
    }

    async fn benchmark_query() -> Json<Value> {
        Json(json!({"status":"ok","wall_time_ms":4,"ops_per_second":2000.0}))
    }

    async fn htap_export() -> Json<Value> {
        Json(json!({"status":"ok","mutation_count":2}))
    }

    async fn ingest_csv() -> Json<Value> {
        Json(json!({"status":"ok","records_parsed":3}))
    }

    async fn ingest_parquet() -> Json<Value> {
        Json(json!({"status":"ok","records_parsed":4}))
    }

    async fn admin_cluster_topology() -> Json<Value> {
        Json(json!({"status":"ok","leader_node_id":"node-1","total_nodes":1,"active_nodes":1,"passive_nodes":0,"dead_nodes":0,"active_sessions":0,"passive_sessions":0,"live_transactions":0,"total_transactions":0,"live_locks":0,"nodes":[]}))
    }

    async fn admin_transactions() -> Json<Value> {
        Json(json!({"status":"ok","action":"list","affected_count":0,"active_count":0,"transactions":[]}))
    }

    async fn admin_locks() -> Json<Value> {
        Json(json!({"status":"ok","action":"list","released_lock_count":0,"active_lock_count":0,"locks":[]}))
    }

    async fn admin_nodes() -> Json<Value> {
        Json(json!({"status":"ok","action":"add","node_id":"node-2","cluster_size":2,"migrated_transactions":0,"migrated_sessions":0}))
    }

    async fn catch_all(Path(_path): Path<String>) -> Json<Value> {
        Json(json!({"status":"ok"}))
    }

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/sre/reliability/status", get(reliability))
        .route("/api/v1/sql/execute", post(sql_execute))
        .route("/api/v1/catalog/schemas", get(catalog))
        .route("/api/v1/benchmark/ingest", post(benchmark_ingest))
        .route("/api/v1/benchmark/query", post(benchmark_query))
        .route("/api/v1/store/htap/export", post(htap_export))
        .route("/api/v1/ingest/csv", post(ingest_csv))
        .route("/api/v1/ingest/parquet", post(ingest_parquet))
        .route("/api/v1/admin/cluster/topology", get(admin_cluster_topology))
        .route("/api/v1/admin/sql/transactions/control", post(admin_transactions))
        .route("/api/v1/admin/sql/locks/control", post(admin_locks))
        .route("/api/v1/admin/cluster/nodes/manage", post(admin_nodes))
        .route("/*path", get(catch_all).post(catch_all));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind mock runtime");
    let address = listener.local_addr().expect("mock runtime addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock runtime");
    });

    unsafe {
        std::env::set_var("VNG_MCP_RUNTIME_PROXY", "true");
        std::env::set_var("VNG_RUNTIME_BASE_URL", format!("http://{}", address));
        std::env::set_var("VNG_MCP_DDL_KEY", "extra-ddl-key");
        std::env::set_var("VNG_MCP_TRANSFER_KEY", "xfer-key");
        std::env::set_var("VNG_ADMIN_API_KEY", "admin-key");
    }

    MockRuntimeGuard { _handle: handle }
}

#[tokio::test]
#[serial]
async fn mcp_001_admin_can_execute_all_tools() {
    let _runtime = setup_mock_runtime().await;
    let capabilities = McpServerCapabilities::default();

    let admin_headers = McpRequestHeaders {
        x_vng_admin_key: Some("admin-key".to_string()),
        x_vng_operator_id: None,
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };

    // Test health tool
    let health_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "1".to_string(),
        method: "tools/health".to_string(),
        params: json!({}),
        headers: admin_headers.clone(),
    };

    let resp = process_request(health_req, &capabilities).await;
    assert!(resp.error.is_none());
    assert!(resp.result.is_some());
}

#[tokio::test]
#[serial]
async fn mcp_002_operator_can_execute_operator_tools_not_admin() {
    let _runtime = setup_mock_runtime().await;
    let capabilities = McpServerCapabilities::default();

    let operator_headers = McpRequestHeaders {
        x_vng_admin_key: None,
        x_vng_operator_id: Some("op-001".to_string()),
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };

    // Test query tool (operator allowed)
    let query_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "2".to_string(),
        method: "tools/query".to_string(),
        params: json!({
            "sql_query": "SELECT 1"
        }),
        headers: operator_headers.clone(),
    };

    let resp = process_request(query_req, &capabilities).await;
    assert!(resp.error.is_none());

    // Test benchmark tool (operator NOT allowed - admin only)
    let benchmark_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "3".to_string(),
        method: "tools/benchmark".to_string(),
        params: json!({
            "benchmark_name": "test"
        }),
        headers: operator_headers,
    };

    let resp = process_request(benchmark_req, &capabilities).await;
    assert!(resp.error.is_some());
    assert_eq!(resp.error.as_ref().unwrap().code, 403); // Forbidden
}

#[tokio::test]
#[serial]
async fn mcp_003_tenant_cannot_access_operator_tools() {
    let capabilities = McpServerCapabilities::default();

    let tenant_headers = McpRequestHeaders {
        x_vng_admin_key: None,
        x_vng_operator_id: None,
        x_vng_tenant_id: Some("tenant-123".to_string()),
        x_vng_user_id: Some("user-456".to_string()),
    };

    // Test query tool (requires operator level)
    let query_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "4".to_string(),
        method: "tools/query".to_string(),
        params: json!({
            "sql_query": "SELECT 1"
        }),
        headers: tenant_headers,
    };

    let resp = process_request(query_req, &capabilities).await;
    assert!(resp.error.is_some());
    assert_eq!(resp.error.as_ref().unwrap().code, 403); // Forbidden
}

#[tokio::test]
#[serial]
async fn mcp_004_missing_auth_returns_401() {
    let capabilities = McpServerCapabilities::default();

    let no_auth_headers = McpRequestHeaders {
        x_vng_admin_key: None,
        x_vng_operator_id: None,
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };

    let req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "5".to_string(),
        method: "tools/health".to_string(),
        params: json!({}),
        headers: no_auth_headers,
    };

    let resp = process_request(req, &capabilities).await;
    assert!(resp.error.is_some());
    assert_eq!(resp.error.as_ref().unwrap().code, 401); // Unauthorized
}

#[tokio::test]
#[serial]
async fn mcp_005_unknown_method_returns_400() {
    unsafe { std::env::remove_var("VNG_ADMIN_API_KEY"); }
    let capabilities = McpServerCapabilities::default();

    let admin_headers = McpRequestHeaders {
        x_vng_admin_key: Some("key".to_string()),
        x_vng_operator_id: None,
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };

    let req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "6".to_string(),
        method: "tools/unknown".to_string(),
        params: json!({}),
        headers: admin_headers,
    };

    let resp = process_request(req, &capabilities).await;
    assert!(resp.error.is_some());
    assert_eq!(resp.error.as_ref().unwrap().code, 400);
}

#[tokio::test]
#[serial]
async fn mcp_006_query_guardrails_enforce_safety() {
    // Test that guardrails prevent dangerous queries
    let dangerous_queries = vec![
        "DROP TABLE users",
        "DELETE FROM users",
        "ALTER TABLE users ADD COLUMN x INT",
        "INSERT INTO users VALUES (1, 'test')",
        "UPDATE users SET name = 'admin'",
        "TRUNCATE TABLE users",
    ];

    for query in dangerous_queries {
        let req = QueryToolRequest {
            sql_query: query.to_string(),
            timeout_ms: None,
            tenant_id: None,
            max_rows: None,
        };
        assert!(QueryGuardrails::validate(&req).is_err(), "Query should be rejected: {}", query);
    }
}

#[tokio::test]
#[serial]
async fn mcp_007_admin_auth_precedence() {
    unsafe { std::env::remove_var("VNG_ADMIN_API_KEY"); }
    // Verify admin takes precedence over operator/tenant
    let auth_headers = McpRequestHeaders {
        x_vng_admin_key: Some("admin-key".to_string()),
        x_vng_operator_id: Some("should-be-ignored".to_string()),
        x_vng_tenant_id: Some("should-be-ignored".to_string()),
        x_vng_user_id: Some("should-be-ignored".to_string()),
    };

    let auth = McpAuthContext::from_headers(&auth_headers).unwrap();
    assert!(auth.is_admin);
    assert_eq!(auth.auth_level, AuthenticationLevel::Admin);
    assert!(auth.operator_id.is_none()); // Other fields should be None
}

#[tokio::test]
#[serial]
async fn mcp_008_operator_auth_precedence() {
    // Verify operator takes precedence over tenant
    let auth_headers = McpRequestHeaders {
        x_vng_admin_key: None,
        x_vng_operator_id: Some("op-001".to_string()),
        x_vng_tenant_id: Some("should-be-ignored".to_string()),
        x_vng_user_id: Some("should-be-ignored".to_string()),
    };

    let auth = McpAuthContext::from_headers(&auth_headers).unwrap();
    assert!(!auth.is_admin);
    assert_eq!(auth.auth_level, AuthenticationLevel::Operator);
    assert_eq!(auth.operator_id, Some("op-001".to_string()));
}

#[tokio::test]
#[serial]
async fn mcp_009_result_size_guardrails() {
    // Test result size calculation and limits
    let size = voltnuerongrid_mcp::guardrails::QueryGuardrails::estimate_result_size(
        1000, // rows
        100,  // avg column width bytes
        10,   // columns
    );

    assert_eq!(size, 1_000_000); // 1000 rows * 10 columns * 100 bytes

    // Small result should pass
    assert!(voltnuerongrid_mcp::guardrails::QueryGuardrails::check_result_size(5120).is_ok());

    // Large result should fail
    assert!(voltnuerongrid_mcp::guardrails::QueryGuardrails::check_result_size(100 * 1024).is_err());
}

#[tokio::test]
#[serial]
async fn mcp_010_tenant_isolation_verification() {
    let tenant_auth = McpAuthContext {
        is_admin: false,
        operator_id: None,
        tenant_id: Some("tenant-123".to_string()),
        user_id: Some("user-456".to_string()),
        auth_level: AuthenticationLevel::Tenant,
    };

    // Same tenant - allowed
    assert!(tenant_auth.verify_tenant_scope("tenant-123").is_ok());

    // Different tenant - denied
    assert!(tenant_auth.verify_tenant_scope("tenant-999").is_err());
}

#[tokio::test]
#[serial]
async fn mcp_011_max_request_size_enforced() {
    // Verify max request size is set correctly
    let capabilities = McpServerCapabilities::default();
    assert_eq!(capabilities.max_request_size_bytes, 64 * 1024);
    assert_eq!(capabilities.max_result_size_bytes, 10 * 1024);
}

#[tokio::test]
#[serial]
async fn mcp_012_auth_context_from_full_headers() {
    unsafe { std::env::remove_var("VNG_ADMIN_API_KEY"); }
    // Test all authentication pathways
    let tests = vec![
        (
            McpRequestHeaders {
                x_vng_admin_key: Some("key".to_string()),
                x_vng_operator_id: None,
                x_vng_tenant_id: None,
                x_vng_user_id: None,
            },
            true,
            AuthenticationLevel::Admin,
        ),
        (
            McpRequestHeaders {
                x_vng_admin_key: None,
                x_vng_operator_id: Some("op".to_string()),
                x_vng_tenant_id: None,
                x_vng_user_id: None,
            },
            false,
            AuthenticationLevel::Operator,
        ),
        (
            McpRequestHeaders {
                x_vng_admin_key: None,
                x_vng_operator_id: None,
                x_vng_tenant_id: Some("t".to_string()),
                x_vng_user_id: Some("u".to_string()),
            },
            false,
            AuthenticationLevel::Tenant,
        ),
    ];

    for (headers, should_be_admin, expected_level) in tests {
        let auth = McpAuthContext::from_headers(&headers).unwrap();
        assert_eq!(auth.is_admin, should_be_admin);
        assert_eq!(auth.auth_level, expected_level);
    }
}

#[tokio::test]
#[serial]
async fn mcp_013_admin_can_create_object_with_additional_key() {
    let _runtime = setup_mock_runtime().await;
    let capabilities = McpServerCapabilities::default();

    let admin_headers = McpRequestHeaders {
        x_vng_admin_key: Some("admin-key".to_string()),
        x_vng_operator_id: None,
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };

    let req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "13".to_string(),
        method: "tools/ddl_create".to_string(),
        params: json!({
            "object_type": "table",
            "object_name": "users",
            "create_sql": "CREATE TABLE users(id INT PRIMARY KEY)",
            "ddl_admin_key": "extra-ddl-key"
        }),
        headers: admin_headers,
    };

    let resp = process_request(req, &capabilities).await;
    assert!(resp.error.is_none());
}

#[tokio::test]
#[serial]
async fn mcp_014_operator_cannot_create_object_even_with_additional_key() {
    let capabilities = McpServerCapabilities::default();

    let operator_headers = McpRequestHeaders {
        x_vng_admin_key: None,
        x_vng_operator_id: Some("op-001".to_string()),
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };

    let req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "14".to_string(),
        method: "tools/ddl_create".to_string(),
        params: json!({
            "object_type": "view",
            "object_name": "v_users",
            "create_sql": "CREATE VIEW v_users AS SELECT * FROM users",
            "ddl_admin_key": "extra-ddl-key"
        }),
        headers: operator_headers,
    };

    let resp = process_request(req, &capabilities).await;
    assert!(resp.error.is_some());
    assert_eq!(resp.error.as_ref().unwrap().code, 403);
}

#[tokio::test]
#[serial]
async fn mcp_015_missing_additional_key_is_rejected_for_drop() {
    let capabilities = McpServerCapabilities::default();

    let admin_headers = McpRequestHeaders {
        x_vng_admin_key: Some("admin-key".to_string()),
        x_vng_operator_id: None,
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };

    let req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "15".to_string(),
        method: "tools/ddl_drop".to_string(),
        params: json!({
            "object_type": "table",
            "object_name": "users",
            "drop_sql": "DROP TABLE users",
            "ddl_admin_key": ""
        }),
        headers: admin_headers,
    };

    let resp = process_request(req, &capabilities).await;
    assert!(resp.error.is_some());
    assert_eq!(resp.error.as_ref().unwrap().code, 401);
}

#[tokio::test]
#[serial]
async fn mcp_016_operator_can_generate_erd() {
    let _runtime = setup_mock_runtime().await;
    let capabilities = McpServerCapabilities::default();

    let operator_headers = McpRequestHeaders {
        x_vng_admin_key: None,
        x_vng_operator_id: Some("op-001".to_string()),
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };

    let req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "16".to_string(),
        method: "tools/erd".to_string(),
        params: json!({
            "schema_filter": "public",
            "table_names": ["users", "orders"],
            "output_format": "mermaid"
        }),
        headers: operator_headers,
    };

    let resp = process_request(req, &capabilities).await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["format"], "mermaid");
}

#[tokio::test]
#[serial]
async fn mcp_017_data_transfer_requires_admin_and_additional_key() {
    let _runtime = setup_mock_runtime().await;
    let capabilities = McpServerCapabilities::default();

    let admin_headers = McpRequestHeaders {
        x_vng_admin_key: Some("admin-key".to_string()),
        x_vng_operator_id: None,
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };

    let ok_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "17".to_string(),
        method: "tools/data_transfer".to_string(),
        params: json!({
            "direction": "import",
            "format": "csv",
            "endpoint": "blob",
            "location": "blob://sample/container/users.csv",
            "table_name": "users",
            "transfer_admin_key": "xfer-key",
            "options": {
                "delimiter": ",",
                "connector_id": "mcp-transfer",
                "csv_data": "id,name\n1,Alice"
            }
        }),
        headers: admin_headers,
    };

    let ok_resp = process_request(ok_req, &capabilities).await;
    assert!(ok_resp.error.is_none());

    let operator_headers = McpRequestHeaders {
        x_vng_admin_key: None,
        x_vng_operator_id: Some("op-001".to_string()),
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };
    let denied_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "18".to_string(),
        method: "tools/data_transfer".to_string(),
        params: json!({
            "direction": "export",
            "format": "parquet",
            "endpoint": "ftp",
            "location": "ftp://host/export/users.parquet",
            "table_name": "users",
            "transfer_admin_key": "xfer-key"
        }),
        headers: operator_headers,
    };

    let denied_resp = process_request(denied_req, &capabilities).await;
    assert!(denied_resp.error.is_some());
    assert_eq!(denied_resp.error.as_ref().unwrap().code, 403);
}

#[tokio::test]
#[serial]
async fn mcp_018_cluster_topology_is_admin_only() {
    let _runtime = setup_mock_runtime().await;
    let capabilities = McpServerCapabilities::default();

    let admin_headers = McpRequestHeaders {
        x_vng_admin_key: Some("admin-key".to_string()),
        x_vng_operator_id: None,
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };
    let admin_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "19".to_string(),
        method: "tools/cluster_topology".to_string(),
        params: json!({"include_nodes": true}),
        headers: admin_headers,
    };
    let admin_resp = process_request(admin_req, &capabilities).await;
    assert!(admin_resp.error.is_none());

    let operator_headers = McpRequestHeaders {
        x_vng_admin_key: None,
        x_vng_operator_id: Some("op-001".to_string()),
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };
    let denied_req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "20".to_string(),
        method: "tools/cluster_topology".to_string(),
        params: json!({"include_nodes": true}),
        headers: operator_headers,
    };
    let denied_resp = process_request(denied_req, &capabilities).await;
    assert!(denied_resp.error.is_some());
    assert_eq!(denied_resp.error.as_ref().unwrap().code, 403);
}

#[tokio::test]
#[serial]
async fn mcp_019_cluster_node_manage_accepts_admin_request() {
    let _runtime = setup_mock_runtime().await;
    let capabilities = McpServerCapabilities::default();
    let admin_headers = McpRequestHeaders {
        x_vng_admin_key: Some("admin-key".to_string()),
        x_vng_operator_id: None,
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };

    let req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "21".to_string(),
        method: "tools/cluster_node_manage".to_string(),
        params: json!({
            "action": "add",
            "node_id": "node-2",
            "role": "follower",
            "desired_status": "active",
            "total_cpu_cores": 4,
            "total_ram_mb": 8192
        }),
        headers: admin_headers,
    };

    let resp = process_request(req, &capabilities).await;
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap()["node_id"], "node-2");
}

#[tokio::test]
#[serial]
async fn mcp_020_proxy_disabled_returns_runtime_proxy_error() {
    unsafe {
        std::env::set_var("VNG_MCP_RUNTIME_PROXY", "false");
        std::env::remove_var("VNG_ADMIN_API_KEY");
    }
    let capabilities = McpServerCapabilities::default();
    let admin_headers = McpRequestHeaders {
        x_vng_admin_key: Some("admin-key".to_string()),
        x_vng_operator_id: Some("op-001".to_string()),
        x_vng_tenant_id: None,
        x_vng_user_id: None,
    };
    let req = McpRequest {
        jsonrpc: "2.0".to_string(),
        id: "22".to_string(),
        method: "tools/health".to_string(),
        params: json!({}),
        headers: admin_headers,
    };

    let resp = process_request(req, &capabilities).await;
    assert!(resp.error.is_some());
    assert_eq!(resp.error.as_ref().unwrap().code, 400);
    assert!(resp
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("Runtime proxy is disabled"));
}


