/// S9-002: Distributed/sharding behaviour prototype.
///
/// This module provides deterministic shard routing for both hash-based and
/// range-based partitioning strategies, without pulling in any external hashing
/// crates.

// ---------------------------------------------------------------------------
// ShardKey
// ---------------------------------------------------------------------------

/// Describes *how* rows are distributed across shards.
#[derive(Debug, Clone, PartialEq)]
pub enum ShardKey {
    /// Distribute rows by a consistent hash of a string key.
    Hash(String),
    /// Distribute rows by comparing a numeric column to a set of ranges.
    Range {
        column: String,
        min: i64,
        max: i64,
    },
    /// Distribute rows in round-robin order (stateless approximation via hash).
    RoundRobin,
}

// ---------------------------------------------------------------------------
// ShardConfig
// ---------------------------------------------------------------------------

/// Static configuration that drives the [`ShardRouter`].
#[derive(Debug, Clone)]
pub struct ShardConfig {
    /// Number of logical shards.
    pub shard_count: usize,
    /// How many replicas each shard keeps (informational; routing is unaffected).
    pub replication_factor: usize,
    /// Partitioning strategy.
    pub key: ShardKey,
}

// ---------------------------------------------------------------------------
// ShardRouter
// ---------------------------------------------------------------------------

/// Resolves which shard(s) a row or query targets.
#[derive(Debug)]
pub struct ShardRouter {
    config: ShardConfig,
}

impl ShardRouter {
    /// Create a new router for the given configuration.
    pub fn new(config: ShardConfig) -> Self {
        assert!(config.shard_count > 0, "shard_count must be > 0");
        Self { config }
    }

    /// Return the shard id for a single row identified by `row_key`.
    ///
    /// The mapping is *deterministic*: the same key always returns the same
    /// shard, regardless of call order or runtime.
    pub fn shard_for_row(&self, row_key: &str) -> usize {
        let h = fnv1a_hash(row_key);
        (h % self.config.shard_count as u64) as usize
    }

    /// Return all shards whose declared range *overlaps* the query range
    /// `[min, max]`.  For `Hash` and `RoundRobin` keys every shard is a
    /// candidate (scatter-gather).
    pub fn shards_for_range(&self, min: i64, max: i64) -> Vec<usize> {
        match &self.config.key {
            ShardKey::Range {
                min: shard_min,
                max: shard_max,
                ..
            } => {
                // If the query range overlaps the shard range, include all
                // shards (single-range config).  For a multi-shard range setup
                // the caller would iterate over multiple ShardConfigs; here we
                // model the simplest useful case.
                if max < *shard_min || min > *shard_max {
                    vec![]
                } else {
                    self.all_shards()
                }
            }
            // For hash/round-robin partitioning a range query must fan out.
            _ => self.all_shards(),
        }
    }

    /// Return all shard ids `[0, shard_count)`.
    pub fn all_shards(&self) -> Vec<usize> {
        (0..self.config.shard_count).collect()
    }
}

// ---------------------------------------------------------------------------
// fnv1a_hash  (no external deps)
// ---------------------------------------------------------------------------

/// FNV-1a 64-bit hash — fast, deterministic, no external dependencies.
pub fn fnv1a_hash(s: &str) -> u64 {
    const OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;
    const FNV_PRIME: u64 = 1_099_511_628_211;

    let mut hash = OFFSET_BASIS;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_router(n: usize) -> ShardRouter {
        ShardRouter::new(ShardConfig {
            shard_count: n,
            replication_factor: 1,
            key: ShardKey::Hash("id".into()),
        })
    }

    #[test]
    fn test_shard_router_hash_is_deterministic() {
        let router = hash_router(8);
        let key = "user-42";
        let first = router.shard_for_row(key);
        // Call many times — must always return the same value.
        for _ in 0..100 {
            assert_eq!(router.shard_for_row(key), first);
        }
        // Sanity: result is within [0, shard_count).
        assert!(first < 8);
    }

    #[test]
    fn test_shard_router_range_returns_all_shards() {
        let router = ShardRouter::new(ShardConfig {
            shard_count: 4,
            replication_factor: 1,
            key: ShardKey::Range {
                column: "ts".into(),
                min: 0,
                max: 1_000_000,
            },
        });
        // A query that overlaps the shard range should get all shards.
        let shards = router.shards_for_range(100, 500);
        assert_eq!(shards, vec![0, 1, 2, 3]);

        // A query entirely outside the shard range should get nothing.
        let shards_outside = router.shards_for_range(2_000_000, 3_000_000);
        assert!(shards_outside.is_empty());
    }

    #[test]
    fn test_shard_router_round_robin_distributes() {
        let router = ShardRouter::new(ShardConfig {
            shard_count: 4,
            replication_factor: 1,
            key: ShardKey::RoundRobin,
        });
        // With enough distinct keys, more than one shard should be hit.
        let keys: Vec<String> = (0..40).map(|i| format!("key-{i}")).collect();
        let mut seen = std::collections::HashSet::new();
        for k in &keys {
            seen.insert(router.shard_for_row(k));
        }
        assert!(
            seen.len() > 1,
            "round-robin should distribute across shards, got {seen:?}"
        );
    }

    #[test]
    fn test_shard_count_boundary() {
        // shard_count = 1 — every key maps to shard 0.
        let router = hash_router(1);
        for i in 0..50 {
            assert_eq!(router.shard_for_row(&format!("k{i}")), 0);
        }

        // Large shard count — result must still be < shard_count.
        let big = hash_router(1024);
        for i in 0..200 {
            let s = big.shard_for_row(&format!("user-{i}"));
            assert!(s < 1024, "shard {s} >= 1024");
        }
    }
}
