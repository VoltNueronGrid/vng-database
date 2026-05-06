//! Observability scaffold — Phase 0.4.
//!
//! Wires up:
//! - `tracing` for structured logging (env-filter, JSON for prod, pretty for dev).
//! - `metrics` + `metrics-exporter-prometheus` for a `/metrics` endpoint.
//!
//! # Environment variables
//!
//! - `VNG_LOG`  — `tracing-subscriber` env filter (default: `info,voltnuerongridd=info`).
//! - `VNG_LOG_FORMAT` — `pretty` (default) or `json`.
//! - `VNG_METRICS_DISABLED` — set to `1` to skip the Prometheus recorder
//!   (used in tests where the recorder leaks across the process).
//!
//! # Why these crates?
//!
//! - `tracing` is the de-facto standard for async Rust observability (used by
//!   tokio, axum, sqlx). Spans propagate across `.await`, which the older `log`
//!   crate cannot.
//! - `metrics` is a façade crate (similar to `log` for tracing). Code emits
//!   `metrics::counter!("foo").increment(1)` without knowing the backend.
//!   `metrics-exporter-prometheus` plugs in a Prometheus-format recorder.
//!   Swapping for OTLP later is a one-line change in `init_observability`.

#![forbid(unsafe_code)]

use std::sync::Once;

static INIT: Once = Once::new();

/// Initialize tracing + metrics. Idempotent — safe to call multiple times,
/// but only the first call has any effect.
///
/// Metrics initialization can be disabled via `VNG_METRICS_DISABLED=1`. This
/// is used in unit tests where the global Prometheus recorder would otherwise
/// be installed permanently on the first test and conflict with subsequent
/// tests in the same binary.
pub fn init_observability() {
    INIT.call_once(|| {
        init_tracing();
        if std::env::var("VNG_METRICS_DISABLED").as_deref() != Ok("1") {
            init_metrics();
        }
    });
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_env("VNG_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,voltnuerongridd=info"));

    let format = std::env::var("VNG_LOG_FORMAT").unwrap_or_else(|_| "pretty".to_string());

    if format == "json" {
        let _ = fmt()
            .with_env_filter(filter)
            .json()
            .with_current_span(true)
            .with_span_list(false)
            .try_init();
    } else {
        let _ = fmt()
            .with_env_filter(filter)
            .with_target(true)
            .compact()
            .try_init();
    }
}

fn init_metrics() {
    use metrics_exporter_prometheus::PrometheusBuilder;

    // Install the recorder. We do NOT install the listener here — instead,
    // we expose `/metrics` from inside the main axum router so it shares
    // TLS, CORS, and middleware with the rest of the API. The recorder
    // returned by `install_recorder()` provides `render()` for that purpose.
    if let Ok(handle) = PrometheusBuilder::new().install_recorder() {
        // Stash the handle in a one-shot OnceLock so the `/metrics` handler
        // can call `.render()` on each scrape.
        let _ = METRICS_HANDLE.set(handle);
    }

    // Pre-register some common counters so they appear in `/metrics` output
    // even before the first event. Helps Prometheus auto-discovery.
    metrics::describe_counter!(
        "vng_http_requests_total",
        "Total number of HTTP requests received, labeled by route and status."
    );
    metrics::describe_histogram!(
        "vng_http_request_duration_seconds",
        "End-to-end HTTP request duration in seconds, labeled by route and method."
    );
    metrics::describe_counter!(
        "vng_sql_execute_total",
        "Total number of SQL execute calls, labeled by route_path and status."
    );
    metrics::describe_counter!(
        "vng_handler_errors_total",
        "Total number of internal handler errors, labeled by kind."
    );
    metrics::describe_histogram!(
        "vng_sql_execute_duration_ms",
        "Wall-clock duration of SQL execute calls, in milliseconds."
    );
    metrics::describe_counter!(
        "vng_database_lifecycle_total",
        "Total CREATE/DROP DATABASE operations by status."
    );
    metrics::describe_counter!(
        "vng_durability_engine_boot",
        "Increments once at process boot, labeled by chosen durability engine kind."
    );
    metrics::describe_counter!(
        "vng_wal_replay_total",
        "SQL statements replayed at boot, by kind (ddl|dml) and source (engine|text_wal)."
    );
    metrics::describe_counter!(
        "vng_wal_append_total",
        "SQL statements appended to durable WAL, by kind (ddl|dml)."
    );
}

static METRICS_HANDLE: std::sync::OnceLock<metrics_exporter_prometheus::PrometheusHandle> =
    std::sync::OnceLock::new();

/// Render the current Prometheus metrics text. Returns an empty string if
/// metrics are disabled.
pub fn render_metrics() -> String {
    METRICS_HANDLE.get().map(|h| h.render()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        // Should not panic even if called twice in the same test binary.
        std::env::set_var("VNG_METRICS_DISABLED", "1");
        init_observability();
        init_observability();
    }
}
