// S11-001 — Scenario R-17: Driver config validation parity
//
// Verifies that all three driver configuration modes (HTTP-only, native-only,
// dual-transport) validate correctly and that an invalid config is rejected.
//
// Run: cargo test -p voltnuerongrid-driver-rust r17_driver_parity

use voltnuerongrid_driver_rust::{DriverConfig, DriverError};

#[test]
fn scenario_passes() {
    r17_http_only_config_valid();
    r17_native_only_config_valid();
    r17_dual_transport_config_valid();
    r17_empty_base_url_rejected();
    r17_empty_session_id_rejected();
    r17_http_fallback_requires_vng_scheme();
    r17_http_fallback_must_be_http_scheme();
}

/// HTTP-only config: base_url is http://, no fallback needed.
fn r17_http_only_config_valid() {
    let cfg = DriverConfig {
        base_url: "http://127.0.0.1:8080".to_string(),
        http_fallback_url: None,
        session_id: "sess-001".to_string(),
        tenant_id: None,
        user_id: None,
        admin_api_key: None,
        operator_id: None,
        route_hint: None,
    };
    assert!(
        cfg.validate().is_ok(),
        "HTTP-only config should validate successfully"
    );
}

/// Native-only config: base_url is vng://, no fallback required.
fn r17_native_only_config_valid() {
    let cfg = DriverConfig {
        base_url: "vng://127.0.0.1:9090".to_string(),
        http_fallback_url: None,
        session_id: "sess-002".to_string(),
        tenant_id: None,
        user_id: None,
        admin_api_key: None,
        operator_id: None,
        route_hint: None,
    };
    assert!(
        cfg.validate().is_ok(),
        "native-only config (vng:// without fallback) should validate successfully"
    );
}

/// Dual-transport config: vng:// base + http:// fallback.
fn r17_dual_transport_config_valid() {
    let cfg = DriverConfig {
        base_url: "vng://127.0.0.1:9090".to_string(),
        http_fallback_url: Some("http://127.0.0.1:8080".to_string()),
        session_id: "sess-003".to_string(),
        tenant_id: Some("tenant-a".to_string()),
        user_id: Some("user-1".to_string()),
        admin_api_key: None,
        operator_id: None,
        route_hint: None,
    };
    assert!(
        cfg.validate().is_ok(),
        "dual-transport config should validate successfully"
    );
}

/// Empty base_url must be rejected.
fn r17_empty_base_url_rejected() {
    let cfg = DriverConfig {
        base_url: "".to_string(),
        http_fallback_url: None,
        session_id: "sess-004".to_string(),
        tenant_id: None,
        user_id: None,
        admin_api_key: None,
        operator_id: None,
        route_hint: None,
    };
    assert!(
        cfg.validate().is_err(),
        "empty base_url must be rejected by validate()"
    );
}

/// Empty session_id must be rejected.
fn r17_empty_session_id_rejected() {
    let cfg = DriverConfig {
        base_url: "http://127.0.0.1:8080".to_string(),
        http_fallback_url: None,
        session_id: "   ".to_string(),
        tenant_id: None,
        user_id: None,
        admin_api_key: None,
        operator_id: None,
        route_hint: None,
    };
    assert!(
        cfg.validate().is_err(),
        "blank session_id must be rejected by validate()"
    );
}

/// http_fallback_url is only valid when base_url uses vng://.
fn r17_http_fallback_requires_vng_scheme() {
    let cfg = DriverConfig {
        base_url: "http://127.0.0.1:8080".to_string(),
        http_fallback_url: Some("http://127.0.0.1:8081".to_string()),
        session_id: "sess-005".to_string(),
        tenant_id: None,
        user_id: None,
        admin_api_key: None,
        operator_id: None,
        route_hint: None,
    };
    assert!(
        cfg.validate().is_err(),
        "http_fallback_url should only be valid when base_url uses vng://"
    );
}

/// http_fallback_url must start with http:// or https://.
fn r17_http_fallback_must_be_http_scheme() {
    let cfg = DriverConfig {
        base_url: "vng://127.0.0.1:9090".to_string(),
        http_fallback_url: Some("ftp://127.0.0.1:8080".to_string()),
        session_id: "sess-006".to_string(),
        tenant_id: None,
        user_id: None,
        admin_api_key: None,
        operator_id: None,
        route_hint: None,
    };
    assert!(
        cfg.validate().is_err(),
        "http_fallback_url with ftp:// scheme must be rejected"
    );
}
