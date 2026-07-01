//! Distributed data-plane helpers (Tasks-7 group C).
//!
//! Pure, unit-testable logic shared by the cross-node data-plane handlers:
//! - C-4 HTAP sync cross-node transport (push mutations to peer OLAP replicas)
//! - C-3 cross-node cache replication (SET/DEL fan-out)
//! - C-5 quorum event bus replication (ordered event fan-out + consumer offsets)
//! - C-1 distributed scheduler (split OLAP subtasks, merge partials, local fallback)
//! - C-2 shard coordinators (DISTRIBUTE BY HASH(col), shard map, write routing,
//!   scatter-gather reads)
//!
//! The network fan-out functions live in `handlers/dataplane.rs`; this module
//! holds the deterministic logic so it can be tested without a live cluster.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use voltnuerongrid_core::sharding::fnv1a_hash;
use voltnuerongrid_store::htap_sync::MutationOp;

use crate::AppState;

// ───────────────────────── C-4 · HTAP cross-node transport ─────────────────────────

/// A row mutation as carried over the cross-node HTAP transport.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ReplicatedMutation {
    pub(crate) sequence: u64,
    pub(crate) table: String,
    pub(crate) primary_key: String,
    pub(crate) payload_json: String,
    /// "insert" | "update" | "delete"
    pub(crate) op: String,
}

impl ReplicatedMutation {
    pub(crate) fn op_enum(&self) -> MutationOp {
        match self.op.as_str() {
            "delete" => MutationOp::Delete,
            "update" => MutationOp::Update,
            _ => MutationOp::Insert,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn op_str(op: &MutationOp) -> &'static str {
        match op {
            MutationOp::Insert => "insert",
            MutationOp::Update => "update",
            MutationOp::Delete => "delete",
        }
    }
}

/// Apply a batch of replicated mutations to the local in-memory OLAP replica.
///
/// Returns `(applied_count, last_applied_sequence)`. The batch is applied
/// in-order; inserts/updates upsert the row keyed by `primary_key`, deletes
/// remove it. This is the receive side of the C-4 push transport.
pub(crate) fn apply_htap_mutations_to_olap(
    state: &AppState,
    mutations: &[ReplicatedMutation],
) -> (usize, u64) {
    let mut olap = state.storage.olap_store.lock().expect("olap_store lock");
    let mut applied = 0usize;
    let mut last_seq = 0u64;
    for m in mutations {
        last_seq = last_seq.max(m.sequence);
        match m.op_enum() {
            MutationOp::Insert | MutationOp::Update => {
                let data: HashMap<String, String> = serde_json::from_str(&m.payload_json)
                    .unwrap_or_else(|_| {
                        let mut d = HashMap::new();
                        d.insert("payload".to_string(), m.payload_json.clone());
                        d
                    });
                olap.insert(m.primary_key.clone(), data);
                applied += 1;
            }
            MutationOp::Delete => {
                olap.remove(&m.primary_key);
                applied += 1;
            }
        }
    }
    (applied, last_seq)
}

/// Export the pending mutations that still need to be shipped to `peer`,
/// based on the per-peer replication cursor. Returns the batch plus the
/// highest sequence in it (0 when empty).
#[allow(dead_code)]
pub(crate) fn htap_batch_for_peer(
    state: &AppState,
    peer: &str,
    max_items: usize,
) -> (Vec<ReplicatedMutation>, u64) {
    let cursor = state
        .cluster
        .htap_peer_cursors
        .lock()
        .ok()
        .and_then(|c| c.get(peer).copied())
        .unwrap_or(0);
    let origin = state.cluster.sync_origin.lock().expect("sync_origin lock");
    let batch: Vec<ReplicatedMutation> = origin
        .export_since(cursor, max_items)
        .into_iter()
        .map(|m| ReplicatedMutation {
            sequence: m.sequence,
            table: m.table,
            primary_key: m.primary_key,
            payload_json: m.payload_json,
            op: ReplicatedMutation::op_str(&m.op).to_string(),
        })
        .collect();
    let last_seq = batch.last().map(|m| m.sequence).unwrap_or(cursor);
    (batch, last_seq)
}

/// Advance the per-peer HTAP replication cursor after a successful ship.
#[allow(dead_code)]
pub(crate) fn advance_htap_peer_cursor(state: &AppState, peer: &str, last_seq: u64) {
    if last_seq == 0 {
        return;
    }
    if let Ok(mut cursors) = state.cluster.htap_peer_cursors.lock() {
        let entry = cursors.entry(peer.to_string()).or_insert(0);
        if last_seq > *entry {
            *entry = last_seq;
        }
    }
}

/// Cross-node HTAP freshness lag in milliseconds: time since the last committed
/// OLTP mutation was recorded in the sync origin. `None` when no mutation has
/// been recorded yet.
pub(crate) fn cross_node_htap_lag_ms(state: &AppState) -> Option<u64> {
    let origin = state.cluster.sync_origin.lock().ok()?;
    let last = origin.last_mutation_epoch_ms();
    if last == 0 {
        return None;
    }
    let now_ms = crate::now_unix_ms_u64();
    Some(now_ms.saturating_sub(last))
}

// ───────────────────────── C-3 · cross-node cache replication ─────────────────────────

/// Apply a replicated cache command (SET/DEL) to the local distributed cache.
/// Returns `true` when the command mutated local state.
pub(crate) fn apply_cache_replication(
    state: &AppState,
    cmd: &str,
    partition_id: &str,
    key: &str,
    value: Option<serde_json::Value>,
    ttl_ms: Option<u64>,
) -> bool {
    let now_ms = crate::now_unix_ms_u64();
    let mut cache = state.ops.distributed_cache.lock().expect("distributed_cache lock");
    match cmd.to_ascii_uppercase().as_str() {
        "SET" => {
            let v = value.unwrap_or(serde_json::Value::Null);
            cache.set(partition_id, key.to_string(), v, ttl_ms, now_ms).is_ok()
        }
        "DEL" => cache.invalidate(partition_id, key).unwrap_or(false),
        _ => false,
    }
}

/// Whether a Redis command mutates state and therefore must be replicated.
#[allow(dead_code)]
pub(crate) fn cache_command_is_replicable(cmd: &str) -> bool {
    matches!(cmd.to_ascii_uppercase().as_str(), "SET" | "DEL")
}

// ───────────────────────── C-5 · quorum event bus replication ─────────────────────────

/// A replicated event, ordered by the source node's transport sequence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ReplicatedEvent {
    pub(crate) transport_sequence: u64,
    pub(crate) stream_name: String,
    pub(crate) origin: String,
    pub(crate) payload_json: String,
}

/// Apply an ordered batch of replicated events to the local event bus.
///
/// Events are sorted by `transport_sequence` before publishing so the local
/// replica observes the same total order as the source node (the ordering
/// guarantee required by C-5). Returns the number of events applied and the
/// highest transport sequence observed.
pub(crate) fn apply_event_replication(
    state: &AppState,
    events: &[ReplicatedEvent],
) -> (usize, u64) {
    use voltnuerongrid_ingest::StreamDirection;
    let mut ordered: Vec<&ReplicatedEvent> = events.iter().collect();
    ordered.sort_by_key(|e| e.transport_sequence);

    let mut applied = 0usize;
    let mut last_seq = 0u64;
    let mut bus = state.ingest.ingest_event_bus.lock().expect("event_bus lock");
    for e in ordered {
        last_seq = last_seq.max(e.transport_sequence);
        if bus
            .publish(
                &e.stream_name,
                StreamDirection::Internal,
                &e.origin,
                &e.payload_json,
                HashMap::new(),
            )
            .is_ok()
        {
            applied += 1;
        }
    }
    drop(bus);

    // Persist the consumer offset so it survives node failure (C-5: offsets survive).
    if last_seq > 0 {
        if let Ok(mut cursors) = state.ingest.ingest_outbox_cursors.lock() {
            use voltnuerongrid_ingest::ReplayCursorStore;
            let _ = cursors.save("cluster.replicated", last_seq);
        }
    }
    (applied, last_seq)
}

// ───────────────────────── C-1 · distributed scheduler ─────────────────────────

/// Partial result returned by one node executing an OLAP subtask.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct OlapSubtaskResult {
    pub(crate) node_id: String,
    pub(crate) rows: usize,
    pub(crate) elapsed_ms: u128,
    pub(crate) data_source: String,
}

/// Merged result of a distributed OLAP query gathered from peer subtasks plus
/// the local partial.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct DistributedOlapResult {
    pub(crate) status: &'static str,
    pub(crate) total_rows: usize,
    pub(crate) partitions: usize,
    pub(crate) per_node: Vec<OlapSubtaskResult>,
    /// True when execution fell back to local-only (single-node or all peers failed).
    pub(crate) local_fallback: bool,
}

/// Merge OLAP partial results from multiple nodes. Row counts sum across
/// partitions (scatter-gather aggregate merge). `local_fallback` is true when
/// only the local partial is present.
pub(crate) fn merge_olap_partials(
    mut partials: Vec<OlapSubtaskResult>,
    local_fallback: bool,
) -> DistributedOlapResult {
    partials.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    let total_rows = partials.iter().map(|p| p.rows).sum();
    DistributedOlapResult {
        status: "ok",
        total_rows,
        partitions: partials.len(),
        per_node: partials,
        local_fallback,
    }
}

// ───────────────────────── C-2 · shard coordinators ─────────────────────────

/// Sharding configuration for one table (`DISTRIBUTE BY HASH(col) SHARDS n`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ShardTableConfig {
    pub(crate) column: String,
    pub(crate) shard_count: usize,
}

/// Parse a `DISTRIBUTE BY HASH(col)` clause (optionally `SHARDS n`) from a
/// CREATE TABLE statement. Returns `None` when the clause is absent.
///
/// Examples:
/// - `CREATE TABLE t (...) DISTRIBUTE BY HASH(id)` → ("id", default_shards)
/// - `... DISTRIBUTE BY HASH(user_id) SHARDS 8` → ("user_id", 8)
pub(crate) fn parse_distribute_by(ddl: &str, default_shards: usize) -> Option<ShardTableConfig> {
    let lower = ddl.to_ascii_lowercase();
    let idx = lower.find("distribute by")?;
    let after = &lower[idx + "distribute by".len()..];
    let hidx = after.find("hash")?;
    let rest = &after[hidx + "hash".len()..];
    let open = rest.find('(')?;
    let close = rest[open + 1..].find(')')?;
    let column = rest[open + 1..open + 1 + close].trim().to_string();
    if column.is_empty() {
        return None;
    }
    // Optional SHARDS n
    let shard_count = rest[open + 1 + close + 1..]
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|w| w[0] == "shards")
        .and_then(|w| w[1].trim_end_matches([',', ';', ')']).parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default_shards);
    Some(ShardTableConfig {
        column,
        shard_count,
    })
}

/// Deterministically map a row key to a shard id in `[0, shard_count)`.
pub(crate) fn shard_for_key(shard_count: usize, key: &str) -> usize {
    if shard_count <= 1 {
        return 0;
    }
    (fnv1a_hash(key) % shard_count as u64) as usize
}

/// Map a shard id to the index of its owning node within the ordered node list
/// (`[local_node, peer_0, peer_1, ...]`). Index 0 is always the local node.
pub(crate) fn owning_node_index(shard_id: usize, node_count: usize) -> usize {
    if node_count == 0 {
        return 0;
    }
    shard_id % node_count
}

/// Register (or replace) the shard configuration for a table.
pub(crate) fn register_shard_config(state: &AppState, table: &str, config: ShardTableConfig) {
    if let Ok(mut registry) = state.storage.shard_registry.lock() {
        registry.insert(table.to_ascii_lowercase(), config);
    }
}

/// Look up the shard configuration for a table, if it is sharded.
pub(crate) fn lookup_shard_config(state: &AppState, table: &str) -> Option<ShardTableConfig> {
    state
        .storage
        .shard_registry
        .lock()
        .ok()
        .and_then(|r| r.get(&table.to_ascii_lowercase()).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_distribute_by_hash_basic() {
        let cfg = parse_distribute_by("CREATE TABLE t (id INT) DISTRIBUTE BY HASH(id)", 4).unwrap();
        assert_eq!(cfg.column, "id");
        assert_eq!(cfg.shard_count, 4);
    }

    #[test]
    fn parse_distribute_by_hash_with_shards() {
        let cfg =
            parse_distribute_by("CREATE TABLE t (uid INT) DISTRIBUTE BY HASH(uid) SHARDS 8", 4)
                .unwrap();
        assert_eq!(cfg.column, "uid");
        assert_eq!(cfg.shard_count, 8);
    }

    #[test]
    fn parse_distribute_by_absent_returns_none() {
        assert!(parse_distribute_by("CREATE TABLE t (id INT)", 4).is_none());
    }

    #[test]
    fn shard_for_key_is_deterministic_and_bounded() {
        for i in 0..200 {
            let k = format!("user-{i}");
            let s = shard_for_key(8, &k);
            assert_eq!(s, shard_for_key(8, &k));
            assert!(s < 8);
        }
    }

    #[test]
    fn owning_node_index_round_robins() {
        assert_eq!(owning_node_index(0, 3), 0);
        assert_eq!(owning_node_index(1, 3), 1);
        assert_eq!(owning_node_index(2, 3), 2);
        assert_eq!(owning_node_index(3, 3), 0);
    }

    #[test]
    fn merge_olap_partials_sums_rows() {
        let partials = vec![
            OlapSubtaskResult {
                node_id: "node-2".into(),
                rows: 5,
                elapsed_ms: 1,
                data_source: "paged_store".into(),
            },
            OlapSubtaskResult {
                node_id: "node-1".into(),
                rows: 7,
                elapsed_ms: 2,
                data_source: "paged_store".into(),
            },
        ];
        let merged = merge_olap_partials(partials, false);
        assert_eq!(merged.total_rows, 12);
        assert_eq!(merged.partitions, 2);
        assert_eq!(merged.per_node[0].node_id, "node-1"); // sorted
        assert!(!merged.local_fallback);
    }

    #[test]
    fn cache_command_replicable_only_for_mutations() {
        assert!(cache_command_is_replicable("SET"));
        assert!(cache_command_is_replicable("del"));
        assert!(!cache_command_is_replicable("GET"));
        assert!(!cache_command_is_replicable("PING"));
    }
}
