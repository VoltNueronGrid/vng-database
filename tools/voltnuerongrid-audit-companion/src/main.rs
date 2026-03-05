fn main() {
    let mut sink = voltnuerongrid_audit::AppendOnlyAuditSink::new();
    let sample = sink.append(
        voltnuerongrid_audit::AuditEventKind::Security,
        "audit-companion",
        "bootstrap_healthcheck",
        "ok",
        "{\"note\":\"ws8a baseline\"}",
    );
    println!(
        "voltnuerongrid-audit-companion baseline ready: last_event_id={}",
        sample.event_id
    );
}
