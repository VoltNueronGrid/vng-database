# Feature Specification: Full ACID Enforcement

**Feature Branch**: `full-acid`

**Created**: 2026-06-26

**Status**: In Progress (75% complete — UNDO log + REPEATABLE READ + SERIALIZABLE done; group commit blocked on P1)

**Priority**: P0 — correctness gate for multi-statement transactions, isolation levels, and crash-safe rollback.

**Input**: Implement UNDO log, correct per-isolation-level snapshot semantics, serializable conflict detection on COMMIT, and group WAL fsync for multi-statement transactions in VoltNueronGrid DB.

---

## Background and Problem Statement

The ACID enforcement infrastructure is partially complete:
- `AcidTransactionRegistry` exists with `AcidTxEntry` per transaction
- `tx_undo_log` exists in `AppState` for before-image tracking
- `record_undo()` helper exists in `handlers/sql.rs`
- `check_serializable_conflict()` exists but is not wired to the COMMIT path for all SERIALIZABLE transactions
- `row_store_snapshot_xid` exists for REPEATABLE READ
- Group commit (batched WAL fsync) is not implemented

Key gaps:
1. **Atomicity**: A multi-statement batch that fails mid-way leaves partial rows — `ROLLBACK` cannot unwind them without an UNDO log per write
2. **Isolation**: `READ COMMITTED`, `REPEATABLE READ`, `SERIALIZABLE` are parsed but all execute with identical `scan_at_snapshot(current_xid)` behavior for non-REPEATABLE-READ transactions  
3. **Durability** (group commit): Every WAL append is an independent flush; production databases batch commits
4. **Serializable COMMIT**: `check_serializable_conflict()` exists in `AcidTransactionRegistry` but is only called in some paths

---

## User Scenarios & Testing

### User Story 1 — ROLLBACK unwinds partial multi-statement batch (Priority: P1)

An operator begins a transaction, INSERTs 3 rows, then issues ROLLBACK. None of the 3 rows should be visible.

**Acceptance Scenarios**:
1. **Given** a transaction with 3 INSERTs followed by ROLLBACK, **When** a SELECT runs after ROLLBACK, **Then** none of the 3 rows are returned.
2. **Given** a transaction that fails mid-batch (e.g., constraint violation on row 2 of 3), **When** ROLLBACK is called, **Then** row 1 is also removed (no partial commit).
3. **Given** nested BEGIN…ROLLBACK inside a batch, **When** executed, **Then** all partial writes from the nested block are reversed.

---

### User Story 2 — REPEATABLE READ sees stable snapshot (Priority: P1)

Within a REPEATABLE READ transaction, repeated identical SELECT statements return the same rows even if a concurrent transaction committed between the reads.

**Acceptance Scenarios**:
1. **Given** transaction T1 starts with `BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ`, **When** T2 commits a new row between T1's two identical SELECTs, **Then** T1's second SELECT does NOT return T2's row.
2. **Given** a REPEATABLE READ transaction running for more than one second, **When** rows are modified by other transactions, **Then** the snapshot remains stable throughout.

---

### User Story 3 — SERIALIZABLE conflict detected at COMMIT (Priority: P1)

Two concurrent SERIALIZABLE transactions with overlapping write-sets abort with 409 CONFLICT.

**Acceptance Scenarios**:
1. **Given** T1 and T2 both begin SERIALIZABLE, T1 reads row A and writes row B, T2 reads row B and writes row A, **When** T1 commits, **Then** T2's COMMIT returns 409 CONFLICT.
2. **Given** `check_serializable_conflict()` is wired to the COMMIT path for all SERIALIZABLE transactions, **When** no overlap exists, **Then** COMMIT succeeds normally.

---

### User Story 4 — Group commit reduces fsync count (Priority: P2)

Under concurrent load, WAL fsync count is less than the number of concurrent transactions committing.

**Acceptance Scenarios**:
1. **Given** 100 concurrent transactions each committing a single row, **When** measured over 1 second, **Then** fsync count < 100 (batched).

---

## Technical Design

### UNDO Log (US1)

`tx_undo_log: Arc<Mutex<HashMap<String, Vec<(String, Option<RowData>)>>>>`  
Already exists in `AppState`. `record_undo()` already exists in `handlers/sql.rs`.

**Remaining work**: Verify `record_undo()` is called BEFORE every `rs.insert()` and `rs.delete()` in the SQL COMMIT path. Wire `apply_undo_log_for_connection()` helper to the ROLLBACK handler path.

### SERIALIZABLE Conflict (US3)

`check_serializable_conflict()` exists in `AcidTransactionRegistry`. 

**Remaining work**: In `handlers/sql.rs` COMMIT path, for transactions with `isolation_level == "serializable"`, call `acid.check_serializable_conflict(&tx_id)` and return 409 if it fails.

### REPEATABLE READ Snapshot (US2)

`row_store_snapshot_xid` exists in `AcidTxEntry` and is set at BEGIN for REPEATABLE READ.

**Remaining work**: Verify all SELECT paths within a REPEATABLE READ transaction use `entry.row_store_snapshot_xid` as the scan snapshot, not the current XID.

### Group Commit (US4) — Blocked on P1

Requires RocksDB to be the primary write path. Implement after P1 (durable row store) is complete.

---

## File Impact

| File | Change |
|---|---|
| `services/voltnuerongridd/src/handlers/sql.rs` | Wire `check_serializable_conflict` to COMMIT for SERIALIZABLE txns; verify `record_undo` before every write |
| `services/voltnuerongridd/src/main.rs` | `AcidTransactionRegistry::apply_undo_log_for_connection` helper |
| `services/voltnuerongridd/src/tests.rs` | New tests: rollback_unwinds_partial_batch, repeatable_read_stable_snapshot, serializable_conflict_409 |

---

## Acceptance Gates

| Gate | Command | Must Pass |
|------|---------|-----------|
| Unit tests | `cargo test -p voltnuerongridd` | 853+ passed |
| ACID isolation tests | `cargo test -p voltnuerongridd -- acid` | All pass |
| ROLLBACK tests | `cargo test -p voltnuerongridd -- rollback` | All pass |

## Definition of Done

- [ ] `ROLLBACK` after partial INSERT batch leaves no partial rows visible
- [ ] `REPEATABLE READ` transaction sees stable snapshot throughout lifetime
- [ ] `SERIALIZABLE` COMMIT returns 409 when write-set overlaps with concurrent serializable read-set
- [ ] `cargo test -p voltnuerongridd` passes (regression-free)
- [ ] New unit tests for each isolation level added to `tests::p3_*` suite
- [ ] Group commit: deferred pending P1 completion — tracked as follow-on

## Related Items

- tasks-v4.md P3
- `services/voltnuerongridd/src/main.rs` — `AcidTransactionRegistry`, `AcidTxEntry`
- `services/voltnuerongridd/src/handlers/sql.rs` — `record_undo`, `check_serializable_conflict`
