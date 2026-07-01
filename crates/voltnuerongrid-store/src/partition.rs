//! Partition and segment routing for HTAP storage.
//!
//! Supports RANGE and HASH partitioning schemes to route writes to
//! segment-specific tail stores and prune segments during scans.

use crate::types::SegmentId;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Partition type and scheme definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartitionType {
    /// No partitioning — all rows in default segment (SegmentId(0)).
    None,
    /// Range-based partitioning by ordered column values.
    Range(RangePartitionScheme),
    /// Hash-based partitioning into deterministic buckets.
    Hash(HashPartitionScheme),
}

/// RANGE partitioning scheme with ordered boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangePartitionScheme {
    /// Ordered list of range partitions.
    pub ranges: Vec<RangePartition>,
}

/// A single range partition with inclusive lower and exclusive upper bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangePartition {
    /// The segment ID this range maps to.
    pub segment_id: SegmentId,
    /// Minimum value (inclusive). None means unbounded below.
    pub lower_bound: Option<String>,
    /// Maximum value (exclusive). None means unbounded above.
    pub upper_bound: Option<String>,
}

/// HASH partitioning scheme with deterministic bucket assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashPartitionScheme {
    /// Number of hash buckets.
    pub num_buckets: u32,
    /// Segment IDs corresponding to each bucket (0 to num_buckets-1).
    pub segments: Vec<SegmentId>,
}

/// Routes rows to appropriate segments based on partition scheme.
#[derive(Debug, Clone)]
pub struct SegmentRouter {
    /// The partition type and scheme for this table.
    pub partition_type: PartitionType,
}

impl SegmentRouter {
    /// Create a new router with no partitioning (all rows → SegmentId(0)).
    pub fn new_unpartitioned() -> Self {
        SegmentRouter {
            partition_type: PartitionType::None,
        }
    }

    /// Create a new router for RANGE partitioning.
    pub fn new_range(scheme: RangePartitionScheme) -> Self {
        SegmentRouter {
            partition_type: PartitionType::Range(scheme),
        }
    }

    /// Create a new router for HASH partitioning.
    pub fn new_hash(scheme: HashPartitionScheme) -> Self {
        SegmentRouter {
            partition_type: PartitionType::Hash(scheme),
        }
    }

    /// Route a partition key value to the appropriate segment.
    ///
    /// # Arguments
    /// - `partition_key`: The value to be routed (as a string)
    /// - `_partition_cols`: Column names (for documentation; not currently used)
    ///
    /// # Returns
    /// - `Ok(SegmentId)` on successful routing
    /// - `Err(String)` if the value is out of range or routing fails
    pub fn route_to_segment(
        &self,
        partition_key: &str,
        _partition_cols: &[String],
    ) -> Result<SegmentId, String> {
        match &self.partition_type {
            PartitionType::None => {
                // Single default segment for non-partitioned tables
                Ok(SegmentId(0))
            }
            PartitionType::Range(scheme) => {
                // Find the range partition containing partition_key
                for part in &scheme.ranges {
                    if self.value_in_range(partition_key, &part.lower_bound, &part.upper_bound) {
                        return Ok(part.segment_id);
                    }
                }
                Err(format!("Value '{}' out of range", partition_key))
            }
            PartitionType::Hash(scheme) => {
                // Hash partition_key into bucket
                if scheme.segments.is_empty() {
                    return Err("Hash partition has no segments".to_string());
                }
                let hash = self.hash_value(partition_key);
                let bucket = (hash % scheme.num_buckets as u64) as usize;
                Ok(scheme.segments[bucket])
            }
        }
    }

    /// Check if a value falls within a range (inclusive lower, exclusive upper).
    fn value_in_range(
        &self,
        val: &str,
        lower: &Option<String>,
        upper: &Option<String>,
    ) -> bool {
        if let Some(ref l) = lower {
            if val < l.as_str() {
                return false;
            }
        }
        if let Some(ref u) = upper {
            if val >= u.as_str() {
                return false;
            }
        }
        true
    }

    /// Compute a deterministic hash for a string value.
    /// Uses DefaultHasher for consistency across calls.
    fn hash_value(&self, val: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        val.hash(&mut hasher);
        hasher.finish()
    }

    /// List segments that could contain rows matching the given predicate.
    ///
    /// For RANGE partitions, only segments whose ranges overlap with the
    /// predicate bounds are included. For HASH partitions, all segments
    /// are returned (no pruning possible without exact values).
    pub fn prune_segments(&self, lower_bound: Option<&str>, upper_bound: Option<&str>) -> Vec<SegmentId> {
        match &self.partition_type {
            PartitionType::None => vec![SegmentId(0)],
            PartitionType::Range(scheme) => {
                let mut result = Vec::new();
                for part in &scheme.ranges {
                    // Check if this partition's range overlaps with [lower_bound, upper_bound)
                    if Self::ranges_overlap(
                        &part.lower_bound,
                        &part.upper_bound,
                        lower_bound,
                        upper_bound,
                    ) {
                        result.push(part.segment_id);
                    }
                }
                if result.is_empty() {
                    // If no overlap, return all segments as fallback
                    scheme.ranges.iter().map(|p| p.segment_id).collect()
                } else {
                    result
                }
            }
            PartitionType::Hash(scheme) => {
                // Hash partitions cannot be pruned without exact values
                scheme.segments.clone()
            }
        }
    }

    /// Check if two ranges overlap.
    /// Range 1: [r1_lower, r1_upper)
    /// Range 2: [r2_lower, r2_upper)
    fn ranges_overlap(
        r1_lower: &Option<String>,
        r1_upper: &Option<String>,
        r2_lower: Option<&str>,
        r2_upper: Option<&str>,
    ) -> bool {
        // Check if r1_upper <= r2_lower (no overlap)
        if let (Some(ref r1u), Some(r2l)) = (r1_upper, r2_lower) {
            if r1u.as_str() <= r2l {
                return false;
            }
        }
        // Check if r2_upper <= r1_lower (no overlap)
        if let (Some(r2u), Some(ref r1l)) = (r2_upper, r1_lower) {
            if r2u <= r1l.as_str() {
                return false;
            }
        }
        // Ranges overlap
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unpartitioned_router_uses_default_segment() {
        let router = SegmentRouter::new_unpartitioned();
        assert_eq!(
            router.route_to_segment("any_value", &[]).unwrap(),
            SegmentId(0)
        );
        assert_eq!(
            router.route_to_segment("another_value", &[]).unwrap(),
            SegmentId(0)
        );
    }

    #[test]
    fn test_range_partition_routing_basic() {
        let scheme = RangePartitionScheme {
            ranges: vec![
                RangePartition {
                    segment_id: SegmentId(0),
                    lower_bound: None,
                    upper_bound: Some("00500".to_string()),
                },
                RangePartition {
                    segment_id: SegmentId(1),
                    lower_bound: Some("00500".to_string()),
                    upper_bound: None,
                },
            ],
        };
        let router = SegmentRouter::new_range(scheme);

        // Below 500
        assert_eq!(router.route_to_segment("00100", &[]).unwrap(), SegmentId(0));
        assert_eq!(router.route_to_segment("00499", &[]).unwrap(), SegmentId(0));

        // At 500 and above
        assert_eq!(router.route_to_segment("00500", &[]).unwrap(), SegmentId(1));
        assert_eq!(router.route_to_segment("00600", &[]).unwrap(), SegmentId(1));
        assert_eq!(
            router.route_to_segment("999999", &[]).unwrap(),
            SegmentId(1)
        );
    }

    #[test]
    fn test_range_partition_routing_three_ranges() {
        let scheme = RangePartitionScheme {
            ranges: vec![
                RangePartition {
                    segment_id: SegmentId(0),
                    lower_bound: None,
                    upper_bound: Some("01000".to_string()),
                },
                RangePartition {
                    segment_id: SegmentId(1),
                    lower_bound: Some("01000".to_string()),
                    upper_bound: Some("05000".to_string()),
                },
                RangePartition {
                    segment_id: SegmentId(2),
                    lower_bound: Some("05000".to_string()),
                    upper_bound: None,
                },
            ],
        };
        let router = SegmentRouter::new_range(scheme);

        assert_eq!(router.route_to_segment("00500", &[]).unwrap(), SegmentId(0));
        assert_eq!(router.route_to_segment("01000", &[]).unwrap(), SegmentId(1));
        assert_eq!(router.route_to_segment("03000", &[]).unwrap(), SegmentId(1));
        assert_eq!(router.route_to_segment("04999", &[]).unwrap(), SegmentId(1));
        assert_eq!(router.route_to_segment("05000", &[]).unwrap(), SegmentId(2));
        assert_eq!(router.route_to_segment("10000", &[]).unwrap(), SegmentId(2));
    }

    #[test]
    fn test_range_partition_out_of_bounds_error() {
        let scheme = RangePartitionScheme {
            ranges: vec![
                RangePartition {
                    segment_id: SegmentId(0),
                    lower_bound: Some("00100".to_string()),
                    upper_bound: Some("00200".to_string()),
                },
            ],
        };
        let router = SegmentRouter::new_range(scheme);

        // Below range
        let err = router.route_to_segment("00050", &[]);
        assert!(err.is_err());

        // Above range
        let err = router.route_to_segment("00300", &[]);
        assert!(err.is_err());

        // Within range
        assert_eq!(router.route_to_segment("00150", &[]).unwrap(), SegmentId(0));
    }

    #[test]
    fn test_hash_partition_routing_deterministic() {
        let scheme = HashPartitionScheme {
            num_buckets: 4,
            segments: vec![SegmentId(0), SegmentId(1), SegmentId(2), SegmentId(3)],
        };
        let router = SegmentRouter::new_hash(scheme);

        // Same key always routes to same segment
        let seg_a1 = router.route_to_segment("key_a", &[]).unwrap();
        let seg_a2 = router.route_to_segment("key_a", &[]).unwrap();
        assert_eq!(seg_a1, seg_a2);

        // Different keys may route differently
        let seg_b = router.route_to_segment("key_b", &[]).unwrap();
        let seg_c = router.route_to_segment("key_c", &[]).unwrap();

        // All should be in valid range
        assert!(seg_a1.0 < 4);
        assert!(seg_b.0 < 4);
        assert!(seg_c.0 < 4);
    }

    #[test]
    fn test_hash_partition_uniform_distribution() {
        let scheme = HashPartitionScheme {
            num_buckets: 8,
            segments: (0..8).map(SegmentId).collect(),
        };
        let router = SegmentRouter::new_hash(scheme);

        // Route many keys and verify they distribute
        let mut bucket_counts = vec![0; 8];
        for i in 0..1000 {
            let key = format!("key_{}", i);
            let seg = router.route_to_segment(&key, &[]).unwrap();
            bucket_counts[seg.0 as usize] += 1;
        }

        // Each bucket should have roughly equal distribution
        // (not exact, but each should be hit multiple times)
        for count in &bucket_counts {
            assert!(*count > 50, "bucket should have at least 50 entries");
            assert!(*count < 250, "bucket should have at most 250 entries");
        }
    }

    #[test]
    fn test_range_partition_pruning_no_predicate() {
        let scheme = RangePartitionScheme {
            ranges: vec![
                RangePartition {
                    segment_id: SegmentId(0),
                    lower_bound: None,
                    upper_bound: Some("01000".to_string()),
                },
                RangePartition {
                    segment_id: SegmentId(1),
                    lower_bound: Some("01000".to_string()),
                    upper_bound: None,
                },
            ],
        };
        let router = SegmentRouter::new_range(scheme);

        let segments = router.prune_segments(None, None);
        assert_eq!(segments.len(), 2);
        assert!(segments.contains(&SegmentId(0)));
        assert!(segments.contains(&SegmentId(1)));
    }

    #[test]
    fn test_range_partition_pruning_with_lower_bound() {
        let scheme = RangePartitionScheme {
            ranges: vec![
                RangePartition {
                    segment_id: SegmentId(0),
                    lower_bound: None,
                    upper_bound: Some("01000".to_string()),
                },
                RangePartition {
                    segment_id: SegmentId(1),
                    lower_bound: Some("01000".to_string()),
                    upper_bound: Some("05000".to_string()),
                },
                RangePartition {
                    segment_id: SegmentId(2),
                    lower_bound: Some("05000".to_string()),
                    upper_bound: None,
                },
            ],
        };
        let router = SegmentRouter::new_range(scheme);

        // Predicate: >= 03000
        let segments = router.prune_segments(Some("03000"), None);
        assert_eq!(segments.len(), 2);
        assert!(segments.contains(&SegmentId(1)));
        assert!(segments.contains(&SegmentId(2)));
    }

    #[test]
    fn test_range_partition_pruning_with_bounds() {
        let scheme = RangePartitionScheme {
            ranges: vec![
                RangePartition {
                    segment_id: SegmentId(0),
                    lower_bound: None,
                    upper_bound: Some("01000".to_string()),
                },
                RangePartition {
                    segment_id: SegmentId(1),
                    lower_bound: Some("01000".to_string()),
                    upper_bound: Some("05000".to_string()),
                },
                RangePartition {
                    segment_id: SegmentId(2),
                    lower_bound: Some("05000".to_string()),
                    upper_bound: None,
                },
            ],
        };
        let router = SegmentRouter::new_range(scheme);

        // Predicate: 02000 <= x < 06000
        let segments = router.prune_segments(Some("02000"), Some("06000"));
        assert_eq!(segments.len(), 2);
        assert!(segments.contains(&SegmentId(1)));
        assert!(segments.contains(&SegmentId(2)));
    }

    #[test]
    fn test_hash_partition_pruning_no_filtering() {
        let scheme = HashPartitionScheme {
            num_buckets: 4,
            segments: vec![SegmentId(0), SegmentId(1), SegmentId(2), SegmentId(3)],
        };
        let router = SegmentRouter::new_hash(scheme);

        // Hash partitions always return all segments (no pruning)
        let segments = router.prune_segments(Some("1000"), Some("2000"));
        assert_eq!(segments.len(), 4);
    }

    #[test]
    fn test_partition_type_clone_and_eq() {
        let pt1 = PartitionType::None;
        let pt2 = PartitionType::None;
        assert_eq!(pt1, pt2);

        let scheme = RangePartitionScheme {
            ranges: vec![RangePartition {
                segment_id: SegmentId(0),
                lower_bound: None,
                upper_bound: Some("100".to_string()),
            }],
        };
        let pt3 = PartitionType::Range(scheme.clone());
        let pt4 = PartitionType::Range(scheme);
        assert_eq!(pt3, pt4);
    }

    #[test]
    fn test_router_clone() {
        let router1 = SegmentRouter::new_unpartitioned();
        let router2 = router1.clone();
        assert_eq!(router1.partition_type, router2.partition_type);
    }
}
