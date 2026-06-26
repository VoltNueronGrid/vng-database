//! BR-1/BR-2: Backup and Restore API
//! POST /api/v1/backup/full    — export row store + catalog to JSON archive
//! GET  /api/v1/backup/list   — list available backup manifests
//! POST /api/v1/restore       — restore from a backup archive

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{AppState, AuthErrorResponse};
use crate::auth::require_admin_api_key;
use crate::audit_helpers::append_audit_event;
use voltnuerongrid_audit::AuditEventKind;

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── DTOs ─────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct BackupManifest {
    pub(crate) backup_id: String,
    pub(crate) backup_type: String,
    pub(crate) created_at_unix_ms: u128,
    pub(crate) table_count: usize,
    pub(crate) row_count: usize,
    pub(crate) catalog_entry_count: usize,
    pub(crate) data_file: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct BackupArchive {
    pub(crate) manifest: BackupManifest,
    /// All rows: key → HashMap<col, val>
    pub(crate) rows: HashMap<String, HashMap<String, String>>,
    /// DDL catalog entries serialised as raw_ddl strings
    pub(crate) catalog_ddl: Vec<String>,
    /// Index definitions: table → list of (index_name, column)
    pub(crate) index_entries: Vec<(String, String, String)>,
}

#[derive(Serialize)]
pub(crate) struct BackupFullResponse {
    pub(crate) status: &'static str,
    pub(crate) manifest: BackupManifest,
}

#[derive(Serialize)]
pub(crate) struct BackupListResponse {
    pub(crate) status: &'static str,
    pub(crate) backups: Vec<BackupManifest>,
}

#[derive(Deserialize)]
pub(crate) struct RestoreRequest {
    pub(crate) backup_id: String,
}

#[derive(Serialize)]
pub(crate) struct RestoreResponse {
    pub(crate) status: &'static str,
    pub(crate) backup_id: String,
    pub(crate) rows_restored: usize,
    pub(crate) catalog_entries_restored: usize,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn backup_dir(state: &AppState) -> std::path::PathBuf {
    let data_dir = &state.runtime_config.storage.data_dir;
    let base = if data_dir.is_empty() { "state" } else { data_dir.as_str() };
    std::path::PathBuf::from(base).join("backups")
}

fn manifest_path(dir: &std::path::Path, backup_id: &str) -> std::path::PathBuf {
    dir.join(format!("{backup_id}.manifest.json"))
}

fn archive_path(dir: &std::path::Path, backup_id: &str) -> std::path::PathBuf {
    dir.join(format!("{backup_id}.archive.json"))
}

fn load_manifest(dir: &std::path::Path, backup_id: &str) -> Option<BackupManifest> {
    let path = manifest_path(dir, backup_id);
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// BR-1: POST /api/v1/backup/full
pub(crate) async fn backup_full(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<BackupFullResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;

    let dir = backup_dir(&state);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(AuthErrorResponse {
            status: "error",
            reason: format!("backup_dir_create_failed: {e}"),
            locale: "en".to_string(),
            localized_message: format!("Could not create backup directory: {e}"),
        })));
    }

    let backup_id = format!("backup-{}", now_unix_secs());

    // Collect rows from row_store
    let rows: HashMap<String, HashMap<String, String>> = {
        let rs = state.row_store.lock().unwrap_or_else(|e| e.into_inner());
        let xid = rs.current_xid();
        rs.scan_at_snapshot(xid)
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    };

    // Collect catalog DDL
    let catalog_ddl: Vec<String> = {
        let cat = state.ddl_catalog.lock().unwrap_or_else(|e| e.into_inner());
        cat.all_entries().iter()
            .map(|e| e.original_statement.clone())
            .collect()
    };

    // Collect index entries
    let index_entries: Vec<(String, String, String)> = {
        let idx = state.index_manager.lock().unwrap_or_else(|e| e.into_inner());
        idx.list_indexes()
            .into_iter()
            .map(|d| (d.table.clone(), d.name.clone(), d.column.clone()))
            .collect()
    };

    let row_count = rows.len();
    let table_count = {
        let mut tables = std::collections::HashSet::new();
        for key in rows.keys() {
            if let Some(colon) = key.find(':') {
                tables.insert(&key[..colon]);
            }
        }
        tables.len()
    };

    let manifest = BackupManifest {
        backup_id: backup_id.clone(),
        backup_type: "full".to_string(),
        created_at_unix_ms: now_unix_ms(),
        table_count,
        row_count,
        catalog_entry_count: catalog_ddl.len(),
        data_file: format!("{backup_id}.archive.json"),
    };

    let archive = BackupArchive {
        manifest: manifest.clone(),
        rows,
        catalog_ddl,
        index_entries,
    };

    // Write archive
    let archive_json = serde_json::to_string_pretty(&archive).unwrap_or_default();
    if let Err(e) = std::fs::write(archive_path(&dir, &backup_id), &archive_json) {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(AuthErrorResponse {
            status: "error",
            reason: format!("backup_write_failed: {e}"),
            locale: "en".to_string(),
            localized_message: format!("Failed to write backup archive: {e}"),
        })));
    }

    // Write manifest
    let manifest_json = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    if let Err(e) = std::fs::write(manifest_path(&dir, &backup_id), &manifest_json) {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(AuthErrorResponse {
            status: "error",
            reason: format!("manifest_write_failed: {e}"),
            locale: "en".to_string(),
            localized_message: format!("Failed to write backup manifest: {e}"),
        })));
    }

    append_audit_event(
        &state,
        AuditEventKind::Storage,
        "admin",
        "backup_full",
        "ok",
        &json!({ "backup_id": backup_id, "row_count": row_count }).to_string(),
    );

    Ok(Json(BackupFullResponse { status: "ok", manifest }))
}

/// GET /api/v1/backup/list
pub(crate) async fn backup_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<BackupListResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;

    let dir = backup_dir(&state);
    let mut backups: Vec<BackupManifest> = Vec::new();

    if dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json")
                    && path.to_str().map(|s| s.contains(".manifest.")).unwrap_or(false)
                {
                    if let Ok(data) = std::fs::read_to_string(&path) {
                        if let Ok(m) = serde_json::from_str::<BackupManifest>(&data) {
                            backups.push(m);
                        }
                    }
                }
            }
        }
    }

    // Sort by creation time descending
    backups.sort_by(|a, b| b.created_at_unix_ms.cmp(&a.created_at_unix_ms));

    Ok(Json(BackupListResponse { status: "ok", backups }))
}

/// BR-2: POST /api/v1/restore
pub(crate) async fn restore_from_backup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RestoreRequest>,
) -> Result<Json<RestoreResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;

    let dir = backup_dir(&state);
    let archive_path = archive_path(&dir, &req.backup_id);

    let archive_json = std::fs::read_to_string(&archive_path).map_err(|e| {
        (StatusCode::NOT_FOUND, Json(AuthErrorResponse {
            status: "error",
            reason: format!("backup_not_found: {e}"),
            locale: "en".to_string(),
            localized_message: format!("Backup '{}' not found: {e}", req.backup_id),
        }))
    })?;

    let archive: BackupArchive = serde_json::from_str(&archive_json).map_err(|e| {
        (StatusCode::UNPROCESSABLE_ENTITY, Json(AuthErrorResponse {
            status: "error",
            reason: format!("backup_parse_failed: {e}"),
            locale: "en".to_string(),
            localized_message: format!("Failed to parse backup archive: {e}"),
        }))
    })?;

    // Restore rows into row_store
    let rows_restored = archive.rows.len();
    {
        let mut rs = state.row_store.lock().unwrap_or_else(|e| e.into_inner());
        // replace_all clears existing rows and replaces with archive rows
        rs.replace_all(archive.rows.into_iter());
    }

    // Restore DDL catalog entries
    let catalog_entries_restored = archive.catalog_ddl.len();
    {
        let mut cat = state.ddl_catalog.lock().unwrap_or_else(|e| e.into_inner());
        let now_ms = now_unix_ms();
        for ddl in &archive.catalog_ddl {
            if let Some(info) = voltnuerongrid_store::ddl_catalog::parse_ddl_info(ddl) {
                let _ = cat.record_create(
                    info.object_kind,
                    &info.database_name,
                    &info.schema_name,
                    &info.object_name,
                    ddl,
                    now_ms,
                    true,
                );
            }
        }
    }

    append_audit_event(
        &state,
        AuditEventKind::Storage,
        "admin",
        "restore_backup",
        "ok",
        &json!({
            "backup_id": req.backup_id,
            "rows_restored": rows_restored,
            "catalog_entries_restored": catalog_entries_restored,
        }).to_string(),
    );

    Ok(Json(RestoreResponse {
        status: "ok",
        backup_id: req.backup_id,
        rows_restored,
        catalog_entries_restored,
    }))
}
