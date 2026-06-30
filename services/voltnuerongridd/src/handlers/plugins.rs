//! HTTP handlers for the versioned plugin marketplace.
//!
//! Routes:
//!   POST   /api/v1/plugins/install         — install a signed plugin
//!   POST   /api/v1/plugins/upgrade         — upgrade to a newer version
//!   POST   /api/v1/plugins/downgrade       — downgrade to a prior version
//!   DELETE /api/v1/plugins/{id}            — uninstall a plugin
//!   GET    /api/v1/plugins/list            — list active plugins

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::{
    helpers::plugins::{now_ms, verify_sha256, PluginEntry, PluginState},
    AppState,
};
use crate::auth::require_admin_api_key;

// ── DTOs ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct PluginInstallRequest {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    /// Hex SHA-256 checksum of the plugin archive.
    pub(crate) checksum_sha256: String,
    /// Whether the manifest carries a valid signature (caller asserts this;
    /// in production the server would verify against a signing key).
    pub(crate) signed: Option<bool>,
    /// Optional base64-encoded plugin archive bytes (used for checksum
    /// verification when provided).
    pub(crate) archive_base64: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PluginUpgradeRequest {
    pub(crate) id: String,
    pub(crate) name: Option<String>,
    pub(crate) new_version: String,
    pub(crate) checksum_sha256: String,
    pub(crate) signed: Option<bool>,
    pub(crate) archive_base64: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PluginDowngradeRequest {
    pub(crate) id: String,
    pub(crate) target_version: String,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /api/v1/plugins/install`
pub(crate) async fn plugin_install_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PluginInstallRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_admin_api_key(&headers, &state) {
        return e.into_response();
    }

    // Optional checksum verification when archive bytes are provided.
    if let Some(b64) = &req.archive_base64 {
        use base64::Engine as _;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            if !verify_sha256(&bytes, &req.checksum_sha256) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "status": "error",
                        "message": "checksum_mismatch: archive SHA-256 does not match provided checksum"
                    })),
                )
                    .into_response();
            }
        }
    }

    // Signature guard: reject explicitly unsigned manifests.
    let signed = req.signed.unwrap_or(false);
    if !signed {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "unsigned_plugin: manifest must be signed before installation"
            })),
        )
            .into_response();
    }

    let entry = PluginEntry {
        id: req.id.clone(),
        name: req.name,
        version: req.version,
        checksum_sha256: req.checksum_sha256,
        signed,
        installed_at_ms: now_ms(),
        state: PluginState::Active,
    };

    let result = state
        .ops.plugin_registry
        .lock()
        .expect("plugin_registry lock")
        .install(entry);

    match result {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "installed",
                "id": req.id,
            })),
        )
            .into_response(),
        Err(msg) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "status": "error", "message": msg })),
        )
            .into_response(),
    }
}

/// `POST /api/v1/plugins/upgrade`
pub(crate) async fn plugin_upgrade_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PluginUpgradeRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_admin_api_key(&headers, &state) {
        return e.into_response();
    }

    // Optional checksum verification.
    if let Some(b64) = &req.archive_base64 {
        use base64::Engine as _;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            if !verify_sha256(&bytes, &req.checksum_sha256) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "status": "error",
                        "message": "checksum_mismatch"
                    })),
                )
                    .into_response();
            }
        }
    }

    let signed = req.signed.unwrap_or(false);
    if !signed {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "status": "error", "message": "unsigned_plugin" })),
        )
            .into_response();
    }

    let name = {
        let reg = state.ops.plugin_registry.lock().expect("plugin_registry lock");
        req.name
            .clone()
            .or_else(|| reg.get_current(&req.id).map(|e| e.name.clone()))
            .unwrap_or_else(|| req.id.clone())
    };

    let entry = PluginEntry {
        id: req.id.clone(),
        name,
        version: req.new_version,
        checksum_sha256: req.checksum_sha256,
        signed,
        installed_at_ms: now_ms(),
        state: PluginState::Active,
    };

    let result = state
        .ops.plugin_registry
        .lock()
        .expect("plugin_registry lock")
        .upgrade(&req.id, entry);

    match result {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "upgraded", "id": req.id })),
        )
            .into_response(),
        Err(msg) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "status": "error", "message": msg })),
        )
            .into_response(),
    }
}

/// `POST /api/v1/plugins/downgrade`
pub(crate) async fn plugin_downgrade_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PluginDowngradeRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_admin_api_key(&headers, &state) {
        return e.into_response();
    }

    let result = state
        .ops.plugin_registry
        .lock()
        .expect("plugin_registry lock")
        .downgrade(&req.id, &req.target_version);

    match result {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "downgraded",
                "id": req.id,
                "version": req.target_version,
            })),
        )
            .into_response(),
        Err(msg) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "status": "error", "message": msg })),
        )
            .into_response(),
    }
}

/// `DELETE /api/v1/plugins/{id}`
pub(crate) async fn plugin_uninstall_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_admin_api_key(&headers, &state) {
        return e.into_response();
    }

    let result = state
        .ops.plugin_registry
        .lock()
        .expect("plugin_registry lock")
        .uninstall(&id);

    match result {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "uninstalled", "id": id })),
        )
            .into_response(),
        Err(msg) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "status": "error", "message": msg })),
        )
            .into_response(),
    }
}

/// `GET /api/v1/plugins/list`
pub(crate) async fn plugin_list_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_admin_api_key(&headers, &state) {
        return e.into_response();
    }

    let active = state
        .ops.plugin_registry
        .lock()
        .expect("plugin_registry lock")
        .list_active();

    let entries: Vec<serde_json::Value> = active
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "name": e.name,
                "version": e.version,
                "signed": e.signed,
                "installed_at_ms": e.installed_at_ms,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "plugins": entries,
            "total": entries.len(),
        })),
    )
        .into_response()
}
