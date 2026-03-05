# WS14 Driver/Security Tuning Playbook (Starter)

| Symptom | Knob | Expected Effect |
|---|---|---|
| Frequent request timeout on mixed HTAP batches | `driver.requestTimeoutMs` increase by 20-50% | Fewer transient timeout failures at cost of longer waits before abort |
| High connection churn | `driver.pool.minConnections` increase | Better warm pool reuse and lower handshake overhead |
| Gateway saturation during burst load | `driver.pool.maxConnections` increase with backend limits | Higher concurrent throughput if backend can absorb |
| Operator auth noise from short-lived tokens | `security.tokenTtlSeconds` increase cautiously | Lower token refresh overhead, balanced against security posture |
| Need stricter control-plane access | `security.mtlsRequired=true` and role allowlist tightening | Stronger operator identity guarantees and reduced unauthorized surface |
