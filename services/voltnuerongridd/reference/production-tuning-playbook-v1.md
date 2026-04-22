# Production Tuning Playbook v1 (Local-First)

## Objective

Provide an operator-ready baseline for performance tuning and stability guardrails during local validation.

## Runtime and Driver Knobs

| Area | Knob | Baseline | Tuning Range | Tradeoff |
|---|---|---:|---:|---|
| Driver | `requestTimeoutMs` | 5000 | 3000-15000 | Higher values reduce transient failures but delay surfacing hard faults |
| Driver | `maxRetries` | 2 | 0-5 | More retries improve resilience but can increase tail latency |
| Driver pool | `minConnections` | 2 | 2-16 | Larger pools improve warm throughput but raise idle memory |
| Driver pool | `maxConnections` | 16 | 16-128 | Higher concurrency needs backend capacity headroom |
| Runtime | benchmark ingest `chunk_target_rows` | 100 | 100-5000 | Larger chunks reduce overhead but can spike memory |
| Runtime | query `max_rows` | 1000 | 100-5000 | Larger pages reduce round trips but increase payload size |

## Benchmark Procedure

1. Run benchmark suite (`S8`) and capture baseline artifacts.
2. Change one knob at a time.
3. Repeat identical workload and compare:
   - throughput delta
   - p95 latency delta
   - memory delta (RSS)
4. Keep only changes with positive trend and no stability regressions.

## Soak and Recovery Procedure

1. Run soak lane (`S9-001`) for target duration.
2. Inject controlled failures (`S9-003`) while soak is active.
3. Verify:
   - health endpoint remains responsive
   - recovery converges
   - no sustained error growth after recovery

## Rollout Guardrails

- Never change timeout + retries + pool limits in one step.
- Require at least two successful local benchmark runs before adopting new defaults.
- Keep a rollback snapshot of prior config before each tuning step.

## Cloud Defer Note

Cloud and remote-cluster tuning evidence is intentionally deferred.
This playbook v1 governs local validation only.
