//! Cross-node data-plane HTTP handlers (Tasks-7 group C).
//!
//! These endpoints are the *receive* side of the distributed data plane and are
//! authenticated with the shared cluster token (`Authorization: Bearer
//! <VNG_CLUSTER_TOKEN>`), exactly like the Raft intra-cluster RPCs. The *send*
//! side (fan-out / scatter-gather) lives in the `fanout_*` / `gather_*` helpers
//! below and is invoked by the data-plane coordinator handlers.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::helpers::dataplane::{
    advance_htap_peer_cursor, apply_cache_replication, apply_event_replication,
    apply_htap_mutations_to_olap, cross_node_htap_lag_ms, htap_batch_for_peer, lookup_shard_config,
    merge_olap_partials, owning_node_index, shard_for_key, DistributedOlapResult,
    OlapSubtaskResult, ReplicatedEvent, ReplicatedMutation,
};
use crate::helpers::execution::execute_olap_query;
use crate::AppState;

/// Validate the shared cluster token on an intra-cluster request.
/// Accepts the admin key as an alternative so operators can drive these paths.
pub(crate) fn require_cluster_token(headers: &HeaderMap, state: &AppState) -> Result<(), StatusCode> {
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    if let Some(expected) = state.cluster.cluster_token.as_ref().as_ref() {
        if !expected.is_empty() && bearer.as_deref() == Some(expected.as_str()) {
            return Ok(());
        }
    }
    let admin_ok = state.auth.admin_api_key.as_ref().is_some_and(|expected| {
        headers
            .get("x-vng-admin-key")
            .and_then(|v| v.to_str().ok())
            .map(|k| k == expected.as_str())
            .unwrap_or(false)
    });
    if admin_ok {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

// ───────────────────────── C-4 · HTAP apply / lag ─────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ClusterHtapApplyRequest {
    pub(crate) mutations: Vec<ReplicatedMutation>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ClusterHtapApplyResponse {
    pub(crate) status: &'static str,
    pub(crate) applied_count: usize,
    pub(crate) last_applied_sequence: u64,
}

/// Receive a batch of committed mutations shipped from the leader and apply
/// them to the local OLAP replica (C-4 push transport, receive side).
pub(crate) async fn cluster_htap_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ClusterHtapApplyRequest>,
) -> Result<(StatusCode, Json<ClusterHtapApplyResponse>), StatusCode> {
    require_cluster_token(&headers, &state)?;
    let (applied, last_seq) = apply_htap_mutations_to_olap(&state, &req.mutations);
    Ok((
        StatusCode::OK,
        Json(ClusterHtapApplyResponse {
            status: "ok",
            applied_count: applied,
            last_applied_sequence: last_seq,
        }),
    ))
}

#[derive(Debug, Serialize)]
pub(crate) struct ClusterHtapLagResponse {
    pub(crate) status: &'static str,
    pub(crate) freshness_lag_ms: Option<u64>,
    pub(crate) peer_count: usize,
}

/// Report the cross-node HTAP freshness lag (C-4 metric).
pub(crate) async fn cluster_htap_lag(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ClusterHtapLagResponse>), StatusCode> {
    require_cluster_token(&headers, &state)?;
    Ok((
        StatusCode::OK,
        Json(ClusterHtapLagResponse {
            status: "ok",
            freshness_lag_ms: cross_node_htap_lag_ms(&state),
            peer_count: state.cluster.raft_peers.len(),
        }),
    ))
}

/// Ship all pending committed mutations to every peer's `cluster_htap_apply`
/// endpoint (C-4 push transport, send side). Best-effort: failed peers keep
/// their cursor and are retried on the next call. Returns the number of peers
/// successfully updated.
#[allow(dead_code)]
pub(crate) async fn fanout_htap_to_peers(state: &AppState, client: &reqwest::Client) -> usize {
    let peers = state.cluster.raft_peers.as_ref().clone();
    if peers.is_empty() {
        return 0;
    }
    let token = state.cluster.cluster_token.as_ref().clone();
    let mut updated = 0usize;
    for peer in &peers {
        let (batch, last_seq) = htap_batch_for_peer(state, peer, 1000);
        if batch.is_empty() {
            continue;
        }
        let url = format!("{peer}/api/v1/cluster/htap/apply");
        let mut builder = client.post(&url).json(&ClusterHtapApplyRequest { mutations: batch });
        if let Some(t) = &token {
            builder = builder.header("Authorization", format!("Bearer {t}"));
        }
        match builder.send().await {
            Ok(resp) if resp.status().is_success() => {
                advance_htap_peer_cursor(state, peer, last_seq);
                updated += 1;
            }
            _ => {}
        }
    }
    updated
}

// ───────────────────────── C-3 · cache replication ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ClusterCacheReplicateRequest {
    pub(crate) cmd: String,
    pub(crate) partition_id: String,
    pub(crate) key: String,
    #[serde(default)]
    pub(crate) value: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) ttl_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ClusterCacheReplicateResponse {
    pub(crate) status: &'static str,
    pub(crate) applied: bool,
}

/// Receive a replicated SET/DEL and apply it to the local cache (C-3 receive).
pub(crate) async fn cluster_cache_replicate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ClusterCacheReplicateRequest>,
) -> Result<(StatusCode, Json<ClusterCacheReplicateResponse>), StatusCode> {
    require_cluster_token(&headers, &state)?;
    let applied = apply_cache_replication(
        &state,
        &req.cmd,
        &req.partition_id,
        &req.key,
        req.value,
        req.ttl_ms,
    );
    Ok((
        StatusCode::OK,
        Json(ClusterCacheReplicateResponse {
            status: "ok",
            applied,
        }),
    ))
}

/// Fan a SET/DEL out to every peer so their caches stay coherent (C-3 send).
#[allow(dead_code)]
pub(crate) async fn fanout_cache_command(
    state: &AppState,
    client: &reqwest::Client,
    req: &ClusterCacheReplicateRequest,
) -> usize {
    let peers = state.cluster.raft_peers.as_ref().clone();
    if peers.is_empty() {
        return 0;
    }
    let token = state.cluster.cluster_token.as_ref().clone();
    let mut ok = 0usize;
    for peer in &peers {
        let url = format!("{peer}/api/v1/cluster/cache/replicate");
        let mut builder = client.post(&url).json(req);
        if let Some(t) = &token {
            builder = builder.header("Authorization", format!("Bearer {t}"));
        }
        if let Ok(resp) = builder.send().await {
            if resp.status().is_success() {
                ok += 1;
            }
        }
    }
    ok
}

// ───────────────────────── C-5 · event bus replication ─────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ClusterEventReplicateRequest {
    pub(crate) events: Vec<ReplicatedEvent>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ClusterEventReplicateResponse {
    pub(crate) status: &'static str,
    pub(crate) applied_count: usize,
    pub(crate) last_sequence: u64,
}

/// Receive an ordered batch of events and apply them to the local event bus,
/// preserving the source node's total order (C-5 receive).
pub(crate) async fn cluster_event_replicate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ClusterEventReplicateRequest>,
) -> Result<(StatusCode, Json<ClusterEventReplicateResponse>), StatusCode> {
    require_cluster_token(&headers, &state)?;
    let (applied, last_seq) = apply_event_replication(&state, &req.events);
    Ok((
        StatusCode::OK,
        Json(ClusterEventReplicateResponse {
            status: "ok",
            applied_count: applied,
            last_sequence: last_seq,
        }),
    ))
}

/// Replicate a batch of events to every peer in order (C-5 send).
#[allow(dead_code)]
pub(crate) async fn fanout_events_to_peers(
    state: &AppState,
    client: &reqwest::Client,
    events: &[ReplicatedEvent],
) -> usize {
    let peers = state.cluster.raft_peers.as_ref().clone();
    if peers.is_empty() || events.is_empty() {
        return 0;
    }
    let token = state.cluster.cluster_token.as_ref().clone();
    let body = ClusterEventReplicateRequest {
        events: events.to_vec(),
    };
    let mut ok = 0usize;
    for peer in &peers {
        let url = format!("{peer}/api/v1/cluster/events/replicate");
        let mut builder = client.post(&url).json(&body);
        if let Some(t) = &token {
            builder = builder.header("Authorization", format!("Bearer {t}"));
        }
        if let Ok(resp) = builder.send().await {
            if resp.status().is_success() {
                ok += 1;
            }
        }
    }
    ok
}

// ───────────────────────── C-1 · distributed scheduler ─────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct OlapSubtaskRequest {
    pub(crate) query: String,
    #[serde(default)]
    pub(crate) max_rows: Option<usize>,
}

/// Execute one OLAP subtask locally and return the partial result (C-1 receive).
pub(crate) async fn cluster_olap_subtask(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<OlapSubtaskRequest>,
) -> Result<(StatusCode, Json<OlapSubtaskResult>), StatusCode> {
    require_cluster_token(&headers, &state)?;
    let result = run_local_olap_subtask(&state, &req.query, req.max_rows);
    Ok((StatusCode::OK, Json(result)))
}

/// Run an OLAP query against the local engine and wrap it as a subtask partial.
pub(crate) fn run_local_olap_subtask(
    state: &AppState,
    query: &str,
    max_rows: Option<usize>,
) -> OlapSubtaskResult {
    let rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
    let data_dir = state.runtime_config.storage.data_dir.clone();
    let rocksdb_rows = {
        let wal = state.storage.wal_engine.lock().unwrap_or_else(|e| e.into_inner());
        if wal.persists_rows() {
            Some(wal.scan_rows_for_db("", rs.current_xid()))
        } else {
            None
        }
    };
    let resp = execute_olap_query(
        query.to_string(),
        max_rows,
        &rs,
        "",
        &data_dir,
        None,
        rocksdb_rows,
    );
    OlapSubtaskResult {
        node_id: state.node_id.clone(),
        rows: resp.rows,
        elapsed_ms: resp.elapsed_ms,
        data_source: resp.data_source,
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct DistributedOlapRequest {
    pub(crate) query: String,
    #[serde(default)]
    pub(crate) max_rows: Option<usize>,
}

/// Coordinator endpoint: split an OLAP query across peer subtasks, gather the
/// partials, and merge them. Falls back to local-only execution when there are
/// no peers or all peers fail (C-1 send + merge).
pub(crate) async fn distributed_olap_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DistributedOlapRequest>,
) -> Result<(StatusCode, Json<DistributedOlapResult>), StatusCode> {
    require_cluster_token(&headers, &state)?;
    let client = reqwest::Client::new();
    let result = gather_distributed_olap(&state, &client, &req.query, req.max_rows).await;
    Ok((StatusCode::OK, Json(result)))
}

/// Execute the local partial, request a partial from each peer, and merge.
pub(crate) async fn gather_distributed_olap(
    state: &AppState,
    client: &reqwest::Client,
    query: &str,
    max_rows: Option<usize>,
) -> DistributedOlapResult {
    let local = run_local_olap_subtask(state, query, max_rows);
    let peers = state.cluster.raft_peers.as_ref().clone();
    if peers.is_empty() {
        return merge_olap_partials(vec![local], true);
    }

    let token = state.cluster.cluster_token.as_ref().clone();
    let mut partials = vec![local];
    let mut any_peer_ok = false;
    for peer in &peers {
        let url = format!("{peer}/api/v1/cluster/scheduler/subtask");
        let mut builder = client.post(&url).json(&OlapSubtaskRequest {
            query: query.to_string(),
            max_rows,
        });
        if let Some(t) = &token {
            builder = builder.header("Authorization", format!("Bearer {t}"));
        }
        if let Ok(resp) = builder.send().await {
            if resp.status().is_success() {
                if let Ok(partial) = resp.json::<OlapSubtaskResult>().await {
                    partials.push(partial);
                    any_peer_ok = true;
                }
            }
        }
    }
    merge_olap_partials(partials, !any_peer_ok)
}

// ───────────────────────── C-2 · shard coordinators ─────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct ShardInfoResponse {
    pub(crate) status: &'static str,
    pub(crate) table: String,
    pub(crate) sharded: bool,
    pub(crate) column: Option<String>,
    pub(crate) shard_count: usize,
    pub(crate) node_count: usize,
    /// Per-shard owning-node index within `[local, peer_0, ...]`.
    pub(crate) shard_owners: Vec<usize>,
    /// Per-shard row counts derived from the local row store (scatter-gather view).
    pub(crate) per_shard_row_counts: Vec<usize>,
}

/// Report a table's shard map plus the per-shard row distribution computed from
/// the local row store (C-2 catalog endpoint).
pub(crate) async fn cluster_shard_info(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(table): Path<String>,
) -> Result<(StatusCode, Json<ShardInfoResponse>), StatusCode> {
    require_cluster_token(&headers, &state)?;
    let node_count = 1 + state.cluster.raft_peers.len();
    let Some(cfg) = lookup_shard_config(&state, &table) else {
        return Ok((
            StatusCode::OK,
            Json(ShardInfoResponse {
                status: "ok",
                table,
                sharded: false,
                column: None,
                shard_count: 0,
                node_count,
                shard_owners: Vec::new(),
                per_shard_row_counts: Vec::new(),
            }),
        ));
    };

    let shard_owners: Vec<usize> = (0..cfg.shard_count)
        .map(|s| owning_node_index(s, node_count))
        .collect();

    // Compute per-shard row counts from the local row store by hashing each
    // row's primary key (the row-key suffix after the `<table>:` prefix).
    let mut per_shard_row_counts = vec![0usize; cfg.shard_count];
    {
        let rs = state.storage.row_store.lock().unwrap_or_else(|e| e.into_inner());
        let prefix = format!("{}:", table.to_ascii_lowercase());
        for (key, _row) in rs.scan_at_snapshot(rs.current_xid()) {
            if let Some(pk) = key.strip_prefix(&prefix) {
                let shard = shard_for_key(cfg.shard_count, pk);
                per_shard_row_counts[shard] += 1;
            }
        }
    }

    Ok((
        StatusCode::OK,
        Json(ShardInfoResponse {
            status: "ok",
            table,
            sharded: true,
            column: Some(cfg.column),
            shard_count: cfg.shard_count,
            node_count,
            shard_owners,
            per_shard_row_counts,
        }),
    ))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ShardRouteRequest {
    pub(crate) table: String,
    pub(crate) primary_key: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ShardRouteResponse {
    pub(crate) status: &'static str,
    pub(crate) table: String,
    pub(crate) sharded: bool,
    pub(crate) shard_id: usize,
    pub(crate) owning_node_index: usize,
    pub(crate) is_local: bool,
}

/// Resolve which shard (and owning node) a write key routes to (C-2 write routing).
pub(crate) async fn cluster_shard_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ShardRouteRequest>,
) -> Result<(StatusCode, Json<ShardRouteResponse>), StatusCode> {
    require_cluster_token(&headers, &state)?;
    let node_count = 1 + state.cluster.raft_peers.len();
    let Some(cfg) = lookup_shard_config(&state, &req.table) else {
        return Ok((
            StatusCode::OK,
            Json(ShardRouteResponse {
                status: "ok",
                table: req.table,
                sharded: false,
                shard_id: 0,
                owning_node_index: 0,
                is_local: true,
            }),
        ));
    };
    let shard_id = shard_for_key(cfg.shard_count, &req.primary_key);
    let owner = owning_node_index(shard_id, node_count);
    Ok((
        StatusCode::OK,
        Json(ShardRouteResponse {
            status: "ok",
            table: req.table,
            sharded: true,
            shard_id,
            owning_node_index: owner,
            is_local: owner == 0,
        }),
    ))
}
