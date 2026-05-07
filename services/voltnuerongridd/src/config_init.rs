use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use voltnuerongrid_auth::{ConfiguredKmsProviderAdapter, SecurityConfigContract};
use voltnuerongrid_ingest::{ManagedEventBusTransport, ManagedReplayCursorStore};
use crate::{KmsRuntimeState, OperatorRole, TenantUserBinding, CONTROL_PLANE_OPERATOR_ROLES};

pub(crate) fn default_allowed_operator_roles() -> HashSet<OperatorRole> {
    CONTROL_PLANE_OPERATOR_ROLES.into_iter().collect()
}

pub(crate) fn load_allowed_operator_roles() -> HashSet<OperatorRole> {
    let parsed = env::var("VNG_ALLOWED_OPERATOR_ROLES")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|entry| OperatorRole::parse(entry.trim()))
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    if parsed.is_empty() {
        default_allowed_operator_roles()
    } else {
        parsed
    }
}

pub(crate) fn default_operator_role_bindings() -> HashMap<String, OperatorRole> {
    HashMap::from([
        ("platform-admin".to_string(), OperatorRole::Dba),
        ("admin".to_string(), OperatorRole::Dba),
        ("automation".to_string(), OperatorRole::Sre),
        ("auto_sre".to_string(), OperatorRole::Sre),
        ("security-bot".to_string(), OperatorRole::Security),
        ("autopilot".to_string(), OperatorRole::AiOperator),
    ])
}

pub(crate) fn default_tenant_user_bindings() -> HashMap<String, TenantUserBinding> {
    HashMap::from([
        (
            "analyst-acme".to_string(),
            TenantUserBinding {
                tenant_id: "acme".to_string(),
                role: "tenant_analyst".to_string(),
            },
        ),
        (
            "admin-acme".to_string(),
            TenantUserBinding {
                tenant_id: "acme".to_string(),
                role: "tenant_admin".to_string(),
            },
        ),
    ])
}

pub(crate) fn load_operator_role_bindings(
    allowed_roles: &HashSet<OperatorRole>,
) -> HashMap<String, OperatorRole> {
    let parsed = env::var("VNG_OPERATOR_ROLE_BINDINGS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|entry| {
                    let (operator_id, role) = entry.split_once(':')?;
                    let operator_id = operator_id.trim();
                    let role = OperatorRole::parse(role.trim())?;
                    if operator_id.is_empty() || !allowed_roles.contains(&role) {
                        return None;
                    }
                    Some((operator_id.to_string(), role))
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    if parsed.is_empty() {
        default_operator_role_bindings()
            .into_iter()
            .filter(|(_, role)| allowed_roles.contains(role))
            .collect()
    } else {
        parsed
    }
}

pub(crate) fn load_runtime_security_config(allowed_operator_roles: &HashSet<OperatorRole>) -> SecurityConfigContract {
    let mut configured_roles = allowed_operator_roles
        .iter()
        .map(|role| role.as_str().to_string())
        .collect::<Vec<_>>();
    configured_roles.sort();

    let kms_failover_key_ref_envs = env::var("VNG_KMS_FAILOVER_KEY_REF_ENVS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| {
            vec![
                "VNG_KMS_KEY_URI_REGION_B".to_string(),
                "VNG_KMS_KEY_URI_REGION_C".to_string(),
            ]
        });

    let config = SecurityConfigContract {
        admin_api_key_env: env::var("VNG_ADMIN_API_KEY_ENV")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "VNG_ADMIN_API_KEY".to_string()),
        admin_header_name: env::var("VNG_ADMIN_HEADER_NAME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "x-vng-admin-key".to_string()),
        tls_required: false,
        mtls_required: false,
        encryption_at_rest_required: true,
        kms_key_ref_env: env::var("VNG_KMS_KEY_REF_ENV")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "VNG_KMS_KEY_URI".to_string()),
        kms_failover_key_ref_envs,
        allowed_operator_roles: configured_roles,
        token_ttl_seconds: env::var("VNG_TOKEN_TTL_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(300),
    };

    config
        .validate()
        .unwrap_or_else(|error| panic!("invalid runtime security config: {error}"));
    config
}

pub(crate) fn load_kms_runtime_state(config: &SecurityConfigContract) -> KmsRuntimeState {
    let mut provider_index = BTreeMap::<String, usize>::new();
    let mut providers = Vec::<ConfiguredKmsProviderAdapter>::new();
    for env_name in config.kms_key_candidates() {
        if let Ok(value) = env::var(&env_name) {
            if !value.trim().is_empty() {
                let candidate = ConfiguredKmsProviderAdapter::from_key_ref(value.trim());
                let provider_name = candidate.provider_name().to_string();
                let provider_slot = *provider_index.entry(provider_name.clone()).or_insert_with(|| {
                    providers.push(candidate);
                    providers.len() - 1
                });
                providers[provider_slot].register_key_ref(&env_name, value.trim());
            }
        }
    }

    KmsRuntimeState {
        providers,
        unavailable_envs: HashSet::new(),
        last_resolution: None,
        last_error: None,
        last_simulation_note: None,
    }
}

pub(crate) fn load_ingest_event_bus() -> ManagedEventBusTransport {
    let broker_mode = env::var("VNG_INGEST_OUTBOX_BROKER_MODE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "file_wal".to_string());
    let broker_target = env::var("VNG_INGEST_EXTERNAL_BROKER_TARGET")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let subject_prefix = env::var("VNG_INGEST_EXTERNAL_BROKER_SUBJECT_PREFIX")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let wal_path = env::var("VNG_INGEST_OUTBOX_WAL_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "state/ingest-outbox-runtime.wal".to_string());
    ManagedEventBusTransport::from_broker_mode_with_target(
        &broker_mode,
        &wal_path,
        broker_target.as_deref(),
        subject_prefix.as_deref(),
    )
    .unwrap_or_else(|error| {
        panic!(
            "failed to initialize ingest event bus broker {broker_mode} with state {wal_path}: {error}"
        )
    })
}

pub(crate) fn load_ingest_outbox_cursor_store() -> ManagedReplayCursorStore {
    let wal_path = env::var("VNG_INGEST_OUTBOX_CURSOR_WAL_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "state/ingest-outbox-cursors.wal".to_string());
    ManagedReplayCursorStore::wal_backed(&wal_path).unwrap_or_else(|error| {
        panic!("failed to initialize ingest outbox cursor store at {wal_path}: {error}")
    })
}
