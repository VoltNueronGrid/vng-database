"""
Fix 3 remaining issues in main.rs:
1. Add oltp_rows definition (missing variable)
2. Fix broken audit test + embedded WAL/chaos tests
3. Fix handler return types (impl IntoResponse -> explicit types for testability)
"""
path = r"D:\by\polap-db\services\voltnuerongridd\src\main.rs"
with open(path, "r", encoding="utf-8") as f:
    content = f.read()

# ─── Fix 1: Add oltp_rows definition ────────────────────────────────────────
# Insert after the planner_path closing `};` and before the S3-WS1-05 OLAP comment.
OLTP_ROWS_ANCHOR = (
    "        dominant\n"
    "    };\n"
    "\n"
    "    // S3-WS1-05: vectorized OLAP executor dispatch"
)
OLTP_ROWS_REPLACEMENT = (
    "        dominant\n"
    "    };\n"
    "\n"
    "    // S4-WS3-02: OLTP physical executor dispatch — if planner says oltp, run point SELECT.\n"
    "    let oltp_rows: Option<Vec<OltpRowResult>> =\n"
    "        if planner_path.as_deref() == Some(\"oltp\") && !olap_statements.is_empty() {\n"
    "            let rs = state.row_store.lock().expect(\"row_store lock oltp select\");\n"
    "            let limit = req.max_rows.unwrap_or(10_000).min(100_000);\n"
    "            let rows = execute_oltp_select(&olap_statements, &rs, limit);\n"
    "            if rows.is_empty() { None } else { Some(rows) }\n"
    "        } else {\n"
    "            None\n"
    "        };\n"
    "\n"
    "    // S3-WS1-05: vectorized OLAP executor dispatch"
)
assert OLTP_ROWS_ANCHOR in content, "FIX1: anchor not found"
content = content.replace(OLTP_ROWS_ANCHOR, OLTP_ROWS_REPLACEMENT, 1)
print("Fix 1 (oltp_rows) OK")

# ─── Fix 2: change handler return types to explicit for testability ──────────
# wal_status: impl IntoResponse -> (StatusCode, Json<WalStatusResponse>)
content = content.replace(
    "async fn wal_status(State(state): State<AppState>) -> impl axum::response::IntoResponse {\n"
    "    use axum::{Json as AJ, http::StatusCode};",
    "async fn wal_status(State(state): State<AppState>) -> (StatusCode, Json<WalStatusResponse>) {\n"
    "    use axum::Json as AJ;",
    1
)
content = content.replace(
    "    (StatusCode::OK, AJ(WalStatusResponse {\n"
    "        status: \"ok\",\n"
    "        wal_len,\n"
    "        latest_sequence: latest_seq,\n"
    "        checkpoint_count,\n"
    "    }))\n"
    "}\n"
    "\n"
    "/// S2-WS2-02: replay WAL records",
    "    (StatusCode::OK, Json(WalStatusResponse {\n"
    "        status: \"ok\",\n"
    "        wal_len,\n"
    "        latest_sequence: latest_seq,\n"
    "        checkpoint_count,\n"
    "    }))\n"
    "}\n"
    "\n"
    "/// S2-WS2-02: replay WAL records",
    1
)

# wal_recover: impl IntoResponse -> (StatusCode, Json<WalRecoverResponse>)
content = content.replace(
    ") -> impl axum::response::IntoResponse {\n"
    "    use axum::{Json as AJ, http::StatusCode};\n"
    "    let dry_run = req.dry_run.unwrap_or(false);\n"
    "    let wal = state.wal_engine.lock().expect(\"wal_engine lock\");\n"
    "    let records = wal.wal_records().to_vec();\n"
    "    drop(wal);",
    ") -> (StatusCode, Json<WalRecoverResponse>) {\n"
    "    let dry_run = req.dry_run.unwrap_or(false);\n"
    "    let wal = state.wal_engine.lock().expect(\"wal_engine lock\");\n"
    "    let records = wal.wal_records().to_vec();\n"
    "    drop(wal);",
    1
)
content = content.replace(
    "    (StatusCode::OK, AJ(WalRecoverResponse {\n"
    "        status: \"ok\",\n"
    "        records_replayed: replayed,\n"
    "        dry_run,\n"
    "    }))\n"
    "}\n"
    "\n"
    "// \u2500\u2500\u2500 S7-WS6-04: Chaos",
    "    (StatusCode::OK, Json(WalRecoverResponse {\n"
    "        status: \"ok\",\n"
    "        records_replayed: replayed,\n"
    "        dry_run,\n"
    "    }))\n"
    "}\n"
    "\n"
    "// \u2500\u2500\u2500 S7-WS6-04: Chaos",
    1
)

# chaos_status: impl IntoResponse -> (StatusCode, Json<ChaosStatusResponse>)
content = content.replace(
    "async fn chaos_status(State(state): State<AppState>) -> impl axum::response::IntoResponse {\n"
    "    use axum::{Json as AJ, http::StatusCode};",
    "async fn chaos_status(State(state): State<AppState>) -> (StatusCode, Json<ChaosStatusResponse>) {",
    1
)
content = content.replace(
    "    (StatusCode::OK, AJ(ChaosStatusResponse {\n"
    "        status: \"ok\",\n"
    "        active_fault_count,\n"
    "        total_injected,\n"
    "        active_faults,\n"
    "    }))\n"
    "}",
    "    (StatusCode::OK, Json(ChaosStatusResponse {\n"
    "        status: \"ok\",\n"
    "        active_fault_count,\n"
    "        total_injected,\n"
    "        active_faults,\n"
    "    }))\n"
    "}",
    1
)
print("Fix 2 (return types) OK")

# ─── Fix 3: Fix broken audit test and embedded WAL/chaos/OLAP tests ──────────
# The broken section starts with the incomplete sink.append( and ends after
# the WAL/chaos/OLAP tests, before the audit export test completion.
BROKEN_AUDIT_START = (
    "        {\n"
    "            let mut sink = state.audit_sink.lock().unwrap();\n"
    "            sink.append(\n"
    "\n"
    "    // \u2500\u2500\u2500 S2-WS2-02: WAL durability + recovery integration tests \u2500\u2500\u2500\u2500\u2500\u2500"
)
idx_broken = content.find(BROKEN_AUDIT_START)
if idx_broken == -1:
    print("ERROR: broken audit start not found")
    exit(1)

# Find the end: the line just before the audit export test completion
# The WAL/chaos/OLAP tests end and then we see the continuation of the audit test
AUDIT_CONTINUE = (
    "                voltnuerongrid_audit::AuditEventKind::Sql,\n"
    "                \"test-actor\",\n"
    "                \"test-action\","
)
idx_continue = content.find(AUDIT_CONTINUE, idx_broken)
if idx_continue == -1:
    print("ERROR: audit continuation not found")
    exit(1)

# Find the close of the audit test (the `}` after audit_log_path assertion)
AUDIT_END = (
    "        assert!(resp.1.0.audit_log_path.is_none());\n"
    "    }\n"
    "\n"
    "    // \u2500\u2500\u2500 S7-WS6-02: Raft consensus"
)
idx_audit_end = content.find(AUDIT_END, idx_continue)
if idx_audit_end == -1:
    print("ERROR: audit test end not found")
    exit(1)

# The OLD broken section from after the sink.append( through the end of the audit test
old_broken = content[idx_broken : idx_audit_end + len(AUDIT_END)]
print(f"Broken section length: {len(old_broken)}")
print(f"First 80 chars: {repr(old_broken[:80])}")
print(f"Last 80 chars: {repr(old_broken[-80:])}")
