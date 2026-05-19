//! Gap #7: User management and authentication handlers.
//!
//! Routes:
//!   POST /api/v1/admin/users          — create a new user account (DBA only)
//!   DELETE /api/v1/admin/users/:id    — delete a user by user_id (DBA only)
//!   POST /api/v1/auth/login           — authenticate and return a session token

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::{AppState, AuthErrorResponse};
use crate::auth::require_sql_runtime_principal;
use crate::user_store::{user_to_wal, UserAccount};
use voltnuerongrid_auth::PrivilegeAction;

// ─── DTOs ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct CreateUserRequest {
    pub(crate) username: String,
    pub(crate) password: String,
    /// "dba", "operator", or "tenant_user"
    pub(crate) role: Option<String>,
    pub(crate) tenant_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CreateUserResponse {
    pub(crate) status: &'static str,
    pub(crate) user_id: String,
    pub(crate) username: String,
    pub(crate) role: String,
}

#[derive(Serialize)]
pub(crate) struct DeleteUserResponse {
    pub(crate) status: &'static str,
    pub(crate) user_id: String,
}

#[derive(Deserialize)]
pub(crate) struct LoginRequest {
    pub(crate) username: String,
    pub(crate) password: String,
}

#[derive(Serialize)]
pub(crate) struct LoginResponse {
    pub(crate) status: &'static str,
    pub(crate) token: String,
    pub(crate) user_id: String,
    pub(crate) username: String,
    pub(crate) role: String,
    pub(crate) expires_at_secs: u64,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /api/v1/admin/users` — create a user account. DBA-only.
pub(crate) async fn admin_create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<CreateUserResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    // Only DBA can create users.
    let _principal = require_sql_runtime_principal(
        &headers, &state, PrivilegeAction::Execute, "admin/users",
    )?;

    // Validate username
    let username = req.username.trim().to_ascii_lowercase();
    if username.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AuthErrorResponse {
                status: "error",
                reason: "username is required".to_string(),
                locale: "en".to_string(),
                localized_message: "Username is required".to_string(),
            }),
        ));
    }

    // Check for duplicate
    {
        let store = state.user_store.lock().expect("user_store lock");
        if store.get_by_username(&username).is_some() {
            return Err((
                StatusCode::CONFLICT,
                Json(AuthErrorResponse {
                    status: "error",
                    reason: format!("user '{username}' already exists"),
                    locale: "en".to_string(),
                    localized_message: format!("User '{username}' already exists"),
                }),
            ));
        }
    }

    let role = req.role.as_deref().unwrap_or("operator").to_ascii_lowercase();
    let password = req.password.clone();

    // bcrypt is CPU-intensive — must run on blocking thread.
    let hash = tokio::task::spawn_blocking(move || {
        bcrypt::hash(&password, 12).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AuthErrorResponse {
            status: "error",
            reason: format!("task error: {e}"),
            locale: "en".to_string(),
            localized_message: "Internal error".to_string(),
        }),
    ))?
    .map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AuthErrorResponse {
            status: "error",
            reason: format!("bcrypt error: {e}"),
            locale: "en".to_string(),
            localized_message: "Internal error".to_string(),
        }),
    ))?;

    let user_id = uuid::Uuid::new_v4().to_string();
    let created_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let account = UserAccount {
        user_id: user_id.clone(),
        username: username.clone(),
        role: role.clone(),
        tenant_id: req.tenant_id.clone(),
        created_ms,
        password_hash: hash,
    };

    // Persist to WAL before mutating in-memory state.
    let wal_line = user_to_wal(&account);
    {
        let mut wal = state.wal_engine.lock().expect("wal_engine lock");
        wal.append_sql(voltnuerongrid_store::SqlWalKind::Ddl, &wal_line);
    }

    // Insert into in-memory store.
    {
        let mut store = state.user_store.lock().expect("user_store lock");
        store.insert(account);
    }

    Ok((
        StatusCode::CREATED,
        Json(CreateUserResponse {
            status: "ok",
            user_id,
            username,
            role,
        }),
    ))
}

/// `DELETE /api/v1/admin/users/:id` — delete a user by user_id. DBA-only.
pub(crate) async fn admin_delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<(StatusCode, Json<DeleteUserResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let _principal = require_sql_runtime_principal(
        &headers, &state, PrivilegeAction::Execute, "admin/users",
    )?;

    let removed = {
        let mut store = state.user_store.lock().expect("user_store lock");
        store.remove_by_id(&user_id)
    };

    if !removed {
        return Err((
            StatusCode::NOT_FOUND,
            Json(AuthErrorResponse {
                status: "error",
                reason: format!("user '{user_id}' not found"),
                locale: "en".to_string(),
                localized_message: format!("User '{user_id}' not found"),
            }),
        ));
    }

    // Invalidate all sessions for this user.
    {
        let mut sessions = state.session_store.lock().expect("session_store lock");
        sessions.remove_by_user(&user_id);
    }

    // WAL: record DROP USER (for crash recovery).
    {
        let mut wal = state.wal_engine.lock().expect("wal_engine lock");
        wal.append_sql(voltnuerongrid_store::SqlWalKind::Ddl, &format!("DROP USER {user_id}"));
    }

    Ok((
        StatusCode::OK,
        Json(DeleteUserResponse {
            status: "ok",
            user_id,
        }),
    ))
}

/// `POST /api/v1/auth/login` — authenticate username+password; return session token.
pub(crate) async fn auth_login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<(StatusCode, Json<LoginResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let username = req.username.trim().to_ascii_lowercase();

    let account = {
        let store = state.user_store.lock().expect("user_store lock");
        store.get_by_username(&username).cloned()
    };

    let account = account.ok_or_else(|| (
        StatusCode::UNAUTHORIZED,
        Json(AuthErrorResponse {
            status: "error",
            reason: "invalid credentials".to_string(),
            locale: "en".to_string(),
            localized_message: "Invalid username or password".to_string(),
        }),
    ))?;

    let stored_hash = account.password_hash.clone();
    let password = req.password.clone();

    let valid = tokio::task::spawn_blocking(move || {
        bcrypt::verify(&password, &stored_hash).unwrap_or(false)
    })
    .await
    .unwrap_or(false);

    if !valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                status: "error",
                reason: "invalid credentials".to_string(),
                locale: "en".to_string(),
                localized_message: "Invalid username or password".to_string(),
            }),
        ));
    }

    // Issue session token.
    let signer = state.session_signer.lock().expect("session_signer lock");
    let token = signer.issue(&account.user_id);
    let (_, expires_at_secs) = signer.verify(&token).expect("just issued token must verify");
    drop(signer);

    let fingerprint = crate::user_store::SessionSigner::fingerprint(&token);
    let entry = crate::user_store::SessionEntry {
        user_id: account.user_id.clone(),
        username: account.username.clone(),
        role: account.role.clone(),
        tenant_id: account.tenant_id.clone(),
        expires_at_secs,
    };
    {
        let mut sessions = state.session_store.lock().expect("session_store lock");
        sessions.insert(fingerprint, entry);
    }

    Ok((
        StatusCode::OK,
        Json(LoginResponse {
            status: "ok",
            token,
            user_id: account.user_id,
            username: account.username,
            role: account.role,
            expires_at_secs,
        }),
    ))
}
