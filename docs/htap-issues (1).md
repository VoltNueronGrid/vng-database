# Real-World Issues in HTAP Databases

This document summarizes why many so‑called HTAP systems fail to deliver *true* hybrid transactional‑analytical behavior in practice, and catalogs design issues, scaling problems, and plausible mitigations.

---

## 1. Why “Real” HTAP Is Hard

Hybrid Transactional/Analytical Processing (HTAP) aims to run OLTP and OLAP workloads on the *same* logical data with:

- Near‑isolated OLTP latency and throughput.
- Fresh data for analytics (seconds or less behind OLTP).
- Minimal operational and architectural complexity.

In practice, current systems are forced into trade‑offs between:

- **Performance isolation vs. data freshness**: separating OLTP and OLAP improves isolation but introduces replication/ETL lag; tight coupling improves freshness but causes interference between workloads.[page:1][page:2]
- **Row vs. column layout**: OLTP prefers row‑oriented (NSM) layouts; OLAP prefers columnar (DSM) layouts. Supporting both typically means maintaining two formats or doing expensive conversions.[page:1][page:2]
- **Single‑engine simplicity vs. multi‑engine realism**: monolithic designs struggle to optimize for both workloads simultaneously; multi‑engine architectures re‑introduce sync, routing, and consistency problems.[page:2][page:5]

A large survey of HTAP systems concludes that no existing architecture simultaneously maximizes performance isolation, data freshness, and efficiency: every real system picks a point on that triangle.[page:2][page:5]


---

## 2. Architectural Patterns and Their Inherent Issues

Modern HTAP systems typically fall into two broad styles:

- **Single‑instance / monolithic HTAP**: one engine and one primary data representation shared by OLTP and OLAP.
- **Dual‑store / hybrid HTAP**: a row‑oriented transactional store plus a column‑oriented analytical store, tightly integrated but still distinct.

A third variant is **distributed HTAP**, which is usually a dual‑store design stretched across a cluster.

### 2.1 Single‑Instance HTAP: Core Problems

Single‑instance systems keep a single logical copy of the data and let OLTP and OLAP share it (e.g., early in‑memory systems like HyPer‑style designs).[page:1][page:2] They run into several fundamental issues.

#### 2.1.1 Consistency and Synchronization Overheads

To let long‑running analytical queries see consistent snapshots while OLTP keeps updating rows, single‑instance systems use variants of:

- **Snapshotting / copy‑on‑write (CoW)**.
- **Multi‑Version Concurrency Control (MVCC)**.

Both introduce heavy overhead in HTAP settings:

- Snapshotting often relies on bulk copying (e.g., `memcpy` of large regions), which causes huge data movement between CPU and memory, saturating memory bandwidth and lowering throughput for OLTP and OLAP.[page:1]
- MVCC avoids full copies but maintains long version chains. Analytical scans must traverse many versions per tuple, generating large amounts of random memory accesses and significantly lowering analytical throughput under write‑heavy workloads.[page:1]

Empirical evaluation in one HTAP study shows transactional throughput drops up to roughly **75%** under snapshotting as analytic query load increases, and analytical throughput drops over **40%** under MVCC as transactional update load increases, compared to isolation baselines.[page:1]

**Implication:** even theoretically elegant MVCC/CoW schemes degrade badly once you mix heavy OLTP writes with wide OLAP scans on the same instance.

**Partial mitigations:**

- Restrict HTAP to relatively *read‑light* OLTP workloads and modest OLAP concurrency.
- Use snapshotting schemes that exploit hardware features like virtual memory CoW or processing‑in‑memory to reduce copying, but these are complex and often hardware‑dependent.[page:1][page:6]
- Limit snapshot frequency or rely on slightly stale snapshots for OLAP to reduce interference, at the cost of freshness.

#### 2.1.2 Storage Layout Conflict: Row vs. Column

A single instance cannot simultaneously be “ideal OLTP row store” and “ideal OLAP column store”.[page:1][page:2][web:21]

- OLTP wants row‑oriented NSM: compact tuples, cheap point‑lookups, cheap updates touching many columns.
- OLAP wants columnar DSM: scans over a few columns, compression, vectorization, and better CPU cache behavior.

Single‑instance designs thus face a forced choice:

- Use a **row layout**: OLTP is fast, but analytical scans suffer (more data read, fewer compression and SIMD gains).
- Use a **column layout**: analytics are fast, but point updates and small transactions become expensive.
- Use a **hybrid layout** (e.g., some columns columnar, others row‑oriented, or vertically/horizontally partitioned hybrids), which introduces:
  - Extra complexity in query planning and data maintenance.
  - Potentially expensive periodic conversion between formats, which hurts freshness and throughput.[page:1][page:2]

A recent survey classifies HTAP storage architectures along exactly these trade‑offs and points out that naive duplication of “full row store + full column store” doubles space and maintenance cost, while partial/hybrid layouts require sophisticated cost models or ML to pick which columns to replicate in which format.[web:21]

**Partial mitigations:**

- Choose a primary layout (e.g., row) and maintain columnar *indexes* or projections only for hot analytical columns, accepting that analytics are somewhat constrained but OLTP remains primary.[web:21]
- Use adaptive storage (hybrid row/column partitions) chosen by cost‑based or learned models, but this adds substantial complexity to the engine and optimizer.[web:21]

#### 2.1.3 Performance Interference and Resource Contention

In a single‑instance design, OLTP and OLAP share:

- CPU cores and caches.
- Memory bandwidth and NUMA interconnects.
- Buffer pool and storage I/O.

Analytical scans are bandwidth‑hungry; they naturally interfere with latency‑sensitive OLTP traffic.[page:1] Even assuming ideal consistency mechanisms (no extra copying), empirical studies still observe around **30% transactional throughput loss** when running analytics concurrently vs. OLTP alone, purely due to contention for shared hardware resources.[page:1]

**Partial mitigations:**

- NUMA‑aware partitioning and pinning: bind OLTP threads and data to certain sockets, OLAP threads to others.
- Priority‑aware schedulers: limit concurrency or priority of OLAP queries when OLTP is under load.
- “Soft” workload isolation via resource groups or cgroups, throttling analytics under pressure.

These improve things but do not fundamentally solve the shared‑resource contention.


### 2.2 Dual‑Store / Hybrid HTAP: Core Problems

To break out of single‑instance constraints, many systems adopt some form of **dual‑store** architecture:

- A **primary row store** (OLTP) and a **secondary column store** (OLAP), sharing logical schema but using different physical layouts.[web:21]
- The two stores may live on the same node (e.g., Oracle in‑memory, SQL Server columnstore, Hyper/HANA‑style delta+main) or in a distributed deployment (TiDB, F1 Lightning, HeatWave).

This approach improves workload‑specific optimization but introduces a different class of issues.

#### 2.2.1 Data Synchronization and Freshness Lag

Dual‑store systems must continuously propagate changes from the OLTP side to the OLAP side:[web:21]

- **Gather updates** from transactional logs or delta tables.
- **Ship updates** to the analytical store.
- **Transform layouts** (row → column, with compression/dictionaries).
- **Apply updates** into large columnar segments or files.

This pipeline is expensive:

- In controlled experiments, the cost of just **shipping** updates (gathering from logs, scanning, and transferring) already reduces transactional throughput by about **15–20%** under moderate write intensity.[page:3]
- The **update application** step (transforming and applying row‑oriented deltas into compressed columnar storage) can reduce transactional throughput by roughly **50–60%** at high write intensities because of decompression, re‑encoding, and data movement.[page:3]

To keep OLTP fast, vendors typically:

- Ship and apply updates asynchronously in batches, or
- Merge delta stores into column stores on a coarse schedule.

This improves throughput but means analytical queries see stale data; measured replication/merge lags of **hundreds of milliseconds to seconds** are common, especially in distributed setups.[web:21]

**Partial mitigations:**

- **Delta + main split**: OLAP queries read from both the large immutable column store and an in‑memory row‑oriented delta store to see recent updates, but this complicates scan logic and can hurt OLAP performance when deltas are large.[web:21]
- **Adaptive merge policies**: merge based on workload and freshness SLAs (e.g., more frequent merges when freshness is critical, less frequent when throughput is more important).[web:21]
- **Log‑based replay**: use change‑data‑capture and replay logs into columnar replicas to avoid re‑scanning base tables, but replay itself still consumes CPU and bandwidth.[web:21]

#### 2.2.2 Layout Conversion and Compression Costs

Transforming a single row‑oriented update (e.g., “update customer balance”) into an efficient columnar representation involves:

- Mapping the row key to column segments.
- Potentially decompressing column segments.
- Updating values and recompressing (often with dictionary encoding or other schemes).[page:3]

HTAP research identifies this “update application” step as one of the main performance bottlenecks in dual‑store designs; a significant fraction of CPU cycles and cache misses go into compression/decompression and dictionary maintenance.[page:3]

**Partial mitigations:**

- Use **append‑only** columnar segments with periodic compaction, avoiding in‑place updates at the cost of more complex read paths and larger storage.[web:21]
- Maintain **delta columns** in an uncompressed or lightly compressed format and apply updates in bulk, amortizing cost.[web:21]
- Use hardware‑aware algorithms and processing‑in‑memory to accelerate dictionary construction and re‑encoding, as explored in Polynesia‑style co‑designed systems.[page:3][page:5]

#### 2.2.3 Consistency Guarantees Across Stores

With two physical stores, you need to define what “fresh” and “consistent” mean:

- Do OLAP queries see only committed data or also in‑flight transactions?
- Is there a defined freshness window (e.g., data up to `T - 500ms` is guaranteed visible)?
- Are cross‑store reads (e.g., joins between OLTP and OLAP tables) transactional?

The HTAP survey formalizes this as a trade‑off between **data freshness** and **performance isolation**: tighter freshness guarantees generally require more frequent propagation or synchronous replication, which slows down OLTP.[web:21]

Example observations:[web:21]

- Systems with strong isolation between OLTP and OLAP instances maintain **high throughput** but may have replication lags on the order of **hundreds of milliseconds to seconds**.
- Systems with near‑real‑time freshness (e.g., microsecond‑scale snapshots) show **20–40% performance degradation** when running mixed workloads compared to isolated workloads.

**Partial mitigations:**

- Expose freshness as an explicit SLA knob and let applications choose per query (e.g., “stale up to X seconds” vs. “must see latest committed”).
- Use multi‑versioned data and timestamped snapshots in the analytical store to decouple scan progress from ongoing propagation, at the cost of extra space and GC overhead.[web:21]

#### 2.2.4 Complexity in Query Routing and Optimization

Your intended “single database where queries route to OLTP or OLAP engine based on pattern” introduces several non‑trivial requirements:

- The optimizer must decide *for each query* whether to run it on the row store, the column store, or a combination.[web:21]
- Cost models must understand very different performance envelopes (point lookups vs. scans) and the cost of reading delta vs. base segments.
- Join planning becomes more complex if some tables are better served from row store and others from column store.

Research systems and modern HTAP products implement **hybrid scan** strategies and access‑path selection to decide row vs. column access.[web:21] But achieving reliable plans requires sophisticated statistics, runtime feedback, and often manual tuning.

**Partial mitigations:**

- Initially constrain query routing using simpler rules (e.g., OLTP engine for short queries with selective predicates; OLAP engine for long scans and aggregates), then enrich with cost‑based decisions and runtime adaptivity.
- Use **materialized views** or specialized columnar projections for known heavy analytics, routing only those queries to OLAP and leaving ad‑hoc small queries on OLTP.


### 2.3 Distributed HTAP: New Failure Modes at Scale

Distributed HTAP systems (TiDB, F1 Lightning, HeatWave, AlloyDB, Snowflake‑style architectures) extend the dual‑store pattern across multiple nodes.[web:21][web:16]

This adds further sources of complexity:

- **Replication latency and bandwidth limits**: change logs must be shipped over the network, so high write rates can saturate links and increase replication lag, directly hurting freshness.[web:21]
- **Distributed transactions and global snapshots**: maintaining consistent read views across nodes requires global timestamps or snapshot protocols, which add coordination overhead and can limit scalability.[web:21]
- **Skew and hotspotting**: if analytical workload is skewed toward certain partitions, those nodes become hotspots, hurting both OLTP and OLAP queries.
- **Failure recovery**: failed replicas must be rebuilt from logs and columnar data, which can be slow and further reduce effective capacity during recovery.

**Partial mitigations:**

- Partition data along workload boundaries and offload heaviest analytical queries to dedicated replicas or nodes.
- Use log compaction and multi‑level delta merging to keep replay manageable.[web:21]
- Employ resource‑aware schedulers that distinguish OLTP vs. OLAP tasks and limit interference across node pools.


---

## 3. Core Technical Pain Points and Their Root Causes

This section catalogs issues you are likely to hit when trying to build a “real” HTAP system where writes land once, and queries automatically route to either an OLTP or OLAP engine.

### 3.1 Data Ingestion Into a Single Logical Database

**Goal:** ingest data once into a unified system, then query it via both transactional and analytical paths.

In practice, this means:

- **Primary write path**: typically into a row‑oriented OLTP engine for transactional semantics.
- **Secondary propagation path**: into analytical structures (column store, indexes, materialized views, or replicas).

Key problems:

1. **Double work per write**: every logical write eventually leads to two physical updates (row + column) or an update plus a downstream transformation, consuming CPU, memory, and I/O.[page:3][web:21]
2. **Latent analytical visibility**: to preserve OLTP throughput, propagation is batched or asynchronous, so analytics are behind the tip of the log.[web:21]
3. **Write‑amplification**: compressed, sorted, or log‑structured columnar formats often require rewriting large segments for small updates, amplifying I/O.[page:3][web:21]
4. **Ordering and idempotency**: ensuring that updates arrive and apply in the correct order, exactly once, in the face of failures in the propagation pipeline.

**Potential design patterns:**

- Use **log‑centric architecture**: treat the OLTP commit log as the “source of truth”, and have both row and column views subscribe to it. This isolates ingestion to one place but does not remove downstream work.
- Accept **eventual consistency** for OLAP: allow analytics to lag by a bounded window and design your product semantics around this.
- Use **append‑only analytics** with background compaction: avoid synchronous rewriting of large segments on each update.


### 3.2 Routing Queries Between OLTP and OLAP Engines

Your desired behavior—“based on the query, it should redirect to either OLTP or OLAP engines”—is conceptually similar to what TiDB, MySQL HeatWave, and Oracle in‑memory systems do.[web:21]

Key issues:

1. **Query classification is non‑trivial**.
   - Simple heuristics (e.g., `SELECT` with aggregates goes to OLAP) break easily on real workloads.
   - You need cost‑based decisions that understand data distribution, index availability, and current system load.[web:21]

2. **Plan stability and predictability**.
   - If routing flips between engines due to small cost estimate changes, users see unpredictable performance.

3. **Cross‑engine joins**.
   - Queries sometimes need data that is “better” located in both engines (e.g., one table only in OLTP, another heavily replicated in OLAP). Supporting efficient cross‑engine joins is complex and often avoided.

4. **Freshness‑aware routing**.
   - The OLAP engine might be behind the OLTP engine. The optimizer must decide whether routing to OLAP is allowed for queries that require up‑to‑the‑latest data.

**Partial mitigation strategies:**

- Start with **conservative routing rules**: send only large scans/aggregates to OLAP; keep everything else on OLTP, then gradually expand routing coverage.
- Expose **hints or engine selection** to the application (e.g., query hints, separate connections or schemas) for critical queries where correctness or latency demands a specific engine.
- Maintain **engine‑specific statistics** and a unified cost model that understands both row and column stores.


### 3.3 Time‑Based vs. Event‑Driven Propagation

You mentioned that injecting data into OLAP is “mostly time‑based (or not, I am not sure)”. In practice, systems use combinations of:

- **Time‑based triggers**: e.g., merge deltas every X ms or seconds.[web:21]
- **Volume‑based triggers**: e.g., merge when delta size exceeds a threshold, such as 64MB, or number of changes exceeds a limit.[web:21]
- **Demand‑based triggers**: e.g., force merge when an OLAP query explicitly requires the freshest data.[web:21]

Each scheme has trade‑offs:

- Frequent time‑based merges improve freshness but increase overhead and reduce throughput.
- Large volume thresholds improve throughput but allow deltas to grow large, which hurts query performance and freshness.
- Demand‑based merges introduce unpredictable latency spikes when a query triggers a large merge.

**Better practice:**

- Implement **adaptive merge policies** that combine time, volume, and workload feedback.
- Prioritize merging hot partitions and columns that are most frequently read analytically.
- Allow per‑query freshness hints so you do not always pay the cost of merging for every OLAP query.


### 3.4 Mixed Workload Scheduling and Resource Management

At scale, mixed workloads magnify scheduling issues:

- OLTP wants low latency and predictable tail behavior.
- OLAP wants throughput and saturates CPU, memory, and I/O.

Research on HTAP resource scheduling emphasizes that tuning thread counts and resource shares is critical; assigning more resources to OLAP improves analytical throughput but can starve OLTP, and vice versa.[web:21]

**Common issues:**

- OLAP queries causing long stalls for OLTP when they compete for locks, buffer pool pages, or log bandwidth.
- OLTP spikes (e.g., traffic bursts) causing starvation for analytical jobs.
- Over‑provisioned clusters where resources are wasted to maintain headroom for both workloads.

**Potential solutions:**

- **Workload‑driven scheduling**: dynamically adjust OLTP/OLAP thread pools based on observed load and SLAs.[web:21]
- **Freshness‑driven modes**: switch between modes (freshness‑priority vs. isolation‑priority) based on application needs, as explored in freshness‑driven schedulers.[web:21]
- Use **separate resource pools** within a cluster for OLTP and OLAP, with controlled sharing.


---

## 4. Scaling Issues as Data and Workloads Grow

When the database grows to hundreds of billions of rows or high write/query rates, all of the above issues worsen.

### 4.1 Data Volume and Memory Pressure

Many high‑performance HTAP prototypes assume **in‑memory** primary and/or analytical stores, which limits practical dataset sizes.[web:18][web:21] As data grows:

- Maintaining dual formats (row + column) doubles or worse the memory footprint, forcing more frequent eviction and I/O.
- Snapshotting and MVCC chains get longer in wall‑clock time, increasing overhead.[page:1]
- Garbage collection for old versions and delta segments becomes significant work.

**Mitigations:**

- Move to **tiered storage** (hot data in memory, warm data on fast SSD, cold data on cheaper storage) with HTAP‑aware caching policies.
- Compress analytical data aggressively and avoid duplicating columns that are rarely used in analytics.


### 4.2 High Update Rates

Real‑world HTAP workloads (e.g., IoT, telemetry, finance) often have very high write rates. Under such conditions:

- Data propagation pipelines might struggle to keep up, causing increasing staleness in analytical views.[web:21]
- MVCC version chains grow rapidly, making analytic reads slower and increasing GC overhead.[page:1]
- Log‑based replay queues can backlog, increasing recovery times after failures.

**Mitigations:**

- Apply strict **back‑pressure** from OLTP to ingestion sources when downstream analytics cannot keep up.
- Partition by time (e.g., time‑series tables) to limit the amount of data that needs frequent update propagation; older partitions can be treated as immutable.


### 4.3 Cluster‑Level Scaling and Skew

In distributed HTAP systems, scale amplifies operational problems:[web:21]

- Skewed workloads create **hot partitions** where both OLTP and OLAP traffic concentrate.
- Rebalancing partitions can require moving both row and columnar data, which is expensive and sometimes disruptive.
- Node failures require rebuilding both transactional state and analytical replicas.

**Mitigations:**

- Use **consistent hashing with virtual nodes** to make redistribution cheaper.
- Allow **asymmetric roles** for nodes: some nodes primarily serve OLTP, others OLAP, with carefully controlled overlap.


---

## 5. Why Many Products Are “Not Really HTAP”

Given these challenges, many commercial systems marketed as HTAP are, in practice:

- OLTP systems with opportunistic column indexes or replicas that support **near‑real‑time analytics on a subset of queries**.
- OLAP systems with limited concurrency controls that can handle only **light transactional updates**.
- Architectures that still rely on **separate engines glued by CDC/ETL**, just with reduced latency.

Industry definitions have also become more relaxed—modern explanations of HTAP sometimes emphasize “unified ingestion and analytics with low lag” more than strict OLTP semantics.[web:19][web:16] That is, a system can claim HTAP if it ingests events and makes them queryable quickly, even if it does not offer full ACID transactional workload isolation on the same engine.

If your bar for “true HTAP” is:

- Strict OLTP semantics.
- Near‑instant analytics on the very latest committed data.
- Minimal performance impact of analytics on OLTP.

then current general‑purpose systems generally fall short, except in fairly constrained workloads where data sizes or update rates are modest, or where you accept some compromise on one of these dimensions.


---

## 6. Practical Design Recommendations

If you are designing your own HTAP‑style system (especially at billion‑scale), the research suggests favoring **clear, explicit trade‑offs** rather than chasing the perfect converged engine.

### 6.1 Prefer Log‑Centric, Dual‑Store Architectures

- Treat your **OLTP log** as the canonical data stream.
- Maintain **separate physical representations**: a row‑oriented store for transactions, a column‑oriented or specialized store for analytics.
- Use **CDC and log‑based replay** to feed analytics, accepting and exposing bounded staleness.[web:21]

This gives you cleaner isolation and simpler reasoning about failure and recovery.

### 6.2 Make Freshness an Explicit Contract

- Let applications specify acceptable freshness targets per query or per workload, and tune propagation frequency/merge policies accordingly.[web:21]
- Provide metrics and observability for actual replication lag and version age, so operators can see how well they meet targets.

### 6.3 Keep Query Routing Initially Simple

- Start with conservative rules for routing queries to OLTP vs. OLAP and gradually introduce cost‑based optimization as you gain stats.
- Offer explicit hints or separate “OLTP vs. analytics” entry points to avoid surprising behavior in critical paths.

### 6.4 Use Time‑Partitioned Storage and Deltas

- Partition tables by time, making older partitions immutable and stored in columnar form; keep recent partitions in row‑oriented or lightly compressed form.
- Periodically roll hot partitions forward based on workload and freshness needs.

### 6.5 Isolate Resources Where Possible

- Use resource groups or distinct node pools for OLTP and OLAP workloads, even if they share underlying data.[web:21]
- Limit OLAP concurrency in proportion to available headroom, rather than allowing unbounded analytical load.

---

## 7. Summary

Research and practice both show that HTAP is fundamentally constrained by three tensions:[page:1][web:21]

- **Freshness vs. Performance Isolation**: closer coupling yields fresher data but worse interference.
- **Row vs. Column Layout**: each format optimizes a different workload; hybrids add complexity and cost.
- **Single vs. Dual Engine**: monolithic designs simplify logical architecture but struggle with conflicting requirements; dual‑store designs improve specialization but re‑introduce synchronization and consistency issues.

Most current HTAP products choose a specific compromise along these axes and then market that choice as “HTAP”. For true, high‑scale HTAP with strict guarantees, your architecture needs to be explicit about which compromises you accept and to build mechanisms (logs, deltas, adaptive merges, scheduling policies) that make those trade‑offs controllable rather than accidental.
