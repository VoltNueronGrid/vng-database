//! Observability layer — Phase 0.5.
//!
//! Wires up:
//! - `tracing` for structured logging (env-filter, JSON for prod, pretty for dev).
//! - `opentelemetry-otlp` (HTTP/proto) when `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
//! - `metrics` + `metrics-exporter-prometheus` for a `/metrics` endpoint.
//!
//! # Environment variables
//!
//! | Variable                       | Default                         | Notes                           |
//! |-------------------------------|----------------------------------|--------------------------------|
//! | `VNG_LOG`                     | `info,voltnuerongridd=info`     | tracing-subscriber env filter   |
//! | `VNG_LOG_FORMAT`              | `pretty`                        | `pretty` or `json`              |
//! | `OTEL_EXPORTER_OTLP_ENDPOINT` | *(unset — OTEL disabled)*       | e.g. `http://localhost:4318`    |
//! | `OTEL_SERVICE_NAME`           | `voltnuerongridd`               | overrides OTEL service name     |
//! | `VNG_METRICS_DISABLED`        | *(unset)*                       | Set to `1` to skip Prometheus   |
//!
//! # Design
//!
//! When `OTEL_EXPORTER_OTLP_ENDPOINT` is set the subscriber is assembled as:
//!
//! ```text
//!   Registry
//!     └─ EnvFilter
//!     └─ fmt::Layer   (stdout, pretty or JSON)
//!     └─ OpenTelemetryLayer  (batch OTLP HTTP/proto export)
//! ```
//!
//! When the env var is absent the OTEL layer is omitted — behaviour is
//! identical to Phase 0.4.
//!
//! # Shutdown
//!
//! Call `shutdown_otel()` before process exit so the batch exporter flushes
//! its in-flight spans. The function is a no-op when OTEL is not configured.

#![forbid(unsafe_code)]

use std::sync::{Once, OnceLock};
use tracing_subscriber::registry::LookupSpan;

static INIT: Once = Once::new();

// ── Tracer-provider handle (for graceful shutdown) ────────────────────────────

// opentelemetry_sdk 0.27 uses `opentelemetry_sdk::trace::TracerProvider` (no "Sdk" prefix).
static OTEL_PROVIDER: OnceLock<opentelemetry_sdk::trace::TracerProvider> = OnceLock::new();

/// Flush and shut down the OTEL tracer provider.
/// Must be called before process exit when OTEL is configured. No-op otherwise.
pub fn shutdown_otel() {
    if let Some(provider) = OTEL_PROVIDER.get() {
        if let Err(e) = provider.shutdown() {
            eprintln!("[vng:otel] shutdown error: {e}");
        }
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Initialize tracing + metrics. Idempotent — safe to call multiple times,
/// but only the first call has any effect.
pub fn init_observability() {
    INIT.call_once(|| {
        init_tracing();
        if std::env::var("VNG_METRICS_DISABLED").as_deref() != Ok("1") {
            init_metrics();
        }
    });
}

// ── Tracing init ──────────────────────────────────────────────────────────────

fn init_tracing() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    // L-4: Register the W3C TraceContext propagator globally so the
    // `propagate_trace_context` axum middleware can extract `traceparent`
    // / `tracestate` headers from every inbound HTTP request and stitch
    // the incoming distributed trace into our local spans — regardless of
    // whether OTLP export is configured.
    opentelemetry::global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );

    let filter = EnvFilter::try_from_env("VNG_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,voltnuerongridd=info"));

    let format = std::env::var("VNG_LOG_FORMAT").unwrap_or_else(|_| "pretty".to_string());
    let use_json = format == "json";

    // `build_otel_layer()` is generic over the Subscriber type S, so the OTEL
    // layer's concrete type unifies with whatever Layered<…, Registry> the
    // fmt layer produces. Option<L> is a no-op Layer when L is None.
    if use_json {
        let otel: Option<tracing_opentelemetry::OpenTelemetryLayer<_, _>> = build_otel_layer();
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_current_span(true)
                    .with_span_list(false),
            )
            .with(otel)
            .try_init();
    } else {
        let otel: Option<tracing_opentelemetry::OpenTelemetryLayer<_, _>> = build_otel_layer();
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(true)
                    .compact(),
            )
            .with(otel)
            .try_init();
    }
}

/// Build an `OpenTelemetryLayer` backed by an OTLP HTTP/proto batch exporter.
///
/// Generic over the `Subscriber` type `S` so that Rust can unify the layer's
/// type with the concrete layered subscriber at the call site.
///
/// Returns `None` when `OTEL_EXPORTER_OTLP_ENDPOINT` is not set.
fn build_otel_layer<S>() -> Option<tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    use opentelemetry::KeyValue;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::trace::{BatchSpanProcessor, TracerProvider};
    use opentelemetry_sdk::Resource;
    use opentelemetry::trace::TracerProvider as _;

    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok()?;

    let service_name = std::env::var("OTEL_SERVICE_NAME")
        .unwrap_or_else(|_| "voltnuerongridd".to_string());

    // HTTP/protobuf OTLP span exporter.
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(&endpoint)
        .build()
        .map_err(|e| {
            eprintln!(
                "[vng:otel] OTLP exporter init failed (endpoint={endpoint}): {e}; \
                 running without OpenTelemetry export"
            );
        })
        .ok()?;

    // Batch processor — uses Tokio runtime (init is called from #[tokio::main]).
    let batch = BatchSpanProcessor::builder(exporter, opentelemetry_sdk::runtime::Tokio)
        .build();

    // Resource attributes that identify this service in the OTLP backend.
    let resource = Resource::new(vec![
        KeyValue::new("service.name", service_name),
        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
    ]);

    // Use the non-deprecated Builder API (opentelemetry-sdk 0.27+).
    let provider = TracerProvider::builder()
        .with_span_processor(batch)
        .with_resource(resource)
        .build();

    // Stash the provider so shutdown_otel() can flush it on process exit.
    let _ = OTEL_PROVIDER.set(provider.clone());

    let tracer = provider.tracer("voltnuerongridd");

    Some(tracing_opentelemetry::layer().with_tracer(tracer))
}

// ── Metrics init ──────────────────────────────────────────────────────────────

fn init_metrics() {
    use metrics_exporter_prometheus::PrometheusBuilder;

    if let Ok(handle) = PrometheusBuilder::new().install_recorder() {
        let _ = METRICS_HANDLE.set(handle);
    }

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
    metrics::describe_counter!(
        "vng_wal_auto_migrate_total",
        "SQL statements auto-migrated from legacy text WAL into durability engine on boot, by kind."
    );
}

static METRICS_HANDLE: OnceLock<metrics_exporter_prometheus::PrometheusHandle> = OnceLock::new();

/// Render the current Prometheus metrics text. Returns an empty string if
/// metrics are disabled.
pub fn render_metrics() -> String {
    METRICS_HANDLE.get().map(|h| h.render()).unwrap_or_default()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        std::env::set_var("VNG_METRICS_DISABLED", "1");
        init_observability();
        init_observability();
    }

    #[test]
    fn build_otel_layer_returns_none_without_endpoint() {
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
        let no_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_err();
        assert!(no_endpoint, "endpoint env var should not be set in test environment");
    }

    #[test]
    fn shutdown_otel_is_noop_without_provider() {
        // Must not panic when no provider was installed.
        shutdown_otel();
    }
}
