//! Cluster topology, leader rotation, transport mutation helpers.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::env;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::json;
use voltnuerongrid_audit::AuditEventKind;
use voltnuerongrid_driver_rust::PoolAcquireError;
use voltnuerongrid_store::htap_sync::{MutationOp, ReplicaReplayState};
use crate::{AppState, AuthErrorResponse, PoolStatsResponse};
use crate::{ClusterNodeRuntime, RuntimeAccessPrincipal, now_unix_ms_u64};
use crate::{FailoverHandoffGapResponse, FailoverHandoffReportResponse};
use crate::{append_runtime_audit_event, locale_from_headers};


pub(crate) fn pool_stats_response(stats: &voltnuerongrid_driver_rust::PoolStats) -> PoolStatsResponse {
    PoolStatsResponse {
        total_connections: stats.total_connections,
        idle_connections: stats.idle_connections,
        active_connections: stats.active_connections,
        failed_connections: stats.failed_connections,
        circuit_breaker_state: stats.circuit_breaker_state.clone(),
        storm_active: stats.storm_active,
        current_rps: stats.current_rps,
        total_acquired: stats.total_acquired,
        total_released: stats.total_released,
        total_rejected: stats.total_rejected,
        total_circuit_opens: stats.total_circuit_opens,
    }
}


pub(crate) fn pool_acquire_error_state(error: &PoolAcquireError) -> &'static str {
    match error {
        PoolAcquireError::PoolExhausted { .. } => "pool_exhausted",
        PoolAcquireError::CircuitOpen { .. } => "circuit_open",
        PoolAcquireError::StormRejection { .. } => "storm_rejected",
        PoolAcquireError::AcquireTimeout { .. } => "acquire_timeout",
    }
}



pub(crate) fn acquire_sql_data_plane_connection(
    state: &AppState,
    headers: &HeaderMap,
    principal: &RuntimeAccessPrincipal,
    route_scope: &str,
) -> Result<String, (StatusCode, Json<AuthErrorResponse>)> {
    let now_ms = now_unix_ms_u64();
    let acquire_result = state
        .driver_pool
        .lock()
        .expect("driver pool lock")
        .acquire(now_ms);
    match acquire_result {
        Ok(connection_id) => Ok(connection_id),
        Err(error) => {
            append_runtime_audit_event(
                state,
                AuditEventKind::Sql,
                principal,
                "sql_data_plane_pool_acquire",
                "rejected",
                json!({
                    "route_scope": route_scope,
                    "reason": error.to_string(),
                }),
            );
            let locale = locale_from_headers(headers);
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AuthErrorResponse {
                    status: "unavailable",
                    reason: "driver_pool_unavailable".to_string(),
                    locale: locale.as_str().to_string(),
                    localized_message: "Service temporarily unavailable".to_string(),
                }),
            ))
        }
    }
}


pub(crate) fn release_sql_data_plane_connection(state: &AppState, connection_id: &str) {
    let now_ms = now_unix_ms_u64();
    let _ = state
        .driver_pool
        .lock()
        .expect("driver pool lock")
        .release(connection_id, now_ms);
}


pub(crate) fn rotate_leader(
    leader_state: &Arc<Mutex<String>>,
    requested_leader: &str,
    fallback_leader: &str,
) -> (String, String) {
    let requested = requested_leader.trim();
    let next = if requested.is_empty() {
        fallback_leader.to_string()
    } else {
        requested.to_string()
    };

    match leader_state.lock() {
        Ok(mut guard) => {
            let previous = guard.clone();
            *guard = next.clone();
            (previous, next)
        }
        Err(_) => (fallback_leader.to_string(), fallback_leader.to_string()),
    }
}


pub(crate) fn record_transport_mutation(
    state: &AppState,
    source_node_id: &str,
    target_node_id: &str,
    transport: &str,
    table: &str,
    primary_key: &str,
    op: MutationOp,
    payload: serde_json::Value,
) -> Option<u64> {
    let Ok(mut transport_state) = state.replication_transport.lock() else {
        return None;
    };
    let encoded = serde_json::to_string(&payload).ok()?;
    Some(
        transport_state
            .publish(
                source_node_id,
                target_node_id,
                transport,
                table,
                primary_key,
                &encoded,
                op,
            )
            .mutation
            .sequence,
    )
}


pub(crate) fn build_failover_handoff_report(
    state: &AppState,
    source_node_id: &str,
    target_node_id: &str,
) -> FailoverHandoffReportResponse {
    let mut replicas = match state.replica_replay_states.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return FailoverHandoffReportResponse {
                handoff_state: "replica_state_lock_error",
                source_node_id: source_node_id.to_string(),
                target_node_id: target_node_id.to_string(),
                last_applied_sequence_before: 0,
                last_applied_sequence_after: 0,
                replay_batch_size: 0,
                applied_count: 0,
                gap_count: 0,
                gaps: Vec::new(),
            }
        }
    };

    let replica = replicas
        .entry(target_node_id.to_string())
        .or_insert_with(|| ReplicaReplayState::new(target_node_id));
    let last_applied_sequence_before = replica.last_applied_sequence;
    let batch = match state.replication_transport.lock() {
        Ok(transport) => transport.export_for_target_since(
            target_node_id,
            last_applied_sequence_before,
            64,
        ),
        Err(_) => {
            return FailoverHandoffReportResponse {
                handoff_state: "replication_transport_lock_error",
                source_node_id: source_node_id.to_string(),
                target_node_id: target_node_id.to_string(),
                last_applied_sequence_before,
                last_applied_sequence_after: last_applied_sequence_before,
                replay_batch_size: 0,
                applied_count: 0,
                gap_count: 0,
                gaps: Vec::new(),
            }
        }
    };
    let batch = if batch.is_empty() {
        match state.sync_origin.lock() {
            Ok(origin) => replica.build_failover_handoff_batch(&origin, 64),
            Err(_) => Vec::new(),
        }
    } else {
        batch
    };
    if batch.is_empty() {
        return FailoverHandoffReportResponse {
            handoff_state: "no_transport_updates",
            source_node_id: source_node_id.to_string(),
            target_node_id: target_node_id.to_string(),
            last_applied_sequence_before,
            last_applied_sequence_after: last_applied_sequence_before,
            replay_batch_size: 0,
            applied_count: 0,
            gap_count: 0,
            gaps: Vec::new(),
        };
    };
    let replay_batch_size = batch.len();
    let report = replica.apply_batch(&batch);

    FailoverHandoffReportResponse {
        handoff_state: if report.gaps.is_empty() { "handoff_applied" } else { "handoff_gap_detected" },
        source_node_id: source_node_id.to_string(),
        target_node_id: target_node_id.to_string(),
        last_applied_sequence_before,
        last_applied_sequence_after: report.last_applied_sequence,
        replay_batch_size,
        applied_count: report.applied_count,
        gap_count: report.gaps.len(),
        gaps: report
            .gaps
            .into_iter()
            .map(|gap| FailoverHandoffGapResponse {
                expected: gap.expected,
                actual: gap.actual,
            })
            .collect(),
    }
}


pub(crate) fn default_node_cpu_cores() -> u32 {
    std::thread::available_parallelism()
        .map(|value| value.get() as u32)
        .unwrap_or(4)
}


pub(crate) fn default_node_ram_mb() -> u64 {
    env::var("VNG_NODE_TOTAL_RAM_MB")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(16_384)
}


pub(crate) fn initial_cluster_nodes(node_id: &str) -> HashMap<String, ClusterNodeRuntime> {
    let mut nodes = HashMap::new();
    nodes.insert(
        node_id.to_string(),
        ClusterNodeRuntime {
            node_id: node_id.to_string(),
            role: "leader".to_string(),
            status: "active".to_string(),
            total_cpu_cores: env::var("VNG_NODE_TOTAL_CPU_CORES")
                .ok()
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or_else(default_node_cpu_cores),
            total_ram_mb: default_node_ram_mb(),
            draining: false,
            last_heartbeat_ms: now_unix_ms_u64(),
        },
    );
    nodes
}

