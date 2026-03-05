#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-auth";

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityConfigContract {
    pub admin_api_key_env: String,
    pub admin_header_name: String,
    pub tls_required: bool,
    pub mtls_required: bool,
    pub encryption_at_rest_required: bool,
    pub kms_key_ref_env: String,
    pub allowed_operator_roles: Vec<String>,
    pub token_ttl_seconds: u64,
}

impl SecurityConfigContract {
    pub fn validate(&self) -> Result<(), String> {
        if self.admin_api_key_env.trim().is_empty() {
            return Err("admin_api_key_env must not be empty".to_string());
        }
        if self.admin_header_name.trim().is_empty() || !self.admin_header_name.starts_with("x-") {
            return Err("admin_header_name must start with 'x-'".to_string());
        }
        if self.mtls_required && !self.tls_required {
            return Err("mtls_required=true requires tls_required=true".to_string());
        }
        if self.encryption_at_rest_required && self.kms_key_ref_env.trim().is_empty() {
            return Err(
                "kms_key_ref_env must not be empty when encryption_at_rest_required=true"
                    .to_string(),
            );
        }
        if self.allowed_operator_roles.is_empty() {
            return Err("allowed_operator_roles must not be empty".to_string());
        }
        if self.token_ttl_seconds < 60 {
            return Err("token_ttl_seconds must be >= 60".to_string());
        }
        Ok(())
    }

    pub fn from_json_str(input: &str) -> Result<Self, String> {
        let config =
            serde_json::from_str::<Self>(input).map_err(|e| format!("json parse failed: {e}"))?;
        config.validate()?;
        Ok(config)
    }

    pub fn from_yaml_str(input: &str) -> Result<Self, String> {
        let config =
            serde_yaml::from_str::<Self>(input).map_err(|e| format!("yaml parse failed: {e}"))?;
        config.validate()?;
        Ok(config)
    }

    pub fn from_properties_str(input: &str) -> Result<Self, String> {
        let mut admin_api_key_env = None;
        let mut admin_header_name = None;
        let mut tls_required = None;
        let mut mtls_required = None;
        let mut encryption_at_rest_required = None;
        let mut kms_key_ref_env = None;
        let mut allowed_operator_roles = None;
        let mut token_ttl_seconds = None;

        for line in input.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                "security.adminApiKeyEnv" => admin_api_key_env = Some(value.to_string()),
                "security.adminHeaderName" => admin_header_name = Some(value.to_string()),
                "security.tlsRequired" => tls_required = Some(parse_bool(value)?),
                "security.mtlsRequired" => mtls_required = Some(parse_bool(value)?),
                "security.encryptionAtRestRequired" => {
                    encryption_at_rest_required = Some(parse_bool(value)?)
                }
                "security.kmsKeyRefEnv" => kms_key_ref_env = Some(value.to_string()),
                "security.allowedOperatorRoles" => {
                    let roles = value
                        .split(',')
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(ToString::to_string)
                        .collect::<Vec<_>>();
                    allowed_operator_roles = Some(roles);
                }
                "security.tokenTtlSeconds" => {
                    token_ttl_seconds = Some(
                        value
                            .parse::<u64>()
                            .map_err(|_| "security.tokenTtlSeconds must be integer".to_string())?,
                    )
                }
                _ => {}
            }
        }

        let config = Self {
            admin_api_key_env: admin_api_key_env
                .ok_or_else(|| "missing security.adminApiKeyEnv".to_string())?,
            admin_header_name: admin_header_name
                .ok_or_else(|| "missing security.adminHeaderName".to_string())?,
            tls_required: tls_required.ok_or_else(|| "missing security.tlsRequired".to_string())?,
            mtls_required: mtls_required
                .ok_or_else(|| "missing security.mtlsRequired".to_string())?,
            encryption_at_rest_required: encryption_at_rest_required
                .ok_or_else(|| "missing security.encryptionAtRestRequired".to_string())?,
            kms_key_ref_env: kms_key_ref_env
                .ok_or_else(|| "missing security.kmsKeyRefEnv".to_string())?,
            allowed_operator_roles: allowed_operator_roles
                .ok_or_else(|| "missing security.allowedOperatorRoles".to_string())?,
            token_ttl_seconds: token_ttl_seconds
                .ok_or_else(|| "missing security.tokenTtlSeconds".to_string())?,
        };
        config.validate()?;
        Ok(config)
    }
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err("boolean value must be true or false".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_security_config_from_json() {
        let json = r#"{
          "admin_api_key_env":"VNG_ADMIN_API_KEY",
          "admin_header_name":"x-vng-admin-key",
          "tls_required":true,
          "mtls_required":false,
          "encryption_at_rest_required":true,
          "kms_key_ref_env":"VNG_KMS_KEY_URI",
          "allowed_operator_roles":["dba","sre"],
          "token_ttl_seconds":300
        }"#;
        let cfg = SecurityConfigContract::from_json_str(json).expect("valid");
        assert_eq!(cfg.admin_api_key_env, "VNG_ADMIN_API_KEY");
        assert_eq!(cfg.kms_key_ref_env, "VNG_KMS_KEY_URI");
    }

    #[test]
    fn validates_security_config_from_properties() {
        let props = r#"
security.adminApiKeyEnv=VNG_ADMIN_API_KEY
security.adminHeaderName=x-vng-admin-key
security.tlsRequired=true
security.mtlsRequired=false
security.encryptionAtRestRequired=true
security.kmsKeyRefEnv=VNG_KMS_KEY_URI
security.allowedOperatorRoles=dba,sre
security.tokenTtlSeconds=300
"#;
        let cfg = SecurityConfigContract::from_properties_str(props).expect("valid");
        assert_eq!(cfg.allowed_operator_roles.len(), 2);
    }

    #[test]
    fn ws5_validates_security_config_from_yaml() {
        let yaml = r#"
admin_api_key_env: VNG_ADMIN_API_KEY
admin_header_name: x-vng-admin-key
tls_required: true
mtls_required: false
encryption_at_rest_required: true
kms_key_ref_env: VNG_KMS_KEY_URI
allowed_operator_roles:
  - dba
  - sre
token_ttl_seconds: 300
"#;
        let cfg = SecurityConfigContract::from_yaml_str(yaml).expect("valid");
        assert!(cfg.encryption_at_rest_required);
    }

    #[test]
    fn ws5_rejects_missing_kms_when_encryption_required() {
        let json = r#"{
          "admin_api_key_env":"VNG_ADMIN_API_KEY",
          "admin_header_name":"x-vng-admin-key",
          "tls_required":true,
          "mtls_required":false,
          "encryption_at_rest_required":true,
          "kms_key_ref_env":"   ",
          "allowed_operator_roles":["dba","sre"],
          "token_ttl_seconds":300
        }"#;
        let err = SecurityConfigContract::from_json_str(json).expect_err("must reject");
        assert!(err.contains("kms_key_ref_env"));
    }
}
