"""Remove stale duplicate audit test section."""
path = r"D:\by\polap-db\services\voltnuerongridd\src\main.rs"
with open(path, "r", encoding="utf-8") as f:
    content = f.read()

# The stale duplicate section to remove
OLD = (
    "\n"
    "    // \u2500\u2500\u2500 S7-WS6-02: Raft consensus                voltnuerongrid_audit::AuditEventKind::Sql,\n"
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
    "    // \u2500\u2500\u2500 S7-WS6-02: Raft consensus \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500"
)

NEW = (
    "\n"
    "    // \u2500\u2500\u2500 S7-WS6-02: Raft consensus \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500"
)

if OLD not in content:
    print("ERROR: old section not found, checking...")
    idx = content.find("// \u2500\u2500\u2500 S7-WS6-02: Raft consensus                voltnuerongrid_audit")
    print(f"Alternate check: {idx}")
    if idx != -1:
        print(repr(content[idx:idx+100]))
    exit(1)

content = content.replace(OLD, NEW, 1)
with open(path, "w", encoding="utf-8") as f:
    f.write(content)
print("Done - removed duplicate audit test stale section")
