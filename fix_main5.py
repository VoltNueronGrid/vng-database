"""
Fix broken audit test section with correct boundaries.
"""
path = r"D:\by\polap-db\services\voltnuerongridd\src\main.rs"
with open(path, "r", encoding="utf-8") as f:
    content = f.read()

# The broken section starts with the incomplete sink.append(
# and ends at (and includes) the "// ─── S7-WS6-02: Raft consensus" that is
# embedded in the middle of the content (from the PROPER_CONTINUATION trailing bit)
# We need to replace everything from the broken `sink.append(` through the end
# of the duplicate `// ─── S7-WS6-02: Raft consensus ──────` line.

BROKEN_SINK_APPEND = "            sink.append(\n\n    // \u2500\u2500\u2500 S2-WS2-02: WAL durability"
RAFT_SECTION_START = "    // \u2500\u2500\u2500 S7-WS6-02: Raft consensus \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500"

idx_broken = content.find(BROKEN_SINK_APPEND)
if idx_broken == -1:
    print("ERROR: broken start not found")
    # Check for the ─── format
    idx_check = content.find("sink.append(\n\n    //")
    print(f"Alternate check at: {idx_check}")
    print(repr(content[idx_check:idx_check+100]))
    exit(1)

idx_raft_second = content.find(RAFT_SECTION_START, idx_broken)
if idx_raft_second == -1:
    print("ERROR: second Raft section not found")
    # Find both occurrences
    idx_raft1 = content.find("// \u2500\u2500\u2500 S7-WS6-02: Raft")
    idx_raft2 = content.find("// \u2500\u2500\u2500 S7-WS6-02: Raft", idx_raft1 + 10)
    print(f"First Raft at: {idx_raft1}, Second at: {idx_raft2}")
    exit(1)

print(f"Broken section: {idx_broken} to {idx_raft_second + len(RAFT_SECTION_START)}")
old_section = content[idx_broken : idx_raft_second + len(RAFT_SECTION_START)]
print(f"Old section length: {len(old_section)}")
print(f"First 80: {repr(old_section[:80])}")
print(f"Last 80: {repr(old_section[-80:])}")

NEW_SECTION = (
    "            sink.append(\n"
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
    "\n"
    "    // \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\n"
    "    // \u2500\u2500\u2500 S7-WS6-02: Raft consensus \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500"
)

assert old_section in content, "ASSERT FAILED: old section not in content"
content = content.replace(old_section, NEW_SECTION, 1)

with open(path, "w", encoding="utf-8") as f:
    f.write(content)
print("Done - replaced broken section and duplicate Raft header")
