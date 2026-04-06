"""Replace broken old handler section with corrected versions."""
path = r"D:\by\polap-db\services\voltnuerongridd\src\main.rs"
with open(path, "r", encoding="utf-8") as f:
    content = f.read()

OLD_START = "3-WS1-05: Parse a simple WHERE clause string into vectorized filter predicates.\n"
OLD_END_MARKER = "\nfn evaluate_deadlock_scan_outcome("

idx_start = content.find(OLD_START)
idx_end   = content.find(OLD_END_MARKER, idx_start)

if idx_start == -1 or idx_end == -1:
    print("ERROR: markers not found")
    exit(1)

old_section = content[idx_start : idx_end + len(OLD_END_MARKER)]

NEW_SECTION = r'''/// S3-WS1-05: parse a WHERE clause string into `VectorizedFilter` predicates.
/// Handles simple `col op val` expressions joined by ` AND `.
fn parse_where_predicates(
    where_clause: &str,
) -> Option<Vec<voltnuerongrid_store::columnar::VectorizedFilter>> {
    use voltnuerongrid_store::columnar::{FilterOp, VectorizedFilter};
    let preds: Vec<VectorizedFilter> = where_clause
        .split(" AND ")
        .filter_map(|clause| {
            let clause = clause.trim();
            // Try each operator longest-first to avoid partial matches.
            let ops: &[(&str, FilterOp)] = &[
                (">=", FilterOp::Gte),
                ("<=", FilterOp::Lte),
                ("!=", FilterOp::Ne),
                (">",  FilterOp::Gt),
                ("<",  FilterOp::Lt),
                ("=",  FilterOp::Eq),
            ];
            for (sym, op) in ops {
                if let Some(pos) = clause.find(sym) {
                    let col = clause[..pos].trim().to_string();
                    let val = clause[pos + sym.len()..].trim()
                        .trim_matches('\'').trim_matches('"').to_string();
                    if !col.is_empty() {
                        return Some(VectorizedFilter { column: col, op: op.clone(), value: val });
                    }
                }
            }
            None
        })
        .collect();
    if preds.is_empty() { None } else { Some(preds) }
}

// ─── S2-WS2-02: WAL durability + recovery handlers ───────────────────────────

/// S2-WS2-02: return WAL engine stats.
async fn wal_status(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    use axum::{Json as AJ, http::StatusCode};
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let records = wal.wal_records();
    let wal_len = records.len();
    let latest_seq = records.last().map(|r| r.sequence).unwrap_or(0);
    let checkpoint_count = wal.checkpoint_count();
    drop(wal);
    (StatusCode::OK, AJ(WalStatusResponse {
        status: "ok",
        wal_len,
        latest_sequence: latest_seq,
        checkpoint_count,
    }))
}

/// S2-WS2-02: replay WAL records into the row store (or dry-run).
async fn wal_recover(
    State(state): State<AppState>,
    axum::extract::Json(req): axum::extract::Json<WalRecoverRequest>,
) -> impl axum::response::IntoResponse {
    use axum::{Json as AJ, http::StatusCode};
    let dry_run = req.dry_run.unwrap_or(false);
    let wal = state.wal_engine.lock().expect("wal_engine lock");
    let records = wal.wal_records().to_vec();
    drop(wal);
    let mut replayed: usize = 0;
    if !dry_run {
        let mut rs = state.row_store.lock().expect("row_store lock wal_recover");
        let xid = rs.begin_xid();
        for rec in &records {
            if rec.value == "__deleted__" {
                rs.delete(xid, &rec.key);
            } else {
                let data: std::collections::HashMap<String, String> =
                    serde_json::from_str(&rec.value)
                        .unwrap_or_else(|_| [("_raw".to_string(), rec.value.clone())]
                            .into_iter().collect());
                rs.insert(xid, &rec.key, data);
            }
            replayed += 1;
        }
    } else {
        replayed = records.len();
    }
    (StatusCode::OK, AJ(WalRecoverResponse {
        status: "ok",
        records_replayed: replayed,
        dry_run,
    }))
}

// ─── S7-WS6-04: Chaos/game-day injection handlers ────────────────────────────

fn now_epoch_ms_chaos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// S7-WS6-04: inject a chaos/game-day fault event.
async fn chaos_inject(
    State(state): State<AppState>,
    axum::extract::Json(req): axum::extract::Json<ChaosInjectRequest>,
) -> impl axum::response::IntoResponse {
    use axum::{Json as AJ, http::StatusCode};
    let event = ChaosEvent {
        fault_type: req.fault_type,
        target_node: req.target_node,
        parameters: req.parameters,
        injected_at_ms: now_epoch_ms_chaos(),
        cleared_at_ms: None,
    };
    let mut cs = state.chaos_state.lock().expect("chaos_state lock");
    cs.active_faults.push(event);
    let count = cs.active_faults.len();
    drop(cs);
    (StatusCode::OK, AJ(serde_json::json!({ "status": "injected", "active_fault_count": count })))
}

/// S7-WS6-04: clear all active faults; move them to history.
async fn chaos_clear(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    use axum::{Json as AJ, http::StatusCode};
    let cleared_at = now_epoch_ms_chaos();
    let mut cs = state.chaos_state.lock().expect("chaos_state lock");
    let mut cleared: Vec<ChaosEvent> = cs.active_faults.drain(..).map(|mut e| {
        e.cleared_at_ms = Some(cleared_at);
        e
    }).collect();
    cs.event_history.append(&mut cleared);
    let history_len = cs.event_history.len();
    drop(cs);
    (StatusCode::OK, AJ(serde_json::json!({ "status": "cleared", "history_len": history_len })))
}

/// S7-WS6-04: return current chaos state summary.
async fn chaos_status(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    use axum::{Json as AJ, http::StatusCode};
    let cs = state.chaos_state.lock().expect("chaos_state lock");
    let active_fault_count = cs.active_faults.len();
    let total_injected = cs.active_faults.len() + cs.event_history.len();
    let active_faults = cs.active_faults.clone();
    drop(cs);
    (StatusCode::OK, AJ(ChaosStatusResponse {
        status: "ok",
        active_fault_count,
        total_injected,
        active_faults,
    }))
}

fn evaluate_deadlock_scan_outcome('''

assert old_section in content, "OLD SECTION NOT FOUND IN CONTENT"
content = content.replace(old_section, NEW_SECTION, 1)

with open(path, "w", encoding="utf-8") as f:
    f.write(content)
print("Handler replacement OK")
