//! HTTP handlers for the UDF (User-Defined Function) management API.
//!
//! Routes:
//!   POST   /api/v1/udf/register  — register a WASM, JS, or Python UDF
//!   GET    /api/v1/udf/list      — list registered UDFs
//!   POST   /api/v1/udf/call      — call a registered UDF by name

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    helpers::udf::{wasm_memory_limit_mb, wasm_fuel_limit, js_timeout_ms, python_timeout_ms},
    AppState,
};
use crate::auth::require_admin_api_key;

// ── DTOs ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct UdfRegisterRequest {
    pub(crate) name: String,
    /// `"rust"` | `"javascript"` | `"python"`
    pub(crate) language: String,
    /// Base64-encoded WASM bytes (required when `language == "rust"`).
    pub(crate) wasm_base64: Option<String>,
    /// Function source code (required when `language == "javascript"` or `"python"`).
    pub(crate) source_code: Option<String>,
}

#[derive(Debug, Serialize)]
struct UdfListEntry {
    name: String,
    language: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UdfCallRequest {
    pub(crate) name: String,
    pub(crate) args: Vec<String>,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /api/v1/udf/register`
///
/// Register a WASM, JavaScript, or Python UDF.  Admin key required.
pub(crate) async fn udf_register_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UdfRegisterRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_admin_api_key(&headers, &state) {
        return e.into_response();
    }

    let lang = req.language.to_ascii_lowercase();
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "status": "error", "message": "name must not be empty" })),
        )
            .into_response();
    }

    let result: Result<(), String> = match lang.as_str() {
        "rust" => {
            let wasm_b64 = match req.wasm_base64 {
                Some(b) => b,
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "status": "error",
                            "message": "wasm_base64 required for language=rust" })),
                    )
                        .into_response();
                }
            };
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&wasm_b64)
                .map_err(|e| format!("wasm_base64_decode_error: {e}"));
            match bytes {
                Ok(b) => state
                    .ops.udf_registry
                    .lock()
                    .expect("udf_registry lock")
                    .register_wasm(&name, b, wasm_memory_limit_mb(), wasm_fuel_limit()),
                Err(e) => Err(e),
            }
        }
        "javascript" => {
            let src = match req.source_code {
                Some(s) => s,
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "status": "error",
                            "message": "source_code required for language=javascript" })),
                    )
                        .into_response();
                }
            };
            state
                .ops.udf_registry
                .lock()
                .expect("udf_registry lock")
                .register_js(&name, &src, js_timeout_ms())
        }
        "python" => {
            let src = match req.source_code {
                Some(s) => s,
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "status": "error",
                            "message": "source_code required for language=python" })),
                    )
                        .into_response();
                }
            };
            state
                .ops.udf_registry
                .lock()
                .expect("udf_registry lock")
                .register_python(&name, &src, python_timeout_ms())
        }
        other => Err(format!("unsupported_language: {other}")),
    };

    match result {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "registered",
                "name": name,
                "language": lang,
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

/// `GET /api/v1/udf/list`
///
/// Return all registered UDFs.  Admin key required.
pub(crate) async fn udf_list_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_admin_api_key(&headers, &state) {
        return e.into_response();
    }

    let entries: Vec<UdfListEntry> = state
        .ops.udf_registry
        .lock()
        .expect("udf_registry lock")
        .list()
        .into_iter()
        .map(|(name, language)| UdfListEntry { name, language })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "udfs": entries,
        })),
    )
        .into_response()
}

/// `POST /api/v1/udf/call`
///
/// Call a registered UDF with positional string arguments.
pub(crate) async fn udf_call_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UdfCallRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_admin_api_key(&headers, &state) {
        return e.into_response();
    }

    let args_ref: Vec<&str> = req.args.iter().map(|s| s.as_str()).collect();
    let result = state
        .ops.udf_registry
        .lock()
        .expect("udf_registry lock")
        .call(&req.name, &args_ref);

    match result {
        Ok(output) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ok",
                "name": req.name,
                "result": output,
            })),
        )
            .into_response(),
        Err(msg) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "status": "error", "message": msg })),
        )
            .into_response(),
    }
}
