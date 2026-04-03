#![forbid(unsafe_code)]

pub const CRATE_NAME: &str = "voltnuerongrid-opt";

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// CacheEntry
// ---------------------------------------------------------------------------

pub struct CacheEntry<V> {
    pub value: V,
    pub created_at_ms: u64,
    pub last_accessed_ms: u64,
    pub access_count: u64,
    pub ttl_ms: Option<u64>,
    pub partition_key: String,
}

impl<V> CacheEntry<V> {
    pub fn new(value: V, partition_key: String, ttl_ms: Option<u64>, now_ms: u64) -> Self {
        CacheEntry {
            value,
            created_at_ms: now_ms,
            last_accessed_ms: now_ms,
            access_count: 0,
            ttl_ms,
            partition_key,
        }
    }

    /// Returns true if ttl is set AND (now_ms - created_at_ms) > ttl_ms.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        if let Some(ttl) = self.ttl_ms {
            now_ms.saturating_sub(self.created_at_ms) > ttl
        } else {
            false
        }
    }

    pub fn touch(&mut self, now_ms: u64) {
        self.last_accessed_ms = now_ms;
        self.access_count += 1;
    }
}

// ---------------------------------------------------------------------------
// EvictionPolicy
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub enum EvictionPolicy {
    Lru,
    Lfu,
    Ttl,
}

// ---------------------------------------------------------------------------
// CacheResiliencePolicy
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct CacheResiliencePolicy {
    pub eviction_policy: EvictionPolicy,
    pub default_ttl_ms: Option<u64>,
    pub max_entries_per_partition: usize,
    pub circuit_breaker_failure_threshold: u32,
    pub circuit_breaker_half_open_after_ms: u64,
}

impl Default for CacheResiliencePolicy {
    fn default() -> Self {
        CacheResiliencePolicy {
            eviction_policy: EvictionPolicy::Lru,
            default_ttl_ms: Some(300_000),
            max_entries_per_partition: 10_000,
            circuit_breaker_failure_threshold: 5,
            circuit_breaker_half_open_after_ms: 30_000,
        }
    }
}

// ---------------------------------------------------------------------------
// CircuitBreakerState
// ---------------------------------------------------------------------------

pub enum CircuitBreakerState {
    Closed,
    Open { opened_at_ms: u64 },
    HalfOpen,
}

// ---------------------------------------------------------------------------
// CacheError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum CacheError {
    CircuitOpen { partition_id: String },
    PartitionNotFound { partition_id: String },
    SerializationError(String),
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::CircuitOpen { partition_id } => {
                write!(f, "Circuit breaker open for partition '{}'", partition_id)
            }
            CacheError::PartitionNotFound { partition_id } => {
                write!(f, "Partition '{}' not found", partition_id)
            }
            CacheError::SerializationError(msg) => {
                write!(f, "Serialization error: {}", msg)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CacheRebalanceResult
// ---------------------------------------------------------------------------

pub struct CacheRebalanceResult {
    pub partition_id: String,
    pub entries_before: usize,
    pub entries_evicted: usize,
    pub entries_after: usize,
    pub rebalanced_at_ms: u64,
}

// ---------------------------------------------------------------------------
// CachePartitionStats
// ---------------------------------------------------------------------------

pub struct CachePartitionStats {
    pub partition_id: String,
    pub entry_count: usize,
    pub total_hits: u64,
    pub total_misses: u64,
    pub total_evictions: u64,
    pub circuit_breaker_state: String,
    pub hit_ratio: f64,
    pub last_rebalance_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// CachePartition
// ---------------------------------------------------------------------------

pub struct CachePartition {
    pub partition_id: String,
    entries: HashMap<String, CacheEntry<serde_json::Value>>,
    pub circuit_breaker: CircuitBreakerState,
    pub consecutive_failures: u32,
    pub policy: CacheResiliencePolicy,
    pub total_hits: u64,
    pub total_misses: u64,
    pub total_evictions: u64,
    pub last_rebalance_ms: Option<u64>,
}

impl CachePartition {
    pub fn new(partition_id: String, policy: CacheResiliencePolicy) -> Self {
        CachePartition {
            partition_id,
            entries: HashMap::new(),
            circuit_breaker: CircuitBreakerState::Closed,
            consecutive_failures: 0,
            policy,
            total_hits: 0,
            total_misses: 0,
            total_evictions: 0,
            last_rebalance_ms: None,
        }
    }

    /// Get an entry by key; evicts the entry if it is expired.
    pub fn get(&mut self, key: &str, now_ms: u64) -> Option<&serde_json::Value> {
        // Separate borrow: check expiry first.
        let is_expired = self
            .entries
            .get(key)
            .map(|e| e.is_expired(now_ms))
            .unwrap_or(false);

        if is_expired {
            self.entries.remove(key);
            self.total_misses += 1;
            return None;
        }

        if let Some(entry) = self.entries.get_mut(key) {
            entry.touch(now_ms);
            self.total_hits += 1;
        } else {
            self.total_misses += 1;
            return None;
        }

        self.entries.get(key).map(|e| &e.value)
    }

    /// Insert a value.  Returns `Err(CircuitOpen)` when the circuit breaker is open.
    pub fn set(
        &mut self,
        key: String,
        value: serde_json::Value,
        ttl_ms: Option<u64>,
        now_ms: u64,
    ) -> Result<(), CacheError> {
        if self.circuit_is_open(now_ms) {
            return Err(CacheError::CircuitOpen {
                partition_id: self.partition_id.clone(),
            });
        }
        // If we passed the open-check while still in Open state, transition to HalfOpen.
        if let CircuitBreakerState::Open { .. } = self.circuit_breaker {
            self.circuit_breaker = CircuitBreakerState::HalfOpen;
        }

        let effective_ttl = ttl_ms.or(self.policy.default_ttl_ms);
        self.evict_lru_if_full(now_ms);
        let entry = CacheEntry::new(value, self.partition_id.clone(), effective_ttl, now_ms);
        self.entries.insert(key, entry);
        Ok(())
    }

    /// Remove a single key; returns true if the key existed.
    pub fn invalidate(&mut self, key: &str) -> bool {
        self.entries.remove(key).is_some()
    }

    /// Clear all entries; returns the number of entries removed.
    pub fn invalidate_all(&mut self) -> usize {
        let count = self.entries.len();
        self.entries.clear();
        count
    }

    /// Remove all expired entries; returns the count evicted.
    pub fn evict_expired(&mut self, now_ms: u64) -> usize {
        let expired_keys: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, e)| e.is_expired(now_ms))
            .map(|(k, _)| k.clone())
            .collect();
        let count = expired_keys.len();
        for key in expired_keys {
            self.entries.remove(&key);
        }
        self.total_evictions += count as u64;
        count
    }

    /// Evict the LRU entry if the partition is at its max capacity.
    pub fn evict_lru_if_full(&mut self, now_ms: u64) {
        if self.entries.len() < self.policy.max_entries_per_partition {
            return;
        }
        // First pass: remove expired entries.
        self.evict_expired(now_ms);
        // Second pass: if still at capacity, remove the least-recently-accessed entry.
        if self.entries.len() >= self.policy.max_entries_per_partition {
            let lru_key = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_accessed_ms)
                .map(|(k, _)| k.clone());
            if let Some(key) = lru_key {
                self.entries.remove(&key);
                self.total_evictions += 1;
            }
        }
    }

    /// Record a backend/downstream failure; opens the circuit if threshold is reached.
    pub fn record_failure(&mut self, now_ms: u64) {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= self.policy.circuit_breaker_failure_threshold {
            self.circuit_breaker = CircuitBreakerState::Open {
                opened_at_ms: now_ms,
            };
        }
    }

    /// Record a successful operation; resets failure counter and closes the circuit.
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.circuit_breaker = CircuitBreakerState::Closed;
    }

    /// Returns true when the circuit is Open AND the half-open cooldown has NOT elapsed yet.
    pub fn circuit_is_open(&self, now_ms: u64) -> bool {
        if let CircuitBreakerState::Open { opened_at_ms } = &self.circuit_breaker {
            let elapsed = now_ms.saturating_sub(*opened_at_ms);
            elapsed < self.policy.circuit_breaker_half_open_after_ms
        } else {
            false
        }
    }

    /// Evict all expired entries from this partition and record the timestamp.
    pub fn rebalance(&mut self, now_ms: u64) -> CacheRebalanceResult {
        let entries_before = self.entries.len();
        let entries_evicted = self.evict_expired(now_ms);
        let entries_after = self.entries.len();
        self.last_rebalance_ms = Some(now_ms);
        CacheRebalanceResult {
            partition_id: self.partition_id.clone(),
            entries_before,
            entries_evicted,
            entries_after,
            rebalanced_at_ms: now_ms,
        }
    }

    pub fn stats(&self) -> CachePartitionStats {
        let cb_state = match &self.circuit_breaker {
            CircuitBreakerState::Closed => "closed".to_string(),
            CircuitBreakerState::Open { .. } => "open".to_string(),
            CircuitBreakerState::HalfOpen => "half_open".to_string(),
        };
        let total = self.total_hits + self.total_misses;
        let hit_ratio = if total == 0 {
            0.0
        } else {
            self.total_hits as f64 / total as f64
        };
        CachePartitionStats {
            partition_id: self.partition_id.clone(),
            entry_count: self.entries.len(),
            total_hits: self.total_hits,
            total_misses: self.total_misses,
            total_evictions: self.total_evictions,
            circuit_breaker_state: cb_state,
            hit_ratio,
            last_rebalance_ms: self.last_rebalance_ms,
        }
    }
}

// ---------------------------------------------------------------------------
// DistributedCacheManager
// ---------------------------------------------------------------------------

pub struct DistributedCacheManager {
    partitions: HashMap<String, CachePartition>,
    default_policy: CacheResiliencePolicy,
}

impl DistributedCacheManager {
    pub fn new(default_policy: CacheResiliencePolicy) -> Self {
        DistributedCacheManager {
            partitions: HashMap::new(),
            default_policy,
        }
    }

    pub fn with_default_policy() -> Self {
        Self::new(CacheResiliencePolicy::default())
    }

    /// Return a mutable reference to a partition, creating it (with the default policy) if absent.
    pub fn ensure_partition(&mut self, partition_id: &str) -> &mut CachePartition {
        if !self.partitions.contains_key(partition_id) {
            let policy = self.default_policy.clone();
            self.partitions
                .insert(partition_id.to_string(), CachePartition::new(partition_id.to_string(), policy));
        }
        self.partitions.get_mut(partition_id).unwrap()
    }

    /// Retrieve a value (cloned) from a partition.  Returns `Err(PartitionNotFound)` when the
    /// partition does not exist.
    pub fn get(
        &mut self,
        partition_id: &str,
        key: &str,
        now_ms: u64,
    ) -> Result<Option<serde_json::Value>, CacheError> {
        let partition = self
            .partitions
            .get_mut(partition_id)
            .ok_or_else(|| CacheError::PartitionNotFound {
                partition_id: partition_id.to_string(),
            })?;
        Ok(partition.get(key, now_ms).cloned())
    }

    pub fn set(
        &mut self,
        partition_id: &str,
        key: String,
        value: serde_json::Value,
        ttl_ms: Option<u64>,
        now_ms: u64,
    ) -> Result<(), CacheError> {
        let partition = self.ensure_partition(partition_id);
        partition.set(key, value, ttl_ms, now_ms)
    }

    pub fn invalidate(
        &mut self,
        partition_id: &str,
        key: &str,
    ) -> Result<bool, CacheError> {
        let partition = self
            .partitions
            .get_mut(partition_id)
            .ok_or_else(|| CacheError::PartitionNotFound {
                partition_id: partition_id.to_string(),
            })?;
        Ok(partition.invalidate(key))
    }

    pub fn invalidate_partition(&mut self, partition_id: &str) -> Result<usize, CacheError> {
        let partition = self
            .partitions
            .get_mut(partition_id)
            .ok_or_else(|| CacheError::PartitionNotFound {
                partition_id: partition_id.to_string(),
            })?;
        Ok(partition.invalidate_all())
    }

    pub fn rebalance_all(&mut self, now_ms: u64) -> Vec<CacheRebalanceResult> {
        self.partitions.values_mut().map(|p| p.rebalance(now_ms)).collect()
    }

    pub fn partition_count(&self) -> usize {
        self.partitions.len()
    }

    pub fn all_stats(&self) -> Vec<CachePartitionStats> {
        self.partitions.values().map(|p| p.stats()).collect()
    }

    pub fn total_entry_count(&self) -> usize {
        self.partitions.values().map(|p| p.entries.len()).sum()
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_entry_ttl_expiry() {
        // TTL = 100 ms, created at t=1000
        let entry: CacheEntry<u64> =
            CacheEntry::new(42u64, "p1".to_string(), Some(100), 1000u64);
        // 50 ms elapsed — not expired
        assert!(!entry.is_expired(1050u64));
        // exactly at TTL — still not expired (uses strict >)
        assert!(!entry.is_expired(1100u64));
        // 101 ms elapsed — expired
        assert!(entry.is_expired(1101u64));
    }

    #[test]
    fn test_cache_partition_hit_miss() {
        let mut partition =
            CachePartition::new("p1".to_string(), CacheResiliencePolicy::default());

        partition
            .set(
                "key1".to_string(),
                serde_json::json!("value1"),
                None,
                1000u64,
            )
            .unwrap();

        // Hit
        let val = partition.get("key1", 2000u64);
        assert!(val.is_some());

        // Miss
        let miss = partition.get("no_such_key", 2000u64);
        assert!(miss.is_none());

        let stats = partition.stats();
        assert_eq!(stats.total_hits, 1);
        assert_eq!(stats.total_misses, 1);
        assert_eq!(stats.hit_ratio, 0.5);
    }

    #[test]
    fn test_cache_partition_circuit_breaker_opens() {
        let policy = CacheResiliencePolicy {
            circuit_breaker_failure_threshold: 3,
            ..Default::default()
        };
        let mut partition = CachePartition::new("p1".to_string(), policy);

        partition.record_failure(1000u64);
        partition.record_failure(2000u64);
        // Two failures — below threshold of 3, circuit still closed.
        assert!(!partition.circuit_is_open(2000u64));

        partition.record_failure(3000u64);
        // Third failure reaches threshold — circuit opens.
        assert!(partition.circuit_is_open(3000u64));

        // set must be rejected while circuit is open.
        let result = partition.set("k".to_string(), serde_json::json!(1), None, 3000u64);
        assert!(matches!(result, Err(CacheError::CircuitOpen { .. })));
    }

    #[test]
    fn test_cache_partition_circuit_breaker_half_open() {
        let policy = CacheResiliencePolicy {
            circuit_breaker_failure_threshold: 2,
            circuit_breaker_half_open_after_ms: 1000,
            ..Default::default()
        };
        let mut partition = CachePartition::new("p1".to_string(), policy);

        // Open the circuit (opened_at_ms = 2000).
        partition.record_failure(1000u64);
        partition.record_failure(2000u64);
        assert!(partition.circuit_is_open(2000u64));
        assert!(partition.circuit_is_open(2999u64)); // 999 ms — still within cooldown

        // Past the half-open threshold: 1001 ms elapsed after open.
        let future_ms = 3001u64;
        assert!(!partition.circuit_is_open(future_ms));

        // A set attempt at this point triggers the Open → HalfOpen transition.
        partition
            .set("k".to_string(), serde_json::json!(1), None, future_ms)
            .unwrap();

        assert_eq!(partition.stats().circuit_breaker_state, "half_open");
    }

    #[test]
    fn test_cache_partition_eviction_on_full() {
        let policy = CacheResiliencePolicy {
            max_entries_per_partition: 2,
            default_ttl_ms: None,
            ..Default::default()
        };
        let mut partition = CachePartition::new("p1".to_string(), policy);

        // Insert "a" at t=1000, then access it at t=2000 to update last_accessed.
        partition
            .set("a".to_string(), serde_json::json!(1), None, 1000u64)
            .unwrap();
        let _ = partition.get("a", 2000u64); // last_accessed_ms for "a" = 2000

        // Insert "b" at t=3000 (last_accessed_ms = 3000); capacity not yet reached.
        partition
            .set("b".to_string(), serde_json::json!(2), None, 3000u64)
            .unwrap();

        // Adding "c" triggers LRU eviction: "a" (last_accessed=2000) < "b" (3000) → "a" evicted.
        partition
            .set("c".to_string(), serde_json::json!(3), None, 4000u64)
            .unwrap();

        assert!(partition.get("a", 5000u64).is_none()); // evicted
        assert!(partition.get("b", 5000u64).is_some());
        assert!(partition.get("c", 5000u64).is_some());
    }

    #[test]
    fn test_distributed_cache_manager_rebalance() {
        let mut mgr = DistributedCacheManager::with_default_policy();

        // Set entry with an explicit 100 ms TTL.
        mgr.set(
            "part1",
            "key1".to_string(),
            serde_json::json!("val"),
            Some(100),
            1000u64,
        )
        .unwrap();

        // Entry exists 50 ms later.
        let v = mgr.get("part1", "key1", 1050u64).unwrap();
        assert!(v.is_some());

        // Rebalance 200 ms after creation — TTL has elapsed.
        let results = mgr.rebalance_all(1200u64);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entries_evicted, 1);

        // Entry is gone post-rebalance.
        let v2 = mgr.get("part1", "key1", 1200u64).unwrap();
        assert!(v2.is_none());
    }

    #[test]
    fn test_distributed_cache_manager_circuit_open_blocks_set() {
        let policy = CacheResiliencePolicy {
            circuit_breaker_failure_threshold: 2,
            ..Default::default()
        };
        let mut mgr = DistributedCacheManager::new(policy);

        // Create partition and drive the circuit open.
        {
            let part = mgr.ensure_partition("part1");
            part.record_failure(1000u64);
            part.record_failure(2000u64); // threshold reached — circuit opens
        }

        // Manager set must propagate CircuitOpen.
        let result = mgr.set(
            "part1",
            "k".to_string(),
            serde_json::json!(1),
            None,
            2000u64,
        );
        assert!(matches!(result, Err(CacheError::CircuitOpen { .. })));
    }
}
