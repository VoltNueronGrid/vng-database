# Scale and Performance Proof (S8/S9) - Local Evidence Plan v1

## Scope

This pack defines a reproducible, local-only execution path for Sprint V3-S8 and V3-S9.
Cloud-hosted or remote-cluster runs are intentionally deferred to final cloud validation.

## S8-001 Formal benchmark suite

- Dataset seed:
  - `bench_small`: 10k rows
  - `bench_medium`: 100k rows
  - `bench_large`: 1M rows
- Workloads:
  - ingest throughput (`/api/v1/benchmark/ingest`)
  - query throughput (`/api/v1/benchmark/query`)
  - update throughput (`/api/v1/sql/transaction` with UPDATE statements)
- Reproducibility controls:
  - fixed seeds in request payload
  - fixed sample counts
  - fixed concurrency and timeout budget
- Artifact:
  - `tests/kpi/results/s8/s8-benchmark-suite.json`

## S8-002 Multithread import optimization + bottleneck elimination

- Evidence strategy:
  - verify runtime benchmark ingest route exists
  - verify `ChunkedLoader` path is active in runtime source
  - run local ingest benchmark and capture throughput trend
- Artifact:
  - `tests/kpi/results/s8/s8-import-optimization.json`

## S8-003 Join/path optimization + paging validation

- Evidence strategy:
  - verify join handlers exist in runtime source
  - verify paging knobs (`max_rows`) are wired in core query paths
  - run local benchmark query pack and collect p50/p95 latency summary
- Artifact:
  - `tests/kpi/results/s8/s8-join-paging.json`

## S8-004 Memory profile + allocator strategy review

- Evidence strategy:
  - capture process RSS before/after benchmark loops
  - capture `/health` availability throughout run
  - include allocator strategy recommendation:
    - default allocator for portability
    - jemalloc profile lane as optional local experiment
- Artifact:
  - `tests/kpi/results/s8/s8-memory-profile.json`

## S9-001 High-concurrency soak test

- Local soak lane:
  - duration: 30-60 minutes configurable
  - mixed read/write loop with bounded parallel workers
  - periodic health checks
- Artifact:
  - `tests/kpi/results/s9/s9-soak-summary.json`

## S9-002 Distributed/sharding behavior prototype

- Local prototype evidence:
  - validate shard-related code paths exist in runtime
  - execute synthetic shard-routed SQL batch and confirm deterministic route metadata
- Artifact:
  - `tests/kpi/results/s9/s9-sharding-prototype.json`

## S9-003 Failure injection + recovery under load

- Local-only failure injection:
  - controlled transient fault simulation against running local service
  - assert recovery windows remain within configured guardrails
- Artifact:
  - `tests/kpi/results/s9/s9-failure-recovery.json`

## S9-004 Production tuning playbook v1

- Baseline playbook:
  - timeout and retry ranges
  - connection pool tuning ranges
  - ingest/query parallelism tuning
  - memory guardrail recommendations
  - rollout and rollback checks
- Artifact:
  - `services/voltnuerongridd/reference/production-tuning-playbook-v1.md`

## Deferred cloud validation

All cloud-provider and hosted-runner evidence remains deferred by design.
Local execution and artifacts are the acceptance source for this phase.
