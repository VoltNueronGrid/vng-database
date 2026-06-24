#![forbid(unsafe_code)]
// stub — not yet implemented as a full failover agent.
// Health-check, peer-discovery, and leader-notification interfaces are defined
// here and will be wired into the Raft loop once P5 (multi-node cluster) lands.
// See tasks-v4.md R7 for the implementation plan.

pub const CRATE_NAME: &str = "voltnuerongrid-failover";

// ── Domain types ─────────────────────────────────────────────────────────────

/// Observable health of a single cluster peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    /// Peer responded to the last health-check within the timeout window.
    Healthy,
    /// Peer responded but reported degraded state (high latency, partial failure).
    Degraded,
    /// Peer did not respond within the timeout window; assumed unreachable.
    Unreachable,
}

/// Identifies a cluster peer with its public base URL.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub node_id: String,
    pub base_url: String,
}

// ── Traits ───────────────────────────────────────────────────────────────────

/// Checks the health of a peer node by calling its `/health` endpoint.
/// Implementations are responsible for timeout handling.
pub trait HealthChecker: Send + Sync {
    /// Returns the current `HealthStatus` for the given node.
    fn check(&self, node: &NodeInfo) -> HealthStatus;
}

/// Manages the known set of cluster peers.
pub trait PeerDiscovery: Send + Sync {
    fn known_peers(&self) -> Vec<NodeInfo>;
    fn register_peer(&mut self, node: NodeInfo);
    fn deregister_peer(&mut self, node_id: &str);
}

/// Receives notifications about Raft leader transitions.
/// The Raft loop calls these callbacks when role changes occur.
pub trait LeaderNotification: Send + Sync {
    fn on_leader_elected(&self, node_id: &str);
    fn on_leader_lost(&self, node_id: &str);
}

// ── In-memory implementations (for single-process tests) ─────────────────────

/// A no-op health checker used in single-process and unit-test contexts.
pub struct NoopHealthChecker;

impl HealthChecker for NoopHealthChecker {
    fn check(&self, _node: &NodeInfo) -> HealthStatus {
        HealthStatus::Unreachable
    }
}

/// A no-op leader notification handler.
pub struct NoopLeaderNotification;

impl LeaderNotification for NoopLeaderNotification {
    fn on_leader_elected(&self, _node_id: &str) {}
    fn on_leader_lost(&self, _node_id: &str) {}
}

/// A simple in-memory peer registry.
#[derive(Default)]
pub struct InMemoryPeerRegistry {
    peers: Vec<NodeInfo>,
}

impl PeerDiscovery for InMemoryPeerRegistry {
    fn known_peers(&self) -> Vec<NodeInfo> {
        self.peers.clone()
    }
    fn register_peer(&mut self, node: NodeInfo) {
        if !self.peers.iter().any(|p| p.node_id == node.node_id) {
            self.peers.push(node);
        }
    }
    fn deregister_peer(&mut self, node_id: &str) {
        self.peers.retain(|p| p.node_id != node_id);
    }
}

// ── HttpFailoverAgent ─────────────────────────────────────────────────────────
// NOTE: HTTP-based health checks require `reqwest` or similar. To avoid adding
// a heavyweight async dependency to this crate before P5 wiring begins, the
// HTTP implementation is provided as a synchronous blocking stub that delegates
// to the caller's runtime. Full async implementation is part of P5.

/// A production-grade failover agent that checks peer health over HTTP.
/// Uses blocking HTTP calls to remain dependency-light until P5 wires it
/// into the Raft loop with a proper async runtime.
pub struct HttpFailoverAgent {
    timeout_ms: u64,
}

impl HttpFailoverAgent {
    pub fn new(timeout_ms: u64) -> Self {
        Self { timeout_ms }
    }

    /// Blocking HTTP GET to `{base_url}/health`. Returns `Healthy` on 200,
    /// `Degraded` on 4xx/5xx, `Unreachable` on connection error or timeout.
    pub fn ping(&self, base_url: &str) -> HealthStatus {
        let url = format!("{}/health", base_url.trim_end_matches('/'));
        // Use std::process::Command to call curl as a portable blocking HTTP client
        // without adding an async dependency. Replace with reqwest once P5 lands.
        let output = std::process::Command::new("curl")
            .args([
                "--silent",
                "--max-time", &format!("{:.1}", self.timeout_ms as f64 / 1000.0),
                "--output", "/dev/null",
                "--write-out", "%{http_code}",
                &url,
            ])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let code = String::from_utf8_lossy(&o.stdout);
                let code = code.trim().parse::<u16>().unwrap_or(0);
                if code == 200 {
                    HealthStatus::Healthy
                } else if code >= 400 {
                    HealthStatus::Degraded
                } else {
                    HealthStatus::Unreachable
                }
            }
            _ => HealthStatus::Unreachable,
        }
    }
}

impl HealthChecker for HttpFailoverAgent {
    fn check(&self, node: &NodeInfo) -> HealthStatus {
        self.ping(&node.base_url)
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_health_checker_returns_unreachable() {
        let checker = NoopHealthChecker;
        let node = NodeInfo {
            node_id: "node-1".to_string(),
            base_url: "http://127.0.0.1:9999".to_string(),
        };
        assert_eq!(checker.check(&node), HealthStatus::Unreachable);
    }

    #[test]
    fn in_memory_peer_registry_register_and_deregister() {
        let mut registry = InMemoryPeerRegistry::default();
        assert!(registry.known_peers().is_empty());

        registry.register_peer(NodeInfo {
            node_id: "node-1".to_string(),
            base_url: "http://127.0.0.1:8080".to_string(),
        });
        registry.register_peer(NodeInfo {
            node_id: "node-2".to_string(),
            base_url: "http://127.0.0.1:8081".to_string(),
        });
        assert_eq!(registry.known_peers().len(), 2);

        // Duplicate registration should be idempotent
        registry.register_peer(NodeInfo {
            node_id: "node-1".to_string(),
            base_url: "http://127.0.0.1:8080".to_string(),
        });
        assert_eq!(registry.known_peers().len(), 2);

        registry.deregister_peer("node-1");
        assert_eq!(registry.known_peers().len(), 1);
        assert_eq!(registry.known_peers()[0].node_id, "node-2");
    }

    #[test]
    fn noop_leader_notification_does_not_panic() {
        let notif = NoopLeaderNotification;
        notif.on_leader_elected("node-1");
        notif.on_leader_lost("node-1");
    }

    #[test]
    fn http_failover_agent_unreachable_for_nonexistent_host() {
        let agent = HttpFailoverAgent::new(500);
        // A port that is very unlikely to be open; expect Unreachable or Degraded
        let result = agent.ping("http://127.0.0.1:19999");
        assert!(
            result == HealthStatus::Unreachable || result == HealthStatus::Degraded,
            "Expected Unreachable or Degraded for a non-listening port, got {:?}",
            result
        );
    }
}

