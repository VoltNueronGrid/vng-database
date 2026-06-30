//! SCALE-1: Horizontal Scale-Out Controller
//!
//! GET  /api/v1/autoscale/status  — current replica count, target, scaling flag
//! POST /api/v1/autoscale/policy  — set scale-up/down thresholds (admin only)
//! POST /api/v1/autoscale/tick    — manually trigger a scale evaluation (admin only, for testing)

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::{AppState, AuthErrorResponse};
use crate::auth::require_admin_api_key;

// ── Domain types ─────────────────────────────────────────────────────────────

/// Scale-out controller policy (persisted in AppState).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutoscalePolicy {
    /// Minimum replica count (always running).
    pub min_replicas: usize,
    /// Maximum replica count (upper bound).
    pub max_replicas: usize,
    /// Query-queue depth threshold that triggers a scale-up event.
    pub scale_up_queue_threshold: usize,
    /// Query-queue depth below which a scale-down event is considered.
    pub scale_down_queue_threshold: usize,
    /// Cooldown in seconds between consecutive scale events.
    pub cooldown_secs: u64,
    /// Backend to use: "kubernetes", "docker", or "none" (default).
    pub backend: String,
}

impl Default for AutoscalePolicy {
    fn default() -> Self {
        Self {
            min_replicas: 1,
            max_replicas: 8,
            scale_up_queue_threshold: env_usize("VNG_AUTOSCALE_QUEUE_THRESHOLD", 100),
            scale_down_queue_threshold: 10,
            cooldown_secs: env_u64("VNG_AUTOSCALE_COOLDOWN_SECS", 60),
            backend: std::env::var("VNG_AUTOSCALE_BACKEND")
                .unwrap_or_else(|_| "none".to_string()),
        }
    }
}

/// Runtime autoscale status (updated by the tick / background evaluator).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AutoscaleStatus {
    /// Current number of active compute replicas.
    pub replicas: usize,
    /// Desired replica target after last evaluation.
    pub target: usize,
    /// Whether a scale event is currently in progress.
    pub scaling: bool,
    /// Timestamp of the last scale event (Unix seconds).
    pub last_scale_at_unix_secs: u64,
    /// Direction of the last event: "up", "down", or "none".
    pub last_scale_direction: String,
    /// The queue depth that triggered the last event (if any).
    pub last_trigger_queue_depth: usize,
}

impl Default for AutoscaleStatus {
    fn default() -> Self {
        Self {
            replicas: 1,
            target: 1,
            scaling: false,
            last_scale_at_unix_secs: 0,
            last_scale_direction: "none".to_string(),
            last_trigger_queue_depth: 0,
        }
    }
}

// ── Scale evaluation logic ────────────────────────────────────────────────────

/// Read current queue depth from AppState.
/// Uses the total number of active per-DB semaphores as a proxy for load.
fn current_queue_depth(state: &AppState) -> usize {
    // Approximate: sum of in-flight connections across all DB semaphores.
    // In a real implementation this would be an atomic counter updated by
    // acquire/release helpers.
    if let Ok(semaphores) = state.storage.db_semaphores.lock() {
        semaphores
            .values()
            .map(|sem| {
                let avail = sem.available_permits();
                crate::DEFAULT_DB_MAX_CONNECTIONS.saturating_sub(avail)
            })
            .sum()
    } else {
        0
    }
}

/// Evaluate scale policy against current metrics and mutate status if needed.
/// Returns `(scaled, direction, new_replicas)`.
pub(crate) fn evaluate_autoscale(
    status: &mut AutoscaleStatus,
    policy: &AutoscalePolicy,
    queue_depth: usize,
    now_secs: u64,
) -> (bool, &'static str, usize) {
    // Respect cooldown.
    if status.scaling
        || (now_secs.saturating_sub(status.last_scale_at_unix_secs)) < policy.cooldown_secs
    {
        return (false, "none", status.replicas);
    }

    if queue_depth >= policy.scale_up_queue_threshold && status.replicas < policy.max_replicas {
        let new_target = (status.replicas + 1).min(policy.max_replicas);
        status.target = new_target;
        status.replicas = new_target;
        status.scaling = true;
        status.last_scale_at_unix_secs = now_secs;
        status.last_scale_direction = "up".to_string();
        status.last_trigger_queue_depth = queue_depth;
        return (true, "up", new_target);
    }

    if queue_depth <= policy.scale_down_queue_threshold && status.replicas > policy.min_replicas {
        let new_target = status.replicas.saturating_sub(1).max(policy.min_replicas);
        status.target = new_target;
        status.replicas = new_target;
        status.scaling = true;
        status.last_scale_at_unix_secs = now_secs;
        status.last_scale_direction = "down".to_string();
        status.last_trigger_queue_depth = queue_depth;
        return (true, "down", new_target);
    }

    (false, "none", status.replicas)
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// SCALE-1: GET /api/v1/autoscale/status
pub(crate) async fn autoscale_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;

    let status = state.ops.autoscale_status.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let queue_depth = current_queue_depth(&state);

    Ok((StatusCode::OK, Json(serde_json::json!({
        "status": "ok",
        "replicas": status.replicas,
        "target": status.target,
        "scaling": status.scaling,
        "last_scale_at_unix_secs": status.last_scale_at_unix_secs,
        "last_scale_direction": status.last_scale_direction,
        "current_queue_depth": queue_depth,
    }))))
}

/// SCALE-1: POST /api/v1/autoscale/policy
pub(crate) async fn autoscale_set_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AutoscalePolicyRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;

    let mut policy = state.ops.autoscale_policy.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(min) = req.min_replicas { policy.min_replicas = min; }
    if let Some(max) = req.max_replicas { policy.max_replicas = max; }
    if let Some(up) = req.scale_up_queue_threshold { policy.scale_up_queue_threshold = up; }
    if let Some(down) = req.scale_down_queue_threshold { policy.scale_down_queue_threshold = down; }
    if let Some(cd) = req.cooldown_secs { policy.cooldown_secs = cd; }
    if let Some(b) = req.backend { policy.backend = b; }
    let updated = policy.clone();
    drop(policy);

    Ok((StatusCode::OK, Json(serde_json::json!({
        "status": "ok",
        "policy": serde_json::to_value(updated).unwrap_or_default(),
    }))))
}

/// SCALE-1: POST /api/v1/autoscale/tick  — manual evaluation trigger for testing.
pub(crate) async fn autoscale_tick(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<AuthErrorResponse>)> {
    require_admin_api_key(&headers, &state)?;

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let queue_depth = current_queue_depth(&state);

    let policy = state.ops.autoscale_policy.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut status = state.ops.autoscale_status.lock().unwrap_or_else(|e| e.into_inner());

    let (scaled, direction, new_replicas) = evaluate_autoscale(&mut status, &policy, queue_depth, now_secs);

    // After evaluation, clear the scaling flag (in production, an orchestrator would clear it
    // once the scale operation completes; here we clear it immediately as we're simulation-only).
    if scaled {
        status.scaling = false;
    }

    Ok((StatusCode::OK, Json(serde_json::json!({
        "status": "ok",
        "scaled": scaled,
        "direction": direction,
        "new_replicas": new_replicas,
        "queue_depth": queue_depth,
    }))))
}

// ── Request DTOs ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct AutoscalePolicyRequest {
    pub min_replicas: Option<usize>,
    pub max_replicas: Option<usize>,
    pub scale_up_queue_threshold: Option<usize>,
    pub scale_down_queue_threshold: Option<usize>,
    pub cooldown_secs: Option<u64>,
    pub backend: Option<String>,
}

// ── Env helpers ───────────────────────────────────────────────────────────────

fn env_usize(var: &str, default: usize) -> usize {
    std::env::var(var)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn env_u64(var: &str, default: u64) -> u64 {
    std::env::var(var)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}
