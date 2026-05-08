# `remaining.md` — handoff for next session (v22)

**Last updated:** 2026-05-08 (session 22 — chunked snapshot transfer, 503 propagation, dead-code cleanup)
**Branch:** `main`
**Latest commit:** (to be set after commit)
**cargo check -p voltnuerongridd:** clean ✓ (27 warnings — all safe to ignore)
**cargo test -p voltnuerongridd:** 750/750 ✓

---

## TL;DR — what landed this session

### ✅ 503 propagation in sql_execute

**`handlers/sql.rs`** — `block_in_place` quorum wait now returns `bool`; on timeout returns HTTP 503 with `"raft_quorum_timeout"` reason instead of silently succeeding.

### ✅ Dead-code removal

**`helpers/sql_parse.rs`** — removed unused `parse_where_predicates` function (~33 lines).

### ✅ Chunked snapshot transfer

Splits the full row-store export into fixed-size chunks (500 rows/chunk) sent sequentially by the leader to lagging followers.

**`raft.rs`** — two new types:
- `RaftSnapshotChunkRequest` — carries `session_id`, `chunk_index`, `is_last`, and `rows: Vec<(String, serde_json::Value)>`
- `RaftSnapshotChunkResponse` — carries `success`, `next_expected_chunk`, `complete`

**`main.rs`** — new supporting types/state:
- `SnapshotChunkSession` struct accumulates in-flight chunks by `session_id`
- `AppState::snapshot_chunk_sessions: Arc<Mutex<HashMap<String, SnapshotChunkSession>>>` field
- `snapshot_chunk_sessions: Arc::new(Mutex::new(HashMap::new()))` initialised in `async fn main()`
- Re-exports updated: `RaftSnapshotChunkRequest`, `RaftSnapshotChunkResponse`

**`handlers/raft.rs`** — new `raft_install_snapshot_chunk` handler at `POST /api/v1/cluster/raft/install_snapshot/chunk`:
- Accepts intra-cluster token OR operator credentials
- Accumulates chunk rows into `snapshot_chunk_sessions`; on `is_last=true` calls `handle_install_snapshot` then `replace_all`
- Rejects stale-term chunks and out-of-order chunks (returns `success: false` with `next_expected_chunk`)

**`router.rs`** — route registered at `/api/v1/cluster/raft/install_snapshot/chunk`

**`helpers/raft_loop.rs`** — `fanout_heartbeat` updated:
- Added `snapshot_index`, `snapshot_term` parameters (captured from `RaftNode` each tick)
- `SNAPSHOT_CHUNK_SIZE = 500` constant
- Peers whose `next_index <= snapshot_index` receive chunked snapshot transfer (fire-and-forget `tokio::spawn`) instead of `AppendEntries`; normal peers still receive `AppendEntries` via `join_set`

**`tests.rs`** — `snapshot_chunk_sessions: Arc::new(Mutex::new(HashMap::new()))` added to test `AppState` initializer.

---

## Known pre-existing issues

- 27 remaining unused-import warnings in the binary (not errors); all safe.

---

## What's still TODO

1. **Chunked snapshot — retry/resume** — if a chunk send fails mid-session, the follower resets to `next_expected_chunk = 0` but the leader's spawned task just aborts. Next tick starts a new session from chunk 0, which is correct but wastes bandwidth for large snapshots. A per-peer session-id can be made stable (e.g. `snap-<leader>-<peer>-<snapshot_index>`) to allow true resume.

2. **Raft snapshot transfer — `next_index` advance** — after a successful chunked install, the follower's `match_index` / `next_index` on the leader is not updated (the spawned task is fire-and-forget). The leader should update `next_index[peer] = snapshot_index + 1` after receiving `complete: true` from the chunk response.

3. **`raft_install_snapshot_chunk` row-store encoding** — same as the existing `raft_install_snapshot` limitation: `serde_json::Value::Object` → `HashMap<String, String>` by stringifying values; non-object rows become empty maps.

4. **Linearisable write — follower DML forwarding** — when a follower receives DML from a client, it should redirect to the leader or buffer until leadership is established (currently eventually consistent).

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
1. **Fix next_index advance after chunked snapshot** — parse chunk response in `fanout_heartbeat` and call `record_append_success` / advance `next_index[peer]`
2. **Stable session-id for chunk resume** — incorporate `snapshot_index` into session key so partial transfers can resume
3. **Follower DML forwarding** — redirect writes received by followers to the current leader

**Environment notes:**
- `VNG_CLUSTER_TOKEN` — shared secret for intra-cluster Raft RPCs
- `VNG_RAFT_PEERS` — comma-separated peer base URLs
- `VNG_RBAC_POLICY_PATH` — JSON RBAC privilege matrix
