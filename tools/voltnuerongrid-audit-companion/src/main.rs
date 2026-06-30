#![forbid(unsafe_code)]

//! Audit Companion Tool (Tasks-7 A-9).
//!
//! Operator-facing CLI over the runtime audit trail. Two usage modes:
//!
//! Subcommand mode (A-9):
//!   audit-companion list   --audit-file <path> [--action <name>] [--limit N]
//!   audit-companion verify --audit-file <path>
//!   audit-companion export --audit-file <path> --out-dir <dir>
//!
//! `--audit-file` may be a local JSON array of `AuditEvent` OR a runtime API
//! URL (http/https) — e.g. `http://127.0.0.1:8080/api/v1/audit/export` — which is
//! fetched live. `verify` surfaces the exact tamper point. `export` writes a
//! portable evidence bundle (`events.jsonl` + `manifest.json`).
//!
//! Legacy report mode (back-compat) is used when no subcommand is given and
//! both `--audit-file` and `--action-file` are supplied.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use voltnuerongrid_ai::AutonomousActionExecutionRecord;
use voltnuerongrid_audit::{AppendOnlyAuditSink, AuditEvent};

fn main() {
    if let Err(error) = run() {
        eprintln!("audit-companion error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let first = args.next();
    match first.as_deref() {
        Some("list") => cmd_list(args.collect()),
        Some("verify") => cmd_verify(args.collect()),
        Some("export") => cmd_export(args.collect()),
        Some("--help") | Some("-h") | None => {
            print_help();
            Ok(())
        }
        // Legacy report mode: reconstruct the original arg list and run it.
        Some(other) => {
            let mut rest: Vec<String> = vec![other.to_string()];
            rest.extend(args);
            legacy_report(rest)
        }
    }
}

fn print_help() {
    println!(
        "audit-companion — VoltNueronGrid audit evidence tool\n\n\
         USAGE:\n  \
         audit-companion list   --audit-file <path|url> [--action <name>] [--limit N]\n  \
         audit-companion verify --audit-file <path|url>\n  \
         audit-companion export --audit-file <path|url> --out-dir <dir>\n\n\
         --audit-file accepts a local JSON file OR an http(s) runtime API URL\n  \
         (e.g. http://127.0.0.1:8080/api/v1/audit/export)."
    );
}

// ── Shared helpers ───────────────────────────────────────────────────────────

/// Load audit events from a local file path or an http(s) runtime API URL.
fn load_events(source: &str) -> Result<Vec<AuditEvent>, String> {
    let content = if source.starts_with("http://") || source.starts_with("https://") {
        fetch_url(source)?
    } else {
        fs::read_to_string(source).map_err(|e| format!("read audit source failed: {e}"))?
    };
    parse_events(&content)
}

/// Parse audit events from either a bare JSON array or an API response object
/// that wraps the events under an `events` field.
fn parse_events(content: &str) -> Result<Vec<AuditEvent>, String> {
    if let Ok(events) = serde_json::from_str::<Vec<AuditEvent>>(content) {
        return Ok(events);
    }
    let value: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("parse audit source failed: {e}"))?;
    if let Some(arr) = value.get("events") {
        return serde_json::from_value::<Vec<AuditEvent>>(arr.clone())
            .map_err(|e| format!("parse events field failed: {e}"));
    }
    Err("audit source did not contain an array of events".to_string())
}

/// Minimal http(s) GET. Uses reqwest's blocking client (a workspace dependency).
fn fetch_url(url: &str) -> Result<String, String> {
    let resp = reqwest::blocking::get(url).map_err(|e| format!("http get failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("http status {}", resp.status()));
    }
    resp.text().map_err(|e| format!("read body failed: {e}"))
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_millis()
}

// ── Subcommands ──────────────────────────────────────────────────────────────

fn cmd_list(args: Vec<String>) -> Result<(), String> {
    let source = flag(&args, "--audit-file").ok_or("--audit-file is required")?;
    let action_filter = flag(&args, "--action");
    let limit: usize = flag(&args, "--limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);

    let events = load_events(source)?;
    let filtered: Vec<&AuditEvent> = events
        .iter()
        .filter(|e| action_filter.map(|a| e.action.eq_ignore_ascii_case(a)).unwrap_or(true))
        .take(limit)
        .collect();

    println!("event_id  kind            action                          outcome");
    println!("--------  --------------  ------------------------------  -------");
    for e in &filtered {
        println!(
            "{:<8}  {:<14}  {:<30}  {}",
            e.event_id,
            format!("{:?}", e.kind),
            truncate(&e.action, 30),
            e.outcome
        );
    }
    println!("\n{} event(s) listed", filtered.len());
    Ok(())
}

fn cmd_verify(args: Vec<String>) -> Result<(), String> {
    let source = flag(&args, "--audit-file").ok_or("--audit-file is required")?;
    let events = load_events(source)?;
    match AppendOnlyAuditSink::verify_chain_tamper_point(&events) {
        None => {
            println!("chain_valid: true ({} events verified)", events.len());
            Ok(())
        }
        Some(event_id) => {
            // Non-zero exit so scripts can detect tampering.
            eprintln!(
                "chain_valid: false — TAMPER DETECTED at event_id {event_id} (of {} events)",
                events.len()
            );
            std::process::exit(2);
        }
    }
}

fn cmd_export(args: Vec<String>) -> Result<(), String> {
    let source = flag(&args, "--audit-file").ok_or("--audit-file is required")?;
    let out_dir = flag(&args, "--out-dir").unwrap_or("audit-evidence-bundle");
    let events = load_events(source)?;

    fs::create_dir_all(out_dir).map_err(|e| format!("create out-dir failed: {e}"))?;

    // events.jsonl — one JSON event per line (portable, append-friendly).
    let jsonl_path = format!("{out_dir}/events.jsonl");
    let mut jsonl = String::new();
    for e in &events {
        let line = serde_json::to_string(e).map_err(|e| format!("serialize event failed: {e}"))?;
        jsonl.push_str(&line);
        jsonl.push('\n');
    }
    fs::write(&jsonl_path, jsonl).map_err(|e| format!("write events.jsonl failed: {e}"))?;

    // manifest.json — bundle metadata + chain integrity result.
    let tamper_point = AppendOnlyAuditSink::verify_chain_tamper_point(&events);
    let manifest = serde_json::json!({
        "bundle_version": 1,
        "generated_epoch_ms": now_epoch_millis(),
        "source": source,
        "event_count": events.len(),
        "chain_valid": tamper_point.is_none(),
        "tamper_point_event_id": tamper_point,
        "events_file": "events.jsonl",
        "first_event_id": events.first().map(|e| e.event_id),
        "last_event_id": events.last().map(|e| e.event_id),
    });
    let manifest_path = format!("{out_dir}/manifest.json");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).map_err(|e| format!("serialize manifest failed: {e}"))?,
    )
    .map_err(|e| format!("write manifest.json failed: {e}"))?;

    println!(
        "evidence bundle written to {out_dir}/ ({} events, chain_valid={})",
        events.len(),
        tamper_point.is_none()
    );
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

// ── Legacy report mode (back-compat) ─────────────────────────────────────────

#[derive(Debug)]
struct CliArgs {
    audit_file: String,
    action_file: String,
    out_file: String,
    trace_id_filter: Option<String>,
    action_filter: Option<String>,
}

fn legacy_report(arg_list: Vec<String>) -> Result<(), String> {
    let args = parse_args(arg_list)?;
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

fn parse_args(arg_list: Vec<String>) -> Result<CliArgs, String> {
    let mut audit_file = None;
    let mut action_file = None;
    let mut out_file = Some("tests/kpi/results/ws8a/audit-companion-report.json".to_string());
    let mut trace_id_filter = None;
    let mut action_filter = None;

    let mut iter = arg_list.into_iter();
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

#[cfg(test)]
mod tests {
    use super::*;
    use voltnuerongrid_audit::AuditEventKind;

    fn sample_events() -> Vec<AuditEvent> {
        let mut sink = AppendOnlyAuditSink::new();
        sink.append(AuditEventKind::Autonomous, "op", "controller_run", "ok", "{}");
        sink.append(AuditEventKind::Sql, "op", "ai_tune_apply_index", "applied", "{}");
        sink.all().to_vec()
    }

    #[test]
    fn parse_events_accepts_bare_array() {
        let events = sample_events();
        let json = serde_json::to_string(&events).unwrap();
        let parsed = parse_events(&json).unwrap();
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn parse_events_accepts_api_envelope() {
        let events = sample_events();
        let envelope = serde_json::json!({ "status": "ok", "events": events });
        let parsed = parse_events(&envelope.to_string()).unwrap();
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn export_writes_bundle_and_verify_detects_tamper() {
        let dir = std::env::temp_dir().join(format!("audit-bundle-{}", now_epoch_millis()));
        let dir_str = dir.to_str().unwrap().to_string();
        let mut events = sample_events();

        // Clean chain → bundle reports valid.
        let src = dir.join("clean.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&src, serde_json::to_string(&events).unwrap()).unwrap();
        cmd_export(vec![
            "--audit-file".into(),
            src.to_str().unwrap().into(),
            "--out-dir".into(),
            dir_str.clone(),
        ])
        .unwrap();
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["chain_valid"], true);
        assert_eq!(manifest["event_count"], 2);
        assert!(dir.join("events.jsonl").exists());

        // Tamper the second event → tamper point detected at its event_id.
        events[1].outcome = "tampered".to_string();
        assert_eq!(
            AppendOnlyAuditSink::verify_chain_tamper_point(&events),
            Some(events[1].event_id)
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
