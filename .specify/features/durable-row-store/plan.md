# Implementation Plan: Durable Row Store

**Branch**: `main` | **Date**: 2026-06-26 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `.specify/features/durable-row-store/spec.md`

## Summary

Bind `PagedRowStore` writes and reads to RocksDB Column Families so that acknowledged row writes survive process restart and SIGKILL. The WAL infrastructure and `store_row`/`scan_rows_for_db` call sites already exist — the primary work is fixing the boot sequence to skip DML SQL text replay when RocksDB is the durability engine, and wiring the `ensure_db_cf` + `drop_db_column_family` paths correctly.

## Technical Context

**Language/Version**: Rust 1.78+

**Primary Dependencies**: `rocksdb` crate (feature-gated), `serde_json` for row serialization, `tokio` for async wrappers

**Storage**: RocksDB at `data/rocksdb/` — Column Family per database: `rows_{db_name}`

**Testing**: `cargo test -p voltnuerongridd`, `pwsh run-crash-recovery-gate.ps1`

**Target Platform**: Linux server (production), macOS (dev)

**Performance Goals**: Boot time < 5 seconds for 1M row store; point read < 1ms p95

**Constraints**: Zero-copy RocksDB reads where possible; page eviction (LRU) when `VNG_ROW_STORE_MAX_ROWS` is set

**Scale/Scope**: Must not regress 853 passing tests; crash recovery gate must pass with 1000+ rows

## Constitution Check

- **Durable HTAP correctness**: PASS — Page-level durability writes row data to RocksDB CF on every INSERT/UPDATE/DELETE; scan reads from RocksDB CF when `persists_rows() == true`
- **Security/RBAC/tenant isolation**: PASS — per-DB CFs provide physical data isolation; `DROP DATABASE` deletes the CF
- **Performance evidence**: PASS — crash recovery gate provides reproducible evidence; `VNG_ROW_STORE_MAX_ROWS` controls RAM growth
- **Modular Rust/reuse**: PASS — reuses existing `RocksDbDurabilityEngine::store_row`, `scan_rows_for_db`, `ensure_db_cf`, `drop_db_column_family`
- **Native interface parity**: PASS — HTTP SQL execute path already calls `store_row`; Raft apply loop already calls `store_row`
- **Autonomous/plugin governance**: N/A
- **Evidence and tracker truth**: Gate artifact at `tests/kpi/results/recovery/crash-recovery-gate.json`

## Project Structure

### Key Files

```text
crates/voltnuerongrid-store/src/
├── mvcc.rs                    # PagedRowStore — in-memory write-through cache
├── rocksdb_engine.rs          # RocksDbDurabilityEngine — store_row, scan_rows_for_db
└── lib.rs                     # DurabilityEngine trait — store_row, persists_rows

services/voltnuerongridd/src/
├── main.rs                    # AppState construction — boot sequence
├── helpers/boot.rs            # replay_dml_into, load_persisted_rows_into
├── handlers/sql.rs            # store_row call sites, scan_rows_for_db call sites
└── helpers/raft_loop.rs       # store_row call sites in Raft apply loop

tests/kpi/scripts/
└── run-crash-recovery-gate.ps1   # Evidence gate
```

## Implementation Phases

### Phase 1: Fix Boot Sequence (Critical)

Skip `replay_dml_into` when RocksDB `persists_rows() == true`. Currently, DML SQL text is always replayed into `PagedRowStore` even when RocksDB holds the authoritative row data. This wastes RAM and creates stale in-memory state.

**Change in `main.rs` row_store initialization:**
```rust
// Before: always replay DML into PagedRowStore
replay_dml_into(&mut rs, &wal_engine);

// After: only replay DML when in-memory engine is active
let use_rocksdb = { wal_engine.lock().unwrap().persists_rows() };
if !use_rocksdb {
    replay_dml_into(&mut rs, &wal_engine);
}
```

### Phase 2: Ensure store_row Coverage

Verify all INSERT/UPDATE/DELETE paths call `wal_engine.store_row`. Current known call sites:
- `handlers/sql.rs` lines 771, 791, 856, 873 — INSERT, DELETE, UPDATE, UPDATE bulk
- `helpers/raft_loop.rs` lines 562, 568, 575 — Raft apply loop

### Phase 3: Ensure scan_rows_for_db Coverage

Verify all SELECT paths use `wal.scan_rows_for_db` when `persists_rows() == true`. Current known call sites:
- `handlers/sql.rs` lines 814, 1682, 2281, 2809, 2955

### Phase 4: Boot Sequence for load_persisted_rows_into

When `persists_rows() == true` (RocksDB), do NOT call `load_persisted_rows_into` — rows are read directly from RocksDB on demand. This is already the case in the C-1 block at main.rs line 1739.

### Phase 5: DROP DATABASE CF Cleanup

Ensure `purge_database_rows` is called in the `DROP DATABASE` SQL handler. Verify `drop_db_column_family` is wired.

## Acceptance Gates

| Gate | Command | Must Pass |
|------|---------|-----------|
| Unit tests | `cargo test -p voltnuerongridd` | 853+ passed |
| Crash recovery | `pwsh run-crash-recovery-gate.ps1` | status: passed, rows_survived: true |
| Store crate | `cargo test -p voltnuerongrid-store` | All pass |

## Definition of Done

- [x] `store_row` called on all INSERT/UPDATE/DELETE paths in sql.rs and raft_loop.rs
- [x] `scan_rows_for_db` used when `persists_rows() == true` for SELECT scans
- [x] `purge_database_rows` called in DROP DATABASE handler
- [ ] `replay_dml_into` skipped when `persists_rows() == true` in main.rs boot sequence
- [x] Crash recovery gate script exists at `tests/kpi/scripts/run-crash-recovery-gate.ps1`
- [ ] E3 crash recovery gate passes with 1000+ rows surviving SIGKILL+restart
- [ ] `cargo test -p voltnuerongridd` passes (regression-free)
