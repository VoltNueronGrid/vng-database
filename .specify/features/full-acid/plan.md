# Implementation Plan: Full ACID Enforcement

**Branch**: `main` | **Date**: 2026-06-26 | **Spec**: [spec.md](spec.md)

## Summary

Wire the existing UNDO log, REPEATABLE READ snapshot, and SERIALIZABLE conflict detection infrastructure to all code paths. The data structures and helpers exist — the work is ensuring they are called correctly on COMMIT and ROLLBACK, and adding tests to prove correctness for each isolation level.

## Technical Context

**Language/Version**: Rust 1.78+

**Primary Dependencies**: `AcidTransactionRegistry`, `PagedRowStore`, `tx_undo_log` in `AppState`

**Storage**: In-memory `PagedRowStore` (group commit deferred pending P1)

**Testing**: `cargo test -p voltnuerongridd -- p3_`

## Constitution Check

- **Durable HTAP correctness**: PASS — UNDO log provides rollback correctness; isolation levels protect read stability
- **Security/RBAC/tenant isolation**: N/A — ACID enforcement is below the RBAC layer
- **Performance evidence**: PARTIAL — group commit evidence deferred to after P1
- **Modular Rust/reuse**: PASS — reuses `AcidTransactionRegistry`, `tx_undo_log`, `record_undo`, `check_serializable_conflict`
- **Native interface parity**: PASS — HTTP SQL execute endpoint is the primary surface
- **Evidence and tracker truth**: New tests in `tests::p3_*` suite

## Implementation Phases

### Phase 1: UNDO Log Wiring (US1 — ROLLBACK)

Verify `record_undo()` is called before EVERY `rs.insert()` and `rs.delete()` in the COMMIT path.

Verify the ROLLBACK handler calls `apply_undo_log()` to restore before-images.

**Key locations in `handlers/sql.rs`:**
- Line ~771: INSERT single row — `record_undo` should be called before `rs.insert`
- Line ~791: DELETE single row — `record_undo` should be called before `rs.delete`  
- Line ~856: UPDATE bulk — `record_undo` should be called for each matched row
- Line ~873: UPDATE single — `record_undo` should be called before `rs.insert`

**ROLLBACK path** should iterate `tx_undo_log[conn_id]` in reverse and call `rs.insert(xid, key, before_data)` or `rs.delete(xid, key)` for each entry.

### Phase 2: SERIALIZABLE Conflict Wiring (US3)

In the COMMIT path of `handlers/sql.rs`, after all DML statements succeed:
```rust
if acid_entry.isolation_level == "serializable" {
    if let Err(conflict) = acid.check_serializable_conflict(&tx_id) {
        // Clean up and return 409
        return Err(conflict_err(conflict));
    }
}
```

### Phase 3: REPEATABLE READ Snapshot Verification (US2)

Verify that when `entry.isolation_level == "repeatable_read"`, all SELECT scans within the transaction use `entry.row_store_snapshot_xid` (already set at BEGIN in `AcidTxEntry::new`).

### Phase 4: Tests

Add `tests::p3_*` tests for:
- `p3_rollback_unwinds_partial_insert_batch`
- `p3_repeatable_read_stable_snapshot`
- `p3_serializable_conflict_returns_409`

## Definition of Done

- [ ] `ROLLBACK` fully implemented — partial batch leaves no visible rows
- [ ] `REPEATABLE READ` uses snapshot at BEGIN — verified by test
- [ ] `SERIALIZABLE` COMMIT checks conflict — returns 409 on overlap
- [ ] 3 new `p3_*` tests passing
- [ ] `cargo test -p voltnuerongridd` regression-free
