use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use voltnuerongrid_audit::AuditEventKind;
use voltnuerongrid_store::htap_sync::ReplicaReplayState;
use crate::{AppState, AuthErrorResponse, AcidTxEntry, PessimisticLockRecord, ClusterNodeRuntime, DriverSession};
use crate::auth::{require_admin_api_key, require_operator_auth};
use crate::audit_helpers::append_audit_event;
use crate::{now_unix_ms, now_unix_ms_u64, default_node_cpu_cores, default_node_ram_mb};

// ─── Admin cluster topology DTOs ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct ClusterTopologyNodeEntry {
    pub(crate) node_id: String,
    pub(crate) role: String,
    pub(crate) status: String,
    pub(crate) total_cpu_cores: u32,
    pub(crate) total_ram_mb: u64,
    pub(crate) used_cpu_pct: f64,
    pub(crate) used_ram_mb: u64,
    pub(crate) active_sessions: usize,
    pub(crate) passive_sessions: usize,
    pub(crate) live_transactions: usize,
    pub(crate) total_transactions: usize,
    pub(crate) live_locks: usize,
    pub(crate) draining: bool,
    pub(crate) last_heartbeat_ms: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminClusterTopologyResponse {
    pub(crate) status: &'static str,
    pub(crate) leader_node_id: String,
    pub(crate) total_nodes: usize,
    pub(crate) active_nodes: usize,
    pub(crate) passive_nodes: usize,
    pub(crate) dead_nodes: usize,
    pub(crate) active_sessions: usize,
    pub(crate) passive_sessions: usize,
    pub(crate) live_transactions: usize,
    pub(crate) total_transactions: usize,
    pub(crate) live_locks: usize,
    pub(crate) nodes: Vec<ClusterTopologyNodeEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminTransactionControlRequest {
    pub(crate) action: String,
    pub(crate) transaction_id: Option<String>,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminTransactionControlResponse {
    pub(crate) status: &'static str,
    pub(crate) action: String,
    pub(crate) affected_count: usize,
    pub(crate) active_count: usize,
    pub(crate) transactions: Vec<AcidTxEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminLockControlRequest {
    pub(crate) action: String,
    pub(crate) lock_id: Option<String>,
    pub(crate) transaction_id: Option<String>,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminLockControlResponse {
    pub(crate) status: &'static str,
    pub(crate) action: String,
    pub(crate) released_lock_count: usize,
    pub(crate) active_lock_count: usize,
    pub(crate) locks: Vec<PessimisticLockRecord>,
    pub(crate) affected_transactions: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminClusterNodeManageRequest {
    pub(crate) action: String,
    pub(crate) node_id: String,
    pub(crate) role: Option<String>,
    pub(crate) desired_status: Option<String>,
    pub(crate) total_cpu_cores: Option<u32>,
    pub(crate) total_ram_mb: Option<u64>,
    pub(crate) target_node_id: Option<String>,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminClusterNodeManageResponse {
    pub(crate) status: &'static str,
    pub(crate) action: String,
    pub(crate) node_id: String,
    pub(crate) cluster_size: usize,
    pub(crate) migrated_transactions: usize,
    pub(crate) migrated_sessions: usize,
    pub(crate) message: String,
}

// ─── Admin database DTOs ──────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub(crate) struct AdminDatabaseRecord {
    pub(crate) name: String,
    pub(crate) owner: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) created_at_ms: u128,
}

impl From<&voltnuerongrid_meta::Database> for AdminDatabaseRecord {
    fn from(db: &voltnuerongrid_meta::Database) -> Self {
        Self {
            name: db.name.as_str().to_string(),
            owner: db.owner.clone(),
            description: db.description.clone(),
            created_at_ms: db.created_at_ms,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct AdminDatabasesListResponse {
    pub(crate) databases: Vec<AdminDatabaseRecord>,
    pub(crate) count: usize,
}

#[derive(Deserialize)]
pub(crate) struct AdminCreateDatabaseRequest {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) owner: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) if_not_exists: bool,
}

#[derive(Serialize)]
pub(crate) struct AdminCreateDatabaseResponse {
    pub(crate) status: &'static str,
    pub(crate) database: Option<AdminDatabaseRecord>,
    pub(crate) already_existed: bool,
}

#[derive(Deserialize, Default)]
pub(crate) struct AdminDropDatabaseQuery {
    #[serde(default)]
    pub(crate) if_exists: bool,
}

#[derive(Serialize)]
pub(crate) struct AdminDropDatabaseResponse {
    pub(crate) status: &'static str,
    pub(crate) dropped: Option<AdminDatabaseRecord>,
    pub(crate) not_found_acceptable: bool,
}

/// `GET /api/v1/admin/databases/{name}/metadata` — return the static
/// metadata-schema layout for a database. Phase 1.4 will replace this with
/// live data-bearing rows; for now this returns the column-list schema so
/// the Studio can render the metadata browser.
#[derive(Serialize)]
pub(crate) struct AdminMetadataLayoutResponse {
    pub(crate) database: String,
    pub(crate) schema: &'static str,
    pub(crate) tables: Vec<voltnuerongrid_meta::MetadataTableSpec>,
}

// ─── Phase 1.4 — live metadata.* rows ──────────────────────────────────────
//
// Implements `MetadataDataProvider` for the service's `AppState`, then
// surfaces the rows over HTTP. Once the DataFusion executor lands
// (Phase 1.7), `SELECT * FROM metadata.tables` will read through this same
// provider.
//
// Design choice: we don't take any locks while building rows — instead we
// snapshot the catalog under its mutex, drop the lock, then walk the
// snapshot. This keeps the catalog mutex hold time short.

struct AppStateMetadataProvider<'a> {
    state: &'a AppState,
}

impl<'a> voltnuerongrid_meta::MetadataDataProvider for AppStateMetadataProvider<'a> {
    fn rows_for(
        &self,
        table: voltnuerongrid_meta::MetadataTable,
        database_name: &voltnuerongrid_meta::DatabaseName,
    ) -> Vec<voltnuerongrid_meta::MetadataRow> {
        use voltnuerongrid_meta::{MetadataRow, MetadataTable};

        let db_str = database_name.as_str();
        let mut out: Vec<MetadataRow> = Vec::new();

        match table {
            MetadataTable::Databases => {
                if let Ok(catalog) = self.state.database_catalog.lock() {
                    for db in catalog.list() {
                        let mut row = MetadataRow::new();
                        row.insert("name".to_string(), db.name.as_str().to_string());
                        row.insert(
                            "owner".to_string(),
                            db.owner.clone().unwrap_or_default(),
                        );
                        row.insert("created_at_ms".to_string(), db.created_at_ms.to_string());
                        row.insert(
                            "description".to_string(),
                            db.description.clone().unwrap_or_default(),
                        );
                        out.push(row);
                    }
                }
            }

            MetadataTable::Tables => {
                if let Ok(catalog) = self.state.ddl_catalog.lock() {
                    for entry in catalog.active_entries() {
                        if entry.database_name != db_str {
                            continue;
                        }
                        if entry.object_kind != "table" {
                            continue;
                        }
                        let mut row = MetadataRow::new();
                        row.insert("database_name".to_string(), entry.database_name.clone());
                        row.insert("schema_name".to_string(), entry.schema_name.clone());
                        row.insert("table_name".to_string(), entry.object_name.clone());
                        row.insert("kind".to_string(), entry.object_kind.clone());
                        row.insert(
                            "created_at_ms".to_string(),
                            entry.created_at_unix_ms.to_string(),
                        );
                        out.push(row);
                    }
                }
            }

            MetadataTable::Schemas => {
                if let Ok(catalog) = self.state.ddl_catalog.lock() {
                    let mut schemas: std::collections::BTreeSet<String> = Default::default();
                    for entry in catalog.active_entries() {
                        if entry.database_name == db_str {
                            schemas.insert(entry.schema_name.clone());
                        }
                    }
                    schemas.insert("metadata".to_string());
                    for schema_name in schemas {
                        let mut row = MetadataRow::new();
                        row.insert("database_name".to_string(), db_str.to_string());
                        row.insert("schema_name".to_string(), schema_name);
                        out.push(row);
                    }
                }
            }

            MetadataTable::Views => {
                if let Ok(catalog) = self.state.ddl_catalog.lock() {
                    for entry in catalog.active_entries() {
                        if entry.database_name != db_str {
                            continue;
                        }
                        if entry.object_kind != "view" && entry.object_kind != "materialized_view" {
                            continue;
                        }
                        let mut row = MetadataRow::new();
                        row.insert("database_name".to_string(), entry.database_name.clone());
                        row.insert("schema_name".to_string(), entry.schema_name.clone());
                        row.insert("view_name".to_string(), entry.object_name.clone());
                        row.insert("definition".to_string(), entry.original_statement.clone());
                        out.push(row);
                    }
                }
            }

            MetadataTable::Routines => {
                if let Ok(catalog) = self.state.ddl_catalog.lock() {
                    for entry in catalog.active_entries() {
                        if entry.database_name != db_str {
                            continue;
                        }
                        if entry.object_kind != "function" && entry.object_kind != "procedure" {
                            continue;
                        }
                        let mut row = MetadataRow::new();
                        row.insert("database_name".to_string(), entry.database_name.clone());
                        row.insert("schema_name".to_string(), entry.schema_name.clone());
                        row.insert("routine_name".to_string(), entry.object_name.clone());
                        row.insert("language".to_string(), "sql".to_string());
                        row.insert("kind".to_string(), entry.object_kind.clone());
                        out.push(row);
                    }
                }
            }

            MetadataTable::Triggers => {
                if let Ok(catalog) = self.state.ddl_catalog.lock() {
                    for entry in catalog.active_entries() {
                        if entry.database_name != db_str {
                            continue;
                        }
                        if entry.object_kind != "trigger" {
                            continue;
                        }
                        let mut row = MetadataRow::new();
                        row.insert("database_name".to_string(), entry.database_name.clone());
                        row.insert("schema_name".to_string(), entry.schema_name.clone());
                        row.insert("trigger_name".to_string(), entry.object_name.clone());
                        row.insert("table_name".to_string(), String::new());
                        row.insert("event".to_string(), String::new());
                        out.push(row);
                    }
                }
            }

            MetadataTable::Settings => {
                let cfg = &self.state.runtime_config;
                let pairs: Vec<(&str, String)> = vec![
                    ("storage.engine", format!("{:?}", cfg.storage.engine).to_ascii_lowercase()),
                    ("storage.data_dir", cfg.storage.data_dir.clone()),
                    (
                        "storage.max_background_jobs",
                        cfg.storage.max_background_jobs.to_string(),
                    ),
                    (
                        "storage.wal_fsync_on_commit",
                        cfg.storage.wal_fsync_on_commit.to_string(),
                    ),
                    ("sql.engine", format!("{:?}", cfg.sql.engine).to_ascii_lowercase()),
                    (
                        "sql.htap_olap_threshold_rows",
                        cfg.sql.htap_olap_threshold_rows.to_string(),
                    ),
                    ("sql.max_result_rows", cfg.sql.max_result_rows.to_string()),
                ];
                for (key, value) in pairs {
                    let mut row = MetadataRow::new();
                    row.insert("database_name".to_string(), db_str.to_string());
                    row.insert("key".to_string(), key.to_string());
                    row.insert("value".to_string(), value);
                    row.insert("scope".to_string(), "server".to_string());
                    out.push(row);
                }
            }

            MetadataTable::Columns
            | MetadataTable::Indexes
            | MetadataTable::Users
            | MetadataTable::Roles
            | MetadataTable::Grants => {
                // Empty — Phase 1.6+ work (real users / roles / indexes).
            }
        }

        out
    }
}

#[derive(Serialize)]
pub(crate) struct AdminMetadataRowsResponse {
    pub(crate) database: String,
    pub(crate) table: String,
    pub(crate) columns: Vec<&'static str>,
    pub(crate) rows: Vec<voltnuerongrid_meta::MetadataRow>,
    pub(crate) row_count: usize,
}

// ─── Server status DTOs ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct ServerStatusTransport {
    pub(crate) http: bool,
    pub(crate) native: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServerStatusConnections {
    pub(crate) active: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServerStatusStorage {
    pub(crate) engine: String,
    pub(crate) status: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServerStatusResponse {
    pub(crate) version: String,
    pub(crate) uptime_s: u64,
    pub(crate) transport: ServerStatusTransport,
    pub(crate) connections: ServerStatusConnections,
    pub(crate) storage: ServerStatusStorage,
}

// ─── Private helpers ──────────────────────────────────────────────────────────

fn release_locks_for_transaction(
    lock_table: &mut HashMap<String, PessimisticLockRecord>,
    wait_graph: &mut HashMap<String, String>,
    transaction_id: &str,
) -> usize {
    let before = lock_table.len();
    lock_table.retain(|_, record| record.transaction_id != transaction_id);
    wait_graph.retain(|waiting_tx, holder_tx| waiting_tx != transaction_id && holder_tx != transaction_id);
    before.saturating_sub(lock_table.len())
}

fn choose_migration_target(
    nodes: &HashMap<String, ClusterNodeRuntime>,
    source_node_id: &str,
    requested_target_node_id: Option<&str>,
) -> Option<String> {
    if let Some(target_node_id) = requested_target_node_id {
        if target_node_id != source_node_id {
            if let Some(node) = nodes.get(target_node_id) {
                if node.status != "dead" {
                    return Some(target_node_id.to_string());
                }
            }
        }
    }

    nodes.iter()
        .filter(|(node_id, node)| node_id.as_str() != source_node_id && node.status != "dead")
        .max_by_key(|(_, node)| if node.status == "active" { 2 } else { 1 })
        .map(|(node_id, _)| node_id.clone())
}

fn migrate_driver_sessions(
    sessions: &mut HashMap<String, DriverSession>,
    source_node_id: &str,
    target_node_id: &str,
) -> usize {
    let mut migrated = 0usize;
    for session in sessions.values_mut() {
        if session.assigned_node_id == source_node_id {
            session.assigned_node_id = target_node_id.to_string();
            migrated += 1;
        }
    }
    migrated
}

fn cluster_topology_snapshot(state: &AppState) -> AdminClusterTopologyResponse {
    let leader_node_id = state
        .leader_node_id
        .lock()
        .map(|value| value.clone())
        .unwrap_or_else(|_| state.node_id.clone());
    let cluster_nodes = state.cluster_nodes.lock().expect("cluster_nodes lock topology");
    let sessions = state.driver_sessions.lock().expect("driver_sessions lock topology");
    let transactions = state.acid_transactions.lock().expect("acid_transactions lock topology");
    let locks = state.pessimistic_locks.lock().expect("pessimistic_locks lock topology");
    let failure_signals = state.cluster_failure_signals.lock().expect("cluster_failure_signals lock topology");

    let mut active_nodes = 0usize;
    let mut passive_nodes = 0usize;
    let mut dead_nodes = 0usize;
    let mut active_sessions = 0usize;
    let mut passive_sessions = 0usize;
    let mut live_transactions = 0usize;
    let total_transactions = transactions.all_transactions().len();
    let live_locks = locks.len();
    let mut nodes = Vec::new();

    for node in cluster_nodes.values() {
        let node_dead = failure_signals.iter().any(|signal| {
            signal.node_id == node.node_id && !signal.resolved && signal.severity.eq_ignore_ascii_case("critical")
        }) || node.status == "dead";
        let effective_status = if node_dead {
            "dead".to_string()
        } else {
            node.status.clone()
        };
        let node_active_sessions = sessions.values().filter(|session| session.assigned_node_id == node.node_id).count();
        let node_live_transactions = transactions
            .active_transactions()
            .into_iter()
            .filter(|entry| entry.assigned_node_id == node.node_id)
            .count();
        let node_total_transactions = transactions
            .all_transactions()
            .into_iter()
            .filter(|entry| entry.assigned_node_id == node.node_id)
            .count();
        let node_live_locks = locks.values().filter(|record| {
            transactions
                .transactions
                .get(&record.transaction_id)
                .map(|entry| entry.assigned_node_id == node.node_id)
                .unwrap_or(false)
        }).count();
        let node_passive_sessions = if effective_status == "passive" { node_active_sessions } else { 0 };
        let node_effective_active_sessions = if effective_status == "active" { node_active_sessions } else { 0 };

        match effective_status.as_str() {
            "active" => active_nodes += 1,
            "passive" => passive_nodes += 1,
            _ => dead_nodes += 1,
        }
        active_sessions += node_effective_active_sessions;
        passive_sessions += node_passive_sessions;
        live_transactions += node_live_transactions;

        let estimated_cpu = ((node_effective_active_sessions as f64 * 4.0)
            + (node_live_transactions as f64 * 6.0)
            + (node_live_locks as f64 * 1.5)
            + if node.role == "leader" { 8.0 } else { 3.0 })
            .min(100.0);
        let estimated_ram = (512u64
            + (node_active_sessions as u64 * 128)
            + (node_live_transactions as u64 * 96)
            + (node_live_locks as u64 * 16))
            .min(node.total_ram_mb.max(512));

        nodes.push(ClusterTopologyNodeEntry {
            node_id: node.node_id.clone(),
            role: node.role.clone(),
            status: effective_status,
            total_cpu_cores: node.total_cpu_cores,
            total_ram_mb: node.total_ram_mb,
            used_cpu_pct: estimated_cpu,
            used_ram_mb: estimated_ram,
            active_sessions: node_effective_active_sessions,
            passive_sessions: node_passive_sessions,
            live_transactions: node_live_transactions,
            total_transactions: node_total_transactions,
            live_locks: node_live_locks,
            draining: node.draining,
            last_heartbeat_ms: node.last_heartbeat_ms,
        });
    }

    nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));

    AdminClusterTopologyResponse {
        status: "ok",
        leader_node_id,
        total_nodes: nodes.len(),
        active_nodes,
        passive_nodes,
        dead_nodes,
        active_sessions,
        passive_sessions,
        live_transactions,
        total_transactions,
        live_locks,
        nodes,
    }
}

// ─── Admin handlers ───────────────────────────────────────────────────────────

pub(crate) async fn admin_cluster_topology(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<AdminClusterTopologyResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;
    Ok((StatusCode::OK, Json(cluster_topology_snapshot(&state))))
}

pub(crate) async fn admin_sql_transaction_control(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AdminTransactionControlRequest>,
) -> Result<(StatusCode, Json<AdminTransactionControlResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;
    let action = req.action.to_ascii_lowercase();
    let now_ms = now_unix_ms();
    let mut affected_count = 0usize;

    match action.as_str() {
        "list" => {}
        "commit" | "rollback" => {
            let Some(transaction_id) = req.transaction_id.as_deref() else {
                return Ok((StatusCode::BAD_REQUEST, Json(AdminTransactionControlResponse {
                    status: "error",
                    action,
                    affected_count: 0,
                    active_count: 0,
                    transactions: Vec::new(),
                })));
            };

            let mut acid = state.acid_transactions.lock().expect("acid_transactions admin control lock");
            affected_count = if action == "commit" {
                usize::from(acid.commit(transaction_id, now_ms))
            } else {
                usize::from(acid.rollback(transaction_id, now_ms))
            };
            drop(acid);

            if action == "rollback" {
                let mut lock_table = state.pessimistic_locks.lock().expect("pessimistic_locks admin rollback lock");
                let mut wait_graph = state.pessimistic_lock_waits.lock().expect("pessimistic_lock_waits admin rollback lock");
                let released = release_locks_for_transaction(&mut lock_table, &mut wait_graph, transaction_id);
                if released > 0 {
                    state.pessimistic_lock_metrics.lock_releases.fetch_add(released as u64, Ordering::Relaxed);
                }
            }

            append_audit_event(
                &state,
                AuditEventKind::Sql,
                "admin",
                "admin_sql_transaction_control",
                if affected_count > 0 { "ok" } else { "not_found" },
                &json!({
                    "action": action,
                    "transaction_id": transaction_id,
                    "reason": req.reason,
                    "affected_count": affected_count,
                })
                .to_string(),
            );
        }
        _ => {
            return Ok((StatusCode::BAD_REQUEST, Json(AdminTransactionControlResponse {
                status: "error",
                action,
                affected_count: 0,
                active_count: 0,
                transactions: Vec::new(),
            })));
        }
    }

    let acid = state.acid_transactions.lock().expect("acid_transactions admin response lock");
    let active_transactions: Vec<AcidTxEntry> = acid.active_transactions().into_iter().map(|entry| entry.clone()).collect();
    let active_count = active_transactions.len();
    Ok((StatusCode::OK, Json(AdminTransactionControlResponse {
        status: "ok",
        action,
        affected_count,
        active_count,
        transactions: active_transactions,
    })))
}

pub(crate) async fn admin_sql_lock_control(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AdminLockControlRequest>,
) -> Result<(StatusCode, Json<AdminLockControlResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;
    let action = req.action.to_ascii_lowercase();
    let mut released_lock_count = 0usize;
    let mut affected_transactions = Vec::new();

    {
        let mut lock_table = state.pessimistic_locks.lock().expect("pessimistic_locks admin control lock");
        let mut wait_graph = state.pessimistic_lock_waits.lock().expect("pessimistic_lock_waits admin control lock");

        match action.as_str() {
            "list" => {}
            "kill_lock" => {
                if let Some(lock_id) = req.lock_id.as_deref() {
                    if let Some(record) = lock_table.remove(lock_id) {
                        affected_transactions.push(record.transaction_id.clone());
                        wait_graph.retain(|waiting_tx, holder_tx| waiting_tx != &record.transaction_id && holder_tx != &record.transaction_id);
                        released_lock_count = 1;
                    }
                } else if let Some(transaction_id) = req.transaction_id.as_deref() {
                    released_lock_count = release_locks_for_transaction(&mut lock_table, &mut wait_graph, transaction_id);
                    if released_lock_count > 0 {
                        affected_transactions.push(transaction_id.to_string());
                    }
                } else {
                    return Ok((StatusCode::BAD_REQUEST, Json(AdminLockControlResponse {
                        status: "error",
                        action,
                        released_lock_count: 0,
                        active_lock_count: lock_table.len(),
                        locks: lock_table.values().cloned().collect(),
                        affected_transactions: Vec::new(),
                    })));
                }
            }
            "kill_deadlock" => {
                let Some(transaction_id) = req.transaction_id.as_deref() else {
                    return Ok((StatusCode::BAD_REQUEST, Json(AdminLockControlResponse {
                        status: "error",
                        action,
                        released_lock_count: 0,
                        active_lock_count: lock_table.len(),
                        locks: lock_table.values().cloned().collect(),
                        affected_transactions: Vec::new(),
                    })));
                };
                released_lock_count = release_locks_for_transaction(&mut lock_table, &mut wait_graph, transaction_id);
                if released_lock_count > 0 {
                    affected_transactions.push(transaction_id.to_string());
                }
                let mut acid = state.acid_transactions.lock().expect("acid_transactions deadlock victim lock");
                let _ = acid.rollback(transaction_id, now_unix_ms());
            }
            _ => {
                return Ok((StatusCode::BAD_REQUEST, Json(AdminLockControlResponse {
                    status: "error",
                    action,
                    released_lock_count: 0,
                    active_lock_count: lock_table.len(),
                    locks: lock_table.values().cloned().collect(),
                    affected_transactions: Vec::new(),
                })));
            }
        }

        if released_lock_count > 0 {
            state.pessimistic_lock_metrics.lock_releases.fetch_add(released_lock_count as u64, Ordering::Relaxed);
        }
    }

    let lock_table = state.pessimistic_locks.lock().expect("pessimistic_locks admin control response lock");
    let locks: Vec<PessimisticLockRecord> = lock_table.values().cloned().collect();
    drop(lock_table);
    append_audit_event(
        &state,
        AuditEventKind::Sql,
        "admin",
        "admin_sql_lock_control",
        "ok",
        &json!({
            "action": action,
            "released_lock_count": released_lock_count,
            "reason": req.reason,
            "affected_transactions": affected_transactions,
        })
        .to_string(),
    );
    Ok((StatusCode::OK, Json(AdminLockControlResponse {
        status: "ok",
        action,
        released_lock_count,
        active_lock_count: locks.len(),
        locks,
        affected_transactions,
    })))
}

pub(crate) async fn admin_cluster_node_manage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AdminClusterNodeManageRequest>,
) -> Result<(StatusCode, Json<AdminClusterNodeManageResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;
    let action = req.action.to_ascii_lowercase();
    let mut migrated_transactions = 0usize;
    let mut migrated_sessions = 0usize;

    match action.as_str() {
        "add" => {
            let mut nodes = state.cluster_nodes.lock().expect("cluster_nodes add lock");
            let entry = nodes.entry(req.node_id.clone()).or_insert(ClusterNodeRuntime {
                node_id: req.node_id.clone(),
                role: req.role.clone().unwrap_or_else(|| "follower".to_string()),
                status: req.desired_status.clone().unwrap_or_else(|| "active".to_string()),
                total_cpu_cores: req.total_cpu_cores.unwrap_or_else(default_node_cpu_cores),
                total_ram_mb: req.total_ram_mb.unwrap_or_else(default_node_ram_mb),
                draining: false,
                last_heartbeat_ms: now_unix_ms_u64(),
            });
            entry.last_heartbeat_ms = now_unix_ms_u64();
            drop(nodes);
            let mut replicas = state.replica_replay_states.lock().expect("replica_replay_states add lock");
            replicas.entry(req.node_id.clone()).or_insert_with(|| ReplicaReplayState::new(&req.node_id));
        }
        "remove" => {
            let target_node_id = {
                let nodes = state.cluster_nodes.lock().expect("cluster_nodes remove select lock");
                choose_migration_target(&nodes, &req.node_id, req.target_node_id.as_deref())
            };

            let Some(target_node_id) = target_node_id else {
                return Ok((StatusCode::CONFLICT, Json(AdminClusterNodeManageResponse {
                    status: "error",
                    action,
                    node_id: req.node_id,
                    cluster_size: 0,
                    migrated_transactions: 0,
                    migrated_sessions: 0,
                    message: "no viable target node available for drain/remove".to_string(),
                })));
            };

            {
                let mut acid = state.acid_transactions.lock().expect("acid_transactions node manage lock");
                migrated_transactions = acid.reassign_active_node(&req.node_id, &target_node_id);
            }
            {
                let mut sessions = state.driver_sessions.lock().expect("driver_sessions node manage lock");
                migrated_sessions = migrate_driver_sessions(&mut sessions, &req.node_id, &target_node_id);
            }
            {
                let mut nodes = state.cluster_nodes.lock().expect("cluster_nodes remove lock");
                nodes.remove(&req.node_id);
                if let Some(target) = nodes.get_mut(&target_node_id) {
                    target.status = "active".to_string();
                    target.draining = false;
                    if *state.leader_node_id.lock().expect("leader_node_id remove lock") == req.node_id {
                        target.role = "leader".to_string();
                    }
                }
            }
            {
                let mut replicas = state.replica_replay_states.lock().expect("replica_replay_states remove lock");
                replicas.remove(&req.node_id);
            }
            {
                let mut leader = state.leader_node_id.lock().expect("leader_node_id node manage lock");
                if *leader == req.node_id {
                    *leader = target_node_id;
                }
            }
        }
        _ => {
            return Ok((StatusCode::BAD_REQUEST, Json(AdminClusterNodeManageResponse {
                status: "error",
                action,
                node_id: req.node_id,
                cluster_size: 0,
                migrated_transactions: 0,
                migrated_sessions: 0,
                message: "unsupported action; expected add or remove".to_string(),
            })));
        }
    }

    let cluster_size = state.cluster_nodes.lock().expect("cluster_nodes count lock").len();
    append_audit_event(
        &state,
        AuditEventKind::Failover,
        "admin",
        "admin_cluster_node_manage",
        "ok",
        &json!({
            "action": action,
            "node_id": req.node_id,
            "reason": req.reason,
            "migrated_transactions": migrated_transactions,
            "migrated_sessions": migrated_sessions,
            "cluster_size": cluster_size,
        })
        .to_string(),
    );
    Ok((StatusCode::OK, Json(AdminClusterNodeManageResponse {
        status: "ok",
        action,
        node_id: req.node_id,
        cluster_size,
        migrated_transactions,
        migrated_sessions,
        message: "cluster membership updated".to_string(),
    })))
}

/// `GET /api/v1/admin/databases` — list all databases.
pub(crate) async fn admin_databases_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<AdminDatabasesListResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;
    let catalog = match state.database_catalog.lock() {
        Ok(g) => g,
        Err(_) => {
            tracing::error!(target: "vng.handler", resource = "database_catalog", "mutex poisoned");
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AuthErrorResponse {
                    status: "error",
                    reason: "database_catalog mutex poisoned".to_string(),
                    locale: "en".to_string(),
                    localized_message: "Database catalog temporarily unavailable".to_string(),
                }),
            ));
        }
    };
    let databases: Vec<AdminDatabaseRecord> =
        catalog.list().into_iter().map(AdminDatabaseRecord::from).collect();
    let count = databases.len();
    Ok((
        StatusCode::OK,
        Json(AdminDatabasesListResponse { databases, count }),
    ))
}

/// `POST /api/v1/admin/databases` — create a new database.
///
/// Returns 201 on success, 409 on conflict (unless `if_not_exists` is true,
/// in which case 200 with `already_existed=true`), 400 on invalid name.
pub(crate) async fn admin_databases_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AdminCreateDatabaseRequest>,
) -> Result<(StatusCode, Json<AdminCreateDatabaseResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;
    let mut catalog = match state.database_catalog.lock() {
        Ok(g) => g,
        Err(_) => {
            tracing::error!(target: "vng.handler", resource = "database_catalog", "mutex poisoned");
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AuthErrorResponse {
                    status: "error",
                    reason: "database_catalog mutex poisoned".to_string(),
                    locale: "en".to_string(),
                    localized_message: "Database catalog temporarily unavailable".to_string(),
                }),
            ));
        }
    };
    let now_ms = now_unix_ms() as u128;
    match catalog.create(
        &req.name,
        now_ms,
        req.owner.as_deref(),
        req.description.as_deref(),
    ) {
        Ok(db) => {
            let record = AdminDatabaseRecord::from(db);
            tracing::info!(
                target: "vng.database",
                name = %record.name,
                owner = ?record.owner,
                "database created"
            );
            metrics::counter!(
                "vng_database_lifecycle_total",
                "operation" => "create",
                "status" => "ok",
            )
            .increment(1);
            Ok((
                StatusCode::CREATED,
                Json(AdminCreateDatabaseResponse {
                    status: "ok",
                    database: Some(record),
                    already_existed: false,
                }),
            ))
        }
        Err(voltnuerongrid_meta::DatabaseCatalogError::AlreadyExists { name }) if req.if_not_exists => {
            let record = catalog.get(&name).map(AdminDatabaseRecord::from);
            metrics::counter!(
                "vng_database_lifecycle_total",
                "operation" => "create",
                "status" => "noop_exists",
            )
            .increment(1);
            Ok((
                StatusCode::OK,
                Json(AdminCreateDatabaseResponse {
                    status: "ok",
                    database: record,
                    already_existed: true,
                }),
            ))
        }
        Err(voltnuerongrid_meta::DatabaseCatalogError::AlreadyExists { name }) => {
            metrics::counter!(
                "vng_database_lifecycle_total",
                "operation" => "create",
                "status" => "conflict",
            )
            .increment(1);
            Err((
                StatusCode::CONFLICT,
                Json(AuthErrorResponse {
                    status: "error",
                    reason: format!("database {name:?} already exists"),
                    locale: "en".to_string(),
                    localized_message: format!("Database {name:?} already exists"),
                }),
            ))
        }
        Err(e) => {
            metrics::counter!(
                "vng_database_lifecycle_total",
                "operation" => "create",
                "status" => "invalid",
            )
            .increment(1);
            let msg = e.to_string();
            Err((
                StatusCode::BAD_REQUEST,
                Json(AuthErrorResponse {
                    status: "error",
                    reason: msg.clone(),
                    locale: "en".to_string(),
                    localized_message: msg,
                }),
            ))
        }
    }
}

/// `DELETE /api/v1/admin/databases/{name}` — drop a database.
pub(crate) async fn admin_databases_drop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(q): Query<AdminDropDatabaseQuery>,
) -> Result<(StatusCode, Json<AdminDropDatabaseResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;
    let mut catalog = match state.database_catalog.lock() {
        Ok(g) => g,
        Err(_) => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AuthErrorResponse {
                    status: "error",
                    reason: "database_catalog mutex poisoned".to_string(),
                    locale: "en".to_string(),
                    localized_message: "Database catalog temporarily unavailable".to_string(),
                }),
            ));
        }
    };
    match catalog.drop_database(&name, q.if_exists) {
        Ok(Some(db)) => {
            let record = AdminDatabaseRecord::from(&db);
            tracing::info!(target: "vng.database", name = %record.name, "database dropped");
            metrics::counter!(
                "vng_database_lifecycle_total",
                "operation" => "drop",
                "status" => "ok",
            )
            .increment(1);
            Ok((
                StatusCode::OK,
                Json(AdminDropDatabaseResponse {
                    status: "ok",
                    dropped: Some(record),
                    not_found_acceptable: false,
                }),
            ))
        }
        Ok(None) => {
            metrics::counter!(
                "vng_database_lifecycle_total",
                "operation" => "drop",
                "status" => "noop_missing",
            )
            .increment(1);
            Ok((
                StatusCode::OK,
                Json(AdminDropDatabaseResponse {
                    status: "ok",
                    dropped: None,
                    not_found_acceptable: true,
                }),
            ))
        }
        Err(voltnuerongrid_meta::DatabaseCatalogError::NotFound { name }) => {
            metrics::counter!(
                "vng_database_lifecycle_total",
                "operation" => "drop",
                "status" => "not_found",
            )
            .increment(1);
            Err((
                StatusCode::NOT_FOUND,
                Json(AuthErrorResponse {
                    status: "error",
                    reason: format!("database {name:?} not found"),
                    locale: "en".to_string(),
                    localized_message: format!("Database {name:?} not found"),
                }),
            ))
        }
        Err(e) => {
            let msg = e.to_string();
            Err((
                StatusCode::BAD_REQUEST,
                Json(AuthErrorResponse {
                    status: "error",
                    reason: msg.clone(),
                    locale: "en".to_string(),
                    localized_message: msg,
                }),
            ))
        }
    }
}

pub(crate) async fn admin_databases_metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<(StatusCode, Json<AdminMetadataLayoutResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;
    let catalog = match state.database_catalog.lock() {
        Ok(g) => g,
        Err(_) => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AuthErrorResponse {
                    status: "error",
                    reason: "database_catalog mutex poisoned".to_string(),
                    locale: "en".to_string(),
                    localized_message: "Database catalog temporarily unavailable".to_string(),
                }),
            ));
        }
    };
    if !catalog.exists(&name) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(AuthErrorResponse {
                status: "error",
                reason: format!("database {name:?} not found"),
                locale: "en".to_string(),
                localized_message: format!("Database {name:?} not found"),
            }),
        ));
    }
    drop(catalog);
    Ok((
        StatusCode::OK,
        Json(AdminMetadataLayoutResponse {
            database: name.trim().to_ascii_lowercase(),
            schema: "metadata",
            tables: voltnuerongrid_meta::metadata_schema_layout(),
        }),
    ))
}

/// `GET /api/v1/admin/databases/:name/metadata/:table` — return live rows
/// for a single metadata table. Phase 1.4.
pub(crate) async fn admin_databases_metadata_rows(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((name, table_str)): Path<(String, String)>,
) -> Result<(StatusCode, Json<AdminMetadataRowsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;

    let db_name = {
        let parsed = match voltnuerongrid_meta::DatabaseName::parse(&name) {
            Ok(p) => p,
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(AuthErrorResponse {
                        status: "error",
                        reason: format!("invalid database name: {e}"),
                        locale: "en".to_string(),
                        localized_message: format!("Invalid database name: {e}"),
                    }),
                ));
            }
        };
        let catalog = match state.database_catalog.lock() {
            Ok(g) => g,
            Err(_) => {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(AuthErrorResponse {
                        status: "error",
                        reason: "database_catalog mutex poisoned".to_string(),
                        locale: "en".to_string(),
                        localized_message: "Database catalog temporarily unavailable".to_string(),
                    }),
                ));
            }
        };
        if !catalog.exists(parsed.as_str()) {
            return Err((
                StatusCode::NOT_FOUND,
                Json(AuthErrorResponse {
                    status: "error",
                    reason: format!("database {:?} not found", parsed.as_str()),
                    locale: "en".to_string(),
                    localized_message: format!("Database {:?} not found", parsed.as_str()),
                }),
            ));
        }
        parsed
    };

    let table = match voltnuerongrid_meta::MetadataTable::parse_name(&table_str) {
        Some(t) => t,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(AuthErrorResponse {
                    status: "error",
                    reason: format!(
                        "metadata table {table_str:?} does not exist; \
                         see GET /api/v1/admin/databases/{}/metadata for the list",
                        db_name.as_str()
                    ),
                    locale: "en".to_string(),
                    localized_message: format!(
                        "Metadata table {table_str:?} does not exist for database {}",
                        db_name.as_str()
                    ),
                }),
            ));
        }
    };

    let provider = AppStateMetadataProvider { state: &state };
    let rows = <AppStateMetadataProvider as voltnuerongrid_meta::MetadataDataProvider>::rows_for(
        &provider, table, &db_name,
    );
    let row_count = rows.len();
    Ok((
        StatusCode::OK,
        Json(AdminMetadataRowsResponse {
            database: db_name.as_str().to_string(),
            table: table.name().to_string(),
            columns: table.columns().to_vec(),
            rows,
            row_count,
        }),
    ))
}

/// `GET /api/v1/admin/runtime-config` — read-only view of the boot-time
/// runtime configuration (storage engine, SQL engine, tunables).
pub(crate) async fn admin_runtime_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<voltnuerongrid_config::RuntimeConfig>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;
    Ok((StatusCode::OK, Json((*state.runtime_config).clone())))
}

pub(crate) async fn admin_server_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ServerStatusResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    Ok((
        StatusCode::OK,
        Json(ServerStatusResponse {
            version: "0.3.2".to_string(),
            uptime_s: 0,
            transport: ServerStatusTransport {
                http: true,
                native: false,
            },
            connections: ServerStatusConnections { active: 0 },
            storage: ServerStatusStorage {
                engine: "voltnuerongrid".to_string(),
                status: "ok".to_string(),
            },
        }),
    ))
}
