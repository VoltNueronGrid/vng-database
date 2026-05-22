# Gap Analysis — VoltNueronGrid Remaining Gaps (post-session 31)

**Prepared:** 2026-05-21 (session 31 close)
**Branch:** `claude/friendly-hertz-3b69fb` → merged to `main`
**Commit:** session 31 close
**Based on:** `gaps-may26-1.md` + `gaps-may20-2.md` + `gaps-3.md` cross-referenced against actual code
**Test baseline:** 801 passed (voltnuerongridd), 0 failed, 1 ignored

This document records all remaining gaps after the full audit pass in session 31.
✅ items below were closed in session 31 and are noted for history only.

---

## What closed in session 31

| Gap ID | Description | Evidence |
|---|---|---|
| H-1 | `prev_log_term` hardcoded to 0 in AppendEntries | `raft_loop.rs` — `per_peer` tuple now includes `prev_log_term` looked up via `RaftNode::term_at()`; `fanout_heartbeat` uses the real term instead of 0 |
| H-2 | Raft log not persisted across restarts | `raft_loop.rs` — `persist_raft_state()` / `load_raft_state()` write/read `{data_dir}/raft_meta.json` atomically (write-to-tmp + rename); `RaftDurableState` captures `current_term`, `voted_for`, `snapshot_index`, `snapshot_term`, `log`; `RaftNode::restore_durable()` restores on boot; `handlers/raft.rs` persists immediately after `raft_vote`, `raft_append`, `raft_install_snapshot`; tick loop persists at the end of each iteration |

**9 new unit tests added** (in `helpers/raft_loop.rs` test module):
- `term_at_zero_returns_zero`
- `term_at_snapshot_index_returns_snapshot_term`
- `term_at_log_entry_returns_correct_term`
- `term_at_missing_entry_returns_zero`
- `term_at_after_compaction_uses_snapshot_term`
- `persist_and_load_raft_state_round_trip`
- `persist_no_op_on_empty_data_dir`
- `load_returns_none_when_no_file`
- `restore_durable_sets_all_fields`

---

## Severity legend

| Icon | Meaning |
|---|---|
| 🔴 | Critical — blocks correctness or production use |
| 🟠 | High — data correctness or durability risk |
| 🟡 | Medium — scale / completeness |
| 🟢 | Low — polish / nice-to-have |

---

## 🔴 Critical — 0 remaining

All critical gaps closed in sessions 16–31.

---

## 🟠 High — 0 remaining

H-1 and H-2 closed in session 31. No high-severity gaps remain.

---

## 🟡 Medium — 9 remaining

---

### M-1 · Raft apply path strips database scope from row keys
**Severity:** 🟡 · **Effort:** S · **Discovered:** session 31 audit

**File:** `services/voltnuerongridd/src/helpers/raft_loop.rs:383, 389, 396`

```rust
wal.store_row("", &k, xid, Some(&d));  // ← empty db string on every Raft apply
```

`apply_dml_command` calls `wal.store_row("", ...)` with an empty db prefix. The direct-write
path uses `db_prefix_key(&db, &raw_k)`. On followers, Raft-applied rows land in the
unscoped namespace, breaking multi-database isolation for replicated writes.

**Remaining work:** Thread the originating database name through `RaftLogEntry.command`
(e.g., prefix the command string with `"db=<name>;"`) and unpack it in `apply_dml_command`.

---

### M-2 · C-1: PagedRowStore `read_latest()` has no RocksDB fallback
**Severity:** 🟡 · **Effort:** M · **Original:** `gaps-3.md C-1`

**File:** `crates/voltnuerongrid-store/src/mvcc.rs:257–271`

Point reads (`read_latest()`, `read_at_snapshot()`) scan only the in-memory `pages` Vec.
If a row is evicted under `VNG_ROW_STORE_MAX_ROWS` or was not replayed on boot (RocksDB path),
a point read returns `None` instead of fetching from RocksDB. MVCC conflict detection for
serializable isolation also reads only `PagedRowStore`, so cross-restart serializable
conflicts are undetectable. A `TODO(C-1)` at `main.rs:1713` documents this explicitly.

**Remaining work:** Add a `DurabilityEngine` reference to `PagedRowStore`; in `read_latest()`,
if the in-memory lookup misses and `engine.persists_rows() == true`, call `engine.get_row(key)`
before returning `None`.

---

### M-3 · C-1: Write-set conflict detection not persisted across restarts
**Severity:** 🟡 · **Effort:** M · **Original:** `gaps-3.md C-1`

`AcidTransactionRegistry` write-set tracking is in-memory. After restart, all write-sets from
previous committed transactions are gone. A transaction that committed just before a restart
will not conflict-check correctly with transactions starting after the restart. Serializable
isolation is only correct within a single process lifetime.

**Remaining work:** Persist the last N committed write-sets to a dedicated RocksDB column
family; reload on boot.

---

### M-4 · M-6: No preemptive statement timeout watchdog
**Severity:** 🟡 · **Effort:** M · **Original:** `gaps-3.md M-6`

**File:** `services/voltnuerongridd/src/handlers/sql.rs:63–70, 1046–1054`

`check_deadline()` polls at fixed code points but cannot cancel a running DataFusion
`collect()` call or a blocking `block_in_place` loop mid-execution. The comment at
`sql.rs:192` says "enforcement is left to a future watchdog task." A 10 s DataFusion
aggregation ignores a 1 s timeout.

**Remaining work:** Wrap the DataFusion `collect()` call in
`tokio::time::timeout(remaining_duration, ...)`. For sync paths, use a `CancellationToken`
polled at each row iteration.

---

### M-5 · `sql_transaction` UPDATE path is single-key only
**Severity:** 🟡 · **Effort:** S · **Discovered:** session 31 audit

**File:** `services/voltnuerongridd/src/handlers/sql.rs:654–664`

`sql_transaction` calls only `extract_update_row_from_sql` (primary-key single-row update).
The bulk-scan `extract_bulk_update_target` helper exists and is used in the direct
`sql_execute` DML path, but `sql_transaction` was not updated. A transaction-wrapped
`UPDATE t SET x=1 WHERE non_pk_col='val'` silently updates at most one row.

**Remaining work:** Mirror the bulk-scan UPDATE logic from `sql_execute` into
`sql_transaction`.

---

### M-6 · M-8 Rule 1: Column values stored as raw strings, not typed
**Severity:** 🟡 · **Effort:** L · **Original:** `gaps-3.md M-8`

**File:** `crates/voltnuerongrid-store/src/mvcc.rs:35`, `rocksdb_engine.rs:795–798`

`RowData = HashMap<String, String>`. All values stored as raw strings; `infer_json_value()`
coerces them only at query output time. DDL-declared column types are not enforced at
write time. No per-row null bitmap on write.

**Remaining work:** Define a typed `RowValue` enum; validate against the DDL schema on
INSERT/UPDATE.

---

### M-7 · M-8 Rule 9: View expansion is text-substitution, not logical independence
**Severity:** 🟡 · **Effort:** L · **Original:** `gaps-3.md M-8`

**File:** `services/voltnuerongridd/src/helpers/sql_parse.rs:498–568`

`expand_view_in_select()` does string-level SQL rewriting (`str::replace`). If the physical
key format changes, every stored view body breaks equally. True logical independence requires
AST-level view resolution decoupled from the storage key format.

**Remaining work:** Store views as parsed AST fragments rather than raw SQL text; rewrite at
query time using the AST layer.

---

### M-8 · M-8 Rule 11: Raft reads are not linearisable (stale reads on followers)
**Severity:** 🟡 · **Effort:** M · **Discovered:** session 31 audit

**File:** `services/voltnuerongridd/src/handlers/sql.rs` (SELECT path)

SELECT reads directly from local state without verifying the node is the current leader.
A stale follower or a deposed leader serves reads from its own (potentially stale) copy
without any read-index or lease-read protocol.

**Remaining work:** Before serving reads, either (a) require `role == Leader` and return
503 on non-leaders, or (b) implement read-index (send a heartbeat, wait for majority
confirmation of current leadership before reading).

---

### M-9 · `information_schema.settings` virtual table missing
**Severity:** 🟡 · **Effort:** S · **Original:** `gaps-3.md M-6`

**File:** `services/voltnuerongridd/src/helpers/information_schema.rs`

The virtual catalog covers `tables`, `columns`, `schemata`, and `pg_catalog.*` but has no
`settings` or `parameters` table exposing runtime configuration via SQL. The Studio
SettingsPanel calls `/api/v1/admin/runtime-config` directly rather than querying via the
SQL editor.

**Remaining work:** Add a `settings` branch to `synthesize_virtual_catalog_response()`
returning runtime config fields (`storage_engine`, `data_dir`, `wal_fsync_on_commit`,
`htap_threshold`, `max_result_rows`) as rows keyed by parameter name.

---

## 🟢 Low — 8 remaining

---

### L-1 · Export button in Studio ResultsPane is a no-op stub
**Severity:** 🟢 · **Effort:** S · **Discovered:** session 31 audit

**File:** `ui/voltnuerongrid-studio/src/components/ResultsPane/ResultsPane.tsx:59`

```tsx
<button className="btn btn-sm">Export ↓</button>   // no onClick
```

Renders but does nothing. Users expect CSV / JSON download.

**Remaining work:** Add `onClick` that converts `result.rows` to CSV / JSON and triggers a
`URL.createObjectURL` download in the browser.

---

### L-2 · Stored procedure execution is a demo stub
**Severity:** 🟢 · **Effort:** L · **Original:** `gaps-may20-2.md §11`

**File:** `services/voltnuerongridd/src/handlers/sql.rs:1066`

`CALL proc_name(args)` is only handled by `try_handle_call_insert_rows_demo` (when
`VNG_DEMO_MODE=true`). No real stored procedure execution path exists.

**Remaining work:** Build a stored-procedure registry that maps `CALL name(args)` to a
catalog of registered Rust closures or user-defined SQL bodies.

---

### L-3 · `ConnectionPoolManager` and `NativeConnectionPool` overlap architecturally
**Severity:** 🟢 · **Effort:** M · **Discovered:** session 31 audit

**File:** `drivers/voltnuerongrid-driver-rust/src/lib.rs:832, 2123`

`NativeConnectionPool` owns real `TcpStream`s and is the active pool.
`ConnectionPoolManager` tracks `PooledConnection` state machines (circuit breakers, storm
detection) but owns no sockets. Both coexist without clear boundary documentation.

**Remaining work:** Document the boundary; eventually merge circuit-breaker logic into
`NativeConnectionPool`.

---

### L-4 · OTEL distributed tracing has no `traceparent` header propagation
**Severity:** 🟢 · **Effort:** S · **Original:** `gaps-3.md`

**File:** `services/voltnuerongridd/src/helpers/sql_parse.rs:6–41`, `observability.rs`

`#[instrument]` is on 5 handlers and OTEL spans emit to OTLP. But incoming W3C
`traceparent` / `tracestate` headers are not extracted — distributed trace context from
upstream services is silently dropped.

**Remaining work:** Add axum middleware using `tracing-opentelemetry`'s
`opentelemetry_http::HeaderExtractor` to propagate incoming trace context.

---

### L-5 · OLAP Parquet cold-start: first 60 s after restart uses in-memory scan
**Severity:** 🟢 · **Effort:** S · **Original:** `gaps-3.md`

**File:** `crates/voltnuerongrid-exec-datafusion/src/datafusion.rs:95–162`

The Parquet prefer-or-fallback path exists and the background flush task runs every 60 s.
In the window between restart and first flush, all OLAP queries fall back to in-memory row
vectors. No tiered storage, no materialized view refresh, no zone maps.

**Remaining work:** Run the flush task once at startup (before the interval fires) to
populate Parquet files immediately. Or reduce the default interval via env var.

---

### L-6 · No crash-recovery integration test
**Severity:** 🟢 · **Effort:** M · **Original:** `gaps-may20-2.md §18`

No test spawns a real process, inserts data, kills it, restarts, and asserts data survived.
All integration tests use in-process `AppState`. KPI gates are self-graded.

**Remaining work:** Build an end-to-end harness (spawned process + HTTP client) that tests
the full restart → RocksDB recovery → SQL query cycle.

---

### L-7 · Scratch `.md` files and `.DS_Store` committed to repo
**Severity:** 🟢 · **Effort:** S · **Original:** `gaps-may20-2.md §20`

`status-tracker.md`, `status-tracker-sprintwise-v1.md`, `understanding.md`, `pending.md`,
`remaining.md`, etc. remain committed. `.DS_Store` gitignore status unverified.

**Remaining work:** Consolidate to one `STATUS.md`; move archives to `docs/archive/`;
add `.DS_Store` to `.gitignore`.

---

### L-8 · Driver tests (7) fail in sandbox due to TCP socket bind restriction
**Severity:** 🟢 · **Effort:** S · **Discovered:** session 31 audit

**File:** `drivers/voltnuerongrid-driver-rust/src/lib.rs` (7 tests)

Seven tests call `TcpListener::bind` on loopback; the Claude sandbox returns `EPERM`.
These would pass in a real environment but break CI parity in sandboxed runs.

**Remaining work:** Mark the 7 affected tests with `#[ignore]` and a comment explaining
the sandbox restriction, consistent with the E2E HTTP roundtrip test in the service crate.

---

## Priority sequencing for next session

| Priority | Gap | Effort | Impact |
|---|---|---|---|
| 1 | M-1 — Raft apply strips db scope | S | Multi-DB correctness on replicated writes |
| 2 | M-9 — `information_schema.settings` virtual table | S | SQL-queryable server config |
| 3 | M-5 — `sql_transaction` bulk UPDATE | S | ACID correctness for non-PK UPDATE in txns |
| 4 | M-4 — Preemptive timeout watchdog | M | True timeout enforcement for slow queries |
| 5 | M-8 — Leader reads (linearisable SELECT) | M | Raft read correctness |
| 6 | L-8 — Driver sandbox test `#[ignore]` | S | CI green across environments |
| 7 | L-1 — Export button stub | S | Studio UX completeness |
| 8 | L-4 — `traceparent` propagation | S | Distributed tracing completeness |
| 9 | L-5 — Parquet cold-start flush | S | OLAP query accuracy after restart |
| 10 | M-2 — C-1 read-miss RocksDB fallback | M | Full production read durability |

---

*Total remaining gaps: 0 critical, 0 high, 9 medium, 8 low — 17 total.*
*Session 31 closed: H-1 (prev_log_term fix), H-2 (Raft log persistence across restarts).*
