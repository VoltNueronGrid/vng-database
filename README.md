# VoltNueronGrid DB

VoltNueronGrid DB is a Rust-first HTAP database platform (OLTP + OLAP) designed to move MDAP workloads from in-memory execution to a persistent, scalable, high-performance database engine.

It is designed for:
- Very fast ingestion of analytical data
- Very low-latency retrieval for both OLTP and multidimensional OLAP workloads
- High concurrency with enterprise security
- Local development and cloud-native SaaS deployment
- Extensibility through plugin architecture and language drivers

## What This Repository Contains

- `reference/voltnuerongrid-db-design.md`: full architecture and technical design
- `reference/voltnuerongrid-ws.md`: work structure and phased delivery plan
- `prompts/prompt-1.md`: source requirements prompt

## Recent Implementation Updates

- WS5 TLS hardening: runtime TLS endpoints now expose cert/key preflight readiness (`cert_present`, `key_present`, `cert_pair_configured`, `preflight_ok`) and rotation only proceeds when both cert/key files exist.

## High level architecture diagram

![VoltNueronGrid DB Architecture Diagram](reference/architecture-diagram-v1.png)

## Core Capabilities (Planned)

- ANSI SQL baseline support with DDL/DML and materialized views
- Native AI assistant for chat-to-SQL, extract, ingest, import, and export
- Autonomous database operations with AI models/agents (self-heal, self-tune, self-secure, self-operate)
- UDF support in Rust, JavaScript ES6, and Python
- High availability, fault tolerance, and autoscaling support
- Separate compute and storage architecture
- Fast multithreaded import from CSV, Parquet, JSON, and Excel
- Plugin-based source ingestion from FTP/FTPS, Azure Blob, AWS S3, Google Cloud Storage, WebDAV, and extensible streaming services
- Plugin ecosystem for vector search, geospatial, full-text search, multimodel, and connector adapters
- Native distributed cache engine (Redis-like interoperability + PostgreSQL-friendly cache invalidation patterns)
- Unified HTAP execution model (transactional row-store + analytical columnar engine)
- Support for huge datasets with partitioning, sharding, indexing, and constraints
- Role-based access control and enterprise governance
- Separate UI client (`voltnuerongrid-studio`) and database engine (`voltnuerongridd`)
- Drivers for Python, Rust, Java, JavaScript, TypeScript, Deno, C, C++, and Perl
- First-party IDE extensions for Visual Studio, Cursor, Antigravity, JetBrains, and Eclipse

## Autonomous AI Actions (Planned)

AI models/agents can perform governed operations behind the scenes, with policy checks and mandatory audit trails:

- Provision and manage databases, schemas, tables, indexes, views, and materialized views
- Create and optimize functions (seeded/UDF), vector indexes, and cache policies
- Create/install/upgrade connector and extension plugins (signed artifacts only)
- Auto-tune indexes, statistics, partitioning, cache settings, and pool limits
- Detect and remediate failures (self-heal), including failover/failback orchestration
- Run backup/restore verification, security rotations, and compliance checks
- Diagnose incidents, propose/execute fixes, and generate post-incident evidence summaries

Execution modes:
- `advisory`: AI recommends only
- `supervised`: AI executes pre-approved action classes
- `fully_autonomous`: AI executes all policy-permitted actions with continuous auditing

## Proposed Platform Components

- SQL and session gateway
- Query optimizer and vectorized execution engine
- Storage engine with WAL, checkpoints, and columnar segments
- Metadata and control plane with consensus and failover
- Ingestion subsystem with parallel pipelines
- Ingestion connector runtime with plugin adapters for cloud/object/protocol sources
- Plugin runtime and extension SDK
- Native cache engine cluster for result/object/metadata acceleration
- AI gateway and policy controls
- Autonomous control plane for AI agents and governed operational actions
- Web UI for analysts and administrators
- IDE extension platform for database operations and management
- Multi-language drivers and SDKs

## Architecture Goals

- Rust memory safety and strong performance characteristics
- SOLID and modular design for long-term extensibility
- Observability-first operations (metrics, logs, traces)
- Security-first controls (RBAC, encryption, auditing)
- Deployment parity between local and cloud environments

## Development Roadmap (High Level)

- **R1 (MVP):** single-node engine, SQL baseline, core ingest, basic RBAC, initial drivers, HTAP baseline
- **R2:** distributed execution, metadata HA, autoscaling, broader drivers, connector plugins GA, distributed HTAP baseline
- **R3:** plugin ecosystem GA, advanced indexing, AI expansion, audit platform maturity, IDE extension suite GA, autonomous operations baseline
- **R4:** managed SaaS maturity, global i18n support, full driver matrix, autonomous operations maturity

## Install And Test (Planned Runbooks)

Current repo state is design-first. The commands below are the intended runbooks once scaffolding is in place.

### Single-Node (Run locally):
```bash
Set-Location "D:\by\polap-db"
$env:VNG_ADMIN_API_KEY="secret"
cargo run -p voltnuerongridd
```

### Command to test benchmark:
```bash
Set-Location "D:\by\polap-db"
$env:VNG_ADMIN_API_KEY="secret"
pwsh ./tests/kpi/scripts/run-req10-benchmark-smoke.ps1 -BaseUrl "http://127.0.0.1:8080"
```

### Single-Node (Local Laptop)

- Prerequisites: Docker Desktop, Rust toolchain, 16 GB RAM recommended
- Start service:
  - `docker compose -f deploy/local/single-node.yml up -d`
- Health check:
  - `curl http://localhost:8080/health`
- Basic test flow:
  - create DB/schema/table
  - ingest sample CSV/Parquet/JSON/Excel
  - run OLTP test (insert/update/select with transactions)
  - run OLAP test (aggregate/group-by over large sample)
- Target test command (planned):
  - `cargo test -p voltnuerongrid-server -- --nocapture`

### Multi-Node (Local Laptop / Workstation)

- Prerequisites: Docker Desktop with Kubernetes or `kind`/`k3d`
- Start 3+ node cluster:
  - `docker compose -f deploy/local/multi-node.yml up -d --scale voltnuerongridd=3`
- Validate cluster:
  - verify leader election, shard distribution, and replication health
- Failure test:
  - stop one node and confirm transaction continuity + failover
- Target test command (planned):
  - `cargo test -p voltnuerongrid-distributed -- --nocapture`

### Cloud Single-Cluster (Azure/AWS/GCP/OCI)

- Deploy via Helm/operator:
  - `helm upgrade --install vng deploy/helm/voltnuerongrid -n vng --create-namespace`
- Configure cloud storage + KMS + TLS secrets
- Run smoke tests:
  - connectivity, ingest pipeline, OLTP transaction test, OLAP dashboard query test

### Cloud Multi-Node HA Test

- Deploy 5+ nodes across at least 3 zones
- Enable strict sync profile for zero-loss workloads
- Run resilience tests:
  - node kill, network partition simulation, rolling upgrade
- Verify:
  - RTO/RPO targets, no data loss (critical profile), audit trail integrity

### What to validate in every environment

- OLTP: transaction latency, lock behavior, commit/rollback correctness
- OLAP: scan/aggregate throughput and P95 latency
- HTAP: mixed workload test (OLTP + OLAP concurrency)
- Connectors: FTP/FTPS, Azure Blob, S3, GCS, WebDAV ingest reliability
- Autonomous ops: AI agent actions logged, policy bounded, and reversible

## Target KPI Table (Pass/Fail)

| KPI | Target (Pass) | Fail Condition |
|---|---|---|
| OLTP p95 latency (single-shard txn) | <= 20 ms | > 20 ms |
| OLTP p99 latency (single-shard txn) | <= 60 ms | > 60 ms |
| OLAP p95 query latency (interactive dashboard workloads) | <= 800 ms | > 800 ms |
| OLAP p99 query latency | <= 1500 ms | > 1500 ms |
| HTAP mixed-workload throughput | >= 25,000 read qps and >= 10,000 write tps (benchmark profile) | Either metric below target |
| Bulk ingest throughput scaling | Near-linear scale (>= 80% efficiency) from 1 to N workers until IO ceiling | < 80% scaling efficiency |
| Failover RTO (critical profile) | <= 30 s | > 30 s |
| Failover RPO (critical profile, strict sync) | 0 data loss | Any committed data loss |
| Connector reliability (stream ingest jobs) | >= 99.95% successful checkpoint-resume recovery | < 99.95% recovery success |
| Autonomous action safety | 100% autonomous actions produce auditable trail and policy check | Missing audit/policy evidence for any action |

## Status

Current state: architecture and delivery documentation is defined.  
Next step: scaffold Rust workspace and implement Phase 1 core engine foundations.

## Github Agents:
- status-tracker-10x-executor agent:
  - Runs your status-tracker workflow as a strict 10-item queue, sequentially.
  - Uses multiple subagents in parallel for read-only discovery each iteration.
  - Implements one slice per iteration across SQL, exec, and service layers.
  - Updates both tracker files each iteration:
  - status-tracker.md
  - status-tracker-sprintwise-v1.md
  - Runs targeted tests and full suites.
  - Creates one commit per successful iteration and pushes immediately.
  - Stops immediately on first failed iteration and reports the blocker.
  - Sample prompts:
    - Run the status-tracker 10x executor from the next pending S3-WS1 session.
    - Run the status-tracker 10x executor for only 3 iterations as a dry run.
    - Run the status-tracker 10x executor starting from Session 96.
  
- run-status-tracker-queue prompt:
  - invoke your custom agent Status Tracker 10x Executor
  - accept only one argument: iteration count
  - auto-discover the next pending status-tracker step
  - not require or request a starting session
  - stop on first failure and report blocker details
  - How to use:
    - run prompt: Run Status Tracker Queue
    - argument example: 3

- combinedly:
  - Run only the custom agent (separately)
    - Open chat agent picker.
    - Select Status Tracker 10x Executor.
    - Give only a number as input.
      Examples: 3, 1, 10
    - Behavior:
      - Runs exactly that many iterations.
      - If you leave it empty, it defaults to 10.
  - Run only the companion prompt (separately)
      - In chat, type slash and choose Run Status Tracker Queue.
      - Pass the number as the argument.
      - Examples:
        ```
          /Run Status Tracker Queue 3
          /Run Status Tracker Queue 1
          /Run Status Tracker Queue 5
        ```
      - Behavior:
        - The prompt passes the count to the agent.
        - It auto-finds the next pending tracker step.
        - No starting session is needed.
        - Run them combined
        - This is effectively option 2, because the prompt is wired to invoke the custom agent.
        - So combined usage is the same command:
        - Example:
          - /Run Status Tracker Queue 4
        - Quick recommendation:
          - Use the prompt for daily use.
          - Use the agent directly only when you want manual control or quick debugging.