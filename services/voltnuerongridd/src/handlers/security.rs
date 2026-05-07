use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use voltnuerongrid_audit::AuditEventKind;
use voltnuerongrid_auth::{KmsKeyProvider, KmsKeyResolution, KmsProviderChain, PrivilegeAction};
use voltnuerongrid_plugins::{
    AttestationType, ConnectorPackageMetadata, PluginManifestSignature, ProvenanceAttestation,
    ProvenanceChain, SbomEntry, SbomInspectionResult, SignedPluginManifest,
};
use crate::{AppState, AuthErrorResponse, RuntimeAccessPrincipal};
use crate::audit_helpers::{append_audit_event, append_runtime_audit_event};
use crate::auth::{require_operator_auth, require_operator_privilege};

// ─── S6-WS5-04: TDE override status response ─────────────────────────────────

#[derive(Serialize)]
pub(crate) struct TdeOverrideStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) override_set: bool,
    pub(crate) override_value: Option<bool>,
    pub(crate) effective_tde_active: bool,
}

// S6-WS5-03: TLS runtime status
#[derive(Serialize)]
pub(crate) struct SecurityTlsStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) tls_required: bool,
    pub(crate) mtls_required: bool,
    pub(crate) cert_source: String,
    pub(crate) key_source: String,
    pub(crate) cert_present: bool,
    pub(crate) key_present: bool,
    pub(crate) cert_pair_configured: bool,
    pub(crate) cert_rotation_supported: bool,
    pub(crate) note: &'static str,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct TlsCertRotateRequest {
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TlsCertRotateResponse {
    pub(crate) status: &'static str,
    pub(crate) rotation_initiated: bool,
    pub(crate) cert_source: String,
    pub(crate) key_source: String,
    pub(crate) cert_present: bool,
    pub(crate) key_present: bool,
    pub(crate) preflight_ok: bool,
    pub(crate) reason: String,
}

// ─── S6-WS5-03: TLS certificate info struct ─────────────────────────────────

#[derive(Serialize)]
pub(crate) struct TlsCertInfoResponse {
    pub(crate) status: &'static str,
    pub(crate) cert_source: String,
    pub(crate) key_source: String,
    pub(crate) cert_present: bool,
    pub(crate) key_present: bool,
    pub(crate) preflight_ok: bool,
    pub(crate) tls_required: bool,
    pub(crate) mtls_required: bool,
    pub(crate) cert_rotation_supported: bool,
}

// S6-WS5-04: TDE runtime status
#[derive(Serialize)]
pub(crate) struct SecurityTdeStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) encryption_at_rest_required: bool,
    pub(crate) tde_active: bool,
    pub(crate) key_env_var: String,
    pub(crate) key_resolved: bool,
    pub(crate) note: &'static str,
}

// ─── S6-WS5-04: TDE toggle structs ────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct TdeToggleRequest {
    pub(crate) enable: bool,
}

#[derive(Serialize)]
pub(crate) struct TdeToggleResponse {
    pub(crate) status: &'static str,
    pub(crate) tde_active: bool,
    pub(crate) override_applied: bool,
}

#[derive(Serialize)]
pub(crate) struct SecurityKmsStatusResponse {
    pub(crate) status: &'static str,
    pub(crate) resolution_state: &'static str,
    pub(crate) encryption_at_rest_required: bool,
    pub(crate) configured_envs: Vec<String>,
    pub(crate) unavailable_envs: Vec<String>,
    pub(crate) selected_env: Option<String>,
    pub(crate) key_ref: Option<String>,
    pub(crate) failover_used: bool,
    pub(crate) last_simulation_note: Option<String>,
    pub(crate) last_error: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct SecurityKmsOutageSimulateRequest {
    pub(crate) unavailable_envs: Vec<String>,
    pub(crate) note: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct SecurityKmsOutageReconcileRequest {
    pub(crate) note: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SecurityKmsOutageResponse {
    pub(crate) status: &'static str,
    pub(crate) resolution_state: &'static str,
    pub(crate) unavailable_envs: Vec<String>,
    pub(crate) selected_env: Option<String>,
    pub(crate) key_ref: Option<String>,
    pub(crate) failover_used: bool,
    pub(crate) note: String,
}

#[derive(Deserialize)]
pub(crate) struct SignedProvenanceRegistrationRequest {
    pub(crate) plugin_id: String,
    pub(crate) plugin_version: String,
    pub(crate) checksum_sha256: String,
    pub(crate) display_name: Option<String>,
    pub(crate) owner: Option<String>,
    pub(crate) license: Option<String>,
    pub(crate) capabilities: Option<Vec<String>>,
    pub(crate) schema_version: Option<String>,
    pub(crate) signature_algorithm: String,
    pub(crate) signature_key_id: String,
    pub(crate) signature_base64: String,
    pub(crate) revoked_key_ids: Option<Vec<String>>,
    pub(crate) attestations: Vec<SignedProvenanceAttestationRequest>,
    pub(crate) sbom_entries: Option<Vec<SignedProvenanceSbomEntryRequest>>,
}

#[derive(Deserialize)]
pub(crate) struct SignedProvenanceAttestationRequest {
    pub(crate) attester_id: String,
    pub(crate) attested_at_ms: Option<u64>,
    pub(crate) attestation_type: String,
    pub(crate) payload_digest_sha256: String,
    pub(crate) signature_base64: String,
    pub(crate) passed: bool,
}

#[derive(Clone, Deserialize)]
pub(crate) struct SignedProvenanceSbomEntryRequest {
    pub(crate) component_name: String,
    pub(crate) component_version: String,
    pub(crate) license: String,
    pub(crate) checksum_sha256: String,
    pub(crate) source_url: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SignedProvenanceRegistrationResponse {
    pub(crate) status: &'static str,
    pub(crate) registration_state: &'static str,
    pub(crate) plugin_id: String,
    pub(crate) plugin_version: String,
    pub(crate) chain_complete: bool,
    pub(crate) chain_digest: String,
    pub(crate) attestation_count: usize,
    pub(crate) passed_attestations: usize,
    pub(crate) sbom_approved: bool,
    pub(crate) sbom_license_violations: usize,
    pub(crate) sbom_missing_checksums: usize,
    pub(crate) audit_records_total: usize,
    pub(crate) error: Option<String>,
}

// ─── Private KMS evaluation snapshot (internal to security handlers) ─────────

struct KmsEvaluationSnapshot {
    status: &'static str,
    resolution_state: &'static str,
    resolution: Option<KmsKeyResolution>,
    unavailable_envs: Vec<String>,
    last_simulation_note: Option<String>,
    last_error: Option<String>,
}

// ─── Security handlers ────────────────────────────────────────────────────────

pub(crate) async fn security_plugins_provenance_register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SignedProvenanceRegistrationRequest>,
) -> Result<Json<SignedProvenanceRegistrationResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "security.supply_chain",
        "security/plugins/provenance/register",
        PrivilegeAction::Manage,
    )?;

    let mut chain = ProvenanceChain::new(req.plugin_id.clone(), req.plugin_version.clone());
    for attestation in &req.attestations {
        let Some(attestation_type) = parse_attestation_type(attestation.attestation_type.as_str()) else {
            return Ok(Json(SignedProvenanceRegistrationResponse {
                status: "error",
                registration_state: "rejected",
                plugin_id: req.plugin_id,
                plugin_version: req.plugin_version,
                chain_complete: false,
                chain_digest: String::new(),
                attestation_count: req.attestations.len(),
                passed_attestations: req.attestations.iter().filter(|entry| entry.passed).count(),
                sbom_approved: false,
                sbom_license_violations: 0,
                sbom_missing_checksums: 0,
                audit_records_total: state
                    .plugin_lifecycle
                    .lock()
                    .map(|manager| manager.audit_trail().len())
                    .unwrap_or(0),
                error: Some("unsupported_attestation_type".to_string()),
            }));
        };

        chain.add_attestation(ProvenanceAttestation {
            attester_id: attestation.attester_id.clone(),
            attested_at_ms: attestation.attested_at_ms.unwrap_or_else(crate::now_unix_ms_u64),
            attestation_type,
            payload_digest_sha256: attestation.payload_digest_sha256.clone(),
            signature_base64: attestation.signature_base64.clone(),
            passed: attestation.passed,
        });
    }

    let sbom_entries = req
        .sbom_entries
        .unwrap_or_default()
        .into_iter()
        .map(|entry| SbomEntry {
            component_name: entry.component_name,
            component_version: entry.component_version,
            license: entry.license,
            checksum_sha256: entry.checksum_sha256,
            source_url: entry.source_url,
        })
        .collect::<Vec<_>>();
    let sbom_result = SbomInspectionResult::inspect(
        req.plugin_id.clone(),
        sbom_entries,
        &["GPL-3.0-only", "AGPL-3.0-only"],
    );

    if !chain.is_complete() || !sbom_result.approved {
        append_audit_event(
            &state,
            AuditEventKind::Security,
            &operator.operator_id,
            "security_plugins_provenance_register",
            "rejected",
            &json!({
                "plugin_id": req.plugin_id,
                "plugin_version": req.plugin_version,
                "chain_complete": chain.is_complete(),
                "sbom_approved": sbom_result.approved,
            })
            .to_string(),
        );
        return Ok(Json(SignedProvenanceRegistrationResponse {
            status: "error",
            registration_state: "rejected",
            plugin_id: req.plugin_id,
            plugin_version: req.plugin_version,
            chain_complete: chain.is_complete(),
            chain_digest: chain.chain_digest,
            attestation_count: chain.attestations.len(),
            passed_attestations: chain.attestations.iter().filter(|entry| entry.passed).count(),
            sbom_approved: sbom_result.approved,
            sbom_license_violations: sbom_result.license_violations.len(),
            sbom_missing_checksums: sbom_result.missing_checksums.len(),
            audit_records_total: state
                .plugin_lifecycle
                .lock()
                .map(|manager| manager.audit_trail().len())
                .unwrap_or(0),
            error: Some("provenance_or_sbom_policy_violation".to_string()),
        }));
    }

    let manifest = SignedPluginManifest {
        schema_version: req.schema_version.unwrap_or_else(|| "v1".to_string()),
        declared_checksum_sha256: req.checksum_sha256.clone(),
        generated_epoch_ms: crate::now_unix_ms(),
        signature: PluginManifestSignature {
            algorithm: req.signature_algorithm,
            key_id: req.signature_key_id,
            signature_base64: req.signature_base64,
        },
        revoked_key_ids: req.revoked_key_ids.unwrap_or_default(),
    };
    let metadata = ConnectorPackageMetadata {
        plugin_id: req.plugin_id.clone(),
        version: req.plugin_version.clone(),
        display_name: req
            .display_name
            .unwrap_or_else(|| req.plugin_id.clone()),
        owner: req.owner.unwrap_or_else(|| "platform-security".to_string()),
        license: req.license.unwrap_or_else(|| "Apache-2.0".to_string()),
        checksum_sha256: req.checksum_sha256,
        capabilities: req
            .capabilities
            .filter(|capabilities| !capabilities.is_empty())
            .unwrap_or_else(|| vec!["ingest.read".to_string()]),
    };

    let register_result = state
        .plugin_lifecycle
        .lock()
        .expect("plugin lifecycle lock")
        .register(
            manifest,
            metadata,
            Some(operator.operator_id.clone()),
            Some(chain.clone()),
            crate::now_unix_ms_u64(),
        );

    let (status, registration_state, error) = match register_result {
        Ok(_) => ("ok", "registered", None),
        Err(error) => ("error", "rejected", Some(error.to_string())),
    };

    let audit_records_total = state
        .plugin_lifecycle
        .lock()
        .map(|manager| manager.audit_trail().len())
        .unwrap_or(0);

    append_audit_event(
        &state,
        AuditEventKind::Security,
        &operator.operator_id,
        "security_plugins_provenance_register",
        status,
        &json!({
            "plugin_id": req.plugin_id,
            "plugin_version": req.plugin_version,
            "chain_complete": chain.is_complete(),
            "chain_digest": chain.chain_digest,
            "registration_state": registration_state,
            "error": error,
        })
        .to_string(),
    );

    Ok(Json(SignedProvenanceRegistrationResponse {
        status,
        registration_state,
        plugin_id: req.plugin_id,
        plugin_version: req.plugin_version,
        chain_complete: chain.is_complete(),
        chain_digest: chain.chain_digest,
        attestation_count: chain.attestations.len(),
        passed_attestations: chain.attestations.iter().filter(|entry| entry.passed).count(),
        sbom_approved: sbom_result.approved,
        sbom_license_violations: sbom_result.license_violations.len(),
        sbom_missing_checksums: sbom_result.missing_checksums.len(),
        audit_records_total,
        error,
    }))
}

/// S6-WS5-03: Initiate a TLS cert rotation (scaffold — records attempt, does not hot-swap certs).
pub(crate) async fn security_tls_rotate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<TlsCertRotateRequest>,
) -> Result<(StatusCode, Json<TlsCertRotateResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/tls/rotate",
        PrivilegeAction::Manage,
    )?;
    let cert_source = std::env::var("VNG_TLS_CERT_PATH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "not_configured".to_string());
    let key_source = std::env::var("VNG_TLS_KEY_PATH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "not_configured".to_string());
    let cert_present = cert_source != "not_configured" && std::path::Path::new(&cert_source).exists();
    let key_present = key_source != "not_configured" && std::path::Path::new(&key_source).exists();
    let preflight_ok = cert_present && key_present;
    let rotation_initiated = preflight_ok;
    let reason = req.reason.unwrap_or_else(|| "manual_rotation".to_string());
    Ok((StatusCode::OK, Json(TlsCertRotateResponse {
        status: "ok",
        rotation_initiated,
        cert_source,
        key_source,
        cert_present,
        key_present,
        preflight_ok,
        reason,
    })))
}

/// S6-WS5-03: Return TLS certificate configuration details.
pub(crate) async fn security_tls_cert_info(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<TlsCertInfoResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let cert_source = std::env::var("VNG_TLS_CERT_PATH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "not_configured".to_string());
    let key_source = std::env::var("VNG_TLS_KEY_PATH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "not_configured".to_string());
    let cert_present = cert_source != "not_configured" && std::path::Path::new(&cert_source).exists();
    let key_present = key_source != "not_configured" && std::path::Path::new(&key_source).exists();
    let preflight_ok = cert_present && key_present;
    let sc = &*state.security_config;
    Ok((StatusCode::OK, Json(TlsCertInfoResponse {
        status: "ok",
        cert_source,
        key_source,
        cert_present,
        key_present,
        preflight_ok,
        tls_required: sc.tls_required,
        mtls_required: sc.mtls_required,
        cert_rotation_supported: false,
    })))
}

/// S6-WS5-04: Override the TDE (Transparent Data Encryption) active state at runtime.
pub(crate) async fn security_tde_toggle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<TdeToggleRequest>,
) -> Result<(StatusCode, Json<TdeToggleResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/tde/toggle",
        PrivilegeAction::Manage,
    )?;
    *state.tde_override.lock().expect("tde_override lock") = Some(req.enable);
    Ok((
        StatusCode::OK,
        Json(TdeToggleResponse {
            status: "ok",
            tde_active: req.enable,
            override_applied: true,
        }),
    ))
}

/// S6-WS5-03: TLS runtime status — reports TLS/mTLS contract state from SecurityConfigContract.
pub(crate) async fn security_tls_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SecurityTlsStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/tls/status",
        PrivilegeAction::Read,
    )?;
    let principal = RuntimeAccessPrincipal::Operator(operator);
    let cert_source = std::env::var("VNG_TLS_CERT_PATH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "not_configured".to_string());
    let key_source = std::env::var("VNG_TLS_KEY_PATH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "not_configured".to_string());
    let cert_present = cert_source != "not_configured" && std::path::Path::new(&cert_source).exists();
    let key_present = key_source != "not_configured" && std::path::Path::new(&key_source).exists();
    let cert_pair_configured = cert_present && key_present;
    let note = if state.security_config.tls_required {
        "TLS required — server must be started with rustls/native-tls adapter"
    } else {
        "TLS not required — plaintext mode (development only)"
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "security_tls_status",
        "ok",
        json!({
            "route_scope": "security/tls/status",
            "tls_required": state.security_config.tls_required,
            "mtls_required": state.security_config.mtls_required,
        }),
    );
    Ok(Json(SecurityTlsStatusResponse {
        status: "ok",
        tls_required: state.security_config.tls_required,
        mtls_required: state.security_config.mtls_required,
        cert_source,
        key_source,
        cert_present,
        key_present,
        cert_pair_configured,
        cert_rotation_supported: false,
        note,
    }))
}

/// S6-WS5-04: TDE runtime status — reports encryption-at-rest state from SecurityConfigContract.
pub(crate) async fn security_tde_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SecurityTdeStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/tde/status",
        PrivilegeAction::Read,
    )?;
    let principal = RuntimeAccessPrincipal::Operator(operator);
    let key_env_var = state.security_config.kms_key_ref_env.clone();
    let key_resolved = std::env::var(&key_env_var)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_some();
    let tde_active = state.security_config.encryption_at_rest_required && key_resolved;
    let note = if tde_active {
        "TDE active: encryption-at-rest required and KMS key resolved"
    } else if state.security_config.encryption_at_rest_required {
        "TDE contract requires encryption but KMS key env var is not set — data NOT encrypted at rest"
    } else {
        "TDE not required in current security contract"
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "security_tde_status",
        "ok",
        json!({
            "route_scope": "security/tde/status",
            "encryption_at_rest_required": state.security_config.encryption_at_rest_required,
            "tde_active": tde_active,
            "key_env_var": key_env_var,
        }),
    );
    Ok(Json(SecurityTdeStatusResponse {
        status: "ok",
        encryption_at_rest_required: state.security_config.encryption_at_rest_required,
        tde_active,
        key_env_var,
        key_resolved,
        note,
    }))
}

/// S6-WS5-04: Return the current TDE runtime override state.
pub(crate) async fn security_tde_override_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<TdeOverrideStatusResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/tde/status",
        PrivilegeAction::Read,
    )?;
    let override_value = *state.tde_override.lock().expect("tde_override lock override");
    let override_set = override_value.is_some();
    let effective_tde_active = override_value
        .unwrap_or(state.security_config.encryption_at_rest_required);
    Ok((StatusCode::OK, Json(TdeOverrideStatusResponse {
        status: "ok",
        override_set,
        override_value,
        effective_tde_active,
    })))
}

pub(crate) async fn security_kms_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SecurityKmsStatusResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/kms",
        PrivilegeAction::Read,
    )?;
    let principal = RuntimeAccessPrincipal::Operator(operator.clone());
    let snapshot = evaluate_kms_runtime(&state);
    let response = build_security_kms_status_response(&state, &snapshot);
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "security_kms_status",
        response.status,
        json!({
            "route_scope": "security/kms",
            "resolution_state": response.resolution_state,
            "selected_env": response.selected_env,
            "failover_used": response.failover_used,
            "unavailable_envs": response.unavailable_envs,
        }),
    );
    Ok(Json(response))
}

pub(crate) async fn security_kms_outage_simulate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SecurityKmsOutageSimulateRequest>,
) -> Result<Json<SecurityKmsOutageResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/kms/outage",
        PrivilegeAction::Manage,
    )?;

    let configured = state
        .security_config
        .kms_key_candidates()
        .into_iter()
        .map(|value| value.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let normalized = req
        .unavailable_envs
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .filter(|value| configured.contains(&value.to_ascii_lowercase()))
        .map(ToString::to_string)
        .collect::<HashSet<_>>();
    let note = req
        .note
        .clone()
        .unwrap_or_else(|| "manual_kms_region_outage_simulation".to_string());

    {
        let mut runtime = state.kms_runtime.lock().expect("kms runtime lock");
        runtime.unavailable_envs = normalized;
        runtime.last_simulation_note = Some(note.clone());
    }

    let principal = RuntimeAccessPrincipal::Operator(operator);
    let snapshot = evaluate_kms_runtime(&state);
    let response = SecurityKmsOutageResponse {
        status: snapshot.status,
        resolution_state: snapshot.resolution_state,
        unavailable_envs: snapshot.unavailable_envs.clone(),
        selected_env: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.selected_env.clone()),
        key_ref: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.key_ref.clone()),
        failover_used: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.failover_used)
            .unwrap_or(false),
        note: note.clone(),
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "security_kms_outage_simulate",
        response.status,
        json!({
            "route_scope": "security/kms/outage",
            "resolution_state": response.resolution_state,
            "selected_env": response.selected_env,
            "failover_used": response.failover_used,
            "unavailable_envs": response.unavailable_envs,
            "note": response.note,
        }),
    );
    Ok(Json(response))
}

pub(crate) async fn security_kms_outage_reconcile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SecurityKmsOutageReconcileRequest>,
) -> Result<Json<SecurityKmsOutageResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    require_operator_auth(&headers, &state)?;
    let operator = require_operator_privilege(
        &headers,
        &state,
        "security.kms",
        "security/kms/outage",
        PrivilegeAction::Manage,
    )?;
    let note = req
        .note
        .clone()
        .unwrap_or_else(|| "manual_kms_region_outage_reconcile".to_string());

    {
        let mut runtime = state.kms_runtime.lock().expect("kms runtime lock");
        runtime.unavailable_envs.clear();
        runtime.last_simulation_note = Some(note.clone());
    }

    let principal = RuntimeAccessPrincipal::Operator(operator);
    let snapshot = evaluate_kms_runtime(&state);
    let response = SecurityKmsOutageResponse {
        status: snapshot.status,
        resolution_state: snapshot.resolution_state,
        unavailable_envs: snapshot.unavailable_envs.clone(),
        selected_env: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.selected_env.clone()),
        key_ref: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.key_ref.clone()),
        failover_used: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.failover_used)
            .unwrap_or(false),
        note: note.clone(),
    };
    append_runtime_audit_event(
        &state,
        AuditEventKind::Security,
        &principal,
        "security_kms_outage_reconcile",
        response.status,
        json!({
            "route_scope": "security/kms/outage",
            "resolution_state": response.resolution_state,
            "selected_env": response.selected_env,
            "failover_used": response.failover_used,
            "note": response.note,
        }),
    );
    Ok(Json(response))
}

// ─── Private helpers ──────────────────────────────────────────────────────────

fn evaluate_kms_runtime(state: &AppState) -> KmsEvaluationSnapshot {
    let mut runtime = state.kms_runtime.lock().expect("kms runtime lock");
    let unavailable_envs = runtime.unavailable_envs.clone();
    for provider in &mut runtime.providers {
        provider.clear_unavailable();
        for env_name in &unavailable_envs {
            provider.mark_unavailable(env_name);
        }
    }

    let mut unavailable_envs = runtime.unavailable_envs.iter().cloned().collect::<Vec<_>>();
    unavailable_envs.sort();

    let providers = runtime
        .providers
        .iter()
        .map(|provider| provider as &dyn KmsKeyProvider)
        .collect::<Vec<_>>();
    let chain = KmsProviderChain::new(providers);

    match state.security_config.resolve_kms_key_ref_with_provider(&chain) {
        Ok(resolution) => {
            runtime.last_error = None;
            runtime.last_resolution = Some(resolution.clone());
            KmsEvaluationSnapshot {
                status: if resolution.failover_used { "degraded" } else { "ok" },
                resolution_state: if resolution.failover_used {
                    "failover_active"
                } else {
                    "primary_active"
                },
                resolution: Some(resolution),
                unavailable_envs,
                last_simulation_note: runtime.last_simulation_note.clone(),
                last_error: None,
            }
        }
        Err(error) => {
            runtime.last_resolution = None;
            runtime.last_error = Some(error.clone());
            KmsEvaluationSnapshot {
                status: "degraded",
                resolution_state: "unresolved",
                resolution: None,
                unavailable_envs,
                last_simulation_note: runtime.last_simulation_note.clone(),
                last_error: Some(error),
            }
        }
    }
}

fn build_security_kms_status_response(
    state: &AppState,
    snapshot: &KmsEvaluationSnapshot,
) -> SecurityKmsStatusResponse {
    SecurityKmsStatusResponse {
        status: snapshot.status,
        resolution_state: snapshot.resolution_state,
        encryption_at_rest_required: state.security_config.encryption_at_rest_required,
        configured_envs: state.security_config.kms_key_candidates(),
        unavailable_envs: snapshot.unavailable_envs.clone(),
        selected_env: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.selected_env.clone()),
        key_ref: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.key_ref.clone()),
        failover_used: snapshot
            .resolution
            .as_ref()
            .map(|resolution| resolution.failover_used)
            .unwrap_or(false),
        last_simulation_note: snapshot.last_simulation_note.clone(),
        last_error: snapshot.last_error.clone(),
    }
}

fn parse_attestation_type(value: &str) -> Option<AttestationType> {
    match value.trim().to_ascii_lowercase().as_str() {
        "build_verification" => Some(AttestationType::BuildVerification),
        "security_scan" => Some(AttestationType::SecurityScan),
        "checksum_verification" => Some(AttestationType::ChecksumVerification),
        "signature_verification" => Some(AttestationType::SignatureVerification),
        "review_approval" => Some(AttestationType::ReviewApproval),
        _ => None,
    }
}
