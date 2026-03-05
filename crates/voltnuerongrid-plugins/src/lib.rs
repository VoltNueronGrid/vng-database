#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-plugins";

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
pub struct ConnectorPluginPackage {
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

#[derive(Default)]
pub struct PluginRegistrationBoundary {
    hooks: Vec<Box<dyn ConnectorValidationHook>>,
}

impl PluginRegistrationBoundary {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
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
        ConnectorPluginPackage {
            metadata: ConnectorPackageMetadata {
                plugin_id: "connector.ftp".to_string(),
                version: "0.1.0".to_string(),
                display_name: "FTP Connector".to_string(),
                owner: "team-ingest".to_string(),
                license: "Apache-2.0".to_string(),
                checksum_sha256: "1234567890abcdef1234567890abcdef".to_string(),
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
        package.metadata.plugin_id.clear();
        package.metadata.checksum_sha256 = "abc".to_string();

        let error = boundary
            .register_connector_package(&mut registry, package)
            .expect_err("expected validation failure");

        match error {
            PluginRegistrationError::ValidationFailed(issues) => {
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
}
