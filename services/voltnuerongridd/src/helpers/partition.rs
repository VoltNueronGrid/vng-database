//! B-4: Physical range partitioning — partition-key→segment mapping and
//! partition pruning for range predicates.
//!
//! A table partitioned with `PARTITION BY RANGE(col) BOUNDARIES (b0, b1, ...)`
//! is split into `boundaries.len() + 1` ordered segments:
//!
//! ```text
//!   segment 0 : (-inf, b0)
//!   segment 1 : [b0,   b1)
//!   ...
//!   segment n : [b_{n-1}, +inf)
//! ```
//!
//! Rows are routed to a segment by their partition-column value, and a SELECT
//! with a range predicate on the partition column can prune the segments that
//! cannot contain matching rows.

use crate::AppState;
use serde::Serialize;

/// Range-partition configuration for one table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RangePartitionConfig {
    pub(crate) column: String,
    /// Ascending, de-duplicated segment boundaries. `n` boundaries → `n+1` segments.
    pub(crate) boundaries: Vec<i64>,
}

impl RangePartitionConfig {
    pub(crate) fn segment_count(&self) -> usize {
        self.boundaries.len() + 1
    }
}

/// Parse `PARTITION BY RANGE(col)` plus an optional `BOUNDARIES (a, b, c)`
/// clause from a CREATE TABLE statement. When no boundaries are given the table
/// is a single-segment partitioned table (boundaries empty). Returns `None`
/// when the partition clause is absent or the column is empty.
pub(crate) fn parse_range_partition(ddl: &str) -> Option<RangePartitionConfig> {
    let lower = ddl.to_ascii_lowercase();
    let idx = lower.find("partition by range")?;
    let after = &lower[idx + "partition by range".len()..];
    let open = after.find('(')?;
    let close = after[open + 1..].find(')')?;
    let column = after[open + 1..open + 1 + close].trim().to_string();
    if column.is_empty() {
        return None;
    }

    let mut boundaries: Vec<i64> = Vec::new();
    if let Some(bidx) = after.find("boundaries") {
        let rest = &after[bidx + "boundaries".len()..];
        if let Some(bopen) = rest.find('(') {
            if let Some(bclose) = rest[bopen + 1..].find(')') {
                let inner = &rest[bopen + 1..bopen + 1 + bclose];
                for tok in inner.split(',') {
                    if let Ok(n) = tok.trim().parse::<i64>() {
                        boundaries.push(n);
                    }
                }
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    Some(RangePartitionConfig { column, boundaries })
}

/// Map a numeric partition-column value to its segment id `[0, segment_count)`.
/// The segment is the count of boundaries strictly less-than-or-equal to which
/// the value falls — i.e. the first boundary greater than the value.
pub(crate) fn segment_for_value(config: &RangePartitionConfig, value: i64) -> usize {
    // partition is the index of the first boundary strictly greater than value.
    config
        .boundaries
        .iter()
        .position(|b| value < *b)
        .unwrap_or(config.boundaries.len())
}

/// Comparison operator for a range predicate on the partition column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RangeOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
}

impl RangeOp {
    pub(crate) fn parse(op: &str) -> Option<Self> {
        match op.trim() {
            "<" => Some(RangeOp::Lt),
            "<=" => Some(RangeOp::Le),
            ">" => Some(RangeOp::Gt),
            ">=" => Some(RangeOp::Ge),
            "=" | "==" => Some(RangeOp::Eq),
            _ => None,
        }
    }
}

/// Given a range predicate `<partition_col> <op> <value>`, return the set of
/// segment ids that may contain matching rows. Segments not returned are pruned.
pub(crate) fn prune_segments(config: &RangePartitionConfig, op: RangeOp, value: i64) -> Vec<usize> {
    let total = config.segment_count();
    let target = segment_for_value(config, value);
    let all: Vec<usize> = (0..total).collect();
    match op {
        // value lives in `target`; lower segments hold strictly-smaller values.
        RangeOp::Lt | RangeOp::Le => all.into_iter().filter(|s| *s <= target).collect(),
        RangeOp::Gt | RangeOp::Ge => all.into_iter().filter(|s| *s >= target).collect(),
        RangeOp::Eq => vec![target],
    }
}

/// Register (or replace) a table's range-partition configuration.
pub(crate) fn register_partition_config(state: &AppState, table: &str, config: RangePartitionConfig) {
    if let Ok(mut reg) = state.storage.partition_segments.lock() {
        reg.insert(table.to_ascii_lowercase(), config);
    }
}

/// Look up a table's range-partition configuration, if partitioned.
pub(crate) fn lookup_partition_config(state: &AppState, table: &str) -> Option<RangePartitionConfig> {
    state
        .storage
        .partition_segments
        .lock()
        .ok()
        .and_then(|r| r.get(&table.to_ascii_lowercase()).cloned())
}

/// Compute per-segment row counts for `table` by scanning the local row store
/// and routing each row by its partition-column value. Rows whose partition
/// column is missing or non-numeric are counted in an "unrouted" bucket.
pub(crate) fn per_segment_row_counts(state: &AppState, table: &str, config: &RangePartitionConfig) -> (Vec<usize>, usize) {
    let mut counts = vec![0usize; config.segment_count()];
    let mut unrouted = 0usize;
    let prefix = format!("{}:", table.to_ascii_lowercase());
    if let Ok(rs) = state.storage.row_store.lock() {
        for (key, data) in rs.scan_at_snapshot(rs.current_xid()) {
            if !key.to_ascii_lowercase().starts_with(&prefix) {
                continue;
            }
            match data
                .get(&config.column)
                .and_then(|v| v.trim().parse::<i64>().ok())
            {
                Some(value) => counts[segment_for_value(config, value)] += 1,
                None => unrouted += 1,
            }
        }
    }
    (counts, unrouted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> RangePartitionConfig {
        RangePartitionConfig { column: "age".to_string(), boundaries: vec![10, 20, 30] }
    }

    #[test]
    fn parse_range_partition_with_boundaries() {
        let c = parse_range_partition(
            "CREATE TABLE people (id INT, age INT) PARTITION BY RANGE(age) BOUNDARIES (10, 20, 30)",
        )
        .expect("parsed");
        assert_eq!(c.column, "age");
        assert_eq!(c.boundaries, vec![10, 20, 30]);
        assert_eq!(c.segment_count(), 4);
    }

    #[test]
    fn parse_range_partition_without_boundaries_is_single_segment() {
        let c = parse_range_partition("CREATE TABLE t (id INT) PARTITION BY RANGE(id)").expect("parsed");
        assert_eq!(c.segment_count(), 1);
    }

    #[test]
    fn segment_routing_is_ordered() {
        let c = cfg();
        assert_eq!(segment_for_value(&c, 5), 0);
        assert_eq!(segment_for_value(&c, 10), 1);
        assert_eq!(segment_for_value(&c, 19), 1);
        assert_eq!(segment_for_value(&c, 25), 2);
        assert_eq!(segment_for_value(&c, 100), 3);
    }

    #[test]
    fn prune_lt_excludes_higher_segments() {
        let c = cfg();
        // age < 15 → value lives in segment 1; segments 0,1 may match, 2,3 pruned.
        let segs = prune_segments(&c, RangeOp::Lt, 15);
        assert_eq!(segs, vec![0, 1]);
    }

    #[test]
    fn prune_gt_excludes_lower_segments() {
        let c = cfg();
        let segs = prune_segments(&c, RangeOp::Gt, 25);
        assert_eq!(segs, vec![2, 3]);
    }

    #[test]
    fn prune_eq_targets_single_segment() {
        let c = cfg();
        assert_eq!(prune_segments(&c, RangeOp::Eq, 25), vec![2]);
    }
}
