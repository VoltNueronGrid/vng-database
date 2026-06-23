# Architecture Repo Facts: VoltNueronGrid DB

**Purpose**: Record observable repository facts that support reverse generation of the project-level 4+1 architecture artifacts.

## Repository Identity

| Fact | Evidence Source | Confidence | Architecture Relevance |
|------|-----------------|------------|--------------------------|
| VoltNueronGrid DB is a Rust-first HTAP database platform for moving MDAP workloads from in-memory execution to persistent OLTP + OLAP storage. | `README.md`; `services/voltnuerongridd/reference/voltnuerongrid-db-design.md` | High | Establishes the central product boundary: database engine, not only UI or API wrapper. |
| The repository is a Rust workspace with crates for core, config, SQL, execution, storage, ingest, plugins, AI, auth, audit, failover, MCP, service, drivers, tools, soak tests, and benchmarks. | `Cargo.toml` | High | Supports modular development boundaries. |
| The product includes runtime service, Studio UI, first-party drivers, MCP/admin integration, deployment assets, and KPI gate scripts. | `README.md`; `ui/voltnuerongrid-studio/package.json`; `tests/kpi/scripts/*.ps1`; `deploy/**` | High | Supports multi-surface scenario and physical boundaries. |
| The current constitution requires durable HTAP correctness, security/RBAC, reproducible performance evidence, native interfaces, governed autonomous/plugin actions, and evidence-backed delivery. | `.specify/memory/constitution.md` | High | Supplies cross-cutting architecture constraints. |

## Entry Points

| Entry Point | Type | Evidence Source | Observed Responsibility | Supported Scenario |
|-------------|------|-----------------|--------------------------|--------------------|
| `voltnuerongridd` service router | Runtime service | `services/voltnuerongridd/src/router.rs` | Exposes health, metrics, SQL, OLAP, failover, admin database/user/grant, SRE, audit, security, autonomous, store, WAL, Raft, ingest, and plugin route groups. | Database operation, administration, HA/failover, observability. |
| Native listener/runtime protocol | Native database connection path | `deploy/local/README.md`; `services/voltnuerongridd/reference/security-compliance-checklist-v1.md`; `drivers/voltnuerongrid-driver-rust/README.md` | Supports optional native listener, TLS/mTLS/bearer-token configuration, and Rust driver native wire I/O. | Programmatic native access. |
| Studio UI | Desktop/web management client | `ui/voltnuerongrid-studio/package.json`; `ui/voltnuerongrid-studio/README.md`; `ui/voltnuerongrid-studio/tests/e2e/*.spec.ts` | Provides React/Tauri management UI, contract checks, and Playwright e2e tests. | Human database management. |
| First-party drivers | Client libraries | `drivers/**/README.md`; `services/voltnuerongridd/reference/driver-core-contract-v1.md` | Define shared configuration, auth header, retry, timeout, error, and transport expectations. | Programmatic app access. |
| Deployment assets | Runtime packaging | `deploy/local/README.md`; `deploy/cloud/README.md`; `deploy/helm/voltnuerongrid/**` | Local install guide, draft cloud profiles, and Helm assets. | Local operation and future cloud deployment. |
| KPI gates and CI | Evidence-producing validation | `tests/kpi/scripts/*.ps1`; `.github/workflows/ci.yml`; `.github/workflows/drivers-ci.yml` | Workstream/release smoke and gate execution surfaces. | Release and tracker evidence. |

## User-Visible Behaviors

| Behavior | Evidence Source | Actor / Trigger | Observable Outcome | Supported Use Case |
|----------|-----------------|-----------------|--------------------|--------------------|
| Execute, analyze, and route SQL across transactional and analytical surfaces. | `services/voltnuerongridd/src/router.rs`; `tests/kpi/scripts/run-ws3-query-routing-smoke.ps1`; design doc | User, driver, or Studio submits SQL. | SQL execution/analysis/route and OLAP/HTAP surfaces exist. | HTAP query operation. |
| Manage databases, metadata, grants, users, sessions, cluster nodes, locks, and transactions. | `README.md`; `services/voltnuerongridd/src/router.rs` | Admin/operator action. | Administrative lifecycle and operations surfaces are visible. | Database administration. |
| Use Studio for connection, schema, query, and dashboard workflows. | `ui/voltnuerongrid-studio/README.md`; `ui/voltnuerongrid-studio/tests/e2e/*.spec.ts`; active user selection dated 2026-06-23 | Studio user creates/selects connection/database. | UI exists, but selected issue reports connection/database lifecycle ambiguity. | Studio management. |
| Load sample/default database resources. | `samples/database/README.md` | User runs ordered sample SQL files. | Sample schemas, tables, views, functions, triggers, AI, plugin, and HTAP demo resources are documented. | Default/sample bootstrap. |
| Validate workstream/release claims. | `tests/kpi/scripts/*.ps1`; constitution | Developer/release workflow. | Gate scripts emit status artifacts. | Evidence-backed delivery. |

## System Boundaries

| Boundary | Evidence Source | Inbound Interaction | Outbound Interaction | Not Proven |
|----------|-----------------|---------------------|----------------------|------------|
| Runtime service | `services/voltnuerongridd/src/router.rs`; `Cargo.toml` | HTTP/native requests from drivers, Studio, MCP, operators. | Database, storage, SQL, auth, audit, failover, Raft, ingest, plugin, autonomous capabilities. | Multi-process production topology. |
| Database/catalog | Router database routes; constitution; active user selection | Admin lifecycle, grants, schema tree, SQL. | Metadata, schema tree, resources, role grants. | Correct Studio behavior for nonexistent DB, empty/default bootstrap, and resource provenance. |
| Driver | Driver contract and READMEs | Language-native configuration and request creation. | Runtime HTTP/native calls and normalized errors. | GA parity for all listed languages. |
| Studio | Studio README/package/tests | Human connection and DB actions. | Runtime contracts and desktop/web shell. | Native protocol testability and DB bootstrap semantics. |
| Security | Security checklist; constitution | Credentials, TLS/native tokens, tenant/operator/admin identities. | RBAC decisions, redacted logs, audit events. | Runtime parameterized query enforcement and persistent admin audit sink are incomplete per checklist. |
| Deployment | Local/cloud READMEs; Helm assets | Local scripts/manifests/env/config. | Runtime process, local volumes, draft cloud templates. | Cloud deployment is explicitly deferred/not tested. |

## Data and State Clues

| Fact / Entity | Evidence Source | Observed Lifecycle Clue | Fact Source | Not Proven |
|---------------|-----------------|-------------------------|-------------|------------|
| Database resources include schemas, tables, views, functions, triggers, AI logs, plugin registry, and audit schema in sample data. | `samples/database/README.md` | Ordered SQL files create a sample database resource set. | Sample docs | Studio creates exactly empty or sample-backed DBs. |
| Runtime config includes storage engine selector, data directory, WAL fsync, SQL engine selector, OLAP threshold, and max result rows. | `vng.config.sample.json` | Config can select RocksDB/default and DataFusion/default with future VNG selectors. | Sample config | Full selector behavior enforcement. |
| WAL/checkpoint/replay/compaction/status route groups exist. | `services/voltnuerongridd/src/router.rs` | Durability/recovery surfaces are externally visible. | Router declarations | Latest-transaction crash recovery across all DB resources. |
| HTAP apply/export/sync/status/stats route groups exist. | `services/voltnuerongridd/src/router.rs`; WS3 scripts | OLTP-to-OLAP sync/status surfaces are visible. | Router and scripts | Complete automatic OLTP/OLAP routing for all query shapes. |
| Auth and tenant isolation are documented as baseline complete. | `security-compliance-checklist-v1.md` | RBAC and tenant isolation evidence is referenced. | Security checklist | Per-database role enforcement for all lifecycle paths. |

## Runtime and Process Clues

| Runtime Fact | Evidence Source | Trigger / Handoff | Failure or Retry Clue | Not Proven |
|--------------|-----------------|-------------------|-----------------------|------------|
| Admin cluster topology, transaction control, lock control, and node management are described and routed. | `README.md`; `router.rs` | Admin/MCP request to runtime control plane. | README states current node add/remove operates on a single-process scaffold. | Multi-process orchestration without data loss. |
| Failover/Raft/snapshot/heartbeat/member/leader/fencing routes and WS6 scripts exist. | `router.rs`; `tests/kpi/scripts/run-ws6-*.ps1` | Cluster probes and events to consensus/failover surfaces. | Chaos/failover scripts exist. | Production RTO/RPO values. |
| Driver retry and timeout behavior is part of the driver contract. | `driver-core-contract-v1.md`; Rust driver README | Client request can apply timeout/retry strategy. | Contract includes timeout/cancellation behavior. | Cross-language conformance for all languages. |
| Gate scripts close workstreams and release packs. | `tests/kpi/scripts/*.ps1`; testing instructions; constitution | Developer/release action invokes scripts and reads JSON artifacts. | Artifact status is authoritative. | Static script presence does not prove latest run status. |

## Development Structure Clues

| Module / Package Area | Evidence Source | Observed Responsibility | Dependency Clue | Boundary Risk |
|-----------------------|-----------------|--------------------------|-----------------|---------------|
| Runtime service | `Cargo.toml`; `router.rs` | Aggregates API routing and handler modules. | Composes workspace capabilities. | Large route surface can blur ownership. |
| Database crates | `Cargo.toml`; constitution | Own SQL, storage, ingest, auth, audit, failover, MCP, plugin, AI capabilities. | Workspace naming and trait-boundary rules are explicit. | Partial/stub crates can create false completion signals. |
| Studio package | Studio package/README/tests | Human management UI. | Depends on runtime API contracts and Tauri shell. | Mocked or ambiguous state can diverge from runtime truth. |
| Driver packages | Driver READMEs and contract | Language-specific connectivity. | Shared contract constrains config/headers/errors/retries. | Driver breadth is a critical traceability gap. |
| Deployment packages | Local/cloud/Helm docs | Environment packaging and profiles. | Runtime env/config drives deployment behavior. | Cloud assets are draft/not tested. |
| Evidence/test packages | KPI scripts; workflows | Validate workstreams, release gates, drivers, smoke tests, CI. | Workstream naming maps to release closure. | Tracker text can diverge from artifacts. |

## Repository-First Projection

### Build Manifest Detection

| Ecosystem | Manifest Evidence | Detection Status | Runtime Surface Notes |
|-----------|-------------------|------------------|-----------------------|
| Rust | `Cargo.toml`; Studio Tauri cargo file | Detected | Core runtime, crates, tools, Rust driver, Tauri shell. |
| Node/TypeScript | Studio package file; driver READMEs | Detected | Studio, contracts, Playwright, TS/Node driver surfaces. |
| PowerShell gates | `tests/kpi/scripts/*.ps1` | Detected | Workstream/release validation. |
| Repository-first artifacts | `.specify/memory/repository-first/**` search | Absent | No dependency matrix or invocation spec exists. |

### First-Party Module Edges

| From Module | To Module | Evidence Source | Observed Direction | Architecture Boundary Meaning |
|-------------|-----------|-----------------|--------------------|-------------------------------|
| Clients/tools | Runtime service | README; driver contract; Studio README | Drivers/Studio/MCP call runtime surfaces. | Runtime coordinates user-visible DB behavior. |
| Runtime service | Workspace capabilities | Cargo manifest; router | Service exposes capabilities owned by crates/handlers. | Capabilities should remain separable. |
| Gate scripts | Runtime and artifacts | KPI scripts; constitution | Scripts exercise runtime and emit evidence. | Evidence controls completion claims. |

### Module Invocation Governance

| Rule Source | Allowed Direction | Forbidden Direction | Architecture Constraint | Risk If Violated |
|-------------|-------------------|---------------------|-------------------------|------------------|
| Constitution | Interfaces call runtime under auth and contracts. | UI/driver bypass of RBAC, tenant isolation, or audit. | Native interfaces are product surface. | Security and behavior divergence. |
| Constitution; Cargo manifest | Runtime composes focused crates. | Monolithic duplicate persistence/auth/driver/plugin logic. | Rust-first modular reuse. | Maintainability drift. |
| Security checklist | Protected flows pass through RBAC/redaction/TLS/tenant checks. | Secrets in logs/plaintext or cross-tenant access. | Security before domain work. | Enterprise trust failure. |

### Dependency Governance Signals

| Signal Source | Dependency / Concern | Signal Type | Affected Boundary | Architecture Review Trigger |
|---------------|----------------------|-------------|-------------------|-----------------------------|
| `vng.config.sample.json` | RocksDB/DataFusion selectors and future VNG selectors | Implementation selection | Storage and SQL | Selector changes require architecture review. |
| Driver core contract | Cross-language driver contract | Compatibility | Driver/runtime | Breaking headers/errors/transports require review. |
| Cloud README | Draft/not-tested cloud templates | Maturity | Physical deployment | Cloud-ready claims require evidence. |
| Traceability matrix | R-10/R-12/R-17 gaps | Gap | Scale, triggers/events, drivers | Completing these requires benchmark/trigger/driver evidence. |

## Physical / Deployment Clues

| Deployment Fact | Evidence Source | Environment / External System | Operational Constraint | Not Proven |
|-----------------|-----------------|-------------------------------|------------------------|------------|
| Local guide supports release build, local HTTP, optional native listener, TLS native listener, health check, extension install, logs, and cloud deferral. | `deploy/local/README.md` | Local/on-prem | Native listener disabled unless configured; TLS uses env paths. | Full multi-node local proof. |
| Cloud deployment is deferred and assets are draft/not tested. | `deploy/cloud/README.md`; `deploy/cloud/**` | AWS, Azure, GCP, Kubernetes/Helm | Cloud claims remain future/deferred. | Production cloud topology. |
| Helm/local files exist. | `deploy/helm/voltnuerongrid/**`; `deploy/local/*.yml` | Kubernetes/local profiles | Packaging assets exist. | Production readiness. |
| Runtime config controls storage, SQL, data dir, WAL fsync, OLAP threshold, max result rows. | `vng.config.sample.json` | Runtime configuration | File/env-driven config. | Full config schema completeness. |

## Git History Signals

| Signal | Evidence Source | Architecture Meaning | Confidence | Review Trigger |
|--------|-----------------|----------------------|------------|----------------|
| Recent commits include Speckit constitution, integration/CORS/SRE/TLS fixes, dynamic create/drop database operations, and schema-tree scoping to active DB. | `git --no-pager log --oneline -8` | Current change axis includes governance, TLS, runtime integration, and database lifecycle isolation. | Medium | Review DB connection/catalog isolation. |
| Recent commit mentions fixing six issues from `vng-issues-1.md`. | `git --no-pager log --oneline -8` | UI/runtime issue closure is active but not independently proven. | Low | Validate issue closure with tests/artifacts. |

## Evidence Gaps

| Gap | Affected View | Why It Blocks Architecture Conclusion |
|-----|---------------|----------------------------------------|
| Studio connection semantics for nonexistent database, empty/default bootstrap, and displayed resource provenance are not proven and are reported broken in active user selection. | Scenario, Logical, Process, Development | Cannot conclude connection/database lifecycle coherence. |
| Per-database roles and connection isolation for all DB-management paths need explicit evidence beyond general RBAC docs. | Scenario, Logical, Process | Cannot conclude full database-level authorization. |
| Latest-transaction durability and crash recovery require gate evidence, not only WAL routes/config. | Logical, Process, Physical | Cannot conclude zero data loss on crash. |
| Complete automatic OLTP/OLAP routing for all query shapes is not proven. | Scenario, Process | Cannot conclude HTAP routing intelligence is complete. |
| Cloud deployment is deferred/draft. | Physical | Cannot conclude production SaaS/cloud topology. |
| Trillion-row scale and extreme latency are not proven in traceability. | Physical, Process | Cannot promote scale claims to architecture conclusions. |
| Repository-first dependency artifacts are absent. | Development | Cannot derive detailed first-party dependency governance from repository-first projections. |
