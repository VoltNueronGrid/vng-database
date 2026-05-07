# `remaining.md` — handoff for next session (v14)

**Last updated:** 2026-05-07 (thirteenth session — Slice 4 complete: sql.rs, sre.rs, store.rs)
**Branch:** `main`
**Total unit tests:** ~603 passing (resilience.rs has 2 pre-existing E0597 failures unrelated to modular work; 28 planner tests in voltnuerongrid-exec also pre-existing failures)
**Phase:** main.rs modular refactor underway — Slices 1–4 complete

---

## TL;DR — what landed this session

### ✅ main.rs modular refactor — Slice 4

**New files:**

- **`services/voltnuerongridd/src/handlers/sql.rs`** (1,423 lines) — 26 DTOs + 9 handlers:
  - Handlers: `sql_transaction`, `sql_pessimistic_lock_acquire`, `sql_pessimistic_lock_release`, `sql_pessimistic_lock_metrics`, `sql_analyze`, `sql_route`, `sql_execute`, `sql_transactions_isolation`, `sql_transactions_active`
  - DTOs: `SqlTransactionRequest`, `SqlAnalyzeRequest`, `AnalyzedStatement`, `SqlAnalyzeResponse`, `SqlRouteRequest`, `RoutedStatementResponse`, `SqlRouteResponse`, `SqlExecuteRequest`, `LegacyAggResult`, `SqlExecuteResponse`, `OltpRowResult`, `OlapVecAggResult`, `UdfExecutionResult`, `UdfFunctionCatalogEntry`, `UdfLanguageGuardPolicy`, `UdfExecutionPlanStep`, `UdfInvocationPlan`, `PessimisticLockAcquireRequest`, `PessimisticLockReleaseRequest`, `PessimisticLockResponse`, `PessimisticLockContentionMetricsResponse`, `TxIsolationEntry`, `TxIsolationStatsResponse`, `OlapQueryRequest`, `OlapQueryResponse`, `AcidTransactionsResponse`

- **`services/voltnuerongridd/src/handlers/sre.rs`** (1,431 lines) — ~35 DTOs + 22 handlers:
  - Handlers: `sre_reliability_status`, `sre_rate_limit_check`, `sre_failure_budget_alerts`, `sre_dr_hook_policy`, `sre_dr_hook_retry_plan`, `sre_dr_hook_schedule`, `sre_dr_hook_trigger`, `sre_dr_hook_status`, `sre_failure_signal`, `sre_failure_reconcile`, `sre_gate_evaluate`, `sre_gate_export`, `sre_cache_set`, `sre_cache_get`, `sre_cache_invalidate`, `sre_cache_rebalance`, `sre_cache_metrics`, `sre_driver_pool_acquire`, `sre_driver_pool_release`, `sre_driver_pool_failure`, `sre_driver_pool_recover`, `sre_driver_pool_stats`
  - Note: `cache_redis_command` and its DTOs (`RedisCacheCommandRequest`, `RedisCacheCommandResponse`) STAY in main.rs (heavily tested there)

- **`services/voltnuerongridd/src/handlers/store.rs`** (1,330 lines) — 43 DTOs + 23 handlers:
  - Handlers: `htap_status`, `store_rows_keys`, `row_store_version`, `htap_stats`, `row_store_snapshot`, `row_store_stats`, `row_store_count`, `row_store_delete`, `store_list_indexes`, `store_create_index`, `store_drop_index`, `store_index_lookup`, `store_add_constraint`, `store_validate_constraint`, `store_rows_scan`, `store_htap_export`, `store_columnar_scan`, `store_columnar_project`, `store_columnar_aggregate`, `store_htap_apply`, `store_htap_olap_scan`, `htap_lag`, `htap_force_sync`

**`pub(crate)` changes in main.rs for Slice 4:**
- `DR_HOOK_COUNTER` static → `pub(crate)`
- Functions: `failure_budget_snapshot`, `rate_limit_policy_snapshot`, `evaluate_rate_limit`, `evaluate_failure_budget_alert`, `build_retry_plan`, `enqueue_dr_hook_task`, `execute_dr_hook`, `latest_dr_hook_records`, `pool_acquire_error_state`, `record_transport_mutation` → all `pub(crate)`

**Shared-type rule in action (stays in main.rs):**
- `PoolStatsResponse` — shared between driver.rs and sre.rs, stays in main.rs as `pub(crate)`
- `RedisCacheCommandRequest/Response` — private to main.rs (function + tests co-located)
- `PessimisticLockContentionMetrics` — AppState field type, stays in main.rs

**main.rs line count:** 30,287 (Slice 3) → 24,962 (after Slice 4, -5,325 lines)

**`cargo check --workspace`**: clean ✓
**`cargo test`**: ~603 passing; 2 pre-existing E0597 in resilience.rs; 28 pre-existing planner failures in voltnuerongrid-exec

---

## What's still TODO

### Phase main.rs refactor (continuing)

5. **Slice 5** — `handlers/wal.rs` (87 WAL handlers — largest, most risk)

6. **Slice 6** — `handlers/audit.rs` (10 audit handlers)

### Phase 3 extended

1. **Real RBAC** — current privilege matrix is hardcoded in `default_rbac_privilege_matrix` (now in `config_init.rs`).

2. **Replication** — leader/follower with the existing Raft skeleton.

---

## How to continue

```
@remaining.md
@services/voltnuerongridd/src/main.rs
@services/voltnuerongridd/src/handlers/store.rs
@services/voltnuerongridd/src/handlers/sre.rs
```

Recommended next steps (in priority order):
1. **Continue main.rs refactor** — Slice 5: wal.rs (87 handlers, highest risk — do carefully)
2. **Slice 6** — audit.rs (10 handlers)
3. **Real RBAC** — replace `default_rbac_privilege_matrix` in `config_init.rs`

**Extraction pattern for handler modules:**
1. Find all `Prefix*` DTOs with `grep -n "^struct Prefix" main.rs`
2. Read the handler function bodies
3. Create `handlers/name.rs` with DTOs + handlers (all `pub(crate)`)
4. Create/update `handlers/mod.rs` with `pub(crate) mod name;`
5. Add `use handlers::name::*;` to main.rs after the existing glob imports
6. Delete DTOs + handlers from main.rs
7. `cargo check --workspace` must be clean before next module

**Shared-type rule:** Types in `AppState` fields (e.g. `ConnectorPlugin`, `PoolStatsResponse`, `AcidTxEntry`, `PessimisticLockContentionMetrics`) must stay in main.rs as `pub(crate)` — never move them to handler modules.

**Key gotcha from Slice 4:** When a type is defined in both main.rs (private) and the new module (pub(crate)), the private main.rs version SHADOWS the glob import. The fix is to delete the private main.rs version — after deletion, the glob import version is used by all code in main.rs scope.

**WAL handlers note:** 87 handlers is a large batch. Consider splitting into multiple sub-batches (wal_write*, wal_read*, wal_replay*, wal_checkpoint*, etc.) to reduce risk.
