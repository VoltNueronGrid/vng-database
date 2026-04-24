# VoltNueronGrid Benchmark Suite

This directory contains the benchmark harness for the `vng-benchmarks` crate.
All benchmarks use `std::time` only — no external criterion dependency.

## How to Run

```bash
# Check compilation
cargo check -p vng-benchmarks

# Run all benchmark tests (small-scale, fast)
cargo test -p vng-benchmarks

# Run a single benchmark module
cargo test -p vng-benchmarks ingest
cargo test -p vng-benchmarks query
cargo test -p vng-benchmarks memory
```

## What Each Benchmark Measures

### `ingest_benchmark.rs`

| Benchmark              | Metric                          | Dataset sizes        |
|------------------------|---------------------------------|----------------------|
| `run_batch_insert`     | rows/sec for batch record insert | 10K / 100K / 1M rows |
| `run_csv_parse`        | rows/sec for CSV line parsing    | 10K / 100K / 1M rows |

Reports: `min_ms`, `max_ms`, `avg_ms`, `p99_ms`, `rows_per_sec`.

### `query_benchmark.rs`

| Benchmark              | Metric                          |
|------------------------|---------------------------------|
| `run_select_benchmark` | SELECT round-trip latency (ns)   |
| `run_join_benchmark`   | JOIN cost estimate time (ns)     |
| `run_paging_benchmark` | Paging strategy selection (ns)   |

### `memory_profile.rs`

| Component              | Description                                               |
|------------------------|-----------------------------------------------------------|
| `MemorySnapshot`       | Point-in-time heap usage snapshot                         |
| `MemoryProfiler`       | Collects snapshots around a workload                      |
| `AllocationReport`     | Summarises peak bytes and growth rate per row processed   |

## Reproducible Dataset Specs

All datasets are **synthetic** and generated deterministically in-process.

### CSV dataset format

```
id,name,value,score,active
1,user_1,value_1,0.1,true
2,user_2,value_2,0.2,false
...
```

- `id`: sequential integer (1-based)
- `name`: `user_<id>`
- `value`: `value_<id>`
- `score`: `(id % 1000) * 0.001` (3 decimal places)
- `active`: `id % 2 == 0` → `"true"` / `"false"`

### Scale tiers

| Tier | Rows    | Expected ingest rate (goal) |
|------|---------|-----------------------------|
| S    | 100     | > 0 rows/sec (test gate)     |
| M    | 10 000  | > 50 000 rows/sec            |
| L    | 100 000 | > 100 000 rows/sec           |
| XL   | 1 000 000 | > 200 000 rows/sec         |

> Note: XL tier runs are skipped in `cargo test` by default to keep CI fast.
> Run manually with `cargo test -- --ignored` to include them.

## Adding a New Benchmark

1. Create `tests/benchmarks/<name>_benchmark.rs`.
2. Re-export your public types from `tests/benchmarks/mod.rs`.
3. Include a `#[test]` that runs at ≤ 1 000 rows and asserts `rows_per_sec > 0`.
