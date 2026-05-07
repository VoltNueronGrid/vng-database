use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use voltnuerongrid_audit::AuditEventKind;
use voltnuerongrid_auth::PrivilegeAction;
use voltnuerongrid_store::htap_sync::MutationOp;
use crate::{AppState, AuthErrorResponse, RuntimeAccessPrincipal, now_unix_ms_u64};
use crate::auth::{require_operator_auth, require_store_runtime_principal, store_table_matches_tenant_namespace, ensure_store_table_access};
use crate::audit_helpers::append_runtime_audit_event;

// ─── Index management DTOs ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct CreateIndexRequest {
    pub(crate) name: String,
    pub(crate) table: String,
    pub(crate) column: String,
    pub(crate) unique: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct CreateIndexResponse {
    pub(crate) status: &'static str,
    pub(crate) index_name: String,
    pub(crate) table: String,
    pub(crate) column: String,
    pub(crate) unique: bool,
}

#[derive(Deserialize)]
pub(crate) struct DropIndexRequest {
    pub(crate) name: String,
}

#[derive(Serialize)]
pub(crate) struct DropIndexResponse {
    pub(crate) status: &'static str,
    pub(crate) dropped: String,
}

#[derive(Serialize)]
pub(crate) struct IndexListEntry {
    pub(crate) name: String,
    pub(crate) table: String,
    pub(crate) column: String,
    pub(crate) kind: String,
    pub(crate) unique: bool,
}

#[derive(Serialize)]
pub(crate) struct ListIndexesResponse {
    pub(crate) status: &'static str,
    pub(crate) indexes: Vec<IndexListEntry>,
}

// ─── S5-WS4-03 / S2-WS2-04: MVCC row store scan structs ──────────────────────

#[derive(Deserialize)]
pub(crate) struct StoreRowsScanRequest {
    /// MVCC snapshot Xid to read at. Defaults to current head Xid.
    pub(crate) snapshot_xid: Option<u64>,
    /// Optional key prefix filter (empty string matches all).
    pub(crate) key_prefix: Option<String>,
    /// Maximum rows returned (capped at 10 000; default 1 000).
    pub(crate) limit: Option<usize>,
}

#[derive(Serialize)]
pub(crate) struct StoreRowEntry {
    pub(crate) key: String,
    pub(crate) data: HashMap<String, String>,
}

#[derive(Serialize)]
pub(crate) struct StoreRowsScanResponse {
    pub(crate) status: &'static str,
    pub(crate) snapshot_xid: u64,
    pub(crate) row_count: usize,
    pub(crate) rows: Vec<StoreRowEntry>,
}

// ─── S2-WS2-04: Row store snapshot export structs ────────────────────────────

#[derive(Serialize)]
pub(crate) struct RowSnapshotEntry {
    pub(crate) key: String,
    pub(crate) payload: HashMap<String, String>,
}

#[derive(Serialize)]
pub(crate) struct RowSnapshotResponse {
    pub(crate) status: &'static str,
    pub(crate) snapshot_xid: u64,
    pub(crate) row_count: usize,
    pub(crate) rows: Vec<RowSnapshotEntry>,
}

/// Response for `GET /api/v1/store/rows/stats`.
#[derive(Serialize)]
pub(crate) struct RowStoreStatsResponse {
    pub(crate) status: &'static str,
    pub(crate) current_xid: u64,
    pub(crate) total_pages: usize,
    pub(crate) total_rows: usize,
    pub(crate) total_visible_rows: usize,
}

// ─── S4-WS3-04: HTAP sync export structs ─────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct StoreHtapExportRequest {
    /// Export mutations with sequence > this value (0 = export all).
    pub(crate) since_sequence: Option<u64>,
    /// Maximum mutations to return (capped at 5 000; default 500).
    pub(crate) max_items: Option<usize>,
}

#[derive(Serialize)]
pub(crate) struct HtapMutationEntry {
    pub(crate) sequence: u64,
    pub(crate) table: String,
    pub(crate) primary_key: String,
    pub(crate) payload_json: String,
    pub(crate) op: String,
}

#[derive(Serialize)]
pub(crate) struct StoreHtapExportResponse {
    pub(crate) status: &'static str,
    pub(crate) since_sequence: u64,
    pub(crate) mutation_count: usize,
    pub(crate) checkpoint_last_sequence: u64,
    pub(crate) mutations: Vec<HtapMutationEntry>,
}

// ─── S4-WS3-03: columnar scan response (vectorized OLAP executor) ─────────────

#[derive(Serialize)]
pub(crate) struct ColumnarScanColumn {
    pub(crate) name: String,
    pub(crate) type_hint: String,
    pub(crate) row_count: usize,
    pub(crate) sample_values: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct ColumnarScanResponse {
    pub(crate) status: &'static str,
    pub(crate) rows_scanned: usize,
    pub(crate) columns_materialized: usize,
    pub(crate) elapsed_us: u128,
    pub(crate) columns: Vec<ColumnarScanColumn>,
}

// ─── S4-WS3-02: Columnar projection query/response ───────────────────────────

#[derive(Debug, Deserialize, Default)]
pub(crate) struct ColumnarProjectQuery {
    /// Comma-separated list of column names to project; empty = all columns.
    pub(crate) columns: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct ColumnarProjectResponse {
    pub(crate) status: &'static str,
    pub(crate) rows_scanned: usize,
    pub(crate) columns_projected: usize,
    pub(crate) elapsed_us: u128,
    pub(crate) columns: Vec<ColumnarScanColumn>,
}

// ─── S4-WS3-03: Columnar aggregate query/response ───────────────────────────

#[derive(Debug, Deserialize, Default)]
pub(crate) struct ColumnarAggregateQuery {
    /// Column name to aggregate; defaults to the first column in the batch.
    pub(crate) column: Option<String>,
    /// Aggregation operation: "count" (default), "sum", "avg", "min", "max".
    pub(crate) op: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct ColumnarAggregateResponse {
    pub(crate) status: &'static str,
    pub(crate) op: String,
    pub(crate) column: String,
    pub(crate) result: String,
    pub(crate) rows_scanned: usize,
}

// ─── S4-WS3-04: HTAP OLAP consumer structs ───────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct OlapApplyMutation {
    pub(crate) sequence: u64,
    pub(crate) primary_key: String,
    pub(crate) payload_json: String,
    pub(crate) op: String, // "insert" | "update" | "delete"
}

#[derive(Deserialize)]
pub(crate) struct StoreHtapApplyRequest {
    pub(crate) mutations: Vec<OlapApplyMutation>,
}

#[derive(Serialize)]
pub(crate) struct StoreHtapApplyResponse {
    pub(crate) status: &'static str,
    pub(crate) applied_count: usize,
    pub(crate) last_applied_sequence: u64,
}

#[derive(Serialize)]
pub(crate) struct OlapScanRow {
    pub(crate) key: String,
    pub(crate) data: HashMap<String, String>,
}

#[derive(Serialize)]
pub(crate) struct StoreHtapOlapScanResponse {
    pub(crate) status: &'static str,
    pub(crate) row_count: usize,
    pub(crate) rows: Vec<OlapScanRow>,
}

/// Response for `GET /api/v1/store/htap/lag`.
#[derive(Serialize)]
pub(crate) struct HtapLagResponse {
    pub(crate) status: &'static str,
    pub(crate) sync_origin_pending: usize,
    pub(crate) olap_row_count: usize,
    pub(crate) estimated_lag_mutations: usize,
}

// ─── S4-WS3-04: HTAP force-sync response ────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct HtapForceSyncResponse {
    pub(crate) status: &'static str,
    pub(crate) mutations_applied: usize,
    pub(crate) olap_row_count_after: usize,
}

// ─── S4-WS3-04: HTAP detailed status response ───────────────────────────────

#[derive(Serialize)]
pub(crate) struct HtapStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) sync_origin_pending: usize,
    pub(crate) olap_row_count: usize,
    pub(crate) last_sync_ms: u64,
    pub(crate) sync_lag_estimate: i64,
    pub(crate) is_synchronized: bool,
}

// ─── S2-WS2-04: Row store prefix count structs ───────────────────────────────

#[derive(Deserialize, Default)]
pub(crate) struct RowCountQuery {
    pub(crate) key_prefix: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct RowCountResponse {
    pub(crate) status: &'static str,
    pub(crate) snapshot_xid: u64,
    pub(crate) key_prefix: Option<String>,
    pub(crate) count: usize,
}

// ─── S2-WS2-04: Row store delete-by-key structs ──────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct RowDeleteRequest {
    pub(crate) key: String,
}

#[derive(Serialize)]
pub(crate) struct RowDeleteResponse {
    pub(crate) status: &'static str,
    pub(crate) key: String,
    pub(crate) deleted: bool,
}

// ─── S11-WS1-10: Row store keys structs ─────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct StoreRowsKeysQuery {
    pub(crate) prefix: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StoreRowsKeysResponse {
    pub(crate) status: &'static str,
    pub(crate) total_keys: usize,
    pub(crate) keys: Vec<String>,
}

// ─── S11-WS1-11: Row store version structs ──────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct RowStoreVersionResponse {
    pub(crate) status: &'static str,
    pub(crate) current_xid: u64,
    pub(crate) page_count: usize,
    pub(crate) total_rows: usize,
}

// ─── S11-WS1-11: HTAP stats structs ─────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct HtapStatsResponse {
    pub(crate) status: &'static str,
    pub(crate) table_count: usize,
    pub(crate) total_entries: usize,
}

// ─── Index lookup / constraint DTOs ──────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct IndexLookupRequest {
    pub(crate) index_name: String,
    pub(crate) value: String,
}

#[derive(Serialize)]
pub(crate) struct IndexLookupResponse {
    pub(crate) status: &'static str,
    pub(crate) index_name: String,
    pub(crate) value: String,
    pub(crate) row_keys: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct AddConstraintRequest {
    pub(crate) name: String,
    pub(crate) table: String,
    pub(crate) column: String,
    pub(crate) kind: String,
}

#[derive(Serialize)]
pub(crate) struct AddConstraintResponse {
    pub(crate) status: &'static str,
    pub(crate) constraint_name: String,
    pub(crate) table: String,
    pub(crate) column: String,
    pub(crate) kind: String,
}

#[derive(Deserialize)]
pub(crate) struct ValidateConstraintRequest {
    pub(crate) table: String,
    pub(crate) column: String,
    pub(crate) value: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct ValidateConstraintResponse {
    pub(crate) status: &'static str,
    pub(crate) valid: bool,
    pub(crate) violation: Option<String>,
}

// ─── Handler functions ────────────────────────────────────────────────────────

pub(crate) async fn htap_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<HtapStatusResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let sync_origin_pending = {
        let so = state.sync_origin.lock().expect("sync_origin lock");
        so.pending_len()
    };
    let olap_row_count = state.olap_store.lock().expect("olap_store lock").len();
    let last_sync_ms = now_unix_ms_u64();
    let sync_lag_estimate = sync_origin_pending as i64 - olap_row_count as i64;
    let is_synchronized = sync_origin_pending == 0;
    Ok((StatusCode::OK, Json(HtapStatusResponse {
        status: "ok",
        sync_origin_pending,
        olap_row_count,
        last_sync_ms,
        sync_lag_estimate,
        is_synchronized,
    })))
}

pub(crate) async fn store_rows_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<StoreRowsKeysQuery>,
) -> Result<(StatusCode, Json<StoreRowsKeysResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock store_rows_keys");
    let all_rows = rs.export_rows_snapshot();
    drop(rs);
    let keys: Vec<String> = all_rows
        .into_iter()
        .map(|(k, _)| k)
        .filter(|k| {
            params.prefix.as_ref().map(|p| k.starts_with(p.as_str())).unwrap_or(true)
        })
        .collect();
    let total_keys = keys.len();
    Ok((StatusCode::OK, Json(StoreRowsKeysResponse {
        status: "ok",
        total_keys,
        keys,
    })))
}

/// S11-WS1-11: Return the current transaction ID and basic stats for the MVCC row store.
pub(crate) async fn row_store_version(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowStoreVersionResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock row_store_version");
    let current_xid = rs.current_xid();
    let page_count = rs.page_count();
    let total_rows = rs.total_row_count();
    drop(rs);
    Ok((StatusCode::OK, Json(RowStoreVersionResponse {
        status: "ok",
        current_xid,
        page_count,
        total_rows,
    })))
}

/// S11-WS1-11: Return entry counts from the in-memory OLAP replica store.
pub(crate) async fn htap_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<HtapStatsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let olap = state.olap_store.lock().expect("olap_store lock htap_stats");
    let table_count = olap.len();
    let total_entries: usize = olap.values().map(|rows| rows.len()).sum();
    drop(olap);
    Ok((StatusCode::OK, Json(HtapStatsResponse {
        status: "ok",
        table_count,
        total_entries,
    })))
}

pub(crate) async fn row_store_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowSnapshotResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let store = state.row_store.lock().expect("row_store lock snapshot");
    let snapshot_xid = store.current_xid();
    let rows: Vec<RowSnapshotEntry> = store
        .export_rows_snapshot()
        .into_iter()
        .map(|(key, payload)| RowSnapshotEntry { key, payload })
        .collect();
    let row_count = rows.len();
    Ok((StatusCode::OK, Json(RowSnapshotResponse {
        status: "ok",
        snapshot_xid,
        row_count,
        rows,
    })))
}

/// S2-WS2-04: Return operational statistics for the MVCC page-based row store.
pub(crate) async fn row_store_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<RowStoreStatsResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock stats");
    let current_xid = rs.current_xid();
    let total_pages = rs.page_count();
    let total_rows = rs.total_row_count();
    let total_visible_rows = rs.visible_row_count(current_xid);
    drop(rs);
    Ok((StatusCode::OK, Json(RowStoreStatsResponse {
        status: "ok",
        current_xid,
        total_pages,
        total_rows,
        total_visible_rows,
    })))
}

/// S2-WS2-04: Count visible rows at current snapshot, optionally filtered by key prefix.
pub(crate) async fn row_store_count(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<RowCountQuery>,
) -> Result<(StatusCode, Json<RowCountResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let rs = state.row_store.lock().expect("row_store lock count");
    let snapshot_xid = rs.current_xid();
    let count = {
        let all_rows = rs.scan_at_snapshot(snapshot_xid);
        match &params.key_prefix {
            Some(prefix) => all_rows.iter().filter(|(k, _)| k.starts_with(prefix.as_str())).count(),
            None => all_rows.len(),
        }
    };
    drop(rs);
    Ok((StatusCode::OK, Json(RowCountResponse {
        status: "ok",
        snapshot_xid,
        key_prefix: params.key_prefix.clone(),
        count,
    })))
}

/// S2-WS2-04: Delete a specific row by key from the row store and WAL.
pub(crate) async fn row_store_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RowDeleteRequest>,
) -> Result<(StatusCode, Json<RowDeleteResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let mut rs = state.row_store.lock().expect("row_store lock delete");
    let snapshot_xid = rs.current_xid();
    let rows = rs.scan_at_snapshot(snapshot_xid);
    let exists = rows.iter().any(|(k, _)| *k == req.key.as_str());
    if exists {
        let xid = rs.begin_xid();
        rs.delete(xid, &req.key);
        drop(rs);
        let mut wal = state.wal_engine.lock().expect("wal_engine lock delete");
        wal.append_mutation(&req.key, "__deleted__");
    } else {
        drop(rs);
    }
    Ok((StatusCode::OK, Json(RowDeleteResponse {
        status: "ok",
        key: req.key,
        deleted: exists,
    })))
}

pub(crate) async fn store_list_indexes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ListIndexesResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/indexes",
    )?;
    let mgr = state.index_manager.lock().expect("index lock");
    let indexes = mgr
        .list_indexes()
        .iter()
        .filter(|descriptor| match &principal {
            RuntimeAccessPrincipal::Operator(_) => true,
            RuntimeAccessPrincipal::TenantUser(user) => {
                store_table_matches_tenant_namespace(&descriptor.table, &user.tenant_id)
            }
        })
        .map(|d| IndexListEntry {
            name: d.name.clone(),
            table: d.table.clone(),
            column: d.column.clone(),
            kind: format!("{:?}", d.kind),
            unique: d.unique,
        })
        .collect();
    let response = ListIndexesResponse {
        status: "ok",
        indexes,
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Storage,
        &principal,
        "store_list_indexes",
        "ok",
        json!({
            "route_scope": "store/indexes",
            "visible_index_count": response.indexes.len(),
        }),
    );
    Ok(Json(response))
}

pub(crate) async fn store_create_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateIndexRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Manage,
        "store/indexes",
    )?;
    ensure_store_table_access(&principal, &headers, &req.table)?;
    use voltnuerongrid_store::index::{IndexDescriptor, IndexKind};
    let unique = req.unique.unwrap_or(false);
    let descriptor = IndexDescriptor {
        name: req.name.clone(),
        table: req.table.clone(),
        column: req.column.clone(),
        kind: IndexKind::BTree,
        unique,
    };
    let mut mgr = state.index_manager.lock().expect("index lock");
    Ok(match mgr.create_index(descriptor) {
        Ok(()) => {
            let response = CreateIndexResponse {
                status: "created",
                index_name: req.name,
                table: req.table,
                column: req.column,
                unique,
            };
            append_runtime_audit_event(
                &state,
                AuditEventKind::Storage,
                &principal,
                "store_create_index",
                "ok",
                json!({
                    "route_scope": "store/indexes/create",
                    "index_name": response.index_name,
                    "table": response.table,
                    "column": response.column,
                    "unique": response.unique,
                }),
            );
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(response).expect("json")),
            )
        }
        Err(e) => (
            StatusCode::CONFLICT,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        ),
    })
}

pub(crate) async fn store_drop_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DropIndexRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Manage,
        "store/indexes",
    )?;
    let mut mgr = state.index_manager.lock().expect("index lock");
    Ok(match mgr.get(&req.name) {
        Some(idx) => {
            ensure_store_table_access(&principal, &headers, &idx.descriptor().table)?;
            match mgr.drop_index(&req.name) {
                Ok(desc) => {
                    let response = DropIndexResponse {
                        status: "dropped",
                        dropped: req.name,
                    };
                    append_runtime_audit_event(
                        &state,
                        AuditEventKind::Storage,
                        &principal,
                        "store_drop_index",
                        "ok",
                        json!({
                            "route_scope": "store/indexes/drop",
                            "index_name": response.dropped,
                            "table": desc.table,
                            "column": desc.column,
                        }),
                    );
                    (
                        StatusCode::OK,
                        Json(serde_json::to_value(response).expect("json")),
                    )
                }
                Err(e) => (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "status": "error", "reason": e.to_string() })),
                ),
            }
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "reason": format!("index '{}' not found", req.name) })),
        ),
    })
}

pub(crate) async fn store_index_lookup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IndexLookupRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/indexes/lookup",
    )?;
    let mgr = state.index_manager.lock().expect("index lock");
    Ok(match mgr.get(&req.index_name) {
        Some(idx) => {
            ensure_store_table_access(&principal, &headers, &idx.descriptor().table)?;
            let row_keys: Vec<String> = idx.lookup(&req.value).iter().map(|s| s.to_string()).collect();
            let response = IndexLookupResponse {
                status: "ok",
                index_name: req.index_name,
                value: req.value,
                row_keys,
            };
            append_runtime_audit_event(
                &state,
                AuditEventKind::Storage,
                &principal,
                "store_index_lookup",
                "ok",
                json!({
                    "route_scope": "store/indexes/lookup",
                    "index_name": response.index_name,
                    "value": response.value,
                    "row_key_count": response.row_keys.len(),
                    "table": idx.descriptor().table,
                }),
            );
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).expect("json")),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "reason": format!("index '{}' not found", req.index_name) })),
        ),
    })
}

pub(crate) async fn store_add_constraint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AddConstraintRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Manage,
        "store/constraints",
    )?;
    ensure_store_table_access(&principal, &headers, &req.table)?;
    use voltnuerongrid_store::constraints::{ConstraintDescriptor, ConstraintKind};
    let kind = match req.kind.to_ascii_lowercase().as_str() {
        "primary_key" | "pk" => ConstraintKind::PrimaryKey,
        "unique" => ConstraintKind::Unique,
        "not_null" => ConstraintKind::NotNull,
        "foreign_key" | "fk" => ConstraintKind::ForeignKey,
        other => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(json!({ "status": "error", "reason": format!("unknown constraint kind: {other}") })),
            ));
        }
    };
    let descriptor = ConstraintDescriptor {
        name: req.name.clone(),
        table: req.table.clone(),
        column: req.column.clone(),
        kind,
    };
    let mut mgr = state.constraint_manager.lock().expect("constraint lock");
    Ok(match mgr.add_constraint(descriptor) {
        Ok(()) => {
            let response = AddConstraintResponse {
                status: "created",
                constraint_name: req.name,
                table: req.table,
                column: req.column,
                kind: req.kind,
            };
            append_runtime_audit_event(
                &state,
                AuditEventKind::Storage,
                &principal,
                "store_add_constraint",
                "ok",
                json!({
                    "route_scope": "store/constraints/add",
                    "constraint_name": response.constraint_name,
                    "table": response.table,
                    "column": response.column,
                    "kind": response.kind,
                }),
            );
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(response).expect("json")),
            )
        }
        Err(e) => (
            StatusCode::CONFLICT,
            Json(json!({ "status": "error", "reason": e.to_string() })),
        ),
    })
}

pub(crate) async fn store_validate_constraint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ValidateConstraintRequest>,
) -> Result<Json<ValidateConstraintResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/constraints/validate",
    )?;
    ensure_store_table_access(&principal, &headers, &req.table)?;
    let mgr = state.constraint_manager.lock().expect("constraint lock");
    Ok(match mgr.validate(&req.table, &req.column, req.value.as_deref()) {
        Ok(()) => {
            let response = ValidateConstraintResponse {
                status: "ok",
                valid: true,
                violation: None,
            };
            append_runtime_audit_event(
                &state,
                AuditEventKind::Storage,
                &principal,
                "store_validate_constraint",
                "ok",
                json!({
                    "route_scope": "store/constraints/validate",
                    "table": req.table,
                    "column": req.column,
                    "valid": response.valid,
                }),
            );
            Json(response)
        }
        Err(v) => {
            let violation = v.to_string();
            append_runtime_audit_event(
                &state,
                AuditEventKind::Storage,
                &principal,
                "store_validate_constraint",
                "ok",
                json!({
                    "route_scope": "store/constraints/validate",
                    "table": req.table,
                    "column": req.column,
                    "valid": false,
                    "violation": violation,
                }),
            );
            Json(ValidateConstraintResponse {
                status: "ok",
                valid: false,
                violation: Some(violation),
            })
        }
    })
}

// S5-WS4-03 / S2-WS2-04: scan MVCC PagedRowStore at a snapshot
pub(crate) async fn store_rows_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<StoreRowsScanRequest>,
) -> Result<(StatusCode, Json<StoreRowsScanResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/rows/scan",
    )?;
    let rs = state.row_store.lock().expect("row_store lock");
    let snapshot_xid = req.snapshot_xid.unwrap_or_else(|| rs.current_xid());
    let key_prefix = req.key_prefix.unwrap_or_default();
    let limit = req.limit.unwrap_or(1_000).min(10_000);
    let rows: Vec<StoreRowEntry> = rs
        .scan_at_snapshot(snapshot_xid)
        .into_iter()
        .filter(|(k, _)| key_prefix.is_empty() || k.starts_with(key_prefix.as_str()))
        .take(limit)
        .map(|(k, d)| StoreRowEntry {
            key: k.to_string(),
            data: d.clone(),
        })
        .collect();
    let row_count = rows.len();
    append_runtime_audit_event(
        &state,
        AuditEventKind::Storage,
        &principal,
        "store_rows_scan",
        "ok",
        json!({
            "route_scope": "store/rows/scan",
            "snapshot_xid": snapshot_xid,
            "row_count": row_count,
            "key_prefix": key_prefix,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(StoreRowsScanResponse {
            status: "ok",
            snapshot_xid,
            row_count,
            rows,
        }),
    ))
}

/// S4-WS3-04: HTAP sync export — returns pending mutations from `RowStoreSyncOrigin`.
pub(crate) async fn store_htap_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<StoreHtapExportRequest>,
) -> Result<(StatusCode, Json<StoreHtapExportResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/htap/export",
    )?;
    let since = req.since_sequence.unwrap_or(0);
    let max_items = req.max_items.unwrap_or(500).min(5_000);
    let mutations = {
        use voltnuerongrid_store::htap_sync::MutationOp;
        let origin = state.sync_origin.lock().expect("sync_origin lock");
        let checkpoint = origin.checkpoint();
        let raw = origin.export_since(since, max_items);
        let entries: Vec<HtapMutationEntry> = raw
            .into_iter()
            .map(|m| HtapMutationEntry {
                sequence: m.sequence,
                table: m.table,
                primary_key: m.primary_key,
                payload_json: m.payload_json,
                op: match m.op {
                    MutationOp::Insert => "insert",
                    MutationOp::Update => "update",
                    MutationOp::Delete => "delete",
                }
                .to_string(),
            })
            .collect();
        (entries, checkpoint.last_sequence)
    };
    let (entries, last_sequence) = mutations;
    let mutation_count = entries.len();
    append_runtime_audit_event(
        &state,
        AuditEventKind::Storage,
        &principal,
        "store_htap_export",
        "ok",
        json!({
            "route_scope": "store/htap/export",
            "since_sequence": since,
            "mutation_count": mutation_count,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(StoreHtapExportResponse {
            status: "ok",
            since_sequence: since,
            mutation_count,
            checkpoint_last_sequence: last_sequence,
            mutations: entries,
        }),
    ))
}

/// S4-WS3-03: vectorized columnar scan — reads committed rows from PagedRowStore
/// and materialises them as typed column batches for OLAP consumers.
pub(crate) async fn store_columnar_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ColumnarScanResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/columnar/scan",
    )?;
    let (batch, stats) = {
        use voltnuerongrid_store::columnar::vectorized_scan;
        let rs = state.row_store.lock().expect("row_store lock columnar_scan");
        let snapshot_xid = rs.current_xid();
        let raw_rows: Vec<(String, std::collections::HashMap<String, String>)> = rs
            .scan_at_snapshot(snapshot_xid)
            .into_iter()
            .map(|(k, d)| (k.to_string(), d.clone()))
            .collect();
        vectorized_scan(&raw_rows, 10_000)
    };
    let columns: Vec<ColumnarScanColumn> = batch
        .column_names
        .iter()
        .filter_map(|name| {
            batch.columns.get(name).map(|cv| {
                use voltnuerongrid_store::columnar::ColumnVector;
                let type_hint = match cv {
                    ColumnVector::Int64(_) => "int64",
                    ColumnVector::Float64(_) => "float64",
                    ColumnVector::Bool(_) => "bool",
                    ColumnVector::Utf8(_) => "utf8",
                    ColumnVector::Null(_) => "null",
                };
                let sample_values: Vec<String> = (0..cv.len().min(3))
                    .filter_map(|i| cv.value_as_str(i))
                    .collect();
                ColumnarScanColumn {
                    name: name.clone(),
                    type_hint: type_hint.to_string(),
                    row_count: cv.len(),
                    sample_values,
                }
            })
        })
        .collect();
    append_runtime_audit_event(
        &state,
        AuditEventKind::Storage,
        &principal,
        "store_columnar_scan",
        "ok",
        json!({
            "route_scope": "store/columnar/scan",
            "rows_scanned": stats.rows_scanned,
            "columns_materialized": stats.columns_materialized,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(ColumnarScanResponse {
            status: "ok",
            rows_scanned: stats.rows_scanned,
            columns_materialized: stats.columns_materialized,
            elapsed_us: stats.elapsed_us,
            columns,
        }),
    ))
}

// ─── S4-WS3-02: Columnar column projection ───────────────────────────────────

/// S4-WS3-02: Project specific columns from the columnar store.
/// Accepts `?columns=a,b,c`; returns all when parameter is absent.
pub(crate) async fn store_columnar_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ColumnarProjectQuery>,
) -> Result<(StatusCode, Json<ColumnarProjectResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_store_runtime_principal(&headers, &state, PrivilegeAction::Read, "store/columnar/project")?;
    let (batch, stats) = {
        use voltnuerongrid_store::columnar::vectorized_scan;
        let rs = state.row_store.lock().expect("row_store lock columnar_project");
        let snapshot_xid = rs.current_xid();
        let raw_rows: Vec<(String, std::collections::HashMap<String, String>)> = rs
            .scan_at_snapshot(snapshot_xid)
            .into_iter()
            .map(|(k, d)| (k.to_string(), d.clone()))
            .collect();
        vectorized_scan(&raw_rows, 10_000)
    };
    // Build projection set — empty means "all columns"
    let requested: Vec<String> = params
        .columns
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    let columns: Vec<ColumnarScanColumn> = batch
        .column_names
        .iter()
        .filter(|name| requested.is_empty() || requested.contains(name))
        .filter_map(|name| {
            batch.columns.get(name).map(|cv| {
                use voltnuerongrid_store::columnar::ColumnVector;
                let type_hint = match cv {
                    ColumnVector::Int64(_) => "int64",
                    ColumnVector::Float64(_) => "float64",
                    ColumnVector::Bool(_) => "bool",
                    ColumnVector::Utf8(_) => "utf8",
                    ColumnVector::Null(_) => "null",
                };
                let sample_values: Vec<String> = (0..cv.len().min(3))
                    .filter_map(|i| cv.value_as_str(i))
                    .collect();
                ColumnarScanColumn {
                    name: name.clone(),
                    type_hint: type_hint.to_string(),
                    row_count: cv.len(),
                    sample_values,
                }
            })
        })
        .collect();
    let columns_projected = columns.len();
    Ok((
        StatusCode::OK,
        Json(ColumnarProjectResponse {
            status: "ok",
            rows_scanned: stats.rows_scanned,
            columns_projected,
            elapsed_us: stats.elapsed_us,
            columns,
        }),
    ))
}

// ─── S4-WS3-03: Columnar vectorized aggregate ───────────────────────────────

/// S4-WS3-03: Run a vectorized aggregate over the OLAP columnar snapshot.
pub(crate) async fn store_columnar_aggregate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ColumnarAggregateQuery>,
) -> Result<(StatusCode, Json<ColumnarAggregateResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_store_runtime_principal(&headers, &state, PrivilegeAction::Read, "store/columnar/aggregate")?;
    use voltnuerongrid_store::columnar::{vectorized_scan, VectorizedAggOp, aggregate_batch};
    let (batch, stats) = {
        let rs = state.row_store.lock().expect("row_store lock columnar_aggregate");
        let snapshot_xid = rs.current_xid();
        let raw_rows: Vec<(String, std::collections::HashMap<String, String>)> = rs
            .scan_at_snapshot(snapshot_xid)
            .into_iter()
            .map(|(k, d)| (k.to_string(), d.clone()))
            .collect();
        vectorized_scan(&raw_rows, 10_000)
    };
    // Resolve the target column (default: first available column).
    let col_name = params.column
        .filter(|c| !c.trim().is_empty())
        .or_else(|| batch.column_names.first().cloned())
        .unwrap_or_else(|| "payload".to_string());
    // Resolve the operation (default: count).
    let agg_op = match params.op.as_deref().unwrap_or("count") {
        "sum" => VectorizedAggOp::Sum,
        "avg" => VectorizedAggOp::Avg,
        "min" => VectorizedAggOp::Min,
        "max" => VectorizedAggOp::Max,
        _ => VectorizedAggOp::Count,
    };
    let op_str = format!("{:?}", agg_op).to_lowercase();
    let mut ops = std::collections::HashMap::new();
    ops.insert(col_name.clone(), agg_op);
    let results = aggregate_batch(&batch, &ops);
    let result_val = results
        .get(&col_name)
        .map_or("null".to_string(), |r| r.value.clone());
    Ok((StatusCode::OK, Json(ColumnarAggregateResponse {
        status: "ok",
        op: op_str,
        column: col_name,
        result: result_val,
        rows_scanned: stats.rows_scanned,
    })))
}

/// Apply a batch of HTAP mutations to the in-memory OLAP replica.
pub(crate) async fn store_htap_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<StoreHtapApplyRequest>,
) -> Result<(StatusCode, Json<StoreHtapApplyResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Write,
        "store/htap/apply",
    )?;
    let mut olap = state.olap_store.lock().expect("olap_store lock");
    let mut last_seq = 0u64;
    let mut applied = 0usize;
    for m in &req.mutations {
        last_seq = last_seq.max(m.sequence);
        match m.op.as_str() {
            "insert" | "update" => {
                // Parse the payload JSON into a HashMap<String, String>
                let data: HashMap<String, String> = serde_json::from_str(&m.payload_json)
                    .unwrap_or_else(|_| {
                        let mut d = HashMap::new();
                        d.insert("payload".to_string(), m.payload_json.clone());
                        d
                    });
                olap.insert(m.primary_key.clone(), data);
                applied += 1;
            }
            "delete" => {
                olap.remove(&m.primary_key);
                applied += 1;
            }
            _ => {}
        }
    }
    drop(olap);
    append_runtime_audit_event(
        &state,
        AuditEventKind::Storage,
        &principal,
        "store_htap_apply",
        "ok",
        json!({
            "route_scope": "store/htap/apply",
            "applied_count": applied,
            "last_applied_sequence": last_seq,
        }),
    );
    Ok((
        StatusCode::OK,
        Json(StoreHtapApplyResponse {
            status: "ok",
            applied_count: applied,
            last_applied_sequence: last_seq,
        }),
    ))
}

/// Scan all rows in the in-memory OLAP replica.
pub(crate) async fn store_htap_olap_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<StoreHtapOlapScanResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/htap/olap/scan",
    )?;
    let olap = state.olap_store.lock().expect("olap_store lock");
    let rows: Vec<OlapScanRow> = olap
        .iter()
        .map(|(k, v)| OlapScanRow { key: k.clone(), data: v.clone() })
        .collect();
    let row_count = rows.len();
    Ok((
        StatusCode::OK,
        Json(StoreHtapOlapScanResponse { status: "ok", row_count, rows }),
    ))
}

/// S4-WS3-04: Return HTAP sync lag — pending mutations in sync_origin vs OLAP row count.
pub(crate) async fn htap_lag(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<HtapLagResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _principal = require_store_runtime_principal(
        &headers,
        &state,
        PrivilegeAction::Read,
        "store/htap/lag",
    )?;
    let sync_origin_pending = state.sync_origin.lock().expect("sync_origin lock htap_lag").pending_len();
    let olap_row_count = state.olap_store.lock().expect("olap_store lock htap_lag").len();
    let estimated_lag_mutations = sync_origin_pending.saturating_sub(olap_row_count);
    Ok((StatusCode::OK, Json(HtapLagResponse {
        status: "ok",
        sync_origin_pending,
        olap_row_count,
        estimated_lag_mutations,
    })))
}

// ─── S4-WS3-04: HTAP force-sync handler ─────────────────────────────────────

/// S4-WS3-04: Drain all pending sync_origin mutations into the in-memory OLAP replica
/// in one atomic sweep, then acknowledge them.
pub(crate) async fn htap_force_sync(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<HtapForceSyncResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_store_runtime_principal(&headers, &state, PrivilegeAction::Write, "store/htap/sync")?;
    // Collect all pending mutations without dropping/acking yet.
    let batch = state.sync_origin.lock().expect("sync_origin lock htap_force_sync")
        .export_batch(usize::MAX);
    let mut applied = 0usize;
    let mut last_seq = 0u64;
    {
        let mut olap = state.olap_store.lock().expect("olap_store lock htap_force_sync");
        for m in &batch {
            last_seq = last_seq.max(m.sequence);
            match m.op {
                MutationOp::Insert | MutationOp::Update => {
                    let data: HashMap<String, String> = serde_json::from_str(&m.payload_json)
                        .unwrap_or_else(|_| {
                            let mut d = HashMap::new();
                            d.insert("payload".to_string(), m.payload_json.clone());
                            d
                        });
                    olap.insert(m.primary_key.clone(), data);
                    applied += 1;
                }
                MutationOp::Delete => {
                    olap.remove(&m.primary_key);
                    applied += 1;
                }
            }
        }
    }
    // Acknowledge all exported mutations so they are removed from the sync_origin queue.
    if last_seq > 0 {
        state.sync_origin.lock().expect("sync_origin ack lock").ack_through(last_seq);
    }
    let olap_row_count_after = state.olap_store.lock().expect("olap_store count lock").len();
    Ok((StatusCode::OK, Json(HtapForceSyncResponse {
        status: "ok",
        mutations_applied: applied,
        olap_row_count_after,
    })))
}
