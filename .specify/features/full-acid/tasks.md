# Tasks: Full ACID Enforcement

**Input**: `.specify/features/full-acid/plan.md`, `.specify/features/full-acid/spec.md`

## Phase 1: Setup — Verify existing infrastructure

- [X] T001 Verify `tx_undo_log` field exists in `AppState` (`services/voltnuerongridd/src/main.rs`)
- [X] T002 Verify `record_undo()` helper exists in `services/voltnuerongridd/src/handlers/sql.rs`
- [X] T003 Verify `check_serializable_conflict()` exists in `AcidTransactionRegistry` (`main.rs`)
- [X] T004 Verify `row_store_snapshot_xid` set at BEGIN for REPEATABLE READ in `AcidTxEntry::new`

---

## Phase 2: ROLLBACK — UNDO Log Wiring (US1)

- [ ] T005 Audit `handlers/sql.rs` INSERT path (~line 771) — ensure `record_undo` is called before `rs.insert`
- [ ] T006 Audit `handlers/sql.rs` DELETE path (~line 791) — ensure `record_undo` is called before `rs.delete`
- [ ] T007 Audit `handlers/sql.rs` UPDATE bulk path (~line 856) — ensure `record_undo` called per matched row
- [ ] T008 Audit `handlers/sql.rs` ROLLBACK handler — ensure it iterates `tx_undo_log[conn_id]` in reverse and restores before-images
- [ ] T009 Add test `p3_rollback_unwinds_partial_insert_batch` in `services/voltnuerongridd/src/tests.rs`

---

## Phase 3: SERIALIZABLE Conflict at COMMIT (US3)

- [ ] T010 In `handlers/sql.rs` COMMIT path: for `isolation_level == "serializable"` transactions, call `acid.check_serializable_conflict(&tx_id)` and return 409 on conflict
- [ ] T011 Add test `p3_serializable_conflict_returns_409` in `tests.rs`

---

## Phase 4: REPEATABLE READ Snapshot (US2)

- [ ] T012 Verify all SELECT scans within a REPEATABLE READ transaction use `entry.row_store_snapshot_xid` as snapshot in `handlers/sql.rs`
- [ ] T013 Add test `p3_repeatable_read_stable_snapshot` in `tests.rs`

---

## Phase 5: Group Commit (US4) — DEFERRED

- [ ] T014 DEFERRED: group commit batched WAL fsync — blocked on P1 (durable row store) completion

---

## Phase 6: Documentation

- [ ] T015 Update `docs/tasks-v4.md` P3 status and acceptance criteria checklist
- [ ] T016 Update tracker REQ-03 (ACID) and REQ-12 (isolation levels) with evidence
