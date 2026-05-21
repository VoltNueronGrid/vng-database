# CLAUDE.md — VoltNueronGrid DB (polap-db)

> Auto-loaded by Claude Code at session start. Keep this up to date after each session.

---

## Project Overview

**VoltNueronGrid DB** (`voltnuerongridd`) is a distributed HTAP database engine written in Rust.
- **Service binary:** `services/voltnuerongridd/` — axum HTTP API, SQL engine, Raft consensus, MVCC store
- **Crates:** `crates/` — modular libraries (store, auth, sql, exec, audit, ingest, driver, mcp, …)
- **Branch for Claude work:** `claude/friendly-hertz-3b69fb` (worktree at `.claude/worktrees/friendly-hertz-3b69fb/`)

---

## Quick Commands

```bash
# Build (from worktree root)
cargo build -p voltnuerongridd

# Test — 749 tests, must stay green
cargo test -p voltnuerongridd

# Check (fast, no test binary)
cargo check -p voltnuerongridd

# Run locally
VNG_NATIVE_LISTENER_ENABLED=false VNG_ADMIN_API_KEY=secret cargo run -p voltnuerongridd
```

> **IMPORTANT — worktree path discipline:**  
> The worktree has its OWN `crates/` at `.claude/worktrees/friendly-hertz-3b69fb/crates/`.  
> Always edit `.claude/worktrees/friendly-hertz-3b69fb/crates/…`, NOT the main repo's `crates/`.  
> The service `Cargo.toml` resolves `path = "../../crates/…"` relative to the worktree.

---

## Key Environment Variables

| Variable | Default | Purpose |
|---|---|---|
| `VNG_ADMIN_API_KEY` | — | Required; admin auth header value |
| `VNG_NODE_ID` | `node-1` | Raft node identity |
| `VNG_RAFT_PEERS` | _(empty)_ | Comma-separated peer base URLs e.g. `http://node-2:8080,http://node-3:8080` |
| `VNG_CLUSTER_TOKEN` | _(none)_ | Shared bearer token for intra-cluster Raft RPCs |
| `VNG_RBAC_POLICY_PATH` | _(none)_ | Path to `RbacPrivilegeMatrix` JSON file |
| `VNG_AUDIT_LOG_PATH` | _(none)_ | Append-only audit log file path |
| `VNG_DATA_DIR` | `./data` | Row-store / WAL persistence directory |
| `VNG_NATIVE_LISTENER_ENABLED` | `true` | Set `false` to skip TLS native listener |

All variables are optional; safe single-node defaults apply when unset.

---

## Architecture — Key Files

### Service binary (`services/voltnuerongridd/src/`)

| File | Role |
|---|---|
| `main.rs` | `AppState` struct, startup, module re-exports. Contains glob imports needed by `tests.rs` — **do NOT remove glob `use` lines** |
| `raft.rs` | `RaftNode`, all Raft logic (elections, log, snapshots, append, vote) |
| `router.rs` | axum route registrations |
| `tests.rs` | 749 unit/integration tests |
| `handlers/sql.rs` | `sql_execute` — DDL, DML, SELECT routing |
| `handlers/raft.rs` | Raft HTTP handlers (vote, append, install_snapshot, status, …) |
| `helpers/raft_loop.rs` | Background tick loop, heartbeat fanout, log apply loop, snapshot fanout |
| `helpers/sql_parse.rs` | SQL parsing utilities |
| `helpers/boot.rs` | WAL replay on startup |
| `auth/` | RBAC privilege checks |

### Store crate (`crates/voltnuerongrid-store/src/`)

| File | Role |
|---|---|
| `mvcc.rs` | `PagedRowStore` — MVCC row store with `replace_all` for snapshot install |

---

## AppState — Important Fields

```rust
pub(crate) struct AppState {
    pub(crate) row_store: Arc<Mutex<PagedRowStore>>,
    pub(crate) raft_state: Arc<Mutex<RaftNode>>,
    pub(crate) raft_peers: Arc<Vec<String>>,
    pub(crate) cluster_token: Arc<Option<String>>,
    // Watch channel: apply loop sends new last_applied → linearisable write handlers wait here
    pub(crate) raft_last_applied_tx: Arc<tokio::sync::watch::Sender<u64>>,
    pub(crate) wal_engine: Arc<Mutex<BoxedDurabilityEngine>>,
    // ... many more
}
```

---

## Session History

| Session | Commit | Key Deliverables |
|---|---|---|
| 1–13 | various | Initial scaffolding, SQL engine, RBAC, ingest, MVCC, WAL |
| 14 | `3e66441` | Full modular refactor — 12 handler + helper modules extracted |
| 15 | `bfd797d` | Phase 3: Real RBAC matrix, Raft replication scaffold |
| 16 | `0564a24` | Tests green (749), DataFusion OLAP, Raft log replication, cluster auth |
| 17 | `6088409` | Raft `next_index`, cluster auth handlers, DataFusion `olap_agg` |
| 18 | `508d6d6` | Raft apply loop, randomised timeouts, log compaction |
| 19 | `0ce3f53` | Leader append path, snapshot transfer DTOs, dead-code audit start |
| 20 | `db230a9` | `replace_all` snapshot install, linearisable writes (watch channel), `handle_install_snapshot`, dead-code cleanup |
| 21 | `dd6ba12` | Linearisable writes, snapshot install, DataFusion, import cleanup |
| 22–23 | `577feb1`/`74308e1` | Chunked snapshot transfer, DML proxy, 503 propagation |
| 24–25 | `6f93c80` | Snapshot edge cases, DML proxy, full response fidelity |
| 26 | `8ae2f4f` | VNG_CLUSTER_TOKEN forwarding through DML proxy, Codacy, startup script |

> **Latest commit on `claude/friendly-hertz-3b69fb`:** `db230a9`  
> **Latest commit on `main`:** `ff495db` (start-vng-local script improvements)

---

## Raft Implementation — Key Concepts

### RaftNode methods
- `tick()` — advance election timer; triggers Follower→Candidate transition
- `handle_vote_request(req)` — respond to RequestVote RPC
- `handle_append_entries(req)` — respond to AppendEntries RPC (heartbeat or log replication)
- `handle_install_snapshot(req)` — apply a full snapshot from leader (truncates log, advances indices)
- `append_command(cmd, peers)` — **fire-and-forget**: pre-advances `last_applied` (caller already wrote state)
- `append_command_pending(cmd, peers)` — **linearisable**: does NOT advance `last_applied`; apply loop is sole writer

### Linearisable write flow (multi-node leader)
1. `sql_execute` calls `append_command_pending` for each DML statement → gets `pending_index`
2. Subscribes to `raft_last_applied_tx` watch channel
3. Waits up to 2s for `last_applied >= pending_index`
4. If timeout → HTTP 503 `raft_quorum_timeout`
5. Apply loop (`apply_committed_entries`) fires `raft_last_applied_tx.send(new_last_applied)` after each batch

### Snapshot transfer flow
- `fanout_heartbeat` checks `next_index[peer] <= snapshot_index`
- If true: exports full row-store, sends `POST /api/v1/cluster/raft/install_snapshot` (chunked)
- Follower: `raft_install_snapshot` handler calls `RaftNode::handle_install_snapshot` then `rs.replace_all(rows)`

---

## Known Issues / Gotchas

1. **Glob imports in `main.rs` are load-bearing for tests** — `use handlers::cdc::*`, `use handlers::admin::*`, etc. are used by `tests.rs` via `crate::*`. Running `cargo fix --bin voltnuerongridd` removes them, breaking the test binary with ~1385 errors. Do NOT run `cargo fix` on `main.rs`.

2. **~20 unused-import warnings remain** — standalone (non-glob) unused imports in `main.rs` and handler files. Safe to remove line-by-line but must avoid touching the glob imports above.

3. **Worktree has its own `crates/` copy** — any change to `crates/voltnuerongrid-store/src/mvcc.rs` must be made in `.claude/worktrees/friendly-hertz-3b69fb/crates/…`, not in the main repo.

---

## What's Still TODO (from remaining.md v21)

### High priority
1. **Integration test for linearisable write path** — no test yet covers the multi-node quorum wait (watch channel + `append_command_pending`). Mock two peers or use two `AppState` instances.
2. **DataFusion OLAP coverage** — some query shapes still use hand-rolled paths in `handlers/sql.rs`. Audit and fill gaps.

### Medium priority
3. **Standalone import cleanup** — surgical per-line removal of ~20 non-glob warnings (never touch glob `use module::*` lines).
4. **`replace_all` unit test** — insert rows, call `replace_all`, verify old rows are gone.
5. **`append_command_pending` unit test** — verify `last_applied` not pre-advanced (multi-node) but `commit_index` is advanced (single-node).

---

## How to Start Next Session

```
@CLAUDE.md
@remaining.md
@services/voltnuerongridd/src/handlers/sql.rs
@services/voltnuerongridd/src/helpers/raft_loop.rs
@services/voltnuerongridd/src/raft.rs
```

Then run `cargo test -p voltnuerongridd` to confirm 749 tests still green before making changes.
