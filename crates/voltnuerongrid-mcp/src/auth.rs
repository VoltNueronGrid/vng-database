//! Authentication and authorization for MCP requests

use serde::{Deserialize, Serialize};
use thiserror::Error;
use crate::McpRequestHeaders;

#[derive(Debug, Error)]
pub enum McpAuthError {
    #[error("Missing required authentication header")]
    MissingCredentials,

    #[error("Invalid API key")]
    InvalidApiKey,

    #[error("Insufficient privileges for this operation")]
    InsufficientPrivilege,

    #[error("Operator identity required but not provided")]
    MissingOperatorId,

    #[error("Tenant scope mismatch")]
    TenantMismatch,
}

/// Authentication level required for a tool
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationLevel {
    /// Requires x-vng-admin-key header
    Admin = 3,
    /// Requires x-vng-operator-id header
    Operator = 2,
    /// Requires x-vng-tenant-id and x-vng-user-id headers
    Tenant = 1,
}

/// Result of authentication check
#[derive(Clone, Debug)]
pub struct McpAuthContext {
    /// Admin authentication successful
    pub is_admin: bool,

    /// Operator identity (if authenticated as operator)
    pub operator_id: Option<String>,

    /// Tenant identity (for tenant-scoped operations)
    pub tenant_id: Option<String>,

    /// User identity within tenant
    pub user_id: Option<String>,

    /// Effective authentication level
    pub auth_level: AuthenticationLevel,
}

impl McpAuthContext {
    /// Parse authentication from request headers
    pub fn from_headers(headers: &McpRequestHeaders) -> Result<Self, McpAuthError> {
        // Check admin first (highest privilege)
        if let Some(admin_key) = &headers.x_vng_admin_key {
            if !admin_key.is_empty() {
                // Validate against VNG_ADMIN_API_KEY if configured
                if let Ok(expected) = std::env::var("VNG_ADMIN_API_KEY") {
                    if !expected.is_empty() && expected != *admin_key {
                        return Err(McpAuthError::InvalidApiKey);
                    }
                }
                return Ok(McpAuthContext {
                    is_admin: true,
                    operator_id: None,
                    tenant_id: None,
                    user_id: None,
                    auth_level: AuthenticationLevel::Admin,
                });
            }
        }

        // Check operator (medium privilege)
        if let Some(operator_id) = &headers.x_vng_operator_id {
            if !operator_id.is_empty() {
                return Ok(McpAuthContext {
                    is_admin: false,
                    operator_id: Some(operator_id.clone()),
                    tenant_id: None,
                    user_id: None,
                    auth_level: AuthenticationLevel::Operator,
                });
            }
        }

        // Check tenant (low privilege)
        if let Some(tenant_id) = &headers.x_vng_tenant_id {
            if let Some(user_id) = &headers.x_vng_user_id {
                if !tenant_id.is_empty() && !user_id.is_empty() {
                    return Ok(McpAuthContext {
                        is_admin: false,
                        operator_id: None,
                        tenant_id: Some(tenant_id.clone()),
                        user_id: Some(user_id.clone()),
                        auth_level: AuthenticationLevel::Tenant,
                    });
                }
            }
        }

        Err(McpAuthError::MissingCredentials)
    }

    /// Require admin authentication
    pub fn require_admin(&self) -> Result<(), McpAuthError> {
        if self.is_admin {
            Ok(())
        } else {
            Err(McpAuthError::InsufficientPrivilege)
        }
    }

    /// Require operator authentication
    pub fn require_operator(&self) -> Result<(), McpAuthError> {
        match self.auth_level {
            AuthenticationLevel::Admin | AuthenticationLevel::Operator => Ok(()),
            _ => Err(McpAuthError::InsufficientPrivilege),
        }
    }

    /// Require tenant authentication
    pub fn require_tenant(&self) -> Result<(), McpAuthError> {
        if self.tenant_id.is_some() && self.user_id.is_some() {
            Ok(())
        } else {
            Err(McpAuthError::InsufficientPrivilege)
        }
    }

    /// Verify tenant scope (for reading tenant-scoped data)
    pub fn verify_tenant_scope(&self, requested_tenant_id: &str) -> Result<(), McpAuthError> {
        // Admin can access any tenant
        if self.is_admin {
            return Ok(());
        }

        // Tenant users can only access their own tenant
        if let Some(tenant_id) = &self.tenant_id {
            if tenant_id == requested_tenant_id {
                return Ok(());
            }
        }

        Err(McpAuthError::TenantMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_admin_auth() {
        unsafe { std::env::remove_var("VNG_ADMIN_API_KEY"); }
        let headers = McpRequestHeaders {
            x_vng_admin_key: Some("secret-key".to_string()),
            x_vng_operator_id: None,
            x_vng_tenant_id: None,
            x_vng_user_id: None,
        };
        let auth = McpAuthContext::from_headers(&headers).unwrap();
        assert!(auth.is_admin);
        assert_eq!(auth.auth_level, AuthenticationLevel::Admin);
        assert!(auth.require_admin().is_ok());
    }

    #[test]
    fn test_operator_auth() {
        let headers = McpRequestHeaders {
            x_vng_admin_key: None,
            x_vng_operator_id: Some("op-001".to_string()),
            x_vng_tenant_id: None,
            x_vng_user_id: None,
        };
        let auth = McpAuthContext::from_headers(&headers).unwrap();
        assert!(!auth.is_admin);
        assert_eq!(auth.auth_level, AuthenticationLevel::Operator);
        assert_eq!(auth.operator_id, Some("op-001".to_string()));
        assert!(auth.require_operator().is_ok());
        assert!(auth.require_admin().is_err());
    }

    #[test]
    fn test_tenant_auth() {
        let headers = McpRequestHeaders {
            x_vng_admin_key: None,
            x_vng_operator_id: None,
            x_vng_tenant_id: Some("tenant-123".to_string()),
            x_vng_user_id: Some("user-456".to_string()),
        };
        let auth = McpAuthContext::from_headers(&headers).unwrap();
        assert!(!auth.is_admin);
        assert_eq!(auth.auth_level, AuthenticationLevel::Tenant);
        assert_eq!(auth.tenant_id, Some("tenant-123".to_string()));
        assert!(auth.require_tenant().is_ok());
        assert!(auth.require_operator().is_err());
    }

    #[test]
    fn test_missing_auth() {
        let headers = McpRequestHeaders {
            x_vng_admin_key: None,
            x_vng_operator_id: None,
            x_vng_tenant_id: None,
            x_vng_user_id: None,
        };
        assert!(McpAuthContext::from_headers(&headers).is_err());
    }

    #[test]
    fn test_tenant_scope_verification() {
        let headers = McpRequestHeaders {
            x_vng_admin_key: None,
            x_vng_operator_id: None,
            x_vng_tenant_id: Some("tenant-123".to_string()),
            x_vng_user_id: Some("user-456".to_string()),
        };
        let auth = McpAuthContext::from_headers(&headers).unwrap();

        // Same tenant should be allowed
        assert!(auth.verify_tenant_scope("tenant-123").is_ok());

        // Different tenant should be denied
        assert!(auth.verify_tenant_scope("tenant-999").is_err());
    }

    #[test]
    fn test_admin_can_access_any_tenant() {
        unsafe { std::env::remove_var("VNG_ADMIN_API_KEY"); }
        let headers = McpRequestHeaders {
            x_vng_admin_key: Some("secret-key".to_string()),
            x_vng_operator_id: None,
            x_vng_tenant_id: None,
            x_vng_user_id: None,
        };
        let auth = McpAuthContext::from_headers(&headers).unwrap();

        // Admin should be able to access any tenant
        assert!(auth.verify_tenant_scope("any-tenant").is_ok());
        assert!(auth.verify_tenant_scope("another-tenant").is_ok());
    }

    #[test]
    fn test_auth_level_ordering() {
        // Verify auth levels are ordered correctly
        assert!(AuthenticationLevel::Admin > AuthenticationLevel::Operator);
        assert!(AuthenticationLevel::Operator > AuthenticationLevel::Tenant);
    }

    #[test]
    fn test_admin_key_validated_against_env_var() {
        unsafe {
            std::env::set_var("VNG_ADMIN_API_KEY", "correct-secret");
        }
        // Wrong key rejected
        let bad_headers = McpRequestHeaders {
            x_vng_admin_key: Some("wrong-key".to_string()),
            x_vng_operator_id: None,
            x_vng_tenant_id: None,
            x_vng_user_id: None,
        };
        assert!(McpAuthContext::from_headers(&bad_headers).is_err());

        // Correct key accepted
        let good_headers = McpRequestHeaders {
            x_vng_admin_key: Some("correct-secret".to_string()),
            x_vng_operator_id: None,
            x_vng_tenant_id: None,
            x_vng_user_id: None,
        };
        let auth = McpAuthContext::from_headers(&good_headers).unwrap();
        assert!(auth.is_admin);

        unsafe {
            std::env::remove_var("VNG_ADMIN_API_KEY");
        }
    }

    #[test]
    fn test_admin_key_accepted_when_env_var_not_set() {
        unsafe {
            std::env::remove_var("VNG_ADMIN_API_KEY");
        }
        // When env var is not set, any non-empty key is accepted (dev mode)
        let headers = McpRequestHeaders {
            x_vng_admin_key: Some("any-key".to_string()),
            x_vng_operator_id: None,
            x_vng_tenant_id: None,
            x_vng_user_id: None,
        };
        let auth = McpAuthContext::from_headers(&headers).unwrap();
        assert!(auth.is_admin);
    }
}