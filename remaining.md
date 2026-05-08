# `remaining.md` — handoff for next session (v16)

**Last updated:** 2026-05-08 (fifteenth session — Phase 3: Real RBAC + Raft replication)
**Branch:** `main`
**Latest commit:** `bfd797d` — pushed to `origin/main` and `vng-database/main`
**cargo check -p voltnuerongridd:** clean ✓

---

## TL;DR — what landed this session

### ✅ Phase 3 — Real RBAC

**`config_init.rs`**
- `load_rbac_privilege_matrix()` — reads `VNG_RBAC_POLICY_PATH` JSON file (full `RbacPrivilegeMatrix` struct); falls back silently to `default_rbac_privilege_matrix()` if unset or parse fails
- JSON format: `{"grants_by_role": {"dba": [{"resource": "...", "scopes": ["..."], "actions": ["read"]}]}}`

**`main.rs`**
- `default_rbac_privilege_matrix()` → `load_rbac_privilege_matrix()` at startup

### ✅ Phase 3 — Raft replication

**`config_init.rs`**
- `load_raft_peers()` — reads `VNG_RAFT_PEERS` (comma-separated base URLs, e.g. `http://node-2:8080,http://node-3:8080`); returns empty vec for single-node default

**`helpers/raft_loop.rs`** (new file)
- `run_raft_tick_loop(state)` — 150ms tick loop spawned at startup
  - Calls `RaftNode::tick()` each iteration; drives Follower→Candidate transition
  - On election start: sends `POST /api/v1/cluster/raft/vote` to all peers via reqwest (100ms timeout); counts votes; quorum = ceil(n/2); single-node wins immediately (no peers → votes=1, quorum=1)
  - On Leader: sends empty `POST /api/v1/cluster/raft/append` heartbeat to each peer every 3 ticks (~450ms) to suppress their election timers

**`main.rs`**
- Fixed hardcoded `RaftNode::new("node-1")` → `RaftNode::new(&node_id)` (uses actual `VNG_NODE_ID`)
- Added `raft_peers: Arc<Vec<String>>` to `AppState`
- Spawns `run_raft_tick_loop(state.clone())` at startup alongside dr_hook_scheduler

**`Cargo.toml`**
- Added `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }`

---

## Known pre-existing issues (not introduced by Phase 3)

`cargo test -p voltnuerongridd` fails to compile the test binary (519 errors):
- **E0616** (~500) — `tests.rs` accesses private fields of response structs defined in handler modules. Fields need `pub(crate)` or tests need to use constructors.
- **E0597** (2) — lifetime issue in `resilience.rs`.
- **E0659** (2) — ambiguous name `build_sre_gate_evaluation`.

These predate Phase 3. `cargo check -p voltnuerongridd` (no test binary) is fully clean.

---

## What's still TODO

### High priority

1. **Fix tests.rs E0616 / E0659** — ~500 field-privacy errors prevent the test binary from building. Fix: add `pub(crate)` to all fields of handler response structs (raft, wal, sre, audit, misc), or refactor tests to avoid direct field access.

2. **DataFusion wiring** — `voltnuerongrid-exec-datafusion` crate has a working executor, but `handlers/sql.rs` still calls hand-rolled OLAP paths. Wire `OlapQueryRequest` through the DataFusion executor.

### Medium priority

3. **Raft log replication** — heartbeat fanout works but leaders don't yet replicate actual log entries to followers. Wire `raft_state.log` entries into `AppendEntries.entries` before each heartbeat.

4. **Vote response parsing** — `run_election` currently checks `resp.vote_granted` from peer JSON. The peer `raft_vote` handler requires a valid operator auth header. Either bypass auth for intra-cluster RPCs or add a cluster-identity bearer token (`VNG_CLUSTER_TOKEN`).

---

## How to continue

```
@remaining.md
@services/voltnuerongridd/src/helpers/raft_loop.rs
@services/voltnuerongridd/src/tests.rs
@services/voltnuerongridd/src/handlers/raft.rs
```

Recommended next steps (in priority order):
1. **Fix tests.rs** — make handler response struct fields `pub(crate)`; fix E0659 by qualifying the ambiguous name
2. **DataFusion wiring** — route OLAP queries through exec-datafusion executor
3. **Raft log replication** — send actual log entries in leader heartbeats
4. **Cluster auth token** — add `VNG_CLUSTER_TOKEN` for intra-cluster Raft RPCs

**Environment note:** `VNG_RBAC_POLICY_PATH` and `VNG_RAFT_PEERS` are new env vars. Unset = safe defaults (hardcoded RBAC, single-node Raft).
