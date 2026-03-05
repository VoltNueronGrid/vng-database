#![forbid(unsafe_code)]

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationOp {
    Insert,
    Update,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowMutation {
    pub sequence: u64,
    pub table: String,
    pub primary_key: String,
    pub payload_json: String,
    pub op: MutationOp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOriginCheckpoint {
    pub last_sequence: u64,
    pub pending_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceGap {
    pub expected: u64,
    pub actual: u64,
}

#[derive(Debug, Default)]
pub struct RowStoreSyncOrigin {
    next_sequence: u64,
    last_acknowledged_sequence: u64,
    pending: Vec<RowMutation>,
}

impl RowStoreSyncOrigin {
    pub fn new() -> Self {
        Self {
            next_sequence: 1,
            last_acknowledged_sequence: 0,
            pending: Vec::new(),
        }
    }

    pub fn append(
        &mut self,
        table: &str,
        primary_key: &str,
        payload_json: &str,
        op: MutationOp,
    ) -> RowMutation {
        let mutation = RowMutation {
            sequence: self.next_sequence,
            table: table.to_string(),
            primary_key: primary_key.to_string(),
            payload_json: payload_json.to_string(),
            op,
        };
        self.next_sequence += 1;
        self.pending.push(mutation.clone());
        mutation
    }

    pub fn export_batch(&self, max_items: usize) -> Vec<RowMutation> {
        self.pending.iter().take(max_items).cloned().collect()
    }

    pub fn ack_through(&mut self, sequence: u64) {
        self.last_acknowledged_sequence = self.last_acknowledged_sequence.max(sequence);
        self.pending.retain(|m| m.sequence > sequence);
    }

    pub fn checkpoint(&self) -> SyncOriginCheckpoint {
        SyncOriginCheckpoint {
            last_sequence: self.last_acknowledged_sequence,
            pending_count: self.pending.len(),
        }
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn remove_sequence_for_fault_injection(&mut self, sequence: u64) -> bool {
        let before = self.pending.len();
        self.pending.retain(|m| m.sequence != sequence);
        before != self.pending.len()
    }

    pub fn detect_sequence_gaps(batch: &[RowMutation]) -> Vec<SequenceGap> {
        if batch.is_empty() {
            return Vec::new();
        }
        let mut sorted = batch.to_vec();
        sorted.sort_by_key(|m| m.sequence);

        let mut gaps = Vec::new();
        let mut expected = sorted[0].sequence;
        for item in sorted {
            if item.sequence != expected {
                gaps.push(SequenceGap {
                    expected,
                    actual: item.sequence,
                });
                expected = item.sequence + 1;
            } else {
                expected += 1;
            }
        }
        gaps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_mutations_with_monotonic_sequence() {
        let mut origin = RowStoreSyncOrigin::new();
        let a = origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
        let b = origin.append("orders", "1", "{\"amount\":120}", MutationOp::Update);
        assert_eq!(a.sequence, 1);
        assert_eq!(b.sequence, 2);
        assert_eq!(origin.pending_len(), 2);
    }

    #[test]
    fn ack_through_removes_flushed_rows() {
        let mut origin = RowStoreSyncOrigin::new();
        origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
        origin.append("orders", "2", "{\"amount\":80}", MutationOp::Insert);
        origin.append("orders", "2", "{\"amount\":90}", MutationOp::Update);

        origin.ack_through(2);
        let batch = origin.export_batch(10);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].sequence, 3);

        let checkpoint = origin.checkpoint();
        assert_eq!(checkpoint.last_sequence, 2);
        assert_eq!(checkpoint.pending_count, 1);
    }

    #[test]
    fn detects_sequence_gap_after_fault_injection() {
        let mut origin = RowStoreSyncOrigin::new();
        origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
        origin.append("orders", "2", "{\"amount\":80}", MutationOp::Insert);
        origin.append("orders", "3", "{\"amount\":90}", MutationOp::Insert);
        let removed = origin.remove_sequence_for_fault_injection(2);
        assert!(removed);

        let batch = origin.export_batch(10);
        let gaps = RowStoreSyncOrigin::detect_sequence_gaps(&batch);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].expected, 2);
        assert_eq!(gaps[0].actual, 3);
    }
}
