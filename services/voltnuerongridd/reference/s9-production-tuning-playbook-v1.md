# VoltNueronGrid Production Tuning Playbook — v1

> Sprint S9 · Scale and Performance Proof Phase 2
> Audience: on-call engineers and platform operators

---

## 1. OS-Level Tuning

### File-descriptor limits

Every connection consumes at least one file descriptor.  Without raising the
limit, `EMFILE` errors appear long before hardware is saturated.

```bash
# /etc/security/limits.conf (Linux) — apply to the daemon user
voltnuerongridd   soft   nofile   65536
voltnuerongridd   hard   nofile   65536

# Verify at runtime
ulimit -n                          # should read 65536
cat /proc/$(pidof voltnuerongridd)/limits | grep 'Max open files'
```

For systemd-managed services add to the unit file:

```ini
[Service]
LimitNOFILE=65536
```

### TCP keepalive

Idle connections held by load balancers or NAT gateways can silently die.
Keepalive probes detect dead peers before the application times out.

```bash
# /etc/sysctl.d/99-vng.conf
net.ipv4.tcp_keepalive_time    = 60     # start probes after 60 s idle
net.ipv4.tcp_keepalive_intvl   = 10     # probe every 10 s
net.ipv4.tcp_keepalive_probes  = 6      # drop after 6 missed probes (~1 min)
net.core.somaxconn             = 4096   # listen backlog
net.ipv4.tcp_max_syn_backlog   = 4096

sysctl --system                         # reload without reboot
```

### Swap pressure

Swapping database pages causes unpredictable latency spikes.  Pin the process
in RAM by making swap nearly unavailable.

```bash
# /etc/sysctl.d/99-vng.conf (add to the same file)
vm.swappiness = 10
```

### Transparent Huge Pages (THP)

THP compaction stalls can cause multi-millisecond pauses.

```bash
echo madvise > /sys/kernel/mm/transparent_hugepage/enabled
echo defer+madvise > /sys/kernel/mm/transparent_hugepage/defrag
```

---

## 2. Runtime Configuration

Environment variables consumed by the daemon:

| Variable | Recommended value | Notes |
|---|---|---|
| `VNG_NATIVE_MAX_CONNECTIONS` | `4096` | Set to ≤ `ulimit -n` minus 256 for stdio/logs |
| `VNG_NATIVE_IDLE_TIMEOUT_MS` | `30000` | 30 s; balance with keepalive settings above |
| `VNG_NATIVE_PORT` | `5678` | Default native binary protocol port |
| `VNG_HTTP_PORT` | `8080` | REST / MCP endpoint |
| `VNG_WAL_SYNC_MODE` | `batch` | `immediate` for strict durability, `batch` for throughput |
| `VNG_LOG_LEVEL` | `info` | Switch to `debug` only during incident investigation |

### Thread pool sizing

```
Worker threads  = min(CPU count × 2, 64)
IO threads      = 4                         # for WAL flush and compaction
```

Set via environment:

```bash
export TOKIO_WORKER_THREADS=$(( $(nproc) * 2 ))
```

For latency-sensitive deployments pin the daemon to isolated CPU cores:

```bash
taskset -c 2-15 voltnuerongridd            # isolate cores 2-15
```

---

## 3. Storage Tuning

### WAL checkpoint interval

Frequent checkpoints limit crash-recovery time but add write amplification.
Infrequent checkpoints reduce write I/O but slow down crash recovery.

Recommended starting point for NVMe storage:

```bash
VNG_WAL_CHECKPOINT_INTERVAL_MS=5000        # checkpoint every 5 s
VNG_WAL_MAX_SEGMENT_BYTES=67108864         # 64 MiB per WAL segment
```

For spinning disks increase to 15 000–30 000 ms and reduce concurrent
compaction threads to 1.

### Columnar page size

Larger pages improve scan throughput; smaller pages reduce read amplification
for point lookups.

| Workload | Recommended page size |
|---|---|
| Analytics / full scans | 1 MiB (`VNG_PAGE_SIZE_BYTES=1048576`) |
| Mixed OLTP/OLAP | 256 KiB (`VNG_PAGE_SIZE_BYTES=262144`) |
| High point-lookup | 64 KiB (`VNG_PAGE_SIZE_BYTES=65536`) |

### MVCC garbage collection threshold

Old row versions accumulate until GC runs.  Low thresholds keep space usage
predictable; high thresholds reduce GC CPU overhead.

```bash
VNG_MVCC_GC_THRESHOLD_ROWS=100000         # run GC after 100 k dead rows
VNG_MVCC_GC_INTERVAL_SECS=60              # also run GC every 60 s
```

If long-running queries are common, raise the interval to avoid GC racing with
active transactions.

---

## 4. Query Tuning

### Keyset pagination (avoid OFFSET on large tables)

```sql
-- Avoid (full scan up to the offset):
SELECT * FROM events ORDER BY id LIMIT 100 OFFSET 50000;

-- Prefer (index seek from the last seen key):
SELECT * FROM events WHERE id > :last_id ORDER BY id LIMIT 100;
```

For time-series data use the timestamp column as the keyset anchor.

### Index selection guidelines

1. Always index columns used in `WHERE`, `JOIN ON`, and `ORDER BY`.
2. Composite indexes: put the highest-cardinality column first when used in
   equality filters; put the range column last.
3. Avoid redundant indexes — each index adds ~10–15 % write overhead.
4. After bulk ingest, run `ANALYZE <table>` to refresh statistics.

### Batch size limits

| Operation | Maximum recommended batch |
|---|---|
| INSERT (columnar) | 50 000 rows per transaction |
| UPDATE / DELETE | 5 000 rows per statement |
| Streaming ingest | 10 MiB per frame |

Larger batches increase memory pressure and can cause lock contention.  If
throughput must be higher, pipeline multiple concurrent batches rather than
enlarging a single one.

---

## 5. Monitoring

### Key signals to watch

| Signal | Normal range | Alert threshold |
|---|---|---|
| Active connections | < 80 % of `VNG_NATIVE_MAX_CONNECTIONS` | > 90 % |
| WAL replication lag | < 100 ms | > 500 ms |
| Query p99 latency (native) | < 20 ms | > 100 ms |
| Query p99 latency (HTTP) | < 50 ms | > 250 ms |
| Error rate (5xx / all) | < 0.1 % | > 1 % |
| GC pause duration | < 5 ms | > 50 ms |
| Heap resident set size | < 80 % of available RAM | > 90 % |

### Useful queries for self-inspection

```sql
-- Active connections
SELECT count(*) FROM vng_internal.connections WHERE state = 'active';

-- Long-running queries (> 5 s)
SELECT query_id, elapsed_ms, query_text
FROM   vng_internal.active_queries
WHERE  elapsed_ms > 5000
ORDER  BY elapsed_ms DESC;

-- WAL position
SELECT current_lsn, checkpoint_lsn,
       (current_lsn - checkpoint_lsn) AS lag_bytes
FROM   vng_internal.wal_status;
```

### Prometheus / OpenMetrics endpoint

```
GET http://<host>:<VNG_HTTP_PORT>/metrics
```

Scrape interval: 15 s.  Relevant metric families:
- `vng_connections_active`
- `vng_query_duration_seconds{quantile="0.99"}`
- `vng_wal_lag_bytes`
- `vng_gc_pause_seconds`
- `vng_errors_total`

---

## 6. Incident Runbook

### 6.1 p99 latency spike

**Symptoms:** `vng_query_duration_seconds{quantile="0.99"}` > threshold for
two or more consecutive scrape intervals.

**Steps:**

1. Check active queries — look for a blocking or long-running query:
   ```sql
   SELECT * FROM vng_internal.active_queries ORDER BY elapsed_ms DESC LIMIT 10;
   ```
2. If a single query is dominating: `KILL QUERY <query_id>;` then investigate
   the plan with `EXPLAIN ANALYZE <query>`.
3. Check for lock contention:
   ```sql
   SELECT * FROM vng_internal.lock_waits;
   ```
4. Check WAL lag — sustained lag can stall MVCC-visible reads.
5. If GC is running concurrently, consider `SET vng.gc_enabled = false;` for
   the duration of the incident and schedule a manual GC off-peak.
6. If the host has memory pressure, restart the daemon during low-traffic
   window after identifying and fixing the root cause.

### 6.2 OOM (Out of Memory)

**Symptoms:** daemon exits with signal 9; kernel log shows `oom_kill_process`.

**Steps:**

1. Capture the OOM report: `dmesg | grep -A 20 oom_kill`.
2. Identify the largest memory consumer from the OOM dump.
3. Common causes and mitigations:
   - **Large sort or aggregation**: add `VNG_QUERY_SPILL_THRESHOLD_MB=512` to
     enable disk-spill for large intermediates.
   - **Cache unbounded**: set `VNG_BUFFER_POOL_MAX_MB` to 70 % of available RAM.
   - **Leak from long-lived connection**: enable connection-level memory
     tracking and set `VNG_NATIVE_MAX_QUERY_MEM_MB=2048` per-session cap.
4. After mitigation, restart the daemon and monitor RSS over 30 minutes.

### 6.3 Connection saturation

**Symptoms:** new connection attempts return `too many clients` or
`ECONNREFUSED`; `vng_connections_active` at or above
`VNG_NATIVE_MAX_CONNECTIONS`.

**Steps:**

1. Identify connection hoarders:
   ```sql
   SELECT client_addr, count(*) AS cnt
   FROM   vng_internal.connections
   GROUP  BY client_addr
   ORDER  BY cnt DESC
   LIMIT  20;
   ```
2. Kill idle connections older than 10 minutes:
   ```sql
   SELECT kill_connection(conn_id)
   FROM   vng_internal.connections
   WHERE  state = 'idle'
     AND  idle_since < now() - interval '10 minutes';
   ```
3. Temporarily raise `VNG_NATIVE_MAX_CONNECTIONS` (up to `ulimit -n` minus
   256) and reload: `kill -HUP $(pidof voltnuerongridd)`.
4. Deploy a connection pooler (e.g. PgBouncer-compatible proxy) in front of
   the daemon if client-side pooling is insufficient.
5. Long-term: audit application code for connection leaks; enforce
   `VNG_NATIVE_IDLE_TIMEOUT_MS` to auto-close idle connections.

---

*Last updated: Sprint S9 — 2026-04-22*
