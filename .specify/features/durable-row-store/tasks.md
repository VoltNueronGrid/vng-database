# Tasks: Durable Row Store

**Input**: `.specify/features/durable-row-store/plan.md`, `.specify/features/durable-row-store/spec.md`

**Prerequisites**: plan.md ✅, spec.md ✅

## Phase 1: Setup

- [X] T001 Verify `store_row` call sites in `handlers/sql.rs` (INSERT, UPDATE, DELETE paths) — lines 771, 791, 856, 873
- [X] T002 Verify `store_row` call sites in `helpers/raft_loop.rs` (Raft apply loop) — lines 562, 568, 575
- [X] T003 Verify `scan_rows_for_db` used for SELECT scans in `handlers/sql.rs` when `persists_rows() == true`
- [X] T004 Verify `purge_database_rows` + `drop_db_column_family` wired in DROP DATABASE handler

---

## Phase 2: Boot Sequence Fix (US1 — rows survive restart)

- [ ] T005 In `services/voltnuerongridd/src/main.rs` — wrap `replay_dml_into` call with `if !persists_rows` check so DML SQL text is NOT replayed into PagedRowStore when RocksDB is active
- [ ] T006 Add test `p1_boot_skips_dml_replay_when_rocksdb_active` verifying that `replay_dml_into` is not called when engine has `persists_rows() == true`

---

## Phase 3: Evidence Gate (US4 — crash recovery gate passes)

- [X] T007 [P] Crash recovery gate script exists at `tests/kpi/scripts/run-crash-recovery-gate.ps1`
- [ ] T008 Run crash recovery gate against live server with 1000+ rows — verify `rows_survived: true` in artifact
- [ ] T009 Update tracker REQ-17 and WS6 with gate evidence timestamp

---

## Phase 4: Multi-Database Isolation (US2)

- [X] T010 `ensure_db_cf` creates per-DB CF on first INSERT to that database
- [X] T011 `scan_rows_for_db(db, xid)` scans only the requested database's CF
- [ ] T012 Add test: rows from `db_a` never appear in `scan_rows_for_db("db_b", ...)` result

---

## Phase 5: Documentation Updates

- [ ] T013 Update `docs/tasks-v4.md` P1 status from NOT STARTED to IN PROGRESS / DONE
- [ ] T014 Update tracker REQ-05 (durability), REQ-17 (zero data loss) with current evidence
