# H-02 HTAP Sync Correctness Spec (Starter)

This spec defines the first executable correctness checks for HTAP row-store to analytics sync.

## Goal

Detect sequence integrity violations early and prevent silent data divergence between transactional row mutations and downstream analytical sync consumers.

## Invariants

1. **Monotonic sequence assignment**  
   Each emitted `RowMutation.sequence` must strictly increase.

2. **No silent sequence gaps in exported batches**  
   Any missing sequence in a batch must be detected and surfaced as a `SequenceGap`.

3. **Ack safety**  
   `ack_through(sequence)` must only remove records at or below the acknowledged sequence and preserve later records.

## Fault Injection (Starter)

`remove_sequence_for_fault_injection(sequence)` deliberately removes a pending mutation to simulate a dropped record.  
`detect_sequence_gaps(batch)` validates that the gap is surfaced.

## Fault Injection (Expanded)

- Duplicate detection:
  - `detect_duplicate_sequences(batch)`
- Reorder detection:
  - `detect_out_of_order(batch)`

## Executable Evidence

- Unit test:
  - `detects_sequence_gap_after_fault_injection` in `crates/voltnuerongrid-store/src/htap_sync.rs`
  - `detects_duplicate_sequences_after_fault_injection` in `crates/voltnuerongrid-store/src/htap_sync.rs`
  - `detects_out_of_order_sequences_after_fault_injection` in `crates/voltnuerongrid-store/src/htap_sync.rs`
  - `replay_after_restore_preserves_integrity_without_faults` in `crates/voltnuerongrid-store/src/htap_sync.rs`
  - `replay_after_restore_detects_gap_when_fault_injected` in `crates/voltnuerongrid-store/src/htap_sync.rs`
- Harness script:
  - `tests/kpi/scripts/run-h02-sync-fault-injection.ps1`
  - `tests/kpi/scripts/run-h02-reorder-duplicate-faults.ps1`
  - `tests/kpi/scripts/run-h02-restart-replay-matrix.ps1`
- Artifact:
  - `tests/kpi/results/h02/htap-sync-fault-injection.json`
  - `tests/kpi/results/h02/htap-sync-reorder-duplicate-faults.json`
  - `tests/kpi/results/h02/h02-restart-replay-matrix.json`

## Next Expansion

- Integrate restart/replay matrix with disk-backed WAL replay once WS2 adapter wiring is complete.
- Extend matrix to cover multi-node replay handoff behavior.

## Completed in this milestone

- Out-of-order mutation replay detection added.
- Duplicate-sequence detection added.
- Restart+replay matrix harness added with explicit integrity checks after restore.
- Matrix now includes persisted WAL recovery test coverage (`recovers_state_from_wal_adapter_records`).
