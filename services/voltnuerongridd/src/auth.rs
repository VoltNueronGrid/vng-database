use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use voltnuerongrid_auth::PrivilegeAction;
use voltnuerongrid_sql::{I18nCatalog, SupportedLocale};
use crate::{
    AppState, AuthErrorResponse, OperatorIdentity, RuntimeAccessPrincipal,
    TenantUserIdentity, CONTROL_PLANE_OPERATOR_ROLES,
};

pub(crate) fn require_admin_api_key(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<(), (StatusCode, Json<AuthErrorResponse>)> {
    let Some(required_key) = state.admin_api_key.as_ref() else {
        return Err(auth_error(headers, "missing_or_invalid_admin_key"));
    };

    let provided = headers
        .get("x-vng-admin-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    if provided != required_key {
        return Err(auth_error(headers, "missing_or_invalid_admin_key"));
    }

    Ok(())
}

pub(crate) fn require_operator_auth(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<(), (StatusCode, Json<AuthErrorResponse>)> {
    let Some(required_key) = state.admin_api_key.as_ref() else {
        return Err(auth_error(headers, "missing_or_invalid_admin_key"));
    };

    let provided = headers
        .get("x-vng-admin-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    if provided != required_key {
        return Err(auth_error(headers, "missing_or_invalid_admin_key"));
    }

    let operator = operator_identity_from_headers(headers, state)
        .ok_or_else(|| auth_error(headers, "missing_or_invalid_operator_identity"))?;
    if !state.allowed_operator_roles.contains(&operator.role) {
        return Err(auth_error(headers, "operator_role_not_allowed"));
    }
    if !CONTROL_PLANE_OPERATOR_ROLES.contains(&operator.role) {
        return Err(auth_error(headers, "operator_role_not_authorized"));
    }

    Ok(())
}

pub(crate) fn require_cluster_failover_privilege(
    headers: &HeaderMap,
    state: &AppState,
    action: PrivilegeAction,
) -> Result<OperatorIdentity, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(headers, state)?;
    require_operator_privilege(headers, state, "cluster.failover", "cluster", action)
}

pub(crate) fn require_operator_privilege(
    headers: &HeaderMap,
    state: &AppState,
    resource: &str,
    scope: &str,
    action: PrivilegeAction,
) -> Result<OperatorIdentity, (StatusCode, Json<AuthErrorResponse>)> {
    let operator = operator_identity_from_headers(headers, state)
        .ok_or_else(|| auth_error(headers, "missing_or_invalid_operator_identity"))?;
    if state
        .rbac_privilege_matrix
        .allows(operator.role.as_str(), resource, scope, action)
    {
        Ok(operator)
    } else {
        Err(forbidden_error(headers, "insufficient_privilege"))
    }
}

pub(crate) fn require_sql_runtime_principal(
    headers: &HeaderMap,
    state: &AppState,
    action: PrivilegeAction,
    sql_scope: &str,
) -> Result<RuntimeAccessPrincipal, (StatusCode, Json<AuthErrorResponse>)> {
    require_runtime_principal(headers, state, "sql.runtime", sql_scope, action)
}

pub(crate) fn require_ingest_runtime_privilege(
    headers: &HeaderMap,
    state: &AppState,
    action: PrivilegeAction,
    ingest_scope: &str,
) -> Result<RuntimeAccessPrincipal, (StatusCode, Json<AuthErrorResponse>)> {
    require_runtime_principal(headers, state, "ingest.connectors", ingest_scope, action)
}

pub(crate) fn require_store_runtime_principal(
    headers: &HeaderMap,
    state: &AppState,
    action: PrivilegeAction,
    store_scope: &str,
) -> Result<RuntimeAccessPrincipal, (StatusCode, Json<AuthErrorResponse>)> {
    require_runtime_principal(headers, state, "storage.catalog", store_scope, action)
}

pub(crate) fn require_audit_runtime_principal(
    headers: &HeaderMap,
    state: &AppState,
    action: PrivilegeAction,
    audit_scope: &str,
) -> Result<RuntimeAccessPrincipal, (StatusCode, Json<AuthErrorResponse>)> {
    require_runtime_principal(headers, state, "observability.audit", audit_scope, action)
}

pub(crate) fn require_autonomous_records_runtime_principal(
    headers: &HeaderMap,
    state: &AppState,
    action: PrivilegeAction,
    records_scope: &str,
) -> Result<RuntimeAccessPrincipal, (StatusCode, Json<AuthErrorResponse>)> {
    require_runtime_principal(
        headers,
        state,
        "observability.autonomous_records",
        records_scope,
        action,
    )
}

pub(crate) fn require_runtime_principal(
    headers: &HeaderMap,
    state: &AppState,
    resource: &str,
    scope: &str,
    action: PrivilegeAction,
) -> Result<RuntimeAccessPrincipal, (StatusCode, Json<AuthErrorResponse>)> {
    let has_operator_headers = headers.contains_key("x-vng-admin-key")
        || headers.contains_key("x-vng-operator-id");

    if has_operator_headers {
        require_operator_auth(headers, state)?;
        let operator = require_operator_privilege(headers, state, resource, scope, action)?;
        return Ok(RuntimeAccessPrincipal::Operator(operator));
    }

    let user = require_tenant_user_privilege(headers, state, resource, scope, action)?;
    Ok(RuntimeAccessPrincipal::TenantUser(user))
}

pub(crate) fn tenant_scoped_scope(tenant_id: &str, scope: &str) -> String {
    format!("tenants/{tenant_id}/{}", scope.trim_start_matches('/'))
}

pub(crate) fn store_table_matches_tenant_namespace(table: &str, tenant_id: &str) -> bool {
    let normalized_table = table.trim().to_ascii_lowercase();
    let normalized_tenant = tenant_id.trim().to_ascii_lowercase();
    normalized_table.starts_with(&format!("tenant/{normalized_tenant}/"))
        || normalized_table.starts_with(&format!("tenant_{normalized_tenant}_"))
        || normalized_table.starts_with(&format!("{normalized_tenant}."))
}

pub(crate) fn ensure_store_table_access(
    principal: &RuntimeAccessPrincipal,
    headers: &HeaderMap,
    table: &str,
) -> Result<(), (StatusCode, Json<AuthErrorResponse>)> {
    match principal {
        RuntimeAccessPrincipal::Operator(_) => Ok(()),
        RuntimeAccessPrincipal::TenantUser(user) => {
            if store_table_matches_tenant_namespace(table, &user.tenant_id) {
                Ok(())
            } else {
                Err(forbidden_error(headers, "insufficient_privilege"))
            }
        }
    }
}

pub(crate) fn require_tenant_user_privilege(
    headers: &HeaderMap,
    state: &AppState,
    resource: &str,
    scope: &str,
    action: PrivilegeAction,
) -> Result<TenantUserIdentity, (StatusCode, Json<AuthErrorResponse>)> {
    let user = tenant_user_identity_from_headers(headers, state)
        .ok_or_else(|| auth_error(headers, "missing_or_invalid_user_identity"))?;
    let expected_scope = tenant_scoped_scope(&user.tenant_id, scope);
    if state
        .rbac_privilege_matrix
        .allows(user.role.as_str(), resource, &expected_scope, action)
    {
        Ok(user)
    } else {
        Err(forbidden_error(headers, "insufficient_privilege"))
    }
}

pub(crate) fn operator_identity_from_headers(
    headers: &HeaderMap,
    state: &AppState,
) -> Option<OperatorIdentity> {
    let operator_id = headers
        .get("x-vng-operator-id")
        .and_then(|value| value.to_str().ok())?
        .trim();
    if operator_id.is_empty() {
        return None;
    }
    let role = state.operator_role_bindings.get(operator_id).copied()?;
    Some(OperatorIdentity {
        operator_id: operator_id.to_string(),
        role,
    })
}

pub(crate) fn tenant_user_identity_from_headers(
    headers: &HeaderMap,
    state: &AppState,
) -> Option<TenantUserIdentity> {
    let user_id = headers
        .get("x-vng-user-id")
        .and_then(|value| value.to_str().ok())?
        .trim();
    let tenant_id = headers
        .get("x-vng-tenant-id")
        .and_then(|value| value.to_str().ok())?
        .trim();
    if user_id.is_empty() || tenant_id.is_empty() {
        return None;
    }
    let binding = state.tenant_user_bindings.get(user_id)?;
    if !binding.tenant_id.eq_ignore_ascii_case(tenant_id) {
        return None;
    }
    Some(TenantUserIdentity {
        user_id: user_id.to_string(),
        tenant_id: tenant_id.to_string(),
        role: binding.role.clone(),
    })
}

pub(crate) fn auth_error(
    headers: &HeaderMap,
    reason: &str,
) -> (StatusCode, Json<AuthErrorResponse>) {
    let locale = locale_from_headers(headers);
    let message_key = if reason == "missing_or_invalid_admin_key" {
        "missing_or_invalid_admin_key"
    } else {
        "unauthorized"
    };
    let localized = I18nCatalog::message(locale, message_key);
    (
        StatusCode::UNAUTHORIZED,
        Json(AuthErrorResponse {
            status: "unauthorized",
            reason: reason.to_string(),
            locale: locale.as_str().to_string(),
            localized_message: localized.message.to_string(),
        }),
    )
}

pub(crate) fn forbidden_error(
    headers: &HeaderMap,
    reason: &str,
) -> (StatusCode, Json<AuthErrorResponse>) {
    let locale = locale_from_headers(headers);
    let localized = I18nCatalog::message(locale, "unauthorized");
    (
        StatusCode::FORBIDDEN,
        Json(AuthErrorResponse {
            status: "forbidden",
            reason: reason.to_string(),
            locale: locale.as_str().to_string(),
            localized_message: localized.message.to_string(),
        }),
    )
}

pub(crate) fn bad_request_error(
    headers: &HeaderMap,
    reason: &str,
) -> (StatusCode, Json<AuthErrorResponse>) {
    let locale = locale_from_headers(headers);
    let localized = I18nCatalog::message(locale, "unauthorized");
    (
        StatusCode::BAD_REQUEST,
        Json(AuthErrorResponse {
            status: "bad_request",
            reason: reason.to_string(),
            locale: locale.as_str().to_string(),
            localized_message: localized.message.to_string(),
        }),
    )
}

pub(crate) fn locale_from_headers(headers: &HeaderMap) -> SupportedLocale {
    headers
        .get("accept-language")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(',').next().unwrap_or("en-US"))
        .map(SupportedLocale::parse)
        .unwrap_or(SupportedLocale::EnUs)
}
