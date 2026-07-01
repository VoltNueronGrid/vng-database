# HTAP Storage & Routing R&D

## 1. Problem statement

Hybrid Transactional/Analytical Processing (HTAP) aims to run OLTP and OLAP workloads on the same live data while preserving high OLTP throughput, low OLTP latency, high OLAP scan performance, and near‑real‑time freshness. [web:12][web:15] In practice, existing systems expose painful tradeoffs between data freshness and performance isolation, and between row‑oriented and column‑oriented storage layouts. [web:12][web:16][web:18]

This document synthesizes current research (HyPer, L‑Store, Metis, joint adaptive storage, and recent HTAP surveys) into a practical design that gets very close to "no tradeoff" for realistic SLAs, while recognizing that perfect optimality for both OLTP and OLAP simultaneously is theoretically unattainable. [web:12][web:13][web:19][web:21][web:25]

---

## 2. Fundamental conflicts in HTAP

### 2.1 Workload characteristics

- **OLTP**: short, latency‑sensitive, update‑heavy transactions with strict consistency requirements and high QPS. [web:18]
- **OLAP**: long‑running, throughput‑oriented, scan‑heavy queries that aggregate over large portions of the dataset. [web:3][web:11]

These workloads pull the system in opposite directions in terms of data layout, concurrency control, and resource scheduling. [web:12][web:18]

### 2.2 Storage format conflict

- Row‑oriented layouts (NSM, slotted pages) are ideal for point lookups and small updates but suffer for wide scans and aggregations. [web:18][web:22]
- Column‑oriented layouts (DSM, compressed column segments) are ideal for scans and vectorized execution but are expensive for single‑row updates and random writes. [web:16][web:18]

All recent surveys explicitly call out the "contradictory format demand" as a core challenge for monolithic HTAP engines. [web:12][web:15][web:21]

### 2.3 Freshness vs isolation

To keep analytics fresh, an HTAP system must synchronize updates from OLTP paths into the structures used by OLAP, e.g., from a row store or delta store into a scan‑optimized column store. [web:12][web:16] Aggressive synchronization improves freshness but consumes CPU, memory bandwidth, and I/O, and can interfere with OLTP latency; lazy synchronization protects OLTP but increases OLAP staleness. [web:12][web:18][web:21]

Surveys and system papers consistently describe this as an unavoidable tradeoff between data freshness and performance isolation. [web:12][web:18][web:21]

### 2.4 Optimizer and routing complexity

In HTAP, the optimizer must choose not only a logical plan but also:

- Which **physical store** to read from (row, column, or both).
- Which **snapshot** or replica to target (live vs consistent snapshot vs remote replica).
- How to account for **sync lag, tail length, and background merge cost** in the cost model. [web:12][web:19][web:21]

Recent work (Metis) shows that classic cost‑based optimizers are insufficient because they ignore HTAP‑specific factors such as data freshness and cross‑workload interference. [web:19]

---

## 3. Survey of existing HTAP architectures

Recent surveys categorize HTAP systems into four main storage architectures. [web:12][web:15][web:21]

### 3.1 Primary row store + in‑memory column store

- OLTP uses a primary row store; OLAP uses an in‑memory columnar copy or projection.
- Examples: Oracle, SQL Server, DB2 BLU with columnar in‑memory analytics on top of row stores. [web:12][web:21]
- Pros: strong OLTP performance; good OLAP performance on hot data.
- Cons: duplication overhead; sync cost; need to choose which columns to replicate. [web:12][web:16]

### 3.2 Distributed row store + column store replica

- OLTP runs on a distributed row store; OLAP runs on replicated columnar nodes receiving asynchronous changes.
- Examples: TiDB / TiFlash, F1 + columnar replicas, some cloud HTAP services. [web:16][web:18][web:21]
- Pros: good performance isolation via separate nodes; natural scale‑out.
- Cons: replication lag and freshness issues; cross‑cluster consistency complexity. [web:12][web:18]

### 3.3 Primary row store + distributed in‑memory column store

- Primary row store for OLTP; distributed in‑memory column store for OLAP tightly integrated into the same service.
- Example: MySQL HeatWave and similar "attached accelerator" architectures. [web:21]
- Pros: strong OLTP; high OLAP throughput on hot, memory‑resident data.
- Cons: complexity of data movement; accelerator capacity limitations; cost of keeping accelerator fresh. [web:21]

### 3.4 Primary column store + delta row store

- Primary column store optimized for OLAP; recent updates stored in a row‑oriented or lightly structured delta store.
- Examples: SAP HANA, HyPer, and L‑Store style systems. [web:13][web:16][web:23]
- Pros: excellent scan performance; reasonably good OLTP via deltas and MVCC.
- Cons: complex merge logic; potential OLTP penalties when delta grows; higher write amplification. [web:13][web:16]

Surveys emphasize that **no single architecture dominates**; each exposes different tradeoffs in the triangle of OLTP performance, OLAP performance, and freshness. [web:12][web:15][web:18][web:21]

---

## 4. Key research building blocks

### 4.1 HyPer: snapshot‑based HTAP

HyPer is an in‑memory hybrid OLTP/OLAP DBMS that uses OS‑level virtual memory snapshots via `fork()` to achieve near‑perfect physical isolation between OLTP and OLAP. [web:14][web:17][web:20][web:23][web:26]

- OLTP runs in the primary process on updatable pages.
- OLAP runs in child processes created by `fork()`; they see a consistent snapshot and operate on copy‑on‑write pages.
- OLAP never locks OLTP data structures and mostly avoids interference except for COW overhead when OLTP writes to snapped pages. [web:14][web:17]

HyPer demonstrates that with enough memory and careful engineering, **a single engine can reach OLTP throughput comparable to dedicated OLTP systems and OLAP performance comparable to specialized column stores** on the same hardware. [web:17][web:20][web:23]

### 4.2 L‑Store: lineage‑based storage

L‑Store proposes a unified real‑time OLTP/OLAP engine using a lineage‑based storage architecture with a read‑optimized columnar base and write‑optimized tail pages. [web:13]

- All updates are appended to tail pages; base pages are read‑optimized and mostly immutable.
- Background merge threads consolidate tail updates into new base pages based on lineage information, without blocking concurrent transactions. [web:13]
- This design supports contention‑free merges and provides both decent OLTP and strong OLAP performance over the same data. [web:13]

L‑Store shows how to decouple **logical correctness (via lineage and MVCC)** from **physical layout (base vs tail)**, enabling non‑blocking reorganization crucial for HTAP. [web:13]

### 4.3 Metis: HTAP‑aware query optimization

Metis rethinks query optimization for HTAP systems by incorporating data freshness, store heterogeneity (row vs column), and workload interference into the optimizer. [web:19]

- It introduces hybrid plans that can read from both row and column stores in a single query.
- It defers some plan choices to runtime and adapts based on current system statistics (tail length, merge lag, contention). [web:19]
- It includes a cost model that explicitly accounts for data freshness and background synchronization overhead. [web:19]

Metis demonstrates that HTAP‑aware optimization can significantly outperform traditional optimizers on mixed workloads. [web:19]

### 4.4 Joint adaptive storage optimization

Recent work on joint adaptive storage for HTAP systems formulates the storage layout problem (which attributes/segments to replicate in columnar form, merge policies, etc.) as a cost‑based optimization that includes synchronization overhead and workload characteristics. [web:25]

- The optimizer considers OLTP and OLAP costs plus sync/merge overhead.
- It adaptively decides which columns/segments get dual‑format storage and how aggressively to merge.

This provides a principled way to manage the row/column tradeoff dynamically rather than statically. [web:25]

### 4.5 HTAP surveys

Multiple recent surveys (2022–2024) synthesize the state of HTAP research and outline five key challenge areas: hybrid workload processing, data organization, data synchronization, query optimization, and resource scheduling. [web:12][web:15][web:18][web:21][web:22]

They collectively underline that:

- Freshness vs isolation is the central unresolved tension.
- Data layout must be adaptive and workload‑aware.
- Optimizers and schedulers must be HTAP‑aware to make good routing decisions.

---

## 5. Proposed HTAP design

The proposed design is not a single existing system but a composition of proven techniques into a coherent architecture suitable for a modern HTAP engine.

### 5.1 High‑level goals

- **Strong OLTP performance**: comparable to a dedicated row‑store OLTP system for typical transactional workloads.
- **Strong OLAP performance**: columnar throughput for scans and aggregates that is close to specialized column stores.
- **Bounded freshness**: tunable staleness guarantees, with the option of zero staleness at a predictable CPU cost.
- **Performance isolation**: OLAP should not unpredictably degrade OLTP latency; interference must be controlled and observable.

### 5.2 Storage architecture: column base + row tail + optional row projection

Each table is partitioned into segments (by range or hash). For each segment, maintain:

1. **Base column store**
   - Compressed, vectorized, scan‑optimized.
   - Contains a stable snapshot of data up to some merge point.
   - Organized in columnar segments with dictionary/RLE/bit‑packing compression where appropriate. [web:16][web:23]

2. **Tail row store (delta)**
   - Append‑only, write‑optimized row format.
   - Stores recent inserts, updates, and deletes, each with MVCC metadata (transaction id, commit timestamp, lineage pointers). [web:13]
   - Logically chained to base via lineage so each logical row has a version chain. [web:13]

3. **Optional row‑projection cache (for hot segments)**
   - In‑memory row‑oriented projection built from tail plus the latest base version for hot rows.
   - Used for OLTP point lookups and small range scans on hot partitions.

The joint storage optimizer decides per segment whether to maintain the row‑projection cache based on workload and cost. [web:25]

### 5.3 MVCC and lineage

- MVCC provides snapshot isolation across both OLTP and OLAP queries. [web:16][web:18]
- Each tuple version (in base or tail) includes:
  - `begin_ts`, `end_ts` (or equivalent transaction metadata).
  - A lineage pointer to the previous version, forming a version chain. [web:13]
- Logical reads reconstruct the visible version by traversing base + tail up to the snapshot timestamp, but in practice this is optimized by storing direct pointers to the latest visible version in the tail. [web:13]

### 5.4 Synchronization: bounded‑freshness merges

A background merge service operates per segment:

- Periodically (or based on thresholds), it reads tail pages, coalesces them with the base, and writes new base pages with updated lineage. [web:13]
- Merges are non‑blocking: OLTP continues writing to new tail pages while the merge reads the old ones. [web:13]
- Merge scheduling respects a **freshness SLA** per table/segment, e.g. "OLAP must see data ≤ X seconds stale" and uses tail growth rate to compute merge frequency. [web:12][web:21][web:25]

For queries that demand **strictly current data (zero staleness)**, the engine uses a hybrid access path that reads both base and tail directly, bypassing waiting for merges. [web:19]

### 5.5 Snapshot‑based OLAP isolation

To minimize interference:

- OLTP runs in the primary process, modifying tail and occasionally causing COW on hot base pages.
- OLAP runs primarily on **snapshots** created via OS `fork()` or equivalent virtual memory snapshotting on each node, as in HyPer. [web:14][web:17][web:20][web:23][web:26]

Properties:

- Snapshots are time‑consistent views defined by a global or per‑partition timestamp.
- OLAP queries execute in child processes pinned to dedicated cores, reading predominantly from base column store pages.
- Copy‑on‑write overhead is proportional to OLTP write rate, but OLAP does not contend for OLTP locks or internal data structures. [web:17]

This provides strong physical isolation by design while retaining shared in‑memory data. [web:17][web:20]

### 5.6 Query routing and HTAP‑aware optimization

The optimizer exposes multiple physical alternatives per logical operator:

- **Row variant**: Accesses row‑projection cache or tail only.
- **Column variant**: Accesses base column store.
- **Hybrid variant**: Combines both, e.g., use column store for filtering and then fetch payloads via row/tail RIDs, or probe hot keys via row cache and the rest via column store. [web:19][web:16]

The cost model incorporates:

- Tail length and merge backlog per segment.
- Cache residency and compression effects for base vs tail.
- OLTP load and latency SLOs (how much OLAP work can share cores).
- Query‑level freshness requirement (`max_staleness_ms`). [web:12][web:19][web:21]

Metis‑style runtime adaptation allows re‑optimization of long queries based on on‑line statistics (e.g. if contention or tail length changes significantly). [web:19]

### 5.7 Resource scheduling and isolation

To protect OLTP:

- Use CPU/memory partitioning (cgroups/NUMA pinning) to allocate dedicated cores and memory budgets to OLTP vs OLAP. [web:18][web:19]
- Treat OLAP as a separate workload class with its own queue and backpressure; throttle or defer OLAP when OLTP latency approaches SLO boundaries. [web:19]
- Optionally, run OLAP snapshots on separate "wimpy" nodes that host snapshots only, as HyPer supports. [web:20]

This converts isolation from a best‑effort property into an explicit scheduling policy.

---

## 6. Why perfect "no‑tradeoff" is impossible

All major HTAP surveys and system papers converge on the conclusion that some form of tradeoff is inherent: optimizing fully for OLTP and OLAP simultaneously on the same hardware and data structures is not achievable. [web:12][web:15][web:18][web:21]

Key reasons:

- Row vs column formats are fundamentally at odds for mixed read/write vs scan workloads. [web:16][web:18]
- Freshness vs isolation: synchronization between OLTP and OLAP structures always consumes resources and must be balanced against latency requirements. [web:12][web:18][web:21]
- Hardware limits: CPU, cache, memory bandwidth, and I/O are finite; scheduling must apportion them between workloads.

The proposed design instead **turns tradeoffs into tunable, explicit knobs**: freshness SLA, merge aggressiveness, row‑projection selection, snapshot frequency, and resource caps.

---

## 7. Implementation and research directions

Potential implementation/research steps based on this design:

1. **Prototype storage engine**
   - Implement column base + tail row store with lineage and MVCC.
   - Add background merge with configurable policies.

2. **Metis‑style optimizer**
   - Extend a cost‑based optimizer to handle row/column/hybrid alternatives.
   - Integrate freshness and interference into the cost model.

3. **Snapshot execution layer**
   - Integrate OS‑level snapshots (e.g. `fork()` on Linux) to run OLAP queries on consistent views.
   - Add resource partitioning and queueing for OLTP vs OLAP.

4. **Joint adaptive storage controller**
   - Periodically evaluate workload statistics and reconfigure row‑projection caches and merge thresholds via a cost model or RL‑based controller. [web:25]

5. **Benchmarking**
   - Use CH‑Benchmark, HTAPBench, and custom workloads to evaluate OLTP throughput, OLAP latency, freshness, and isolation properties. [web:12][web:21]

This architecture is grounded in published research and provides a concrete path toward an HTAP engine that behaves "almost tradeoff‑free" for realistic enterprise workloads while making the remaining tradeoffs explicit and controllable.
