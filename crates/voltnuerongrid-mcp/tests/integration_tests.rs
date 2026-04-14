/// Integration tests for VoltNueronGrid MCP Server
/// Tests the full end-to-end flow including authentication, authorization, guardrails, and tool execution

use serde_json::json;
use voltnuerongrid_mcp::{
    McpRequest, McpRequestHeaders, McpServerCapabilities, process_request,
    auth::{McpAuthContext, AuthenticationLevel},
    tools::QueryToolRequest,
    guardrails::QueryGuardrails,
};

#[tokio::test]
async fn mcp_001_admin_can_execute_all_tools() {
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
async fn mcp_002_operator_can_execute_operator_tools_not_admin() {
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
async fn mcp_005_unknown_method_returns_400() {
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
async fn mcp_007_admin_auth_precedence() {
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
async fn mcp_011_max_request_size_enforced() {
    // Verify max request size is set correctly
    let capabilities = McpServerCapabilities::default();
    assert_eq!(capabilities.max_request_size_bytes, 64 * 1024);
    assert_eq!(capabilities.max_result_size_bytes, 10 * 1024);
}

#[tokio::test]
async fn mcp_012_auth_context_from_full_headers() {
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
