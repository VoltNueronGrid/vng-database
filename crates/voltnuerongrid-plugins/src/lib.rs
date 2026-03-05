#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-plugins";

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use voltnuerongrid_ingest::{
    ConnectorDescriptor, ConnectorDirection, ConnectorRegistry, IngestRecord, StaticInMemoryConnector,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorPackageMetadata {
    pub plugin_id: String,
    pub version: String,
    pub display_name: String,
    pub owner: String,
    pub license: String,
    pub checksum_sha256: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifestSignature {
    pub algorithm: String,
    pub key_id: String,
    pub signature_base64: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedPluginManifest {
    pub schema_version: String,
    pub declared_checksum_sha256: String,
    pub generated_epoch_ms: u128,
    pub signature: PluginManifestSignature,
    pub revoked_key_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorPluginPackage {
    pub manifest: SignedPluginManifest,
    pub metadata: ConnectorPackageMetadata,
    pub descriptor: ConnectorDescriptor,
    pub bootstrap_records: Vec<IngestRecord>,
}

pub trait ConnectorValidationHook: Send + Sync {
    fn validate(&self, package: &ConnectorPluginPackage) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRegistrationError {
    ValidationFailed(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    Development,
    Internal,
    Production,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyTrustRecord {
    pub trust_level: TrustLevel,
    pub revoked: bool,
}

pub trait SigningKeyring: Send + Sync {
    fn lookup_key(&self, key_id: &str) -> Option<KeyTrustRecord>;
}

#[derive(Debug, Clone, Default)]
pub struct InMemorySigningKeyring {
    key_records: HashMap<String, KeyTrustRecord>,
}

impl InMemorySigningKeyring {
    pub fn with_records(records: HashMap<String, KeyTrustRecord>) -> Self {
        Self {
            key_records: records,
        }
    }
}

impl SigningKeyring for InMemorySigningKeyring {
    fn lookup_key(&self, key_id: &str) -> Option<KeyTrustRecord> {
        self.key_records.get(key_id).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureVerificationPolicy {
    pub required_algorithm: String,
    pub minimum_trust_level: TrustLevel,
    pub require_non_revoked_key: bool,
}

impl Default for SignatureVerificationPolicy {
    fn default() -> Self {
        Self {
            required_algorithm: "ed25519".to_string(),
            minimum_trust_level: TrustLevel::Production,
            require_non_revoked_key: true,
        }
    }
}

pub struct SignaturePolicyHook {
    policy: SignatureVerificationPolicy,
    keyring: Box<dyn SigningKeyring>,
}

impl SignaturePolicyHook {
    pub fn new(policy: SignatureVerificationPolicy, keyring: Box<dyn SigningKeyring>) -> Self {
        Self { policy, keyring }
    }
}

impl ConnectorValidationHook for SignaturePolicyHook {
    fn validate(&self, package: &ConnectorPluginPackage) -> Result<(), String> {
        let signature = &package.manifest.signature;
        if signature.algorithm != self.policy.required_algorithm {
            return Err("manifest signature algorithm is not allowed by policy".to_string());
        }

        let key_info = self
            .keyring
            .lookup_key(&signature.key_id)
            .ok_or_else(|| "manifest key_id is unknown to keyring".to_string())?;

        if package
            .manifest
            .revoked_key_ids
            .iter()
            .any(|revoked| revoked == &signature.key_id)
        {
            return Err("manifest key_id is listed in manifest.revoked_key_ids".to_string());
        }

        if self.policy.require_non_revoked_key && key_info.revoked {
            return Err("manifest key_id is revoked in keyring".to_string());
        }

        if key_info.trust_level < self.policy.minimum_trust_level {
            return Err("manifest key trust level is below policy minimum".to_string());
        }

        // Placeholder signature verification until cryptographic signature validation lands.
        if signature.signature_base64.len() < 16 {
            return Err("manifest signature payload is too short".to_string());
        }

        Ok(())
    }
}

#[derive(Default)]
pub struct ChecksumVerificationHook;

impl ConnectorValidationHook for ChecksumVerificationHook {
    fn validate(&self, package: &ConnectorPluginPackage) -> Result<(), String> {
        let computed = compute_package_checksum_sha256(package);
        if package.metadata.checksum_sha256 != package.manifest.declared_checksum_sha256 {
            return Err(
                "metadata.checksum_sha256 must match manifest.declared_checksum_sha256".to_string(),
            );
        }
        if package.manifest.declared_checksum_sha256 != computed {
            return Err("manifest checksum does not match computed package digest".to_string());
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct PluginRegistrationBoundary {
    hooks: Vec<Box<dyn ConnectorValidationHook>>,
}

impl PluginRegistrationBoundary {
    pub fn new() -> Self {
        Self::new_with_keyring(Box::new(default_signing_keyring()))
    }

    pub fn new_with_keyring(keyring: Box<dyn SigningKeyring>) -> Self {
        let mut boundary = Self { hooks: Vec::new() };
        boundary.register_hook(Box::new(ChecksumVerificationHook));
        boundary.register_hook(Box::new(SignaturePolicyHook::new(
            SignatureVerificationPolicy::default(),
            keyring,
        )));
        boundary
    }

    pub fn register_hook(&mut self, hook: Box<dyn ConnectorValidationHook>) {
        self.hooks.push(hook);
    }

    pub fn register_connector_package(
        &self,
        registry: &mut ConnectorRegistry,
        package: ConnectorPluginPackage,
    ) -> Result<(), PluginRegistrationError> {
        let mut issues = validate_metadata_and_capabilities(&package);
        for hook in &self.hooks {
            if let Err(issue) = hook.validate(&package) {
                issues.push(issue);
            }
        }

        if !issues.is_empty() {
            return Err(PluginRegistrationError::ValidationFailed(issues));
        }

        let connector = StaticInMemoryConnector::new(
            package.descriptor.clone(),
            package.bootstrap_records.clone(),
        );
        registry.register(Box::new(connector));
        Ok(())
    }
}

fn validate_metadata_and_capabilities(package: &ConnectorPluginPackage) -> Vec<String> {
    let mut issues = Vec::new();

    if package.manifest.schema_version.trim().is_empty() {
        issues.push("manifest.schema_version must not be empty".to_string());
    }
    if package.manifest.signature.algorithm.trim().is_empty() {
        issues.push("manifest.signature.algorithm must not be empty".to_string());
    }
    if package.manifest.signature.key_id.trim().is_empty() {
        issues.push("manifest.signature.key_id must not be empty".to_string());
    }
    if package.manifest.signature.signature_base64.trim().is_empty() {
        issues.push("manifest.signature.signature_base64 must not be empty".to_string());
    }
    if package.manifest.revoked_key_ids.iter().any(|k| k.trim().is_empty()) {
        issues.push("manifest.revoked_key_ids must not contain empty entries".to_string());
    }

    if package.metadata.plugin_id.trim().is_empty() {
        issues.push("metadata.plugin_id must not be empty".to_string());
    }
    if package.metadata.version.trim().is_empty() {
        issues.push("metadata.version must not be empty".to_string());
    }
    if package.metadata.display_name.trim().is_empty() {
        issues.push("metadata.display_name must not be empty".to_string());
    }
    if package.metadata.checksum_sha256.trim().len() < 16 {
        issues.push("metadata.checksum_sha256 is too short".to_string());
    }

    let capabilities = &package.metadata.capabilities;
    match package.descriptor.direction {
        ConnectorDirection::Inbound => {
            if !capabilities.iter().any(|c| c == "ingest.read") {
                issues.push("inbound connectors require capability ingest.read".to_string());
            }
        }
        ConnectorDirection::Outbound => {
            if !capabilities.iter().any(|c| c == "ingest.write") {
                issues.push("outbound connectors require capability ingest.write".to_string());
            }
        }
        ConnectorDirection::Bidirectional => {
            if !capabilities.iter().any(|c| c == "ingest.read") {
                issues.push("bidirectional connectors require capability ingest.read".to_string());
            }
            if !capabilities.iter().any(|c| c == "ingest.write") {
                issues.push("bidirectional connectors require capability ingest.write".to_string());
            }
        }
    }

    issues
}

fn default_signing_keyring() -> InMemorySigningKeyring {
    let mut records = HashMap::new();
    records.insert(
        "ws7-signer-1".to_string(),
        KeyTrustRecord {
            trust_level: TrustLevel::Production,
            revoked: false,
        },
    );
    records.insert(
        "ws7-signer-dev".to_string(),
        KeyTrustRecord {
            trust_level: TrustLevel::Development,
            revoked: false,
        },
    );
    records.insert(
        "ws7-signer-revoked".to_string(),
        KeyTrustRecord {
            trust_level: TrustLevel::Production,
            revoked: true,
        },
    );
    InMemorySigningKeyring::with_records(records)
}

pub fn compute_package_checksum_sha256(package: &ConnectorPluginPackage) -> String {
    let mut hasher = Sha256::new();
    hasher.update(package.metadata.plugin_id.as_bytes());
    hasher.update(b"|");
    hasher.update(package.metadata.version.as_bytes());
    hasher.update(b"|");
    hasher.update(package.metadata.display_name.as_bytes());
    hasher.update(b"|");
    hasher.update(package.metadata.owner.as_bytes());
    hasher.update(b"|");
    hasher.update(package.metadata.license.as_bytes());
    hasher.update(b"|");
    hasher.update(package.descriptor.id.as_bytes());
    hasher.update(b"|");
    hasher.update(package.descriptor.display_name.as_bytes());
    hasher.update(b"|");
    hasher.update(format!("{:?}", package.descriptor.format).as_bytes());
    hasher.update(b"|");
    hasher.update(format!("{:?}", package.descriptor.direction).as_bytes());
    hasher.update(b"|");

    let mut capabilities = package.metadata.capabilities.clone();
    capabilities.sort();
    for capability in capabilities {
        hasher.update(capability.as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"|");

    for record in &package.bootstrap_records {
        hasher.update(record.key.as_bytes());
        hasher.update(b"=");
        hasher.update(record.payload.as_bytes());
        hasher.update(b";");
    }

    let bytes = hasher.finalize();
    let mut checksum = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        checksum.push_str(&format!("{byte:02x}"));
    }
    checksum
}

#[cfg(test)]
mod tests {
    use super::*;
    use voltnuerongrid_ingest::{ConnectorDirection, IngestFormat};

    struct RequireOwnerPrefixHook;
    impl ConnectorValidationHook for RequireOwnerPrefixHook {
        fn validate(&self, package: &ConnectorPluginPackage) -> Result<(), String> {
            if package.metadata.owner.starts_with("team-") {
                Ok(())
            } else {
                Err("metadata.owner must start with team-".to_string())
            }
        }
    }

    fn valid_package() -> ConnectorPluginPackage {
        let mut package = ConnectorPluginPackage {
            manifest: SignedPluginManifest {
                schema_version: "v1".to_string(),
                declared_checksum_sha256: String::new(),
                generated_epoch_ms: 1_700_000_000_000,
                signature: PluginManifestSignature {
                    algorithm: "ed25519".to_string(),
                    key_id: "ws7-signer-1".to_string(),
                    signature_base64: "c2lnbmF0dXJlLWJhc2U2NA==".to_string(),
                },
                revoked_key_ids: Vec::new(),
            },
            metadata: ConnectorPackageMetadata {
                plugin_id: "connector.ftp".to_string(),
                version: "0.1.0".to_string(),
                display_name: "FTP Connector".to_string(),
                owner: "team-ingest".to_string(),
                license: "Apache-2.0".to_string(),
                checksum_sha256: String::new(),
                capabilities: vec!["ingest.read".to_string()],
            },
            descriptor: ConnectorDescriptor {
                id: "ftp-inbound".to_string(),
                display_name: "FTP Inbound".to_string(),
                format: IngestFormat::Stream,
                direction: ConnectorDirection::Inbound,
            },
            bootstrap_records: vec![IngestRecord {
                key: "sample-1".to_string(),
                payload: "{\"source\":\"ftp\"}".to_string(),
            }],
        };
        let checksum = compute_package_checksum_sha256(&package);
        package.metadata.checksum_sha256 = checksum.clone();
        package.manifest.declared_checksum_sha256 = checksum;
        package
    }

    #[test]
    fn registers_valid_package() {
        let mut registry = ConnectorRegistry::default();
        let mut boundary = PluginRegistrationBoundary::new();
        boundary.register_hook(Box::new(RequireOwnerPrefixHook));
        let package = valid_package();

        boundary
            .register_connector_package(&mut registry, package)
            .expect("package should register");

        assert!(registry.has_connector("ftp-inbound"));
        let batch = registry
            .read_batch("ftp-inbound", 1)
            .expect("connector should be registered");
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn rejects_package_with_missing_required_fields() {
        let mut registry = ConnectorRegistry::default();
        let boundary = PluginRegistrationBoundary::new();
        let mut package = valid_package();
        package.manifest.schema_version.clear();
        package.metadata.plugin_id.clear();
        package.metadata.checksum_sha256 = "abc".to_string();

        let error = boundary
            .register_connector_package(&mut registry, package)
            .expect_err("expected validation failure");

        match error {
            PluginRegistrationError::ValidationFailed(issues) => {
                assert!(issues.iter().any(|i| i.contains("manifest.schema_version")));
                assert!(issues.iter().any(|i| i.contains("plugin_id")));
                assert!(issues.iter().any(|i| i.contains("checksum_sha256")));
            }
        }
    }

    #[test]
    fn rejects_package_when_capability_missing() {
        let mut registry = ConnectorRegistry::default();
        let boundary = PluginRegistrationBoundary::new();
        let mut package = valid_package();
        package.metadata.capabilities.clear();

        let error = boundary
            .register_connector_package(&mut registry, package)
            .expect_err("expected capability failure");

        match error {
            PluginRegistrationError::ValidationFailed(issues) => {
                assert!(issues.iter().any(|i| i.contains("ingest.read")));
            }
        }
    }

    #[test]
    fn rejects_package_when_custom_hook_fails() {
        let mut registry = ConnectorRegistry::default();
        let mut boundary = PluginRegistrationBoundary::new();
        boundary.register_hook(Box::new(RequireOwnerPrefixHook));
        let mut package = valid_package();
        package.metadata.owner = "security".to_string();

        let error = boundary
            .register_connector_package(&mut registry, package)
            .expect_err("expected hook failure");

        match error {
            PluginRegistrationError::ValidationFailed(issues) => {
                assert!(issues.iter().any(|i| i.contains("owner")));
            }
        }
    }

    #[test]
    fn rejects_package_when_manifest_checksum_is_tampered() {
        let mut registry = ConnectorRegistry::default();
        let boundary = PluginRegistrationBoundary::new();
        let mut package = valid_package();
        package.manifest.declared_checksum_sha256 =
            "00000000000000000000000000000000".to_string();

        let error = boundary
            .register_connector_package(&mut registry, package)
            .expect_err("expected checksum failure");
        match error {
            PluginRegistrationError::ValidationFailed(issues) => {
                assert!(issues.iter().any(|i| i.contains("checksum")));
            }
        }
    }

    #[test]
    fn rejects_package_when_manifest_key_is_unknown() {
        let mut registry = ConnectorRegistry::default();
        let boundary = PluginRegistrationBoundary::new();
        let mut package = valid_package();
        package.manifest.signature.key_id = "unknown-key".to_string();

        let error = boundary
            .register_connector_package(&mut registry, package)
            .expect_err("expected keyring failure");
        match error {
            PluginRegistrationError::ValidationFailed(issues) => {
                assert!(issues.iter().any(|i| i.contains("unknown")));
            }
        }
    }

    #[test]
    fn rejects_package_when_manifest_key_is_revoked() {
        let mut registry = ConnectorRegistry::default();
        let boundary = PluginRegistrationBoundary::new();
        let mut package = valid_package();
        package.manifest.signature.key_id = "ws7-signer-revoked".to_string();
        package.manifest.revoked_key_ids = vec!["ws7-signer-revoked".to_string()];

        let error = boundary
            .register_connector_package(&mut registry, package)
            .expect_err("expected revoked key failure");
        match error {
            PluginRegistrationError::ValidationFailed(issues) => {
                assert!(issues.iter().any(|i| i.contains("revoked")));
            }
        }
    }

    #[test]
    fn rejects_package_when_manifest_key_trust_too_low() {
        let mut registry = ConnectorRegistry::default();
        let boundary = PluginRegistrationBoundary::new();
        let mut package = valid_package();
        package.manifest.signature.key_id = "ws7-signer-dev".to_string();

        let error = boundary
            .register_connector_package(&mut registry, package)
            .expect_err("expected trust-level failure");
        match error {
            PluginRegistrationError::ValidationFailed(issues) => {
                assert!(issues.iter().any(|i| i.contains("trust level")));
            }
        }
    }
}
