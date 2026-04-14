# WS2 Durability Bootstrap Contract

This contract defines the first validation-ready storage durability primitives for WS2.

## Core Primitives

- `DurabilityConfig`
  - `wal_enabled`
  - `checkpoint_interval_seconds`
  - `max_wal_records_before_checkpoint`
- `WalRecord`
  - `sequence`
  - `timestamp_epoch_ms`
  - `key`
  - `value`
- `CheckpointManifest`
  - `checkpoint_id`
  - `last_sequence`
  - `entry_count`

## Bootstrap Engine

`InMemoryDurabilityEngine` is the starter validation implementation that supports:

1. Append mutation to state + WAL (`append_mutation`)
2. Read latest value by key (`get`)
3. Threshold-based checkpoint decision (`maybe_checkpoint`)
4. Forced checkpoint (`force_checkpoint`)
5. Inspect latest checkpoint metadata (`latest_checkpoint`)
6. Recover state from persisted WAL records (`recover_from_records`, `recover_from_adapter`)

This engine is not a final storage engine; it is an executable contract to validate sequencing and checkpoint behavior before disk-backed durability lands.

## Validation

- Unit tests in `crates/voltnuerongrid-store/src/lib.rs`
- WAL adapter interface + file I/O boundary in `crates/voltnuerongrid-store/src/wal_adapter.rs`
- Persisted-recovery test from file WAL in `crates/voltnuerongrid-store/src/lib.rs` (`recovers_state_from_wal_adapter_records`)
- Smoke script: `tests/kpi/scripts/run-store-durability-smoke.ps1`
- Artifact: `tests/kpi/results/ws2/store-durability-smoke.json`
- Smoke script (disk adapter): `tests/kpi/scripts/run-ws2-disk-wal-smoke.ps1`
- Artifact (disk adapter): `tests/kpi/results/ws2/disk-wal-adapter-smoke.json`
