# HTAP Engine Spec

## 1. Scope and goals

This spec defines a hybrid transactional/analytical processing (HTAP) engine architecture that combines:

- Strong OLTP characteristics (high QPS, low latency) on live data.
- High‑throughput OLAP query processing on the same logical dataset.
- Explicit, tunable bounds on data freshness for OLAP.
- Predictable performance isolation between OLTP and OLAP workloads.

It is informed by research on HyPer, L‑Store, Metis, joint adaptive storage optimization, and recent HTAP surveys. [web:12][web:13][web:15][web:19][web:21][web:25]

This spec is implementation‑oriented: it describes components, interfaces, and key invariants rather than algorithmic proofs.

---

## 2. Architecture overview

### 2.1 Logical components

- **Storage Engine**
  - Columnar base store.
  - Row‑oriented tail (delta) store.
  - Optional row‑projection cache for hot segments.
- **Transaction Manager (TM)**
  - MVCC, transaction IDs, commit protocol.
- **Merge Manager (MM)**
  - Background tail‑to‑base merges.
  - Freshness SLA enforcement.
- **Snapshot Manager (SM)**
  - Creation and lifecycle of OLAP snapshots.
- **Query Processor (QP)**
  - OLTP executor.
  - OLAP executor.
  - HTAP‑aware optimizer.
- **Resource Manager (RM)**
  - CPU/memory partitioning.
  - Workload classes and admission control.

### 2.2 Deployment model

- Shared‑nothing cluster of nodes.
- Each node manages a subset of partitions.
- Optional dedicated OLAP nodes (snapshot consumers only).

---

## 3. Data model and partitioning

### 3.1 Tables and schemas

- Relational tables with fixed schemas.
- Supported types: integer, float, string, timestamp, and basic composites.
- All tables must have a logical primary key.

### 3.2 Partitioning

- Tables are partitioned by range or hash on primary key (configurable per table).
- Partition is the unit of:
  - Storage layout (base and tail segments).
  - Snapshotting.
  - Merge scheduling.
  - Resource accounting.

### 3.3 Segmentation

Within each partition, data is grouped into **segments** (e.g. 2–128 MB of compressed base data):

- Segments are the unit of base storage, tail association, and merge decisions.

---

## 4. Storage layout

### 4.1 Base column store

Each segment's base store:

- Columnar layout with one file/region per column.
- Columns stored in fixed‑size blocks (e.g. 64K–1M values).
- Compression per block (dictionary, RLE, bit‑packing) chosen via heuristics.
- Blocks are immutable once written; updates happen via tail.

Metadata:

- Segment ID, row ID range, min/max values per column, compression metadata.
- Snapshot visibility range: `[min_commit_ts, max_commit_ts]` covered by this base version.

### 4.2 Tail row store

Tail store per partition:

- Append‑only log of row versions.
- Fixed‑size pages, each page contains multiple versions.
- Each version record includes:
  - Logical row ID.
  - Payload (full row or changed columns plus indirection).
  - `begin_ts`, `end_ts`.
  - Pointer to previous version (lineage). [web:13]

Tail store is moderately compressed but optimized for write throughput and recent reads.

### 4.3 Row‑projection cache

For hot segments, the engine maintains a row‑oriented projection:

- In‑memory structure mapping row ID to the latest committed version visible to OLTP.
- Rebuilt lazily from tail and base during merges or background maintenance.

A segment is considered **row‑cached** if it has an active row‑projection; otherwise, OLTP reads use tail + direct base access.

### 4.4 Storage invariants

- All logical rows are represented by:
  - Zero or one base version.
  - Zero or more tail versions.
- Base versions never change in place.
- Tail append operations preserve lineage and MVCC constraints.

---

## 5. Concurrency control and consistency

### 5.1 Transaction model

- Snapshot isolation with optional read committed.
- Transactions have a unique `txid` and logical timestamp (`ts`).
- Read set and write set tracked per transaction.

### 5.2 Read visibility rules

Given snapshot timestamp `T_snap`:

- A version `v` is visible if `v.begin_ts ≤ T_snap < v.end_ts`.
- For a row ID, visible version is the first version in its lineage chain satisfying the above.

### 5.3 Writes

- Inserts: allocate new row ID, create tail record with `begin_ts = commit_ts` and `end_ts = +∞`.
- Updates: create tail record with new values, `begin_ts = commit_ts`, `end_ts = +∞`, and set `end_ts` of previous visible version to `commit_ts`.
- Deletes: similar to update with tombstone flag.

### 5.4 Conflicts

- Write–write conflicts resolved at commit using row‑level checks.
- Read–write anomalies are prevented by snapshot isolation semantics.

---

## 6. Merge Manager (MM)

### 6.1 Responsibilities

- Periodically merge tail records into new base segments.
- Maintain lineage and MVCC correctness.
- Enforce per‑table/partition freshness SLAs.

### 6.2 Merge triggers

Per partition/segment, merges can be triggered by:

- Tail size threshold (bytes or number of versions).
- Freshness SLA: maximum allowed `now - max_commit_ts_in_base`.
- Background policy (e.g. idle time consolidation).

### 6.3 Merge procedure

1. Identify a merge window `[T_min, T_max]` of tail records.
2. Scan base and tail for the segment.
3. For each logical row ID in the segment:
   - Determine the latest version with `begin_ts ≤ T_max` and `end_ts > T_max`.
   - Materialize that version into new base columnar blocks.
4. Write new base segment files with updated metadata (`max_commit_ts = T_max`).
5. Atomically swap segment metadata to point to new base.
6. Mark merged tail records as obsolete; reclaim later.

### 6.4 Invariants

- OLTP never blocks on merges; it writes to new tail pages.
- OLAP snapshots see either old or new base, never a mix.
- Freshness SLA is met by scheduling merges frequently enough given workload.

---

## 7. Snapshot Manager (SM)

### 7.1 Snapshot semantics

A snapshot is defined by a global or per‑partition timestamp `T_snap`.

- All OLAP queries running under a snapshot see exactly the set of versions visible at `T_snap`.

### 7.2 Implementation

- On each node, snapshots are implemented via OS‑level `fork()` or equivalent virtual memory snapshot mechanism, as in HyPer. [web:14][web:17][web:23][web:26]
- The parent process continues OLTP; the child process serves OLAP queries for that snapshot.
- Copy‑on‑write isolates OLAP reads from OLTP writes at page granularity.

### 7.3 Snapshot lifecycle

- Snapshots are created:
  - Periodically (e.g. every N seconds) for recurring analytics.
  - On‑demand for specific OLAP sessions.
- Snapshots are reference‑counted and reclaimed when no queries use them.

### 7.4 Snapshot freshness

- For a given OLAP query, `max_staleness_ms` parameter determines whether it can use an existing snapshot or needs a new one.

---

## 8. Query Processor (QP) and optimizer

### 8.1 Workload classes

- **OLTP queries**: short transactions, served by parent process on live data.
- **OLAP queries**: long, read‑only analytics, served by snapshot processes.

### 8.2 Physical access paths

For table scans, the optimizer can choose:

- `SCAN_ROW` (row‑projection or tail only).
- `SCAN_COLUMN` (base columnar segments).
- `SCAN_HYBRID` (combination):
  - Use base column for filters/projections.
  - Join with tail/row via row ID to fetch latest versions.

### 8.3 Cost model inputs

- Per segment:
  - Tail length, number of recent versions.
  - Presence of row‑projection cache.
  - Base compression ratio and expected scan cost.
- System state:
  - OLTP QPS and latency SLO.
  - Available CPU and memory budgets for OLAP.
  - Merge backlog.
- Query requirements:
  - `max_staleness_ms`.
  - Estimated selectivity and cardinality.

### 8.4 Plan generation

- Generate alternative plans using different access paths (row/column/hybrid) and target stores (live vs snapshot).
- Use Metis‑style heuristics and cost models to select the plan that satisfies freshness requirements while minimizing interference. [web:19]

### 8.5 Runtime adaptation

- Long‑running queries may re‑optimize portions of their plans based on updated statistics (e.g. increased contention or tail growth).
- Operators may switch from row to column paths or vice versa when beneficial.

---

## 9. Resource Manager (RM)

### 9.1 CPU and memory partitioning

- Use OS mechanisms (cgroups, NUMA pinning) to reserve cores and memory for:
  - OLTP parent processes.
  - OLAP snapshot processes.
- Enforce limits per workload class.

### 9.2 Admission control

- OLAP queries are queued when resource budgets are exhausted or when OLTP latency approaches SLO thresholds.
- RM can deprioritize or cancel low‑priority OLAP queries to protect OLTP.

### 9.3 Metrics and SLOs

- Track:
  - OLTP latency distribution (p50, p95, p99).
  - OLAP query latency and throughput.
  - Data freshness (per table/partition).
  - Merge lag and tail sizes.

SLOs are defined per workload class and per table/partition; RM interacts with MM and SM to adjust policies.

---

## 10. Configuration knobs

Key tunables exposed to operators:

- Per table:
  - Partitioning strategy.
  - Freshness SLA (max staleness for OLAP).
  - Merge thresholds (tail size, time‑based).
- Per workload class:
  - CPU/memory budgets.
  - Priority weights.
- Per query:
  - `max_staleness_ms`.
  - Priority (optional).

---

## 11. Future extensions

- **Adaptive controller**: ML or control‑theoretic agent to auto‑tune merge frequency, snapshot intervals, and row‑projection selection based on workload. [web:25]
- **Secondary indexes**: HTAP‑aware index structures with dual‑format maintenance.
- **Cloud‑native deployment**: autoscaling of OLAP snapshot nodes based on workload.

This spec provides a concrete blueprint for implementing a near "no‑tradeoff" HTAP engine by combining state‑of‑the‑art research ideas into a cohesive system design.
