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

// --- Supply-Chain Provenance ------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceAttestation {
    pub attester_id: String,
    pub attested_at_ms: u64,
    pub attestation_type: AttestationType,
    pub payload_digest_sha256: String,
    pub signature_base64: String,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttestationType {
    BuildVerification,
    SecurityScan,
    ChecksumVerification,
    SignatureVerification,
    ReviewApproval,
}

impl AttestationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BuildVerification => "build_verification",
            Self::SecurityScan => "security_scan",
            Self::ChecksumVerification => "checksum_verification",
            Self::SignatureVerification => "signature_verification",
            Self::ReviewApproval => "review_approval",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceChain {
    pub plugin_id: String,
    pub plugin_version: String,
    pub attestations: Vec<ProvenanceAttestation>,
    pub chain_complete: bool,
    pub chain_digest: String,
}

impl ProvenanceChain {
    pub fn new(plugin_id: String, plugin_version: String) -> Self {
        let mut chain = Self {
            plugin_id,
            plugin_version,
            attestations: Vec::new(),
            chain_complete: false,
            chain_digest: String::new(),
        };
        chain.chain_digest = chain.compute_chain_digest();
        chain
    }

    pub fn add_attestation(&mut self, att: ProvenanceAttestation) {
        self.attestations.push(att);
        self.chain_complete = self.is_complete();
        self.chain_digest = self.compute_chain_digest();
    }

    pub fn is_complete(&self) -> bool {
        let has_checksum = self.attestations.iter().any(|att| {
            att.passed && matches!(att.attestation_type, AttestationType::ChecksumVerification)
        });
        let has_signature = self.attestations.iter().any(|att| {
            att.passed && matches!(att.attestation_type, AttestationType::SignatureVerification)
        });
        let has_build_or_review = self.attestations.iter().any(|att| {
            att.passed
                && matches!(
                    att.attestation_type,
                    AttestationType::BuildVerification | AttestationType::ReviewApproval
                )
        });

        has_checksum && has_signature && has_build_or_review
    }

    pub fn compute_chain_digest(&self) -> String {
        let mut digests = self
            .attestations
            .iter()
            .map(|a| a.payload_digest_sha256.clone())
            .collect::<Vec<_>>();
        digests.sort();
        let joined = digests.join(":");
        let bytes = Sha256::digest(joined.as_bytes());
        hex_encode(&bytes)
    }

    pub fn summary(&self) -> ProvenanceChainSummary {
        let passed_count = self.attestations.iter().filter(|att| att.passed).count();
        ProvenanceChainSummary {
            plugin_id: self.plugin_id.clone(),
            plugin_version: self.plugin_version.clone(),
            attestation_count: self.attestations.len(),
            passed_count,
            chain_complete: self.chain_complete,
            chain_digest: self.chain_digest.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceChainSummary {
    pub plugin_id: String,
    pub plugin_version: String,
    pub attestation_count: usize,
    pub passed_count: usize,
    pub chain_complete: bool,
    pub chain_digest: String,
}

// --- SBOM (Software Bill of Materials) inspection ---------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbomEntry {
    pub component_name: String,
    pub component_version: String,
    pub license: String,
    pub checksum_sha256: String,
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbomInspectionResult {
    pub plugin_id: String,
    pub entries: Vec<SbomEntry>,
    pub license_violations: Vec<String>,
    pub missing_checksums: Vec<String>,
    pub approved: bool,
}

impl SbomInspectionResult {
    pub fn inspect(plugin_id: String, entries: Vec<SbomEntry>, disallowed_licenses: &[&str]) -> Self {
        let mut license_violations = Vec::new();
        let mut missing_checksums = Vec::new();

        for entry in &entries {
            if disallowed_licenses
                .iter()
                .any(|license| *license == entry.license)
            {
                license_violations.push(entry.component_name.clone());
            }
            if entry.checksum_sha256.trim().is_empty() {
                missing_checksums.push(entry.component_name.clone());
            }
        }

        let approved = license_violations.is_empty() && missing_checksums.is_empty();
        Self {
            plugin_id,
            entries,
            license_violations,
            missing_checksums,
            approved,
        }
    }
}

// --- Supply-Chain Audit Record ----------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupplyChainEventKind {
    PluginRegistered,
    PluginDeregistered,
    SignatureVerificationFailed,
    ChecksumMismatch,
    ProvenanceChainIncomplete,
    SbomViolation,
    KeyRevocationDetected,
}

impl SupplyChainEventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PluginRegistered => "plugin_registered",
            Self::PluginDeregistered => "plugin_deregistered",
            Self::SignatureVerificationFailed => "signature_verification_failed",
            Self::ChecksumMismatch => "checksum_mismatch",
            Self::ProvenanceChainIncomplete => "provenance_chain_incomplete",
            Self::SbomViolation => "sbom_violation",
            Self::KeyRevocationDetected => "key_revocation_detected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSupplyChainAuditRecord {
    pub record_id: String,
    pub plugin_id: String,
    pub plugin_version: String,
    pub event_kind: SupplyChainEventKind,
    pub timestamp_ms: u64,
    pub details: String,
    pub operator_id: Option<String>,
    pub provenance_digest: Option<String>,
}

// --- Plugin Registry ---------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredPlugin {
    pub plugin_id: String,
    pub plugin_version: String,
    pub manifest: SignedPluginManifest,
    pub metadata: ConnectorPackageMetadata,
    pub registered_at_ms: u64,
    pub registration_operator_id: Option<String>,
    pub provenance_chain: Option<ProvenanceChain>,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRegistryError {
    AlreadyRegistered { plugin_id: String, version: String },
    NotFound { plugin_id: String },
    ValidationFailed(String),
    RegistryFull { max_plugins: usize },
}

impl std::fmt::Display for PluginRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRegistered { plugin_id, version } => {
                write!(f, "plugin {}@{} is already registered", plugin_id, version)
            }
            Self::NotFound { plugin_id } => write!(f, "plugin {} not found", plugin_id),
            Self::ValidationFailed(reason) => write!(f, "plugin validation failed: {}", reason),
            Self::RegistryFull { max_plugins } => {
                write!(f, "plugin registry is full (max={})", max_plugins)
            }
        }
    }
}

impl std::error::Error for PluginRegistryError {}

pub struct PluginLifecycleManager {
    plugins: HashMap<String, RegisteredPlugin>,
    audit_trail: Vec<PluginSupplyChainAuditRecord>,
    max_plugins: usize,
    audit_counter: u64,
    boundary: PluginRegistrationBoundary,
}

impl PluginLifecycleManager {
    pub fn new(max_plugins: usize) -> Self {
        Self {
            plugins: HashMap::new(),
            audit_trail: Vec::new(),
            max_plugins,
            audit_counter: 0,
            boundary: PluginRegistrationBoundary::default(),
        }
    }

    pub fn register(
        &mut self,
        manifest: SignedPluginManifest,
        metadata: ConnectorPackageMetadata,
        operator_id: Option<String>,
        provenance: Option<ProvenanceChain>,
        now_ms: u64,
    ) -> Result<String, PluginRegistryError> {
        let _boundary = &self.boundary;
        if self.plugins.len() >= self.max_plugins {
            return Err(PluginRegistryError::RegistryFull {
                max_plugins: self.max_plugins,
            });
        }

        if metadata.plugin_id.trim().is_empty() {
            return Err(PluginRegistryError::ValidationFailed(
                "metadata.plugin_id must not be empty".to_string(),
            ));
        }
        if metadata.version.trim().is_empty() {
            return Err(PluginRegistryError::ValidationFailed(
                "metadata.version must not be empty".to_string(),
            ));
        }
        if manifest.schema_version.trim().is_empty() {
            return Err(PluginRegistryError::ValidationFailed(
                "manifest.schema_version must not be empty".to_string(),
            ));
        }
        if manifest.declared_checksum_sha256 != metadata.checksum_sha256 {
            return Err(PluginRegistryError::ValidationFailed(
                "manifest.declared_checksum_sha256 must match metadata.checksum_sha256"
                    .to_string(),
            ));
        }

        let key = format!("{}@{}", metadata.plugin_id, metadata.version);
        if self.plugins.contains_key(&key) {
            return Err(PluginRegistryError::AlreadyRegistered {
                plugin_id: metadata.plugin_id,
                version: metadata.version,
            });
        }

        let plugin_id = metadata.plugin_id.clone();
        let plugin_version = metadata.version.clone();
        let provenance_digest = provenance.as_ref().map(|p| p.chain_digest.clone());
        let registered = RegisteredPlugin {
            plugin_id: plugin_id.clone(),
            plugin_version: plugin_version.clone(),
            manifest,
            metadata,
            registered_at_ms: now_ms,
            registration_operator_id: operator_id.clone(),
            provenance_chain: provenance,
            active: true,
        };

        self.plugins.insert(key, registered);
        self.emit_audit(
            plugin_id.clone(),
            plugin_version,
            SupplyChainEventKind::PluginRegistered,
            "plugin registered".to_string(),
            operator_id,
            provenance_digest,
            now_ms,
        );

        Ok(plugin_id)
    }

    pub fn deregister(
        &mut self,
        plugin_id: &str,
        version: &str,
        operator_id: Option<String>,
        now_ms: u64,
    ) -> Result<(), PluginRegistryError> {
        let key = format!("{}@{}", plugin_id, version);
        let plugin = self
            .plugins
            .get_mut(&key)
            .ok_or_else(|| PluginRegistryError::NotFound {
                plugin_id: plugin_id.to_string(),
            })?;

        plugin.active = false;
        let provenance_digest = plugin.provenance_chain.as_ref().map(|p| p.chain_digest.clone());
        self.emit_audit(
            plugin_id.to_string(),
            version.to_string(),
            SupplyChainEventKind::PluginDeregistered,
            "plugin deregistered".to_string(),
            operator_id,
            provenance_digest,
            now_ms,
        );

        Ok(())
    }

    pub fn query_by_id(&self, plugin_id: &str) -> Vec<&RegisteredPlugin> {
        self.plugins
            .values()
            .filter(|plugin| plugin.plugin_id == plugin_id)
            .collect()
    }

    pub fn list_active(&self) -> Vec<&RegisteredPlugin> {
        self.plugins.values().filter(|plugin| plugin.active).collect()
    }

    pub fn audit_trail(&self) -> &[PluginSupplyChainAuditRecord] {
        &self.audit_trail
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    fn emit_audit(
        &mut self,
        plugin_id: String,
        version: String,
        kind: SupplyChainEventKind,
        details: String,
        operator_id: Option<String>,
        provenance_digest: Option<String>,
        now_ms: u64,
    ) {
        self.audit_counter += 1;
        let record = PluginSupplyChainAuditRecord {
            record_id: format!("sc-audit-{}", self.audit_counter),
            plugin_id,
            plugin_version: version,
            event_kind: kind,
            timestamp_ms: now_ms,
            details,
            operator_id,
            provenance_digest,
        };
        self.audit_trail.push(record);
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
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

    fn make_test_manifest() -> SignedPluginManifest {
        SignedPluginManifest {
            schema_version: "1.0".into(),
            declared_checksum_sha256:
                "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd".into(),
            generated_epoch_ms: 1_700_000_000_000,
            signature: PluginManifestSignature {
                algorithm: "hmac-sha256".into(),
                key_id: "prod-key-001".into(),
                signature_base64: "dGVzdHNpZ25hdHVyZXBhc3Nib3VuZGFyeXZlcmlmeWluZw==".into(),
            },
            revoked_key_ids: vec![],
        }
    }

    fn make_test_metadata(id: &str, version: &str) -> ConnectorPackageMetadata {
        ConnectorPackageMetadata {
            plugin_id: id.into(),
            version: version.into(),
            display_name: format!("Test Plugin {}", id),
            owner: "test-team".into(),
            license: "Apache-2.0".into(),
            checksum_sha256:
                "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd".into(),
            capabilities: vec!["ingest.read".into()],
        }
    }

    fn make_attestation(kind: AttestationType, digest: &str, passed: bool) -> ProvenanceAttestation {
        ProvenanceAttestation {
            attester_id: "ci-pipeline-prod".into(),
            attested_at_ms: 1_700_000_000_100,
            attestation_type: kind,
            payload_digest_sha256: digest.into(),
            signature_base64: "sig".into(),
            passed,
        }
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

    #[test]
    fn test_provenance_chain_completeness() {
        let mut chain = ProvenanceChain::new("plugin-a".into(), "1.0.0".into());
        chain.add_attestation(make_attestation(
            AttestationType::ChecksumVerification,
            "aa",
            true,
        ));
        chain.add_attestation(make_attestation(
            AttestationType::SignatureVerification,
            "bb",
            true,
        ));
        chain.add_attestation(make_attestation(
            AttestationType::BuildVerification,
            "cc",
            true,
        ));
        assert!(chain.is_complete());
    }

    #[test]
    fn test_provenance_chain_incomplete() {
        let mut chain = ProvenanceChain::new("plugin-a".into(), "1.0.0".into());
        chain.add_attestation(make_attestation(
            AttestationType::ChecksumVerification,
            "aa",
            true,
        ));
        assert!(!chain.is_complete());
    }

    #[test]
    fn test_provenance_chain_digest_changes_on_add() {
        let mut chain = ProvenanceChain::new("plugin-a".into(), "1.0.0".into());
        let before = chain.chain_digest.clone();
        chain.add_attestation(make_attestation(
            AttestationType::ChecksumVerification,
            "aa",
            true,
        ));
        let after = chain.chain_digest.clone();
        assert_ne!(before, after);
    }

    #[test]
    fn test_sbom_inspection_approves_clean() {
        let entries = vec![SbomEntry {
            component_name: "serde".into(),
            component_version: "1.0.0".into(),
            license: "MIT".into(),
            checksum_sha256: "abc".into(),
            source_url: None,
        }];
        let result = SbomInspectionResult::inspect("plugin-a".into(), entries, &["GPL"]);
        assert!(result.approved);
        assert!(result.license_violations.is_empty());
        assert!(result.missing_checksums.is_empty());
    }

    #[test]
    fn test_sbom_inspection_finds_violations() {
        let entries = vec![SbomEntry {
            component_name: "libx".into(),
            component_version: "2.1.0".into(),
            license: "GPL".into(),
            checksum_sha256: "abc".into(),
            source_url: None,
        }];
        let result = SbomInspectionResult::inspect("plugin-a".into(), entries, &["GPL"]);
        assert!(!result.approved);
        assert_eq!(result.license_violations, vec!["libx".to_string()]);
    }

    #[test]
    fn test_plugin_lifecycle_register_deregister() {
        let mut manager = PluginLifecycleManager::new(8);
        let manifest = make_test_manifest();
        let metadata = make_test_metadata("plugin-a", "1.0.0");

        let plugin_id = manager
            .register(
                manifest,
                metadata,
                Some("operator-1".into()),
                None,
                1_700_000_000_200,
            )
            .expect("register should succeed");

        assert_eq!(plugin_id, "plugin-a");
        assert_eq!(manager.query_by_id("plugin-a").len(), 1);

        manager
            .deregister("plugin-a", "1.0.0", Some("operator-2".into()), 1_700_000_000_300)
            .expect("deregister should succeed");

        assert!(manager
            .list_active()
            .iter()
            .all(|plugin| plugin.plugin_id != "plugin-a"));
    }

    #[test]
    fn test_plugin_lifecycle_prevents_duplicate_registration() {
        let mut manager = PluginLifecycleManager::new(8);
        manager
            .register(
                make_test_manifest(),
                make_test_metadata("plugin-a", "1.0.0"),
                None,
                None,
                1_700_000_000_200,
            )
            .expect("first registration should succeed");

        let err = manager
            .register(
                make_test_manifest(),
                make_test_metadata("plugin-a", "1.0.0"),
                None,
                None,
                1_700_000_000_201,
            )
            .expect_err("second registration should fail");

        match err {
            PluginRegistryError::AlreadyRegistered { plugin_id, version } => {
                assert_eq!(plugin_id, "plugin-a");
                assert_eq!(version, "1.0.0");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn test_plugin_lifecycle_audit_trail_grows() {
        let mut manager = PluginLifecycleManager::new(8);
        manager
            .register(
                make_test_manifest(),
                make_test_metadata("plugin-b", "2.0.0"),
                Some("operator-1".into()),
                None,
                1_700_000_000_500,
            )
            .expect("registration should succeed");
        manager
            .deregister("plugin-b", "2.0.0", Some("operator-1".into()), 1_700_000_000_600)
            .expect("deregister should succeed");

        assert_eq!(manager.audit_trail().len(), 2);
        assert_eq!(manager.audit_trail()[0].event_kind.as_str(), "plugin_registered");
        assert_eq!(
            manager.audit_trail()[1].event_kind.as_str(),
            "plugin_deregistered"
        );
    }
}
