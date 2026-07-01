//! H9-15: Distributed partition ownership and optional OLAP snapshot nodes.
//!
//! This module provides the placement registry that tracks which cluster nodes
//! own which partitions, routes queries to the appropriate node, and manages
//! rebalance plans for partition migration.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::types::PartitionId;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Role a node can play in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "rocksdb", derive(serde::Serialize, serde::Deserialize))]
pub enum NodeRole {
    /// Handles OLTP writes and reads; owns live tail data.
    Oltp,
    /// Serves OLAP queries from snapshot/base; accepts no OLTP writes.
    OlapSnapshot,
    /// Handles both OLTP and OLAP (default single-node mode).
    Hybrid,
}

/// A node in the cluster with role and capabilities.
#[derive(Debug, Clone)]
pub struct ClusterNode {
    pub node_id: String,
    pub role: NodeRole,
    pub base_url: String,
    pub available: bool,
    pub last_heartbeat_ms: u64,
}

/// Placement of a single partition across the cluster.
#[derive(Debug, Clone)]
pub struct PartitionPlacement {
    pub partition_id: PartitionId,
    pub table_name: String,
    /// OLTP primary owner.
    pub primary_node_id: String,
    /// Other OLTP replicas.
    pub replica_node_ids: Vec<String>,
    /// OlapSnapshot nodes serving this partition.
    pub olap_snapshot_node_ids: Vec<String>,
    /// Optimistic-concurrency version; increment on each update.
    pub version: u64,
}

/// A plan to move/rebalance a partition.
#[derive(Debug, Clone)]
pub struct RebalancePlan {
    pub plan_id: String,
    pub partition_id: PartitionId,
    pub from_node_id: String,
    pub to_node_id: String,
    pub reason: String,
    pub estimated_bytes: u64,
    pub status: RebalanceStatus,
    pub created_at_ms: u64,
}

/// Lifecycle of a rebalance plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebalanceStatus {
    Pending,
    InProgress,
    Complete,
    Failed(String),
}

/// Query routing decision for distributed execution.
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub partition_id: PartitionId,
    pub target_node_id: String,
    pub access_path: DistributedAccessPath,
    pub reason: String,
}

/// How a query should be executed across the cluster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistributedAccessPath {
    /// Local node handles both base and tail.
    LocalHybrid,
    /// Forward to the OLTP primary.
    RemoteOltp,
    /// Forward to a dedicated OLAP snapshot node.
    RemoteOlapSnapshot,
    /// Fan out and merge results.
    BroadcastHybrid,
}

// ---------------------------------------------------------------------------
// Metrics snapshot
// ---------------------------------------------------------------------------

/// Point-in-time snapshot of placement registry metrics.
#[derive(Debug, Clone)]
pub struct PlacementMetrics {
    pub partition_count: usize,
    pub node_count: usize,
    pub olap_snapshot_node_count: usize,
    pub rebalances_total: u64,
    pub rebalance_failures_total: u64,
    pub routing_decisions_total: u64,
}

// ---------------------------------------------------------------------------
// PlacementRegistry
// ---------------------------------------------------------------------------

/// The placement registry — owns all partition and node metadata for the local
/// node's view of the cluster.
pub struct PlacementRegistry {
    local_node_id: String,
    local_role: NodeRole,
    placements: Arc<RwLock<HashMap<PartitionId, PartitionPlacement>>>,
    nodes: Arc<RwLock<HashMap<String, ClusterNode>>>,
    rebalance_plans: Arc<Mutex<Vec<RebalancePlan>>>,
    routing_decisions_total: Arc<AtomicU64>,
    rebalances_total: Arc<AtomicU64>,
    rebalance_failures_total: Arc<AtomicU64>,
}

impl PlacementRegistry {
    /// Create a new registry for the given local node.
    pub fn new(local_node_id: String, local_role: NodeRole) -> Self {
        PlacementRegistry {
            local_node_id,
            local_role,
            placements: Arc::new(RwLock::new(HashMap::new())),
            nodes: Arc::new(RwLock::new(HashMap::new())),
            rebalance_plans: Arc::new(Mutex::new(Vec::new())),
            routing_decisions_total: Arc::new(AtomicU64::new(0)),
            rebalances_total: Arc::new(AtomicU64::new(0)),
            rebalance_failures_total: Arc::new(AtomicU64::new(0)),
        }
    }

    // -----------------------------------------------------------------------
    // Node management
    // -----------------------------------------------------------------------

    /// Register or update a cluster node.
    pub fn register_node(&self, node: ClusterNode) {
        let mut nodes = self
            .nodes
            .write()
            .expect("invariant: nodes lock is not poisoned");
        nodes.insert(node.node_id.clone(), node);
    }

    /// Get a node by ID.
    pub fn get_node(&self, node_id: &str) -> Option<ClusterNode> {
        let nodes = self
            .nodes
            .read()
            .expect("invariant: nodes lock is not poisoned");
        nodes.get(node_id).cloned()
    }

    /// List all registered nodes.
    pub fn list_nodes(&self) -> Vec<ClusterNode> {
        let nodes = self
            .nodes
            .read()
            .expect("invariant: nodes lock is not poisoned");
        nodes.values().cloned().collect()
    }

    /// List nodes filtered by role.
    pub fn nodes_by_role(&self, role: NodeRole) -> Vec<ClusterNode> {
        let nodes = self
            .nodes
            .read()
            .expect("invariant: nodes lock is not poisoned");
        nodes.values().filter(|n| n.role == role).cloned().collect()
    }

    // -----------------------------------------------------------------------
    // Placement management
    // -----------------------------------------------------------------------

    /// Register or update placement for a partition.
    pub fn register_placement(&self, placement: PartitionPlacement) {
        let mut placements = self
            .placements
            .write()
            .expect("invariant: placements lock is not poisoned");
        placements.insert(placement.partition_id, placement);
    }

    /// Get placement for a partition.
    pub fn get_placement(&self, partition_id: PartitionId) -> Option<PartitionPlacement> {
        let placements = self
            .placements
            .read()
            .expect("invariant: placements lock is not poisoned");
        placements.get(&partition_id).cloned()
    }

    /// List all placements.
    pub fn list_placements(&self) -> Vec<PartitionPlacement> {
        let placements = self
            .placements
            .read()
            .expect("invariant: placements lock is not poisoned");
        placements.values().cloned().collect()
    }

    // -----------------------------------------------------------------------
    // Query routing
    // -----------------------------------------------------------------------

    /// Route a query for a partition.
    ///
    /// Routing rules (evaluated in order):
    /// 1. If the local node is the primary OLTP owner → `LocalHybrid`.
    /// 2. If this is an OLAP query (`prefer_olap_snapshot`) and there is at
    ///    least one available OlapSnapshot node assigned to the partition →
    ///    `RemoteOlapSnapshot` (first available wins).
    /// 3. If the local node is an OlapSnapshot node and the query is a read →
    ///    `LocalHybrid` (serve from local snapshot).
    /// 4. If there is a remote OLTP primary and the query is a write →
    ///    `RemoteOltp`.
    /// 5. Fallback → `LocalHybrid`.
    pub fn route_query(
        &self,
        partition_id: PartitionId,
        is_write: bool,
        prefer_olap_snapshot: bool,
    ) -> RoutingDecision {
        self.routing_decisions_total.fetch_add(1, Ordering::Relaxed);

        let placement_opt = self.get_placement(partition_id);

        // Rule 1 — local node is the primary.
        if let Some(ref p) = placement_opt {
            if p.primary_node_id == self.local_node_id {
                return RoutingDecision {
                    partition_id,
                    target_node_id: self.local_node_id.clone(),
                    access_path: DistributedAccessPath::LocalHybrid,
                    reason: "local node is primary OLTP owner".to_string(),
                };
            }
        }

        // Rule 2 — OLAP query with available OlapSnapshot nodes.
        if prefer_olap_snapshot && !is_write {
            if let Some(ref p) = placement_opt {
                let nodes_guard = self
                    .nodes
                    .read()
                    .expect("invariant: nodes lock is not poisoned");
                let target = p.olap_snapshot_node_ids.iter().find(|nid| {
                    nodes_guard
                        .get(*nid)
                        .map(|n| n.available && n.role == NodeRole::OlapSnapshot)
                        .unwrap_or(false)
                });
                if let Some(node_id) = target {
                    return RoutingDecision {
                        partition_id,
                        target_node_id: node_id.clone(),
                        access_path: DistributedAccessPath::RemoteOlapSnapshot,
                        reason: "dedicated OlapSnapshot node available".to_string(),
                    };
                }
            }
        }

        // Rule 3 — local node is an OlapSnapshot and this is a read.
        if !is_write && self.local_role == NodeRole::OlapSnapshot {
            return RoutingDecision {
                partition_id,
                target_node_id: self.local_node_id.clone(),
                access_path: DistributedAccessPath::LocalHybrid,
                reason: "local OlapSnapshot node serving read".to_string(),
            };
        }

        // Rule 4 — remote OLTP primary exists and this is a write.
        if is_write {
            if let Some(ref p) = placement_opt {
                let primary = p.primary_node_id.clone();
                if !primary.is_empty() && primary != self.local_node_id {
                    return RoutingDecision {
                        partition_id,
                        target_node_id: primary,
                        access_path: DistributedAccessPath::RemoteOltp,
                        reason: "forwarding write to remote OLTP primary".to_string(),
                    };
                }
            }
        }

        // Rule 5 — fallback.
        RoutingDecision {
            partition_id,
            target_node_id: self.local_node_id.clone(),
            access_path: DistributedAccessPath::LocalHybrid,
            reason: "fallback: local hybrid".to_string(),
        }
    }

    /// Validate that an OLAP-only node does not accept write operations.
    ///
    /// Returns `Err` if the local node is `OlapSnapshot`, which must never
    /// accept direct writes.
    pub fn can_accept_write(&self, partition_id: PartitionId) -> Result<(), String> {
        if self.local_role == NodeRole::OlapSnapshot {
            return Err(format!(
                "node '{}' is OlapSnapshot and cannot accept writes for partition {:?}",
                self.local_node_id, partition_id
            ));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Rebalance planning
    // -----------------------------------------------------------------------

    /// Create a rebalance plan to move a partition between nodes.
    pub fn create_rebalance_plan(
        &self,
        partition_id: PartitionId,
        from_node_id: String,
        to_node_id: String,
        reason: String,
        estimated_bytes: u64,
    ) -> RebalancePlan {
        self.rebalances_total.fetch_add(1, Ordering::Relaxed);

        let created_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let plan_id = format!("plan-{}-{}", partition_id.0, created_at_ms);

        let plan = RebalancePlan {
            plan_id,
            partition_id,
            from_node_id,
            to_node_id,
            reason,
            estimated_bytes,
            status: RebalanceStatus::Pending,
            created_at_ms,
        };

        self.rebalance_plans
            .lock()
            .expect("invariant: rebalance_plans lock is not poisoned")
            .push(plan.clone());

        plan
    }

    /// Update the status of an existing rebalance plan by `plan_id`.
    ///
    /// If no matching plan is found the call is a no-op.
    pub fn update_rebalance_status(&self, plan_id: &str, status: RebalanceStatus) {
        if matches!(status, RebalanceStatus::Failed(_)) {
            self.rebalance_failures_total
                .fetch_add(1, Ordering::Relaxed);
        }

        let mut plans = self
            .rebalance_plans
            .lock()
            .expect("invariant: rebalance_plans lock is not poisoned");

        if let Some(plan) = plans.iter_mut().find(|p| p.plan_id == plan_id) {
            plan.status = status;
        }
    }

    /// Get all rebalance plans.
    pub fn rebalance_plans(&self) -> Vec<RebalancePlan> {
        self.rebalance_plans
            .lock()
            .expect("invariant: rebalance_plans lock is not poisoned")
            .clone()
    }

    /// Get only plans with `Pending` status.
    pub fn pending_rebalance_plans(&self) -> Vec<RebalancePlan> {
        self.rebalance_plans
            .lock()
            .expect("invariant: rebalance_plans lock is not poisoned")
            .iter()
            .filter(|p| p.status == RebalanceStatus::Pending)
            .cloned()
            .collect()
    }

    // -----------------------------------------------------------------------
    // Metrics
    // -----------------------------------------------------------------------

    /// Return a snapshot of current placement metrics.
    pub fn metrics(&self) -> PlacementMetrics {
        let placements = self
            .placements
            .read()
            .expect("invariant: placements lock is not poisoned");
        let nodes = self
            .nodes
            .read()
            .expect("invariant: nodes lock is not poisoned");

        let olap_snapshot_node_count = nodes
            .values()
            .filter(|n| n.role == NodeRole::OlapSnapshot)
            .count();

        PlacementMetrics {
            partition_count: placements.len(),
            node_count: nodes.len(),
            olap_snapshot_node_count,
            rebalances_total: self.rebalances_total.load(Ordering::Relaxed),
            rebalance_failures_total: self.rebalance_failures_total.load(Ordering::Relaxed),
            routing_decisions_total: self.routing_decisions_total.load(Ordering::Relaxed),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn oltp_node(id: &str) -> ClusterNode {
        ClusterNode {
            node_id: id.to_string(),
            role: NodeRole::Oltp,
            base_url: format!("http://{}:8080", id),
            available: true,
            last_heartbeat_ms: 0,
        }
    }

    fn olap_node(id: &str) -> ClusterNode {
        ClusterNode {
            node_id: id.to_string(),
            role: NodeRole::OlapSnapshot,
            base_url: format!("http://{}:8080", id),
            available: true,
            last_heartbeat_ms: 0,
        }
    }

    fn hybrid_node(id: &str) -> ClusterNode {
        ClusterNode {
            node_id: id.to_string(),
            role: NodeRole::Hybrid,
            base_url: format!("http://{}:8080", id),
            available: true,
            last_heartbeat_ms: 0,
        }
    }

    fn placement(pid: u32, primary: &str, olap_nodes: Vec<&str>) -> PartitionPlacement {
        PartitionPlacement {
            partition_id: PartitionId(pid),
            table_name: "orders".to_string(),
            primary_node_id: primary.to_string(),
            replica_node_ids: vec![],
            olap_snapshot_node_ids: olap_nodes.iter().map(|s| s.to_string()).collect(),
            version: 1,
        }
    }

    #[test]
    fn test_register_and_get_node() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        reg.register_node(oltp_node("node-1"));
        let n = reg.get_node("node-1").expect("node should be present");
        assert_eq!(n.node_id, "node-1");
        assert_eq!(n.role, NodeRole::Oltp);
    }

    #[test]
    fn test_get_node_returns_none_for_unknown() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        assert!(reg.get_node("ghost").is_none());
    }

    #[test]
    fn test_list_nodes_by_role() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        reg.register_node(oltp_node("node-1"));
        reg.register_node(olap_node("snap-1"));
        reg.register_node(olap_node("snap-2"));
        reg.register_node(hybrid_node("hybrid-1"));

        let olap_nodes = reg.nodes_by_role(NodeRole::OlapSnapshot);
        assert_eq!(olap_nodes.len(), 2);

        let oltp_nodes = reg.nodes_by_role(NodeRole::Oltp);
        assert_eq!(oltp_nodes.len(), 1);

        let hybrid_nodes = reg.nodes_by_role(NodeRole::Hybrid);
        assert_eq!(hybrid_nodes.len(), 1);
    }

    #[test]
    fn test_register_and_get_placement() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        reg.register_placement(placement(10, "node-1", vec![]));
        let p = reg.get_placement(PartitionId(10)).expect("placement should exist");
        assert_eq!(p.partition_id, PartitionId(10));
        assert_eq!(p.primary_node_id, "node-1");
    }

    #[test]
    fn test_route_to_local_when_primary() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        reg.register_placement(placement(1, "node-1", vec![]));

        let decision = reg.route_query(PartitionId(1), false, false);
        assert_eq!(decision.access_path, DistributedAccessPath::LocalHybrid);
        assert_eq!(decision.target_node_id, "node-1");
    }

    #[test]
    fn test_route_to_olap_snapshot_when_available_and_preferred() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        reg.register_node(olap_node("snap-1"));
        reg.register_placement(placement(2, "node-2", vec!["snap-1"]));

        let decision = reg.route_query(PartitionId(2), false, true);
        assert_eq!(decision.access_path, DistributedAccessPath::RemoteOlapSnapshot);
        assert_eq!(decision.target_node_id, "snap-1");
    }

    #[test]
    fn test_route_to_local_when_no_olap_snapshots() {
        // prefer_olap_snapshot=true but no OlapSnapshot nodes registered
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        reg.register_placement(placement(3, "node-2", vec![]));

        let decision = reg.route_query(PartitionId(3), false, true);
        // Falls through to LocalHybrid fallback
        assert_eq!(decision.access_path, DistributedAccessPath::LocalHybrid);
    }

    #[test]
    fn test_olap_snapshot_node_rejects_writes() {
        let reg = PlacementRegistry::new("snap-1".to_string(), NodeRole::OlapSnapshot);
        let result = reg.can_accept_write(PartitionId(5));
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("OlapSnapshot"), "error should mention OlapSnapshot: {}", msg);
    }

    #[test]
    fn test_oltp_node_accepts_writes() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        assert!(reg.can_accept_write(PartitionId(5)).is_ok());
    }

    #[test]
    fn test_hybrid_node_accepts_writes() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Hybrid);
        assert!(reg.can_accept_write(PartitionId(5)).is_ok());
    }

    #[test]
    fn test_create_rebalance_plan() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        let plan = reg.create_rebalance_plan(
            PartitionId(7),
            "node-1".to_string(),
            "node-2".to_string(),
            "load balance".to_string(),
            1024 * 1024,
        );
        assert_eq!(plan.partition_id, PartitionId(7));
        assert_eq!(plan.from_node_id, "node-1");
        assert_eq!(plan.to_node_id, "node-2");
        assert_eq!(plan.status, RebalanceStatus::Pending);
        assert_eq!(plan.estimated_bytes, 1024 * 1024);
        assert!(plan.plan_id.starts_with("plan-7-"));
    }

    #[test]
    fn test_update_rebalance_status_to_complete() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        let plan = reg.create_rebalance_plan(
            PartitionId(8),
            "node-1".to_string(),
            "node-3".to_string(),
            "test".to_string(),
            512,
        );
        reg.update_rebalance_status(&plan.plan_id, RebalanceStatus::Complete);
        let plans = reg.rebalance_plans();
        let updated = plans.iter().find(|p| p.plan_id == plan.plan_id).unwrap();
        assert_eq!(updated.status, RebalanceStatus::Complete);
    }

    #[test]
    fn test_update_rebalance_status_to_failed() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        let plan = reg.create_rebalance_plan(
            PartitionId(9),
            "node-1".to_string(),
            "node-4".to_string(),
            "test".to_string(),
            256,
        );
        reg.update_rebalance_status(
            &plan.plan_id,
            RebalanceStatus::Failed("disk full".to_string()),
        );
        let plans = reg.rebalance_plans();
        let updated = plans.iter().find(|p| p.plan_id == plan.plan_id).unwrap();
        assert!(matches!(&updated.status, RebalanceStatus::Failed(msg) if msg == "disk full"));
        assert_eq!(reg.metrics().rebalance_failures_total, 1);
    }

    #[test]
    fn test_pending_rebalance_plans_excludes_complete() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        let p1 = reg.create_rebalance_plan(
            PartitionId(10),
            "node-1".to_string(),
            "node-2".to_string(),
            "r1".to_string(),
            100,
        );
        let p2 = reg.create_rebalance_plan(
            PartitionId(11),
            "node-1".to_string(),
            "node-3".to_string(),
            "r2".to_string(),
            200,
        );
        reg.update_rebalance_status(&p1.plan_id, RebalanceStatus::Complete);

        let pending = reg.pending_rebalance_plans();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].plan_id, p2.plan_id);
    }

    #[test]
    fn test_metrics_track_node_and_partition_counts() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        reg.register_node(oltp_node("node-1"));
        reg.register_node(olap_node("snap-1"));
        reg.register_node(olap_node("snap-2"));
        reg.register_placement(placement(1, "node-1", vec!["snap-1"]));
        reg.register_placement(placement(2, "node-1", vec!["snap-2"]));

        let m = reg.metrics();
        assert_eq!(m.partition_count, 2);
        assert_eq!(m.node_count, 3);
        assert_eq!(m.olap_snapshot_node_count, 2);
        assert_eq!(m.rebalances_total, 0);
        assert_eq!(m.rebalance_failures_total, 0);
        assert_eq!(m.routing_decisions_total, 0);
    }

    #[test]
    fn test_routing_decisions_counter_increments() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Hybrid);
        reg.route_query(PartitionId(1), false, false);
        reg.route_query(PartitionId(2), true, false);
        reg.route_query(PartitionId(3), false, true);
        assert_eq!(reg.metrics().routing_decisions_total, 3);
    }

    #[test]
    fn test_list_placements_returns_all() {
        let reg = PlacementRegistry::new("node-1".to_string(), NodeRole::Oltp);
        reg.register_placement(placement(20, "node-1", vec![]));
        reg.register_placement(placement(21, "node-1", vec![]));
        reg.register_placement(placement(22, "node-2", vec![]));
        assert_eq!(reg.list_placements().len(), 3);
    }

    #[test]
    fn test_route_write_to_remote_oltp_primary() {
        // local node is NOT the primary; write should be forwarded to primary
        let reg = PlacementRegistry::new("node-2".to_string(), NodeRole::Oltp);
        reg.register_placement(placement(30, "node-1", vec![]));

        let decision = reg.route_query(PartitionId(30), true, false);
        assert_eq!(decision.access_path, DistributedAccessPath::RemoteOltp);
        assert_eq!(decision.target_node_id, "node-1");
    }
}
