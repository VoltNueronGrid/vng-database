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

## Executable Evidence

- Unit test:
  - `detects_sequence_gap_after_fault_injection` in `crates/voltnuerongrid-store/src/htap_sync.rs`
- Harness script:
  - `tests/kpi/scripts/run-h02-sync-fault-injection.ps1`
- Artifact:
  - `tests/kpi/results/h02/htap-sync-fault-injection.json`

## Next Expansion

- Add out-of-order mutation replay scenarios.
- Add duplicate-sequence detection.
- Add checkpoint + restart continuity tests for sequence invariants.
