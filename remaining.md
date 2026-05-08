# `remaining.md` — handoff for next session (v21)

**Last updated:** 2026-05-08 (session 20 — Raft snapshot install, linearisable writes)
**Branch:** `claude/friendly-hertz-3b69fb`
**Latest commit:** session 20 (pending push)
**cargo test -p voltnuerongridd:** 749 passed, 0 failed ✓

---

## TL;DR — what landed in session 20

### ✅ PagedRowStore::replace_all (snapshot install)

**`crates/voltnuerongrid-store/src/mvcc.rs`**
- Added `replace_all(&mut self, rows: impl IntoIterator<Item = (String, RowData)>)` — resets pages to a single empty page, clears write intents, then inserts all rows in a new transaction.
- Used by `raft_install_snapshot` to atomically replace the full row-store state from a leader snapshot.

**`services/voltnuerongridd/src/handlers/raft.rs`**
- Added `raft_install_snapshot` handler at `POST /api/v1/cluster/raft/install_snapshot`
- Accepts intra-cluster token OR operator credentials (same dual-auth as `raft_append`)
- Calls `RaftNode::handle_install_snapshot` first (advances `snapshot_index`, `last_applied`), then `rs.replace_all(req.rows)` if successful

**`services/voltnuerongridd/src/router.rs`**
- Registered `POST /api/v1/cluster/raft/install_snapshot`

### ✅ RaftNode methods for leader append

**`services/voltnuerongridd/src/raft.rs`**
- `append_command(command, total_peers) -> u64` — fire-and-forget: appends entry, pre-advances `last_applied` so the apply loop never re-executes, advances `commit_index` on single-node.
- `append_command_pending(command, total_peers) -> u64` — linearisable path: appends entry, does NOT advance `last_applied` (apply loop is sole writer), advances `commit_index` on single-node only.
- `handle_install_snapshot(req) -> RaftInstallSnapshotResponse` — truncates log, updates snapshot_index/term, advances commit_index/last_applied to snapshot boundary.
- `RaftInstallSnapshotRequest` / `RaftInstallSnapshotResponse` structs added.

### ✅ Linearisable leader writes in sql_execute

**`services/voltnuerongridd/src/handlers/sql.rs`**
- DML path branches on `is_multi_node_leader` (role=Leader AND peers>0):
  - **Multi-node leader**: calls `append_command_pending` for each DML statement, then subscribes to `raft_last_applied_tx` watch channel and waits up to 2s for `last_applied >= max_pending_index`. Returns HTTP 503 `raft_quorum_timeout` if quorum not reached.
  - **Single-node / non-leader**: direct row_store write + `append_command` (unchanged behaviour).

**`services/voltnuerongridd/src/main.rs`** / **`tests.rs`**
- Added `raft_last_applied_tx: Arc<tokio::sync::watch::Sender<u64>>` to `AppState`.
- Initialized as `tokio::sync::watch::channel(0).0` in both `main()` and test fixture.

**`services/voltnuerongridd/src/helpers/raft_loop.rs`**
- `apply_committed_entries` now sends to `raft_last_applied_tx` after advancing `last_applied`, unblocking any waiting linearisable write handlers.
- `fanout_heartbeat` detects `needs_snapshot` (when `next_index[peer] <= snapshot_index`) and sends `POST /install_snapshot` with the full row-store snapshot instead of log entries.

### ✅ Dead-code audit (partial)

**`services/voltnuerongridd/src/helpers/sql_parse.rs`**
- Removed `parse_where_predicates` function (dead code).

**Note on unused-import sweep**: `cargo fix --bin voltnuerongridd --allow-dirty` removed too aggressively — glob imports needed by `tests.rs` were dropped, causing 1385 test compile errors. The fix was reverted for `main.rs`. ~20 warnings remain (unused standalone imports in `main.rs` + handler files). These are safe to remove but require careful per-line edits that don't disturb the glob imports that `tests.rs` relies on.

---

## Previous sessions summary (sessions 16–19)

- Session 16: tests green, DataFusion OLAP wire-up, Raft log replication, cluster auth
- Session 17: Raft `next_index`, cluster auth handlers, DataFusion `olap_agg`
- Session 18: Raft apply loop, randomised timeouts, log compaction
- Session 19: Leader append path, snapshot transfer DTOs, dead-code audit start

---

## What's still TODO

### High priority

1. **Verify linearisable write path end-to-end** — the watch channel and `append_command_pending` are wired up, but no integration test covers the multi-node quorum wait. Add a test that spawns two `AppState` instances (or mocks two peers) and confirms a DML write blocks until `raft_last_applied_tx.send()` fires.

2. **DataFusion wiring** — `voltnuerongrid-exec-datafusion` has a working executor, but `handlers/sql.rs` still calls hand-rolled OLAP paths for some query shapes. Validate coverage and fill gaps.

### Medium priority

3. **Unused-import sweep (safe subset)** — ~20 standalone `use` warnings remain in `main.rs`. Remove them line by line, being careful NOT to remove glob imports (`use handlers::cdc::*` etc.) that `tests.rs` depends on. Alternatively, make tests.rs import explicitly so glob imports can be safely removed.

4. **replace_all test** — add a unit test in `crates/voltnuerongrid-store/src/mvcc.rs` (or its test file) that exercises `replace_all` (insert rows, call replace_all with new rows, verify old rows are gone).

5. **append_command_pending test** — add a test in `services/voltnuerongridd/src/raft.rs` that verifies `last_applied` is NOT pre-advanced (multi-node case) but `commit_index` IS advanced (single-node case).

---

## How to continue

```
@remaining.md
@services/voltnuerongridd/src/handlers/sql.rs
@services/voltnuerongridd/src/helpers/raft_loop.rs
@services/voltnuerongridd/src/raft.rs
```

Recommended next steps (in priority order):
1. **Integration test for linearisable writes** — confirm quorum wait works
2. **DataFusion coverage** — audit which OLAP shapes are wired vs. hand-rolled
3. **Standalone import cleanup** — surgical removal of the ~20 non-glob warnings
4. **replace_all + append_command_pending unit tests**

**Environment note:** `VNG_CLUSTER_TOKEN`, `VNG_RAFT_PEERS`, `VNG_RBAC_POLICY_PATH`, `VNG_NODE_ID` are the key env vars. All default safely for single-node dev.
