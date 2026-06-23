# Feature Specification: Durable Row Store

**Feature Branch**: `durable-row-store`

**Created**: 2026-06-23

**Status**: Draft

**Priority**: P0 — blocks ACID crash recovery, REQ-17 (zero data loss), REQ-16 (encryption-at-rest for row data), and multi-node Raft correctness.

**Input**: Replace the in-memory `PagedRowStore` with a page-level durable storage backend so that acknowledged row writes survive process restart and crash without data loss.

---

## Background and Problem Statement

`PagedRowStore` in `crates/voltnuerongrid-store/src/mvcc.rs` is an in-memory MVCC row store. All row data is lost when the `voltnuerongridd` process exits. The current durability picture:

| Layer | Implementation | Durability |
|---|---|---|
| WAL durability | `raft_meta.json`, `acid_write_sets.json`, `persist_committed_write_sets` | ✅ Survives restart (Raft log + write-set journal) |
| Page-level durability | `PagedRowStore` (in-memory) | ❌ All row data lost on exit |

This gap means:
- REQ-17 "zero data loss" cannot be guaranteed
- REQ-16 "encryption-at-rest" has no data to encrypt durably
- The crash-recovery gate (`run-crash-recovery-gate.ps1`) reports `rows_survived=false`
- WS6 RTO/RPO scores are valid for Raft leadership recovery but not for data recovery

**Terminology adopted throughout this project:**
- **WAL durability** = write-ahead log records that track committed statements for crash recovery (already implemented)
- **Page-level durability** = row data written to a persistent on-disk store (this feature)

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Rows survive a process kill+restart (Priority: P1)

An operator inserts 100 rows, then the `voltnuerongridd` process is killed (SIGKILL / force-terminate). After restart, all 100 rows are queryable via `SELECT`.

**Why this priority**: This is the minimum bar for claiming "durable database." Without it, every other feature that acknowledges a write is lying to the caller.

**Independent Test**: Can be fully tested by running `run-crash-recovery-gate.ps1 -RequireRowSurvival` against a live server, inserting rows, killing the process, restarting, and querying.

**Acceptance Scenarios**:

1. **Given** 100 rows are inserted and committed via `/api/v1/sql/execute`, **When** the process is force-killed and restarted, **Then** `SELECT * FROM <table>` returns all 100 rows with correct values.
2. **Given** the server restarts after a crash mid-transaction (uncommitted), **When** the server comes back up, **Then** the uncommitted rows are NOT visible (crash consistency).
3. **Given** rows spanning multiple MVCC versions, **When** the server restarts, **Then** only committed versions are visible at the correct snapshot.

---

### User Story 2 — Multi-database isolation survives restart (Priority: P1)

After restart, rows from database `db_a` are not visible in database `db_b`.

**Why this priority**: Database-level isolation is a fundamental correctness property.

**Independent Test**: Create two DBs, insert distinct rows, restart, verify isolation is maintained.

**Acceptance Scenarios**:

1. **Given** databases `db_a` and `db_b` each have rows, **When** the process restarts, **Then** `SELECT` in `db_a` returns only `db_a` rows and `SELECT` in `db_b` returns only `db_b` rows.

---

### User Story 3 — WAL replay recovers committed rows (Priority: P2)

Rows written to the Raft log (WAL durability layer) are replayed into the row store on startup.

**Why this priority**: This bridges WAL durability and page-level durability — the WAL already tracks committed DML statements; replay should reconstruct the row store without needing a separate persistence path.

**Independent Test**: Insert rows via the full Raft+SQL path, kill, restart, verify `raft_meta.json` replay restores the rows.

**Acceptance Scenarios**:

1. **Given** committed DML is in `raft_meta.json` log, **When** server restarts, **Then** `apply_committed_entries` replays the log and all committed rows are present.
2. **Given** a snapshot was installed before crash, **When** server restarts, **Then** the snapshot rows are loaded and log replay starts from `snapshot_index + 1`.

---

### User Story 4 — Crash recovery gate passes (Priority: P2)

`run-crash-recovery-gate.ps1 -RequireRowSurvival` exits 0.

**Why this priority**: This is the evidence gate that closes the "zero data loss" credibility gap in REQ-17 and WS6.

**Independent Test**: Run the crash-recovery gate script end-to-end with `-RequireRowSurvival`.

**Acceptance Scenarios**:

1. **Given** the gate runs with `-RequireRowSurvival`, **When** it completes, **Then** `crash-recovery-smoke.json` contains `status:"passed"`, `rows_survived:true`, `page_level_durability_implemented:true`.

---

## Technical Design Constraints

### Option A — WAL-only replay (lowest effort, recommended first step)

On server startup, replay all committed DML from the Raft log (`raft_meta.json`) by running each committed statement through `apply_dml_command`. This re-populates `PagedRowStore` from the durable Raft log without changing the row store itself.

**Pros:** No changes to `PagedRowStore`, leverages existing WAL code, minimal risk.
**Cons:** Replay time grows with log size; requires log compaction to stay fast.
**Effort:** S (< 1 week).

### Option B — File-backed PagedRowStore (medium effort)

Serialize `PagedRowStore` pages to a file (e.g. JSON or binary) on every commit; load on startup.

**Pros:** Fast startup for large datasets; decoupled from log size.
**Cons:** Durability window = last flush; atomic write needed to avoid torn pages.
**Effort:** M (2–3 weeks).

### Option C — RocksDB primary store (high effort, full production)

Route all primary row writes through the existing `DurabilityEngine` / `RocksDB` adapter (`crates/voltnuerongrid-store/src/rocksdb_engine.rs`). `PagedRowStore` becomes a read cache.

**Pros:** Production-grade durability, atomic writes, compaction, point reads.
**Cons:** Significant refactor; requires all write paths to go through `DurabilityEngine`.
**Effort:** L (1–2 months).

**Recommended approach:** Implement Option A first (WAL replay on boot) to unblock REQ-17 and the crash-recovery gate. Then track Option B/C as separate follow-on work.

---

## File Impact

| File | Change |
|---|---|
| `services/voltnuerongridd/src/helpers/boot.rs` | Add WAL replay loop: iterate committed log entries, call `apply_dml_command` on each |
| `services/voltnuerongridd/src/raft.rs` | Expose `committed_log_entries()` or iterate via `RaftNode` snapshot+log API |
| `services/voltnuerongridd/src/helpers/raft_loop.rs` | Ensure `persist_raft_state` is called before any committed entry acknowledgment |
| `crates/voltnuerongrid-store/src/mvcc.rs` | Optional: add `export_pages()` / `import_pages()` for Option B |
| `tests/kpi/scripts/run-crash-recovery-gate.ps1` | Already created; update once rows survive to enforce `-RequireRowSurvival` |

---

## Acceptance Gates

| Gate | Command / Script | Must Pass |
|---|---|---|
| Cargo unit tests | `cargo test -p voltnuerongridd` | 818+ passed, 0 failed |
| Store crate tests | `cargo test -p voltnuerongrid-store` | All pass |
| Crash recovery gate (hard) | `pwsh run-crash-recovery-gate.ps1 -SkipServerManagement -RequireRowSurvival` (after WAL replay implemented) | `rows_survived:true` |
| WS6 gate | `pwsh run-ws6-gate.ps1` | status:passed |
| WS5 gate | `pwsh run-ws5-gate.ps1` | status:passed |

---

## Definition of Done

- [ ] Committed rows are present after `cargo run -p voltnuerongridd` is killed and restarted
- [ ] `cargo test -p voltnuerongridd` passes with 818+ tests
- [ ] `run-crash-recovery-gate.ps1 -RequireRowSurvival` exits 0
- [ ] `crash-recovery-smoke.json` has `page_level_durability_implemented: true`
- [ ] Status tracker REQ-17 and WS6 completion updated from 75% to ≥90%
- [ ] `gaps-4.md` C1 gap closed with evidence commit SHA

---

## Related Items

- Gap tracker: `docs/gaps-4.md` — C1 (durable row store)
- Crash recovery gate: `tests/kpi/scripts/run-crash-recovery-gate.ps1`
- REQ-17 in `docs/archive/status_tracker.md`
- WAL persistence: `services/voltnuerongridd/src/helpers/raft_loop.rs` — `persist_raft_state`
- RocksDB adapter: `crates/voltnuerongrid-store/src/rocksdb_engine.rs`
