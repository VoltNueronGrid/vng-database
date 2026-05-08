# `remaining.md` — handoff for next session (v15)

**Last updated:** 2026-05-08 (fourteenth session — full modular refactor complete + compile fixed)
**Branch:** `main`
**Total unit tests:** 556 passing (voltnuerongridd service; pre-existing failures in resilience.rs / exec-datafusion unchanged)
**Phase:** main.rs modular refactor DONE — all slices merged, compile verified clean

---

## TL;DR — what landed this session

### ✅ main.rs modular refactor — Slices 5–10 (merged from vng-database/main)

All remaining handler and helper modules were merged from the upstream branch. The post-merge tree had **422 compile errors** caused by missing `use` statements in each extracted module (previously everything lived in main.rs scope). All 422 errors were fixed, achieving a clean compile.

**Final module layout:**

```
handlers/
  audit.rs      rows.rs       wal.rs
  cluster.rs    sre.rs
  dr.rs         sql.rs
  misc.rs       store.rs
  raft.rs

helpers/
  boot.rs           env_helpers.rs    native_protocol.rs
  cluster.rs        execution.rs      sql_parse.rs
  dr_hook.rs        time.rs           udf.rs
```

**Key fixes applied (422 → 0 errors):**

1. `router.rs` — changed `use handlers::*` → `use crate::handlers::*` for all 15 handler modules; added `use axum::routing::options;` and `use axum::middleware::from_fn;`; moved inner middleware functions from `main()` to module level: `add_cors`, `options_preflight`, `track_http_metrics`, `coarsen_route_for_metrics`

2. `main.rs` — made the following `pub(crate)`:
   - Statics: `TX_COUNTER`, `PESSIMISTIC_LOCK_COUNTER`, `WS22_GATE_DEADLOCK_DETECTIONS`, `WS22_GATE_SCAN_CAP_TIMEOUTS`
   - Const: `DEADLOCK_SCAN_MAX_HOPS`
   - Enums: `NativeFrameType`, `NativeCommandKind`, `DeadlockScanOutcome`
   - Functions: `run_native_connection`, `native_read_framed`
   - Added comprehensive re-export block (`pub(crate) use auth::...`, `pub(crate) use handlers::sre::...`, etc.)
   - Fixed duplicate raft import: kept `use raft::RaftNode;` + `pub(crate) use raft::{all other types};`

3. `handlers/misc.rs` — made `NativeFrame`, `NativeListenerConfig`, `NativeAdapter`, `HealthResponse`, `FailoverHandoffGapResponse`, `FailoverHandoffReportResponse` fields all `pub(crate)`; added full import block including `use tokio::io::AsyncWriteExt;`

4. `handlers/raft.rs` — fixed `role: raft::RaftRole` → `role: RaftRole`; added crate-level imports for Raft types

5. All handler/helper modules — added their required `use` statements (std, serde, voltnuerongrid-*, crate::)

**main.rs line count:** ~24,962 (Slice 4) → **1,936 lines** (all slices complete)

**`cargo check -p voltnuerongridd`**: clean ✓  
**`cargo test -p voltnuerongridd`**: 556 passing ✓  
**Latest commit:** `a50ece1` — pushed to both `origin/claude/friendly-hertz-3b69fb` and `vng-database/main`

---

## What's still TODO

### Verification

1. **Phase 2.3 check** — verify deprecated text WAL helpers are gone:
   ```
   grep -rn "#\[deprecated\]" services/voltnuerongridd/src/
   ```

### Phase 3 extended

2. **Real RBAC** — `helpers/boot.rs::default_rbac_privilege_matrix()` is hardcoded. Replace with configurable matrix loaded from `config_init.rs`. The `voltnuerongrid-config` crate already has the scaffolding.

3. **Replication** — wire up the Raft skeleton for leader/follower replication:
   - `raft_append` / `raft_vote` handlers exist in `handlers/raft.rs`
   - The `RaftNode` in `main.rs` needs a background tick task + HTTP fanout to peers
   - Peer list comes from `ClusterNodeRuntime` map in `AppState`

### Phase 4

4. **DataFusion integration** — `voltnuerongrid-exec-datafusion` crate exists and compiles, but `handlers/sql.rs` still calls hand-rolled OLAP paths. Wire `OlapQueryRequest` through the DataFusion executor.

---

## How to continue

```
@remaining.md
@services/voltnuerongridd/src/main.rs
@services/voltnuerongridd/src/handlers/
@services/voltnuerongridd/src/helpers/
```

Recommended next steps (in priority order):
1. **Phase 2.3 verification** — `grep -rn "#\[deprecated\]" services/voltnuerongridd/src/`
2. **Real RBAC** — replace hardcoded `default_rbac_privilege_matrix` in `helpers/boot.rs` with config-driven matrix
3. **Raft replication** — background tick + peer fanout
4. **DataFusion wiring** — route OLAP queries through exec-datafusion executor

**Key invariants to preserve:**
- All types that are `AppState` fields stay in `main.rs` as `pub(crate)` — never move to handler modules
- `cargo check -p voltnuerongridd` must be clean after every change
- 556 unit tests must continue passing
