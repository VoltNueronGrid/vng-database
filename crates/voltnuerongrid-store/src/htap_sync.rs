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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOriginSnapshot {
    pub next_sequence: u64,
    pub last_acknowledged_sequence: u64,
    pub pending: Vec<RowMutation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceGap {
    pub expected: u64,
    pub actual: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateSequence {
    pub sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfOrderSequence {
    pub previous: u64,
    pub current: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicaReplayState {
    pub node_id: String,
    pub last_applied_sequence: u64,
    pub applied: Vec<RowMutation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicaReplayReport {
    pub applied_count: usize,
    pub last_applied_sequence: u64,
    pub gaps: Vec<SequenceGap>,
    pub duplicates: Vec<DuplicateSequence>,
    pub out_of_order: Vec<OutOfOrderSequence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicationTransportEvent {
    pub transport_sequence: u64,
    pub source_node_id: String,
    pub target_node_id: String,
    pub transport: String,
    pub mutation: RowMutation,
}

#[derive(Debug, Default)]
pub struct InMemoryReplicationTransport {
    next_transport_sequence: u64,
    next_mutation_sequence: u64,
    events: Vec<ReplicationTransportEvent>,
}

impl InMemoryReplicationTransport {
    pub fn new() -> Self {
        Self {
            next_transport_sequence: 1,
            next_mutation_sequence: 1,
            events: Vec::new(),
        }
    }

    pub fn publish(
        &mut self,
        source_node_id: &str,
        target_node_id: &str,
        transport: &str,
        table: &str,
        primary_key: &str,
        payload_json: &str,
        op: MutationOp,
    ) -> ReplicationTransportEvent {
        let event = ReplicationTransportEvent {
            transport_sequence: self.next_transport_sequence,
            source_node_id: source_node_id.to_string(),
            target_node_id: target_node_id.to_string(),
            transport: transport.to_string(),
            mutation: RowMutation {
                sequence: self.next_mutation_sequence,
                table: table.to_string(),
                primary_key: primary_key.to_string(),
                payload_json: payload_json.to_string(),
                op,
            },
        };
        self.next_transport_sequence += 1;
        self.next_mutation_sequence += 1;
        self.events.push(event.clone());
        event
    }

    pub fn export_for_target_since(
        &self,
        target_node_id: &str,
        last_mutation_sequence: u64,
        max_items: usize,
    ) -> Vec<RowMutation> {
        self.events
            .iter()
            .filter(|event| {
                (event.target_node_id == target_node_id || event.target_node_id == "*")
                    && event.mutation.sequence > last_mutation_sequence
            })
            .take(max_items)
            .map(|event| event.mutation.clone())
            .collect()
    }

    pub fn pending_for_target(&self, target_node_id: &str) -> usize {
        self.events
            .iter()
            .filter(|event| event.target_node_id == target_node_id || event.target_node_id == "*")
            .count()
    }
}

impl ReplicaReplayState {
    pub fn new(node_id: &str) -> Self {
        Self {
            node_id: node_id.to_string(),
            last_applied_sequence: 0,
            applied: Vec::new(),
        }
    }

    pub fn apply_batch(&mut self, batch: &[RowMutation]) -> ReplicaReplayReport {
        let mut gaps = Vec::new();
        if let Some(first) = batch.first() {
            let expected = self.last_applied_sequence + 1;
            if first.sequence != expected {
                gaps.push(SequenceGap {
                    expected,
                    actual: first.sequence,
                });
            }
        }

        gaps.extend(RowStoreSyncOrigin::detect_sequence_gaps(batch));
        let duplicates = RowStoreSyncOrigin::detect_duplicate_sequences(batch);
        let out_of_order = RowStoreSyncOrigin::detect_out_of_order(batch);

        let applied_count = if gaps.is_empty() && duplicates.is_empty() && out_of_order.is_empty() {
            let count = batch.len();
            self.applied.extend(batch.iter().cloned());
            if let Some(last) = batch.last() {
                self.last_applied_sequence = last.sequence;
            }
            count
        } else {
            0
        };

        ReplicaReplayReport {
            applied_count,
            last_applied_sequence: self.last_applied_sequence,
            gaps,
            duplicates,
            out_of_order,
        }
    }

    pub fn build_failover_handoff_batch(
        &self,
        origin: &RowStoreSyncOrigin,
        max_items: usize,
    ) -> Vec<RowMutation> {
        origin.export_since(self.last_applied_sequence, max_items)
    }
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

    pub fn export_since(&self, last_sequence: u64, max_items: usize) -> Vec<RowMutation> {
        self.pending
            .iter()
            .filter(|mutation| mutation.sequence > last_sequence)
            .take(max_items)
            .cloned()
            .collect()
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

    pub fn snapshot(&self) -> SyncOriginSnapshot {
        SyncOriginSnapshot {
            next_sequence: self.next_sequence,
            last_acknowledged_sequence: self.last_acknowledged_sequence,
            pending: self.pending.clone(),
        }
    }

    pub fn restore(snapshot: SyncOriginSnapshot) -> Self {
        Self {
            next_sequence: snapshot.next_sequence,
            last_acknowledged_sequence: snapshot.last_acknowledged_sequence,
            pending: snapshot.pending,
        }
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

    pub fn detect_duplicate_sequences(batch: &[RowMutation]) -> Vec<DuplicateSequence> {
        let mut seen = std::collections::HashSet::new();
        let mut duplicates = Vec::new();
        for item in batch {
            if !seen.insert(item.sequence) {
                duplicates.push(DuplicateSequence {
                    sequence: item.sequence,
                });
            }
        }
        duplicates
    }

    pub fn detect_out_of_order(batch: &[RowMutation]) -> Vec<OutOfOrderSequence> {
        let mut issues = Vec::new();
        for window in batch.windows(2) {
            let previous = window[0].sequence;
            let current = window[1].sequence;
            if current <= previous {
                issues.push(OutOfOrderSequence { previous, current });
            }
        }
        issues
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

    #[test]
    fn preserves_continuity_after_checkpoint_and_restore() {
        let mut origin = RowStoreSyncOrigin::new();
        origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
        origin.append("orders", "2", "{\"amount\":80}", MutationOp::Insert);
        origin.ack_through(1);

        let snapshot = origin.snapshot();
        let mut restored = RowStoreSyncOrigin::restore(snapshot);
        assert_eq!(restored.pending_len(), 1);
        assert_eq!(restored.checkpoint().last_sequence, 1);

        let appended = restored.append("orders", "3", "{\"amount\":150}", MutationOp::Insert);
        assert_eq!(appended.sequence, 3);
        let batch = restored.export_batch(10);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].sequence, 2);
        assert_eq!(batch[1].sequence, 3);
    }

    #[test]
    fn detects_duplicate_sequences_after_fault_injection() {
        let mut origin = RowStoreSyncOrigin::new();
        let first = origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
        let second = origin.append("orders", "2", "{\"amount\":80}", MutationOp::Insert);
        let mut batch = vec![first.clone(), second];
        batch.push(RowMutation {
            sequence: first.sequence,
            table: "orders".to_string(),
            primary_key: "dup".to_string(),
            payload_json: "{\"amount\":999}".to_string(),
            op: MutationOp::Update,
        });
        let duplicates = RowStoreSyncOrigin::detect_duplicate_sequences(&batch);
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].sequence, 1);
    }

    #[test]
    fn detects_out_of_order_sequences_after_fault_injection() {
        let mut origin = RowStoreSyncOrigin::new();
        let first = origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
        let second = origin.append("orders", "2", "{\"amount\":80}", MutationOp::Insert);
        let third = origin.append("orders", "3", "{\"amount\":90}", MutationOp::Insert);
        let batch = vec![first, third, second];
        let out_of_order = RowStoreSyncOrigin::detect_out_of_order(&batch);
        assert_eq!(out_of_order.len(), 1);
        assert_eq!(out_of_order[0].previous, 3);
        assert_eq!(out_of_order[0].current, 2);
    }

    #[test]
    fn replay_after_restore_preserves_integrity_without_faults() {
        let mut origin = RowStoreSyncOrigin::new();
        origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
        origin.append("orders", "2", "{\"amount\":80}", MutationOp::Insert);
        origin.append("orders", "3", "{\"amount\":90}", MutationOp::Insert);
        origin.ack_through(1);

        let snapshot = origin.snapshot();
        let restored = RowStoreSyncOrigin::restore(snapshot);
        let replay_batch = restored.export_batch(10);

        assert!(RowStoreSyncOrigin::detect_sequence_gaps(&replay_batch).is_empty());
        assert!(RowStoreSyncOrigin::detect_duplicate_sequences(&replay_batch).is_empty());
        assert!(RowStoreSyncOrigin::detect_out_of_order(&replay_batch).is_empty());
    }

    #[test]
    fn replay_after_restore_detects_gap_when_fault_injected() {
        let mut origin = RowStoreSyncOrigin::new();
        origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
        origin.append("orders", "2", "{\"amount\":80}", MutationOp::Insert);
        origin.append("orders", "3", "{\"amount\":90}", MutationOp::Insert);

        let snapshot = origin.snapshot();
        let mut restored = RowStoreSyncOrigin::restore(snapshot);
        let removed = restored.remove_sequence_for_fault_injection(2);
        assert!(removed);

        let replay_batch = restored.export_batch(10);
        let gaps = RowStoreSyncOrigin::detect_sequence_gaps(&replay_batch);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].expected, 2);
        assert_eq!(gaps[0].actual, 3);
    }

    #[test]
    fn multi_node_replica_replay_applies_contiguous_transport_batch() {
        let mut origin = RowStoreSyncOrigin::new();
        origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
        origin.append("orders", "2", "{\"amount\":80}", MutationOp::Insert);
        origin.append("orders", "3", "{\"amount\":90}", MutationOp::Insert);

        let mut replica = ReplicaReplayState::new("node-b");
        let batch = origin.export_since(0, 10);
        let report = replica.apply_batch(&batch);

        assert_eq!(report.applied_count, 3);
        assert_eq!(report.last_applied_sequence, 3);
        assert!(report.gaps.is_empty());
        assert!(report.duplicates.is_empty());
        assert!(report.out_of_order.is_empty());
        assert_eq!(replica.applied.len(), 3);
    }

    #[test]
    fn multi_node_failover_handoff_replays_only_unapplied_mutations() {
        let mut origin = RowStoreSyncOrigin::new();
        origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
        origin.append("orders", "2", "{\"amount\":80}", MutationOp::Insert);
        origin.append("orders", "3", "{\"amount\":90}", MutationOp::Insert);
        origin.append("orders", "4", "{\"amount\":110}", MutationOp::Update);

        let mut replica = ReplicaReplayState::new("node-b");
        let initial_report = replica.apply_batch(&origin.export_since(0, 2));
        assert_eq!(initial_report.applied_count, 2);
        assert_eq!(replica.last_applied_sequence, 2);

        let handoff_batch = replica.build_failover_handoff_batch(&origin, 10);
        assert_eq!(handoff_batch.len(), 2);
        assert_eq!(handoff_batch[0].sequence, 3);
        assert_eq!(handoff_batch[1].sequence, 4);

        let handoff_report = replica.apply_batch(&handoff_batch);
        assert_eq!(handoff_report.applied_count, 2);
        assert_eq!(handoff_report.last_applied_sequence, 4);
        assert!(handoff_report.gaps.is_empty());
    }

    #[test]
    fn multi_node_failover_handoff_reports_gap_when_transport_drops_sequence() {
        let mut origin = RowStoreSyncOrigin::new();
        origin.append("orders", "1", "{\"amount\":100}", MutationOp::Insert);
        origin.append("orders", "2", "{\"amount\":80}", MutationOp::Insert);
        origin.append("orders", "3", "{\"amount\":90}", MutationOp::Insert);
        origin.append("orders", "4", "{\"amount\":110}", MutationOp::Update);

        let mut replica = ReplicaReplayState::new("node-b");
        let initial_report = replica.apply_batch(&origin.export_since(0, 2));
        assert_eq!(initial_report.applied_count, 2);

        let mut handoff_batch = replica.build_failover_handoff_batch(&origin, 10);
        handoff_batch.retain(|mutation| mutation.sequence != 3);
        let report = replica.apply_batch(&handoff_batch);

        assert_eq!(report.applied_count, 0);
        assert_eq!(report.gaps.len(), 1);
        assert_eq!(report.gaps[0].expected, 3);
        assert_eq!(report.gaps[0].actual, 4);
        assert_eq!(replica.last_applied_sequence, 2);
    }

    #[test]
    fn multi_node_replication_transport_exports_only_targeted_events() {
        let mut transport = InMemoryReplicationTransport::new();
        transport.publish(
            "node-a",
            "node-b",
            "raft",
            "orders",
            "1",
            "{\"amount\":100}",
            MutationOp::Insert,
        );
        transport.publish(
            "node-a",
            "node-c",
            "raft",
            "orders",
            "2",
            "{\"amount\":110}",
            MutationOp::Insert,
        );
        transport.publish(
            "node-a",
            "*",
            "raft",
            "orders",
            "3",
            "{\"amount\":120}",
            MutationOp::Update,
        );

        let node_b = transport.export_for_target_since("node-b", 0, 10);
        assert_eq!(node_b.len(), 2);
        assert_eq!(node_b[0].sequence, 1);
        assert_eq!(node_b[1].sequence, 3);

        let node_c = transport.export_for_target_since("node-c", 0, 10);
        assert_eq!(node_c.len(), 2);
        assert_eq!(node_c[0].sequence, 2);
        assert_eq!(node_c[1].sequence, 3);
    }

    #[test]
    fn multi_node_replication_transport_respects_last_applied_sequence() {
        let mut transport = InMemoryReplicationTransport::new();
        transport.publish(
            "node-a",
            "node-b",
            "raft",
            "orders",
            "1",
            "{\"amount\":100}",
            MutationOp::Insert,
        );
        transport.publish(
            "node-a",
            "node-b",
            "raft",
            "orders",
            "2",
            "{\"amount\":110}",
            MutationOp::Update,
        );

        let replay = transport.export_for_target_since("node-b", 1, 10);
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].sequence, 2);
    }
}
