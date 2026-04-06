"""
Fix remaining issues in main.rs:
1. Fix chaos_inject/chaos_clear return types for testability
2. Fix broken audit test body + properly place new tests
"""
path = r"D:\by\polap-db\services\voltnuerongridd\src\main.rs"
with open(path, "r", encoding="utf-8") as f:
    content = f.read()

# ─── Fix chaos_inject return type ─────────────────────────────────────────────
content = content.replace(
    "async fn chaos_inject(\n"
    "    State(state): State<AppState>,\n"
    "    axum::extract::Json(req): axum::extract::Json<ChaosInjectRequest>,\n"
    ") -> impl axum::response::IntoResponse {\n"
    "    use axum::{Json as AJ, http::StatusCode};",
    "async fn chaos_inject(\n"
    "    State(state): State<AppState>,\n"
    "    axum::extract::Json(req): axum::extract::Json<ChaosInjectRequest>,\n"
    ") -> (StatusCode, Json<serde_json::Value>) {",
    1
)
content = content.replace(
    "    (StatusCode::OK, AJ(serde_json::json!({ \"status\": \"injected\", \"active_fault_count\": count })))\n"
    "}\n"
    "\n"
    "/// S7-WS6-04: clear all active faults",
    "    (StatusCode::OK, Json(serde_json::json!({ \"status\": \"injected\", \"active_fault_count\": count })))\n"
    "}\n"
    "\n"
    "/// S7-WS6-04: clear all active faults",
    1
)

# ─── Fix chaos_clear return type ──────────────────────────────────────────────
content = content.replace(
    "async fn chaos_clear(State(state): State<AppState>) -> impl axum::response::IntoResponse {\n"
    "    use axum::{Json as AJ, http::StatusCode};",
    "async fn chaos_clear(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {",
    1
)
content = content.replace(
    "    (StatusCode::OK, AJ(serde_json::json!({ \"status\": \"cleared\", \"history_len\": history_len })))\n"
    "}\n"
    "\n"
    "/// S7-WS6-04: return current chaos",
    "    (StatusCode::OK, Json(serde_json::json!({ \"status\": \"cleared\", \"history_len\": history_len })))\n"
    "}\n"
    "\n"
    "/// S7-WS6-04: return current chaos",
    1
)

# Also fix wal_status return type (already changed to (StatusCode, Json<WalStatusResponse>)
# but uses `AJ` alias - need to find and fix
content = content.replace(
    "async fn wal_status(State(state): State<AppState>) -> (StatusCode, Json<WalStatusResponse>) {\n"
    "    use axum::Json as AJ;",
    "async fn wal_status(State(state): State<AppState>) -> (StatusCode, Json<WalStatusResponse>) {",
    1
)
print("Return types fixed")

# ─── Fix broken audit test section ────────────────────────────────────────────
BROKEN_START = (
    "            sink.append(\n"
    "\n"
    "    // \u2500\u2500\u2500 S2-WS2-02: WAL durability + recovery integration tests \u2500\u2500\u2500\u2500\u2500\u2500"
)
PROPER_CONTINUATION = (
    "                voltnuerongrid_audit::AuditEventKind::Sql,\n"
    "                \"test-actor\",\n"
    "                \"test-action\",\n"
    "                \"ok\",\n"
    "                \"{}\",\n"
    "            );\n"
    "            sink.append(\n"
    "                voltnuerongrid_audit::AuditEventKind::Security,\n"
    "                \"test-actor\",\n"
    "                \"test-security-action\",\n"
    "                \"ok\",\n"
    "                \"{}\",\n"
    "            );\n"
    "        }\n"
    "        let resp = audit_export(State(state.clone()), headers).await.unwrap();\n"
    "        // At least the 2 events we manually appended\n"
    "        assert!(resp.1.0.event_count >= 2);\n"
    "        assert!(!resp.1.0.file_backed); // no VNG_AUDIT_LOG_PATH set in test\n"
    "        assert!(resp.1.0.audit_log_path.is_none());\n"
    "    }\n"
    "\n"
    "    // \u2500\u2500\u2500 S7-WS6-02: Raft consensus"
)

idx_broken = content.find(BROKEN_START)
idx_continuation = content.find(PROPER_CONTINUATION, idx_broken)

if idx_broken == -1:
    print("ERROR: broken start not found")
    exit(1)
if idx_continuation == -1:
    print("ERROR: proper continuation not found")
    exit(1)

# Extract the embedded WAL/chaos/OLAP tests (between broken start and continuation)
embedded_section = content[idx_broken + len(BROKEN_START) : idx_continuation]
print(f"Embedded tests section length: {len(embedded_section)}")

# The new correct audit test body + the extracted new tests
# Note: embedded_section contains the WAL/chaos/OLAP tests but with wrong handler signatures
# We need to rewrite them with correct signatures (no headers arg)

NEW_TESTS = (
    "    // \u2500\u2500\u2500 S2-WS2-02: WAL durability + recovery integration tests \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\n"
    "\n"
    "    #[tokio::test]\n"
    "    async fn s2_ws2_02_wal_status_returns_zero_on_fresh_state() {\n"
    "        let state = state_with_key(Some(\"test-key\"));\n"
    "        let (status, Json(body)) = wal_status(State(state)).await;\n"
    "        assert_eq!(status, StatusCode::OK);\n"
    "        assert_eq!(body.wal_len, 0);\n"
    "        assert_eq!(body.latest_sequence, 0);\n"
    "    }\n"
    "\n"
    "    #[tokio::test]\n"
    "    async fn s2_ws2_02_commit_writes_wal_records() {\n"
    "        let state = state_with_key(Some(\"test-key\"));\n"
    "        let headers = operator_headers(\"test-key\", \"admin\");\n"
    "        let tx_req = SqlTransactionRequest {\n"
    "            statements: vec![\n"
    "                \"BEGIN\".to_string(),\n"
    "                \"INSERT INTO items (id, name) VALUES ('item:1', 'alpha')\".to_string(),\n"
    "                \"INSERT INTO items (id, name) VALUES ('item:2', 'beta')\".to_string(),\n"
    "                \"COMMIT\".to_string(),\n"
    "            ],\n"
    "            isolation_level: None,\n"
    "        };\n"
    "        sql_transaction(State(state.clone()), headers, Json(tx_req)).await.ok();\n"
    "        let (status, Json(body)) = wal_status(State(state)).await;\n"
    "        assert_eq!(status, StatusCode::OK);\n"
    "        assert!(body.wal_len >= 2, \"WAL should have at least 2 records after COMMIT\");\n"
    "    }\n"
    "\n"
    "    #[tokio::test]\n"
    "    async fn s2_ws2_02_wal_recover_dry_run_does_not_change_row_store() {\n"
    "        let state = state_with_key(Some(\"test-key\"));\n"
    "        let headers = operator_headers(\"test-key\", \"admin\");\n"
    "        let tx_req = SqlTransactionRequest {\n"
    "            statements: vec![\n"
    "                \"BEGIN\".to_string(),\n"
    "                \"INSERT INTO orders (id, total) VALUES ('ord:1', '99')\".to_string(),\n"
    "                \"COMMIT\".to_string(),\n"
    "            ],\n"
    "            isolation_level: None,\n"
    "        };\n"
    "        sql_transaction(State(state.clone()), headers, Json(tx_req)).await.ok();\n"
    "        let rows_before = state.row_store.lock().unwrap().visible_row_count();\n"
    "        let recover_req = WalRecoverRequest { dry_run: Some(true) };\n"
    "        let (_, Json(body)) = wal_recover(\n"
    "            State(state.clone()),\n"
    "            axum::extract::Json(recover_req),\n"
    "        ).await;\n"
    "        assert!(body.dry_run);\n"
    "        assert!(body.records_replayed >= 1);\n"
    "        let rows_after = state.row_store.lock().unwrap().visible_row_count();\n"
    "        assert_eq!(rows_before, rows_after, \"dry_run must not modify row store\");\n"
    "    }\n"
    "\n"
    "    // \u2500\u2500\u2500 S7-WS6-04: Chaos injection integration tests \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\n"
    "\n"
    "    #[tokio::test]\n"
    "    async fn s7_ws6_04_chaos_status_returns_empty_initially() {\n"
    "        let state = state_with_key(Some(\"test-key\"));\n"
    "        let (_, Json(body)) = chaos_status(State(state)).await;\n"
    "        assert_eq!(body.active_fault_count, 0);\n"
    "        assert_eq!(body.total_injected, 0);\n"
    "    }\n"
    "\n"
    "    #[tokio::test]\n"
    "    async fn s7_ws6_04_chaos_inject_records_active_fault() {\n"
    "        let state = state_with_key(Some(\"test-key\"));\n"
    "        let body = ChaosInjectRequest {\n"
    "            fault_type: \"network_partition\".to_string(),\n"
    "            target_node: Some(\"node-2\".to_string()),\n"
    "            parameters: [(\"loss_pct\".to_string(), \"50\".to_string())].into_iter().collect(),\n"
    "        };\n"
    "        let (ok_status, _) = chaos_inject(State(state.clone()), axum::extract::Json(body)).await;\n"
    "        assert_eq!(ok_status, StatusCode::OK);\n"
    "        let (_, Json(status)) = chaos_status(State(state)).await;\n"
    "        assert_eq!(status.active_fault_count, 1);\n"
    "        assert_eq!(status.total_injected, 1);\n"
    "        assert_eq!(status.active_faults[0].fault_type, \"network_partition\");\n"
    "    }\n"
    "\n"
    "    #[tokio::test]\n"
    "    async fn s7_ws6_04_chaos_clear_removes_active_faults() {\n"
    "        let state = state_with_key(Some(\"test-key\"));\n"
    "        for fault in [\"node_crash\", \"packet_loss\"] {\n"
    "            let body = ChaosInjectRequest {\n"
    "                fault_type: fault.to_string(),\n"
    "                target_node: None,\n"
    "                parameters: HashMap::new(),\n"
    "            };\n"
    "            chaos_inject(State(state.clone()), axum::extract::Json(body)).await;\n"
    "        }\n"
    "        let (_, Json(before)) = chaos_status(State(state.clone())).await;\n"
    "        assert_eq!(before.active_fault_count, 2);\n"
    "        chaos_clear(State(state.clone())).await;\n"
    "        let (_, Json(after)) = chaos_status(State(state)).await;\n"
    "        assert_eq!(after.active_fault_count, 0, \"active faults should be cleared\");\n"
    "        assert_eq!(after.total_injected, 2, \"history should be preserved\");\n"
    "    }\n"
    "\n"
    "    // \u2500\u2500\u2500 S3-WS1-05 + S4-WS3-03: planner filter pushdown integration tests \u2500\u2500\u2500\u2500\n"
    "\n"
    "    #[tokio::test]\n"
    "    async fn s3_ws1_05_olap_filter_pushdown_reduces_batch() {\n"
    "        let state = state_with_key(Some(\"test-key\"));\n"
    "        let headers = operator_headers(\"test-key\", \"admin\");\n"
    "        let tx_req = SqlTransactionRequest {\n"
    "            statements: vec![\n"
    "                \"BEGIN\".to_string(),\n"
    "                \"INSERT INTO products (id, category) VALUES ('p:1', 'electronics')\".to_string(),\n"
    "                \"INSERT INTO products (id, category) VALUES ('p:2', 'books')\".to_string(),\n"
    "                \"INSERT INTO products (id, category) VALUES ('p:3', 'electronics')\".to_string(),\n"
    "                \"COMMIT\".to_string(),\n"
    "            ],\n"
    "            isolation_level: None,\n"
    "        };\n"
    "        sql_transaction(State(state.clone()), headers.clone(), Json(tx_req)).await.ok();\n"
    "        let exec_req = SqlExecuteRequest {\n"
    "            sql_batch: \"SELECT COUNT(*) FROM products GROUP BY category\".to_string(),\n"
    "            max_rows: None,\n"
    "        };\n"
    "        let resp = sql_execute(State(state), headers, Json(exec_req)).await.unwrap();\n"
    "        assert_eq!(resp.1.0.planner_path.as_deref(), Some(\"olap\"));\n"
    "        assert!(resp.1.0.olap_agg_results.is_some());\n"
    "    }\n"
)

# Build the replacement for the broken section
OLD_BROKEN = content[idx_broken : idx_continuation]
NEW_FIXED = (
    "            sink.append(\n"
    + PROPER_CONTINUATION[:PROPER_CONTINUATION.find("    // \u2500\u2500\u2500 S7-WS6-02: Raft")]
    + "\n"
    + NEW_TESTS
    + "\n"
    + "    // \u2500\u2500\u2500 S7-WS6-02: Raft consensus"
)

assert OLD_BROKEN in content, "ASSERT FAILED: old broken section not found"
content = content.replace(OLD_BROKEN, NEW_FIXED, 1)

with open(path, "w", encoding="utf-8") as f:
    f.write(content)
print("All fixes applied and file written")
