# `remaining.md` — handoff for next session (v23)

**Last updated:** 2026-05-08 (session 23 — chunked snapshot improvements, encoding fidelity, follower DML forwarding)
**Branch:** `main`
**Latest commit:** (to be set after commit)
**cargo check -p voltnuerongridd:** clean ✓ (27 warnings — all safe to ignore)
**cargo test -p voltnuerongridd:** 750/750 ✓

---

## TL;DR — what landed this session

### ✅ next_index advance after chunked snapshot (Item 1)

**`helpers/raft_loop.rs`** — the spawned chunk-send task now parses the final `RaftSnapshotChunkResponse`.  When `complete == true`, the leader calls `record_append_success(peer_url, snapshot_index, total_nodes)` which advances `next_index[peer] = snapshot_index + 1`. This stops the leader re-sending the full snapshot on every heartbeat tick.

### ✅ Stable session-id for chunk resume (Item 2)

**`helpers/raft_loop.rs`** — session-id changed from `snap-{node_id}-{peer_url}` to `snap-{node_id}-{peer_url}-{snapshot_index}`. Each unique snapshot version gets its own session key, so incomplete sessions from older snapshots never interfere with a newer snapshot transfer.

### ✅ Row-store encoding fidelity (Item 3)

**`handlers/raft.rs`** — added `fn json_value_to_str(v: &serde_json::Value) -> String` helper. Replaces `.as_str().unwrap_or("")` in both `raft_install_snapshot` and `raft_install_snapshot_chunk`. Numbers, booleans, and other non-string JSON scalars now round-trip correctly instead of becoming empty strings.

### ✅ Follower DML forwarding (Item 4)

**`main.rs`** — two new `AppState` fields:
- `node_url: Arc<Option<String>>` — loaded from `VNG_NODE_URL` env var; the leader's own advertised base URL
- `current_leader_url: Arc<Mutex<Option<String>>>` — updated from `x-vng-leader-url` header on every accepted AppendEntries

**`helpers/raft_loop.rs`** — `fanout_heartbeat` sends `x-vng-leader-url: <VNG_NODE_URL>` header in every AppendEntries RPC so followers learn the leader's URL.

**`handlers/raft.rs`** (`raft_append`) — stores `x-vng-leader-url` header into `state.current_leader_url` when an AppendEntries is accepted.

**`handlers/sql.rs`** (`sql_execute`) — follower DML path now returns HTTP 503 with reason `"not_leader: retry DML against leader at <url>"` (or `"not_leader: no known leader yet; retry later"`) in multi-node clusters (`state.raft_peers.len() > 0`). Single-node mode falls through unchanged so existing single-node tests pass.

---

## What's still TODO

1. **Follower DML — pre-apply check** — the local row-store write still happens before the leadership check returns 503. A future PR could add an early leadership check at the top of the DML execution path to avoid the wasted write on followers.

2. **`VNG_NODE_URL` documentation** — the new env var for leader URL broadcasting is not yet mentioned in any README or deployment guide.

3. **Chunked snapshot — `is_last` edge case for empty row stores** — when the row store is empty (0 rows), `chunks` is empty and `total_chunks = 0`, so the loop body never executes and no final chunk is sent to the follower. The follower's session is never completed. A guard should send a single empty `is_last=true` chunk.

4. **Follower DML forwarding — transparent proxy** — current implementation returns 503 and asks the client to retry. A future improvement could transparently proxy the forwarded request to the leader and return the leader's response directly.

---

## How to continue

```
@remaining.md
@services/voltnuerongridd/src/raft.rs
@services/voltnuerongridd/src/helpers/raft_loop.rs
@services/voltnuerongridd/src/handlers/raft.rs
@services/voltnuerongridd/src/handlers/sql.rs
```

Recommended next steps (in priority order):
1. **Fix empty row-store edge case** — when `all_rows` is empty, emit one chunk with `is_last=true` and `rows=[]`
2. **Pre-apply leadership check in sql_execute** — check leader role before the DML row-store block, return 503 early
3. **Document `VNG_NODE_URL`** in deployment notes

**Environment notes:**
- `VNG_CLUSTER_TOKEN` — shared secret for intra-cluster Raft RPCs
- `VNG_RAFT_PEERS` — comma-separated peer base URLs
- `VNG_NODE_URL` — this node's own advertised base URL (new — used by followers to learn leader URL)
- `VNG_RBAC_POLICY_PATH` — JSON RBAC privilege matrix
