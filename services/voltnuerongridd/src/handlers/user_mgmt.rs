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

/// M-3: Build a 503 AuthErrorResponse for a poisoned mutex — keeps the
/// auth handler alive instead of panicking on `.expect()`.
#[inline]
fn lock_poisoned(what: &str) -> (StatusCode, Json<AuthErrorResponse>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(AuthErrorResponse {
            status: "error",
            reason: format!("{what} mutex poisoned"),
            locale: "en".to_string(),
            localized_message: "Service temporarily unavailable".to_string(),
        }),
    )
}

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
        let store = match state.user_store.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned("user_store")),
        };
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
        match state.wal_engine.lock() {
            Ok(mut wal) => { wal.append_sql(voltnuerongrid_store::SqlWalKind::Ddl, &wal_line); }
            Err(_) => return Err(lock_poisoned("wal_engine")),
        }
    }

    // Insert into in-memory store.
    {
        match state.user_store.lock() {
            Ok(mut store) => { store.insert(account); }
            Err(_) => return Err(lock_poisoned("user_store")),
        }
    }

    // O-2: audit the user-account creation.
    crate::audit_helpers::append_audit_event(
        &state,
        voltnuerongrid_audit::AuditEventKind::Security,
        &user_id,
        "admin_create_user",
        "ok",
        &format!("{{\"username\":\"{}\",\"role\":\"{}\"}}", username.replace('"', ""), role.replace('"', "")),
    );

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

/// `GET /api/v1/admin/users` — list all user accounts. DBA-only.
pub(crate) async fn admin_list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    let _principal = require_sql_runtime_principal(
        &headers, &state, PrivilegeAction::Execute, "admin/users",
    )?;

    let users: Vec<serde_json::Value> = {
        let store = match state.user_store.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned("user_store")),
        };
        store.all().map(|u| serde_json::json!({
            "user_id": u.user_id,
            "username": u.username,
            "role": u.role,
            "tenant_id": u.tenant_id,
            "created_ms": u.created_ms,
        })).collect()
    };
    let count = users.len();

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "users": users,
            "count": count,
        })),
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
        let mut store = match state.user_store.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned("user_store")),
        };
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
        match state.session_store.lock() {
            Ok(mut sessions) => { sessions.remove_by_user(&user_id); }
            Err(_) => return Err(lock_poisoned("session_store")),
        }
    }

    // WAL: record DROP USER (for crash recovery).
    {
        match state.wal_engine.lock() {
            Ok(mut wal) => { wal.append_sql(voltnuerongrid_store::SqlWalKind::Ddl, &format!("DROP USER {user_id}")); }
            Err(_) => return Err(lock_poisoned("wal_engine")),
        }
    }

    // O-2: audit the user-account deletion.
    crate::audit_helpers::append_audit_event(
        &state,
        voltnuerongrid_audit::AuditEventKind::Security,
        &user_id,
        "admin_delete_user",
        "ok",
        &format!("{{\"user_id\":\"{}\"}}", user_id.replace('"', "")),
    );

    Ok((
        StatusCode::OK,
        Json(DeleteUserResponse {
            status: "ok",
            user_id,
        }),
    ))
}

/// `DELETE /api/v1/admin/users/:id/sessions` — revoke all active sessions for a user. DBA-only.
///
/// Useful for forced sign-out after a password change or account compromise.
pub(crate) async fn admin_revoke_user_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<(StatusCode, axum::Json<serde_json::Value>), (StatusCode, axum::Json<AuthErrorResponse>)> {
    let _principal = require_sql_runtime_principal(
        &headers, &state, PrivilegeAction::Execute, "admin/users",
    )?;

    // Verify the user actually exists before revoking sessions.
    {
        let store = match state.user_store.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned("user_store")),
        };
        if store.get_by_id(&user_id).is_none() {
            return Err((
                StatusCode::NOT_FOUND,
                axum::Json(AuthErrorResponse {
                    status: "error",
                    reason: format!("user '{user_id}' not found"),
                    locale: "en".to_string(),
                    localized_message: format!("User '{user_id}' not found"),
                }),
            ));
        }
    }

    let sessions_revoked = {
        let mut sessions = match state.session_store.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned("session_store")),
        };
        let before = sessions
            .sessions_for_user(&user_id)
            .len();
        sessions.remove_by_user(&user_id);
        before
    };

    Ok((
        StatusCode::OK,
        axum::Json(serde_json::json!({
            "status": "ok",
            "user_id": user_id,
            "sessions_revoked": sessions_revoked,
        })),
    ))
}

/// `POST /api/v1/auth/login` — authenticate username+password; return session token.
#[tracing::instrument(skip_all, name = "auth.login")]
pub(crate) async fn auth_login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<(StatusCode, Json<LoginResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    let username = req.username.trim().to_ascii_lowercase();

    let account = {
        let store = match state.user_store.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned("user_store")),
        };
        store.get_by_username(&username).cloned()
    };

    let account = account.ok_or_else(|| {
        // O-2: audit the rejected login (unknown user) before returning 401.
        crate::audit_helpers::append_audit_event(
            &state,
            voltnuerongrid_audit::AuditEventKind::Security,
            &username,
            "auth_login",
            "rejected",
            &format!("{{\"reason\":\"unknown_user\",\"username\":\"{}\"}}", username.replace('"', "")),
        );
        (
        StatusCode::UNAUTHORIZED,
        Json(AuthErrorResponse {
            status: "error",
            reason: "invalid credentials".to_string(),
            locale: "en".to_string(),
            localized_message: "Invalid username or password".to_string(),
        }),
    )})?;

    let stored_hash = account.password_hash.clone();
    let password = req.password.clone();

    let valid = tokio::task::spawn_blocking(move || {
        bcrypt::verify(&password, &stored_hash).unwrap_or(false)
    })
    .await
    .unwrap_or(false);

    if !valid {
        // O-2: audit the rejected login (bad password) before returning 401.
        crate::audit_helpers::append_audit_event(
            &state,
            voltnuerongrid_audit::AuditEventKind::Security,
            &account.user_id,
            "auth_login",
            "rejected",
            &format!("{{\"reason\":\"invalid_password\",\"username\":\"{}\"}}", username.replace('"', "")),
        );
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
    let signer = match state.session_signer.lock() {
        Ok(g) => g,
        Err(_) => return Err(lock_poisoned("session_signer")),
    };
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
        match state.session_store.lock() {
            Ok(mut sessions) => { sessions.insert(fingerprint, entry); }
            Err(_) => return Err(lock_poisoned("session_store")),
        }
    }

    // O-2: audit the successful login.
    crate::audit_helpers::append_audit_event(
        &state,
        voltnuerongrid_audit::AuditEventKind::Security,
        &account.user_id,
        "auth_login",
        "ok",
        &format!("{{\"username\":\"{}\",\"role\":\"{}\"}}", account.username.replace('"', ""), account.role.replace('"', "")),
    );

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

/// `POST /api/v1/auth/token/rotate` — exchange a valid session token for a fresh
/// one. The old token is invalidated immediately. Requires the current valid token
/// in the `Authorization: Bearer <token>` header.
///
/// Returns the new token in the same shape as the login response.
/// Returns 401 if the token is missing, expired, or already invalidated.
#[tracing::instrument(skip_all, name = "auth.token_rotate")]
pub(crate) async fn auth_token_rotate(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<(StatusCode, Json<LoginResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    // Extract the current token from Authorization: Bearer <token>
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string());

    let current_token = bearer.ok_or_else(|| (
        StatusCode::UNAUTHORIZED,
        Json(AuthErrorResponse {
            status: "error",
            reason: "missing_token".to_string(),
            locale: "en".to_string(),
            localized_message: "Authorization: Bearer <token> header required".to_string(),
        }),
    ))?;

    // Verify the current token is valid and look up the session
    let (user_id, _expires) = {
        let signer = match state.session_signer.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned("session_signer")),
        };
        signer.verify(&current_token).ok_or_else(|| (
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                status: "error",
                reason: "invalid_or_expired_token".to_string(),
                locale: "en".to_string(),
                localized_message: "Token is invalid or has expired".to_string(),
            }),
        ))?
    };

    // Find the session entry
    let old_fingerprint = crate::user_store::SessionSigner::fingerprint(&current_token);
    let entry = {
        let sessions = match state.session_store.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned("session_store")),
        };
        sessions.lookup(&old_fingerprint).cloned()
    };

    let entry = entry.ok_or_else(|| (
        StatusCode::UNAUTHORIZED,
        Json(AuthErrorResponse {
            status: "error",
            reason: "session_not_found".to_string(),
            locale: "en".to_string(),
            localized_message: "Session has been revoked or does not exist".to_string(),
        }),
    ))?;

    // Issue a new token
    let (new_token, new_expires_at_secs) = {
        let signer = match state.session_signer.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned("session_signer")),
        };
        let t = signer.issue(&user_id);
        let (_, exp) = signer.verify(&t).expect("just-issued token must verify");
        (t, exp)
    };

    let new_fingerprint = crate::user_store::SessionSigner::fingerprint(&new_token);
    let new_entry = crate::user_store::SessionEntry {
        user_id: entry.user_id.clone(),
        username: entry.username.clone(),
        role: entry.role.clone(),
        tenant_id: entry.tenant_id.clone(),
        expires_at_secs: new_expires_at_secs,
    };

    // Atomically: remove old session, insert new session
    {
        let mut sessions = match state.session_store.lock() {
            Ok(g) => g,
            Err(_) => return Err(lock_poisoned("session_store")),
        };
        sessions.remove_by_fingerprint(&old_fingerprint);
        sessions.insert(new_fingerprint, new_entry);
    }

    Ok((
        StatusCode::OK,
        Json(LoginResponse {
            status: "ok",
            token: new_token,
            user_id: entry.user_id,
            username: entry.username,
            role: entry.role,
            expires_at_secs: new_expires_at_secs,
        }),
    ))
}
