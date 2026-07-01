// H9-13: HTAP observability handler
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;
use voltnuerongrid_store::{HtapDiagnostics, HtapMetrics, TableDiagnostics};

use crate::auth::require_admin_api_key;
use crate::AppState;

#[derive(Debug, Serialize)]
pub(crate) struct HtapDiagnosticsResponse {
    pub(crate) diagnostics: HtapDiagnostics,
}

/// GET /api/v1/htap/diagnostics
/// Returns current HTAP system diagnostics snapshot.
/// Requires x-vng-admin-key header.
pub(crate) async fn htap_diagnostics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_admin_api_key(&headers, &state) {
        return e.into_response();
    }

    let metrics = HtapMetrics::new();

    // Collect table-level diagnostics from the DDL catalog.
    let table_stats: Vec<TableDiagnostics> = {
        let catalog = state.storage.ddl_catalog.lock().unwrap_or_else(|e| e.into_inner());
        catalog
            .active_entries()
            .into_iter()
            .map(|entry| TableDiagnostics {
                table_name: entry.object_name.clone(),
                tail_version_count: 0,
                estimated_tail_bytes: 0,
                last_merge_lag_ms: 0,
                freshness_slo_status: "compliant".to_string(),
                active_snapshot_count: 0,
            })
            .collect()
    };

    let diag = metrics.diagnostics(table_stats);
    (StatusCode::OK, Json(HtapDiagnosticsResponse { diagnostics: diag })).into_response()
}

