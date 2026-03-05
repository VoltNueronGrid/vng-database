#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::env;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use voltnuerongrid_ai::AutonomousActionExecutionRecord;
use voltnuerongrid_audit::AuditEvent;

#[derive(Debug)]
struct CliArgs {
    audit_file: String,
    action_file: String,
    out_file: String,
    trace_id_filter: Option<String>,
    action_filter: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("audit-companion error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let audit_events = load_audit_events(&args.audit_file)?;
    let action_records = load_action_records(&args.action_file)?;

    let filtered_audit = filter_audit_events(
        &audit_events,
        args.trace_id_filter.as_deref(),
        args.action_filter.as_deref(),
    );
    let filtered_actions = filter_action_records(
        &action_records,
        args.trace_id_filter.as_deref(),
        args.action_filter.as_deref(),
    );

    let linked_trace_matches = count_linked_trace_matches(&filtered_audit, &filtered_actions);

    let report = serde_json::json!({
        "status": "ok",
        "generated_epoch_ms": now_epoch_millis(),
        "trace_id_filter": args.trace_id_filter,
        "action_filter": args.action_filter,
        "total_audit_events": filtered_audit.len(),
        "total_action_records": filtered_actions.len(),
        "linked_trace_matches": linked_trace_matches,
        "audit_events": filtered_audit,
        "action_records": filtered_actions,
    });

    if let Some(parent) = std::path::Path::new(&args.out_file).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("create output dir failed: {e}"))?;
        }
    }
    let serialized = serde_json::to_string_pretty(&report)
        .map_err(|e| format!("serialize report failed: {e}"))?;
    fs::write(&args.out_file, serialized).map_err(|e| format!("write report failed: {e}"))?;

    println!("audit companion report written: {}", args.out_file);
    Ok(())
}

fn parse_args() -> Result<CliArgs, String> {
    let mut audit_file = None;
    let mut action_file = None;
    let mut out_file = Some("tests/kpi/results/ws8a/audit-companion-report.json".to_string());
    let mut trace_id_filter = None;
    let mut action_filter = None;

    let mut iter = env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--audit-file" => audit_file = iter.next(),
            "--action-file" => action_file = iter.next(),
            "--out" => out_file = iter.next(),
            "--trace-id" => trace_id_filter = iter.next(),
            "--action" => action_filter = iter.next(),
            "--help" | "-h" => {
                return Err("usage: --audit-file <path> --action-file <path> [--out <path>] [--trace-id <id>] [--action <name>]".to_string());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(CliArgs {
        audit_file: audit_file.ok_or_else(|| "--audit-file is required".to_string())?,
        action_file: action_file.ok_or_else(|| "--action-file is required".to_string())?,
        out_file: out_file.unwrap_or_else(|| "audit-companion-report.json".to_string()),
        trace_id_filter,
        action_filter,
    })
}

fn load_audit_events(path: &str) -> Result<Vec<AuditEvent>, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("read audit file failed: {e}"))?;
    serde_json::from_str::<Vec<AuditEvent>>(&content)
        .map_err(|e| format!("parse audit file failed: {e}"))
}

fn load_action_records(path: &str) -> Result<Vec<AutonomousActionExecutionRecord>, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("read action file failed: {e}"))?;
    serde_json::from_str::<Vec<AutonomousActionExecutionRecord>>(&content)
        .map_err(|e| format!("parse action file failed: {e}"))
}

fn filter_action_records(
    records: &[AutonomousActionExecutionRecord],
    trace_id_filter: Option<&str>,
    action_filter: Option<&str>,
) -> Vec<AutonomousActionExecutionRecord> {
    records
        .iter()
        .filter(|record| {
            let trace_matches = trace_id_filter
                .map(|trace_id| record.trace_id == trace_id)
                .unwrap_or(true);
            let action_matches = action_filter
                .map(|action| record.action.eq_ignore_ascii_case(action))
                .unwrap_or(true);
            trace_matches && action_matches
        })
        .cloned()
        .collect()
}

fn filter_audit_events(
    events: &[AuditEvent],
    trace_id_filter: Option<&str>,
    action_filter: Option<&str>,
) -> Vec<AuditEvent> {
    events
        .iter()
        .filter(|event| {
            let trace_matches = trace_id_filter
                .map(|trace_id| event_trace_id(event).as_deref() == Some(trace_id))
                .unwrap_or(true);
            let action_matches = action_filter
                .map(|action| event.action.eq_ignore_ascii_case(action))
                .unwrap_or(true);
            trace_matches && action_matches
        })
        .cloned()
        .collect()
}

fn count_linked_trace_matches(
    audit_events: &[AuditEvent],
    action_records: &[AutonomousActionExecutionRecord],
) -> usize {
    let action_traces: HashSet<String> = action_records.iter().map(|r| r.trace_id.clone()).collect();
    audit_events
        .iter()
        .filter(|event| {
            event_trace_id(event)
                .map(|trace_id| action_traces.contains(&trace_id))
                .unwrap_or(false)
        })
        .count()
}

fn event_trace_id(event: &AuditEvent) -> Option<String> {
    let parsed = serde_json::from_str::<serde_json::Value>(&event.details_json).ok()?;
    parsed
        .get("trace_id")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
}

fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_millis()
}
