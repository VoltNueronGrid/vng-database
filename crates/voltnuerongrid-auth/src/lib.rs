#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-auth";

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityConfigContract {
    pub admin_api_key_env: String,
    pub admin_header_name: String,
    pub tls_required: bool,
    pub mtls_required: bool,
    pub encryption_at_rest_required: bool,
    pub kms_key_ref_env: String,
    #[serde(default)]
    pub kms_failover_key_ref_envs: Vec<String>,
    pub allowed_operator_roles: Vec<String>,
    pub token_ttl_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KmsKeyResolution {
    pub selected_env: String,
    pub key_ref: String,
    pub failover_used: bool,
}

pub trait KmsKeyProvider {
    fn lookup_key_ref(&self, env_name: &str) -> Result<Option<String>, String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KmsProviderKind {
    Generic,
    AwsCli,
    AzureCli,
    GcpCli,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InMemoryKmsProviderAdapter {
    provider_name: String,
    key_refs: BTreeMap<String, String>,
    unavailable_envs: BTreeSet<String>,
}

impl InMemoryKmsProviderAdapter {
    pub fn new(provider_name: &str) -> Self {
        Self {
            provider_name: provider_name.trim().to_string(),
            key_refs: BTreeMap::new(),
            unavailable_envs: BTreeSet::new(),
        }
    }

    pub fn provider_name(&self) -> &str {
        &self.provider_name
    }

    pub fn register_key_ref(&mut self, env_name: &str, key_ref: &str) {
        self.key_refs
            .insert(env_name.trim().to_string(), key_ref.trim().to_string());
    }

    pub fn mark_unavailable(&mut self, env_name: &str) {
        self.unavailable_envs.insert(env_name.trim().to_string());
    }

    pub fn clear_unavailable(&mut self) {
        self.unavailable_envs.clear();
    }

    pub fn unavailable_envs(&self) -> Vec<String> {
        self.unavailable_envs.iter().cloned().collect()
    }
}

impl KmsKeyProvider for InMemoryKmsProviderAdapter {
    fn lookup_key_ref(&self, env_name: &str) -> Result<Option<String>, String> {
        let trimmed = env_name.trim();
        if self.unavailable_envs.contains(trimmed) {
            return Ok(None);
        }
        Ok(self.key_refs.get(trimmed).cloned())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudCliKmsProviderAdapter {
    provider_kind: Option<KmsProviderKind>,
    provider_name: String,
    key_refs: BTreeMap<String, String>,
    unavailable_envs: BTreeSet<String>,
}

impl CloudCliKmsProviderAdapter {
    pub fn new(provider_kind: KmsProviderKind, provider_name: &str) -> Self {
        Self {
            provider_kind: Some(provider_kind),
            provider_name: provider_name.trim().to_string(),
            key_refs: BTreeMap::new(),
            unavailable_envs: BTreeSet::new(),
        }
    }

    pub fn register_key_ref(&mut self, env_name: &str, key_ref: &str) {
        self.key_refs
            .insert(env_name.trim().to_string(), key_ref.trim().to_string());
    }

    pub fn mark_unavailable(&mut self, env_name: &str) {
        self.unavailable_envs.insert(env_name.trim().to_string());
    }

    pub fn clear_unavailable(&mut self) {
        self.unavailable_envs.clear();
    }
}

impl KmsKeyProvider for CloudCliKmsProviderAdapter {
    fn lookup_key_ref(&self, env_name: &str) -> Result<Option<String>, String> {
        let trimmed = env_name.trim();
        if self.unavailable_envs.contains(trimmed) {
            return Ok(None);
        }
        let Some(key_ref) = self.key_refs.get(trimmed) else {
            return Ok(None);
        };
        let Some(provider_kind) = self.provider_kind else {
            return Err(format!("kms provider kind not configured for {}", self.provider_name));
        };
        verify_cloud_kms_key_ref(provider_kind, key_ref)?;
        Ok(Some(key_ref.clone()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfiguredKmsProviderAdapter {
    Generic(InMemoryKmsProviderAdapter),
    CloudCli(CloudCliKmsProviderAdapter),
}

impl ConfiguredKmsProviderAdapter {
    pub fn from_key_ref(key_ref: &str) -> Self {
        match detect_kms_provider_kind(key_ref) {
            KmsProviderKind::Generic => Self::Generic(InMemoryKmsProviderAdapter::new("generic")),
            KmsProviderKind::AwsCli => Self::CloudCli(CloudCliKmsProviderAdapter::new(
                KmsProviderKind::AwsCli,
                "aws-cli",
            )),
            KmsProviderKind::AzureCli => Self::CloudCli(CloudCliKmsProviderAdapter::new(
                KmsProviderKind::AzureCli,
                "azure-cli",
            )),
            KmsProviderKind::GcpCli => Self::CloudCli(CloudCliKmsProviderAdapter::new(
                KmsProviderKind::GcpCli,
                "gcloud-cli",
            )),
        }
    }

    pub fn provider_name(&self) -> &str {
        match self {
            Self::Generic(adapter) => adapter.provider_name(),
            Self::CloudCli(adapter) => &adapter.provider_name,
        }
    }

    pub fn register_key_ref(&mut self, env_name: &str, key_ref: &str) {
        match self {
            Self::Generic(adapter) => adapter.register_key_ref(env_name, key_ref),
            Self::CloudCli(adapter) => adapter.register_key_ref(env_name, key_ref),
        }
    }

    pub fn mark_unavailable(&mut self, env_name: &str) {
        match self {
            Self::Generic(adapter) => adapter.mark_unavailable(env_name),
            Self::CloudCli(adapter) => adapter.mark_unavailable(env_name),
        }
    }

    pub fn clear_unavailable(&mut self) {
        match self {
            Self::Generic(adapter) => adapter.clear_unavailable(),
            Self::CloudCli(adapter) => adapter.clear_unavailable(),
        }
    }
}

impl KmsKeyProvider for ConfiguredKmsProviderAdapter {
    fn lookup_key_ref(&self, env_name: &str) -> Result<Option<String>, String> {
        match self {
            Self::Generic(adapter) => adapter.lookup_key_ref(env_name),
            Self::CloudCli(adapter) => adapter.lookup_key_ref(env_name),
        }
    }
}

pub struct KmsProviderChain<'a> {
    providers: Vec<&'a dyn KmsKeyProvider>,
}

impl<'a> KmsProviderChain<'a> {
    pub fn new(providers: Vec<&'a dyn KmsKeyProvider>) -> Self {
        Self { providers }
    }
}

impl KmsKeyProvider for KmsProviderChain<'_> {
    fn lookup_key_ref(&self, env_name: &str) -> Result<Option<String>, String> {
        for provider in &self.providers {
            if let Some(value) = provider.lookup_key_ref(env_name)? {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
}

pub fn detect_kms_provider_kind(key_ref: &str) -> KmsProviderKind {
    let trimmed = key_ref.trim();
    if trimmed.starts_with("arn:aws:kms:") || trimmed.starts_with("aws-kms://") {
        KmsProviderKind::AwsCli
    } else if trimmed.starts_with("azure-kms://") || trimmed.contains(".vault.azure.net/keys/") {
        KmsProviderKind::AzureCli
    } else if trimmed.starts_with("gcp-kms://") || trimmed.starts_with("projects/") {
        KmsProviderKind::GcpCli
    } else {
        KmsProviderKind::Generic
    }
}

fn verify_cloud_kms_key_ref(provider_kind: KmsProviderKind, key_ref: &str) -> Result<(), String> {
    match provider_kind {
        KmsProviderKind::AwsCli => verify_aws_kms_key_ref(key_ref),
        KmsProviderKind::AzureCli => verify_azure_kms_key_ref(key_ref),
        KmsProviderKind::GcpCli => verify_gcp_kms_key_ref(key_ref),
        KmsProviderKind::Generic => Ok(()),
    }
}

fn verify_aws_kms_key_ref(key_ref: &str) -> Result<(), String> {
    let normalized = key_ref.trim().trim_start_matches("aws-kms://");
    let mut segments = normalized.split(':');
    let Some("arn") = segments.next() else {
        return Err(format!("aws kms key ref must be an ARN: {key_ref}"));
    };
    let Some("aws") = segments.next() else {
        return Err(format!("aws kms key ref missing partition: {key_ref}"));
    };
    let Some("kms") = segments.next() else {
        return Err(format!("aws kms key ref missing service name: {key_ref}"));
    };
    let region = segments
        .next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("aws kms key ref missing region: {key_ref}"))?;

    run_cli_command(
        "aws",
        &["kms", "describe-key", "--key-id", normalized, "--region", region, "--output", "json"],
    )
}

fn verify_azure_kms_key_ref(key_ref: &str) -> Result<(), String> {
    let normalized = key_ref.trim().trim_start_matches("azure-kms://");
    if !normalized.contains(".vault.azure.net/keys/") {
        return Err(format!("azure key ref must target a Key Vault key id: {key_ref}"));
    }
    run_cli_command("az", &["keyvault", "key", "show", "--id", normalized, "--output", "json"])
}

fn verify_gcp_kms_key_ref(key_ref: &str) -> Result<(), String> {
    let normalized = key_ref.trim().trim_start_matches("gcp-kms://");
    let segments = normalized.split('/').collect::<Vec<_>>();
    if segments.len() < 8 {
        return Err(format!("gcp kms key ref must be a full resource path: {key_ref}"));
    }
    let project = segments
        .windows(2)
        .find(|pair| pair[0] == "projects")
        .map(|pair| pair[1])
        .ok_or_else(|| format!("gcp kms key ref missing project: {key_ref}"))?;
    let location = segments
        .windows(2)
        .find(|pair| pair[0] == "locations")
        .map(|pair| pair[1])
        .ok_or_else(|| format!("gcp kms key ref missing location: {key_ref}"))?;
    let keyring = segments
        .windows(2)
        .find(|pair| pair[0] == "keyRings")
        .map(|pair| pair[1])
        .ok_or_else(|| format!("gcp kms key ref missing keyRing: {key_ref}"))?;
    let key = segments
        .windows(2)
        .find(|pair| pair[0] == "cryptoKeys")
        .map(|pair| pair[1])
        .ok_or_else(|| format!("gcp kms key ref missing cryptoKey: {key_ref}"))?;

    run_cli_command(
        "gcloud",
        &[
            "kms",
            "keys",
            "describe",
            key,
            "--keyring",
            keyring,
            "--location",
            location,
            "--project",
            project,
            "--format",
            "json",
        ],
    )
}

fn run_cli_command(program: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| format!("{program} invocation failed: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    Err(format!("{program} command failed: {detail}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegeAction {
    Read,
    Write,
    Execute,
    Manage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceGrant {
    pub resource: String,
    pub scopes: Vec<String>,
    pub actions: Vec<PrivilegeAction>,
}

impl ResourceGrant {
    pub fn allows(&self, resource: &str, scope: &str, action: PrivilegeAction) -> bool {
        self.resource.eq_ignore_ascii_case(resource)
            && self.actions.contains(&action)
            && self.scopes.iter().any(|allowed| scope_matches(allowed, scope))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RbacPrivilegeMatrix {
    pub grants_by_role: BTreeMap<String, Vec<ResourceGrant>>,
}

impl RbacPrivilegeMatrix {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant_role(&mut self, role: &str, grant: ResourceGrant) {
        self.grants_by_role
            .entry(role.trim().to_ascii_lowercase())
            .or_default()
            .push(grant);
    }

    pub fn grants_for_role(&self, role: &str) -> &[ResourceGrant] {
        self.grants_by_role
            .get(&role.trim().to_ascii_lowercase())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn allows(
        &self,
        role: &str,
        resource: &str,
        scope: &str,
        action: PrivilegeAction,
    ) -> bool {
        self.grants_for_role(role)
            .iter()
            .any(|grant| grant.allows(resource, scope, action))
    }
}

fn scope_matches(allowed_scope: &str, requested_scope: &str) -> bool {
    let allowed_scope = allowed_scope.trim();
    let requested_scope = requested_scope.trim();
    if allowed_scope == "*" {
        return true;
    }

    let allowed_segments: Vec<&str> = allowed_scope
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let requested_segments: Vec<&str> = requested_scope
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();

    let mut index = 0usize;
    while index < allowed_segments.len() && index < requested_segments.len() {
        let allowed = allowed_segments[index];
        let requested = requested_segments[index];

        if allowed == "*" {
            return true;
        }
        if allowed.starts_with('{') && allowed.ends_with('}') {
            if requested.is_empty() {
                return false;
            }
            index += 1;
            continue;
        }
        if !allowed.eq_ignore_ascii_case(requested) {
            return false;
        }
        index += 1;
    }

    if index == allowed_segments.len() && index == requested_segments.len() {
        return true;
    }

    index + 1 == allowed_segments.len() && allowed_segments.get(index) == Some(&"*")
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
        let primary_env = self.kms_key_ref_env.trim();
        let primary_normalized = primary_env.to_ascii_lowercase();
        let mut seen = BTreeMap::new();
        if !primary_normalized.is_empty() {
            seen.insert(primary_normalized, primary_env.to_string());
        }
        for env_name in &self.kms_failover_key_ref_envs {
            let trimmed = env_name.trim();
            let normalized = trimmed.to_ascii_lowercase();
            if normalized.is_empty() {
                return Err("kms failover env names must not be empty".to_string());
            }
            if let Some(previous) = seen.insert(normalized, trimmed.to_string()) {
                return Err(format!(
                    "kms failover env names must be unique; duplicate detected for {previous}"
                ));
            }
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
        let mut kms_failover_key_ref_envs = None;
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
                "security.kmsFailoverKeyRefEnvs" => {
                    let envs = value
                        .split(',')
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(ToString::to_string)
                        .collect::<Vec<_>>();
                    kms_failover_key_ref_envs = Some(envs);
                }
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
            kms_failover_key_ref_envs: kms_failover_key_ref_envs.unwrap_or_default(),
            allowed_operator_roles: allowed_operator_roles
                .ok_or_else(|| "missing security.allowedOperatorRoles".to_string())?,
            token_ttl_seconds: token_ttl_seconds
                .ok_or_else(|| "missing security.tokenTtlSeconds".to_string())?,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn kms_key_candidates(&self) -> Vec<String> {
        let mut candidates = Vec::new();
        if !self.kms_key_ref_env.trim().is_empty() {
            candidates.push(self.kms_key_ref_env.trim().to_string());
        }
        for env_name in &self.kms_failover_key_ref_envs {
            let trimmed = env_name.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !candidates.iter().any(|candidate| candidate.eq_ignore_ascii_case(trimmed)) {
                candidates.push(trimmed.to_string());
            }
        }
        candidates
    }

    pub fn resolve_kms_key_ref_from_env_map(
        &self,
        env_values: &BTreeMap<String, String>,
    ) -> Result<KmsKeyResolution, String> {
        for (index, env_name) in self.kms_key_candidates().into_iter().enumerate() {
            if let Some(value) = env_values.get(&env_name) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return Ok(KmsKeyResolution {
                        selected_env: env_name,
                        key_ref: trimmed.to_string(),
                        failover_used: index > 0,
                    });
                }
            }
        }
        Err(format!(
            "no configured kms key reference resolved from env candidates: {}",
            self.kms_key_candidates().join(", ")
        ))
    }

    pub fn resolve_kms_key_ref_with_provider<P: KmsKeyProvider>(
        &self,
        provider: &P,
    ) -> Result<KmsKeyResolution, String> {
        for (index, env_name) in self.kms_key_candidates().into_iter().enumerate() {
            if let Some(value) = provider.lookup_key_ref(&env_name)? {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return Ok(KmsKeyResolution {
                        selected_env: env_name,
                        key_ref: trimmed.to_string(),
                        failover_used: index > 0,
                    });
                }
            }
        }
        Err(format!(
            "no configured kms key reference resolved from provider candidates: {}",
            self.kms_key_candidates().join(", ")
        ))
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
                    "kms_failover_key_ref_envs":["VNG_KMS_KEY_URI_REGION_B","VNG_KMS_KEY_URI_REGION_C"],
          "allowed_operator_roles":["dba","sre"],
          "token_ttl_seconds":300
        }"#;
        let cfg = SecurityConfigContract::from_json_str(json).expect("valid");
        assert_eq!(cfg.admin_api_key_env, "VNG_ADMIN_API_KEY");
        assert_eq!(cfg.kms_key_ref_env, "VNG_KMS_KEY_URI");
                assert_eq!(
                        cfg.kms_failover_key_ref_envs,
                        vec![
                                "VNG_KMS_KEY_URI_REGION_B".to_string(),
                                "VNG_KMS_KEY_URI_REGION_C".to_string()
                        ]
                );
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
security.kmsFailoverKeyRefEnvs=VNG_KMS_KEY_URI_REGION_B,VNG_KMS_KEY_URI_REGION_C
security.allowedOperatorRoles=dba,sre
security.tokenTtlSeconds=300
"#;
        let cfg = SecurityConfigContract::from_properties_str(props).expect("valid");
        assert_eq!(cfg.allowed_operator_roles.len(), 2);
    assert_eq!(cfg.kms_failover_key_ref_envs.len(), 2);
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
kms_failover_key_ref_envs:
    - VNG_KMS_KEY_URI_REGION_B
    - VNG_KMS_KEY_URI_REGION_C
allowed_operator_roles:
  - dba
  - sre
token_ttl_seconds: 300
"#;
        let cfg = SecurityConfigContract::from_yaml_str(yaml).expect("valid");
        assert!(cfg.encryption_at_rest_required);
        assert_eq!(cfg.kms_failover_key_ref_envs.len(), 2);
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

    #[test]
    fn h05_resolves_primary_kms_region_when_available() {
        let cfg = SecurityConfigContract {
            admin_api_key_env: "VNG_ADMIN_API_KEY".to_string(),
            admin_header_name: "x-vng-admin-key".to_string(),
            tls_required: true,
            mtls_required: false,
            encryption_at_rest_required: true,
            kms_key_ref_env: "VNG_KMS_KEY_URI".to_string(),
            kms_failover_key_ref_envs: vec!["VNG_KMS_KEY_URI_REGION_B".to_string()],
            allowed_operator_roles: vec!["dba".to_string(), "sre".to_string()],
            token_ttl_seconds: 300,
        };
        let env_values = BTreeMap::from([
            ("VNG_KMS_KEY_URI".to_string(), "kms://region-a/key-primary".to_string()),
            (
                "VNG_KMS_KEY_URI_REGION_B".to_string(),
                "kms://region-b/key-secondary".to_string(),
            ),
        ]);

        let resolution = cfg
            .resolve_kms_key_ref_from_env_map(&env_values)
            .expect("primary resolution");

        assert_eq!(resolution.selected_env, "VNG_KMS_KEY_URI");
        assert_eq!(resolution.key_ref, "kms://region-a/key-primary");
        assert!(!resolution.failover_used);
    }

    #[test]
    fn h05_falls_back_to_secondary_kms_region_when_primary_missing() {
        let cfg = SecurityConfigContract {
            admin_api_key_env: "VNG_ADMIN_API_KEY".to_string(),
            admin_header_name: "x-vng-admin-key".to_string(),
            tls_required: true,
            mtls_required: false,
            encryption_at_rest_required: true,
            kms_key_ref_env: "VNG_KMS_KEY_URI".to_string(),
            kms_failover_key_ref_envs: vec![
                "VNG_KMS_KEY_URI_REGION_B".to_string(),
                "VNG_KMS_KEY_URI_REGION_C".to_string(),
            ],
            allowed_operator_roles: vec!["dba".to_string(), "sre".to_string()],
            token_ttl_seconds: 300,
        };
        let env_values = BTreeMap::from([
            (
                "VNG_KMS_KEY_URI_REGION_B".to_string(),
                "kms://region-b/key-secondary".to_string(),
            ),
            (
                "VNG_KMS_KEY_URI_REGION_C".to_string(),
                "kms://region-c/key-tertiary".to_string(),
            ),
        ]);

        let resolution = cfg
            .resolve_kms_key_ref_from_env_map(&env_values)
            .expect("failover resolution");

        assert_eq!(resolution.selected_env, "VNG_KMS_KEY_URI_REGION_B");
        assert_eq!(resolution.key_ref, "kms://region-b/key-secondary");
        assert!(resolution.failover_used);
    }

    #[test]
    fn h05_rejects_duplicate_kms_failover_env_names() {
        let json = r#"{
          "admin_api_key_env":"VNG_ADMIN_API_KEY",
          "admin_header_name":"x-vng-admin-key",
          "tls_required":true,
          "mtls_required":false,
          "encryption_at_rest_required":true,
          "kms_key_ref_env":"VNG_KMS_KEY_URI",
          "kms_failover_key_ref_envs":["VNG_KMS_KEY_URI","VNG_KMS_KEY_URI_REGION_B"],
          "allowed_operator_roles":["dba","sre"],
          "token_ttl_seconds":300
        }"#;

        let err = SecurityConfigContract::from_json_str(json).expect_err("must reject duplicates");

        assert!(err.contains("duplicate"));
    }

    #[test]
    fn h05_fails_when_all_kms_regions_are_unavailable() {
        let cfg = SecurityConfigContract {
            admin_api_key_env: "VNG_ADMIN_API_KEY".to_string(),
            admin_header_name: "x-vng-admin-key".to_string(),
            tls_required: true,
            mtls_required: false,
            encryption_at_rest_required: true,
            kms_key_ref_env: "VNG_KMS_KEY_URI".to_string(),
            kms_failover_key_ref_envs: vec!["VNG_KMS_KEY_URI_REGION_B".to_string()],
            allowed_operator_roles: vec!["dba".to_string(), "sre".to_string()],
            token_ttl_seconds: 300,
        };

        let err = cfg
            .resolve_kms_key_ref_from_env_map(&BTreeMap::new())
            .expect_err("must reject unavailable regions");

        assert!(err.contains("no configured kms key reference resolved"));
    }

    #[test]
    fn h05_provider_adapter_resolves_primary_and_failover_regions() {
        let cfg = SecurityConfigContract {
            admin_api_key_env: "VNG_ADMIN_API_KEY".to_string(),
            admin_header_name: "x-vng-admin-key".to_string(),
            tls_required: true,
            mtls_required: false,
            encryption_at_rest_required: true,
            kms_key_ref_env: "VNG_KMS_KEY_URI".to_string(),
            kms_failover_key_ref_envs: vec!["VNG_KMS_KEY_URI_REGION_B".to_string()],
            allowed_operator_roles: vec!["dba".to_string(), "sre".to_string()],
            token_ttl_seconds: 300,
        };
        let mut adapter = InMemoryKmsProviderAdapter::new("generic");
        adapter.register_key_ref("VNG_KMS_KEY_URI", "kms://region-a/key-primary");
        adapter.register_key_ref("VNG_KMS_KEY_URI_REGION_B", "kms://region-b/key-secondary");

        let primary = cfg
            .resolve_kms_key_ref_with_provider(&adapter)
            .expect("provider primary resolution");
        assert_eq!(primary.selected_env, "VNG_KMS_KEY_URI");
        assert!(!primary.failover_used);

        adapter.mark_unavailable("VNG_KMS_KEY_URI");
        let failover = cfg
            .resolve_kms_key_ref_with_provider(&adapter)
            .expect("provider failover resolution");
        assert_eq!(failover.selected_env, "VNG_KMS_KEY_URI_REGION_B");
        assert!(failover.failover_used);
    }

    #[test]
    fn h05_provider_chain_searches_multiple_adapters() {
        let cfg = SecurityConfigContract {
            admin_api_key_env: "VNG_ADMIN_API_KEY".to_string(),
            admin_header_name: "x-vng-admin-key".to_string(),
            tls_required: true,
            mtls_required: false,
            encryption_at_rest_required: true,
            kms_key_ref_env: "VNG_KMS_KEY_URI".to_string(),
            kms_failover_key_ref_envs: vec!["VNG_KMS_KEY_URI_REGION_B".to_string()],
            allowed_operator_roles: vec!["dba".to_string(), "sre".to_string()],
            token_ttl_seconds: 300,
        };

        let primary = InMemoryKmsProviderAdapter::new("empty");
        let mut secondary = InMemoryKmsProviderAdapter::new("fallback");
        secondary.register_key_ref("VNG_KMS_KEY_URI_REGION_B", "kms://region-b/key-secondary");
        let chain = KmsProviderChain::new(vec![&primary, &secondary]);

        let resolution = cfg
            .resolve_kms_key_ref_with_provider(&chain)
            .expect("chain resolution");

        assert_eq!(resolution.selected_env, "VNG_KMS_KEY_URI_REGION_B");
        assert!(resolution.failover_used);
    }

    #[test]
    fn detects_cloud_kms_provider_kind_from_key_ref() {
        assert_eq!(detect_kms_provider_kind("arn:aws:kms:us-east-1:123:key/abc"), KmsProviderKind::AwsCli);
        assert_eq!(detect_kms_provider_kind("https://sample.vault.azure.net/keys/demo/123"), KmsProviderKind::AzureCli);
        assert_eq!(detect_kms_provider_kind("projects/sample/locations/us-central1/keyRings/main/cryptoKeys/demo"), KmsProviderKind::GcpCli);
        assert_eq!(detect_kms_provider_kind("kms://region-a/key-primary"), KmsProviderKind::Generic);
    }

    #[test]
    fn ws5_rbac_privilege_matrix_allows_exact_resource_scope() {
        let mut matrix = RbacPrivilegeMatrix::new();
        matrix.grant_role(
            "sre",
            ResourceGrant {
                resource: "cluster.failover".to_string(),
                scopes: vec!["cluster".to_string()],
                actions: vec![PrivilegeAction::Execute],
            },
        );

        assert!(matrix.allows(
            "sre",
            "cluster.failover",
            "cluster",
            PrivilegeAction::Execute,
        ));
        assert!(!matrix.allows(
            "sre",
            "cluster.failover",
            "cluster",
            PrivilegeAction::Read,
        ));
    }

    #[test]
    fn ws5_rbac_privilege_matrix_allows_wildcard_scopes() {
        let mut matrix = RbacPrivilegeMatrix::new();
        matrix.grant_role(
            "security",
            ResourceGrant {
                resource: "observability.audit".to_string(),
                scopes: vec!["audit/*".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );

        assert!(matrix.allows(
            "security",
            "observability.audit",
            "audit/events",
            PrivilegeAction::Read,
        ));
        assert!(!matrix.allows(
            "security",
            "observability.audit",
            "autonomous/actions",
            PrivilegeAction::Read,
        ));
    }

    #[test]
    fn ws5_rbac_privilege_matrix_allows_tenant_scope_templates() {
        let mut matrix = RbacPrivilegeMatrix::new();
        matrix.grant_role(
            "tenant_analyst",
            ResourceGrant {
                resource: "sql.runtime".to_string(),
                scopes: vec!["tenants/{tenant}/sql/analyze".to_string()],
                actions: vec![PrivilegeAction::Read],
            },
        );

        assert!(matrix.allows(
            "tenant_analyst",
            "sql.runtime",
            "tenants/acme/sql/analyze",
            PrivilegeAction::Read,
        ));
        assert!(!matrix.allows(
            "tenant_analyst",
            "sql.runtime",
            "tenants/acme/sql/execute",
            PrivilegeAction::Read,
        ));
    }
}
