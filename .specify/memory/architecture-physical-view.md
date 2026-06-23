# Physical View

**Input**: `.specify/memory/architecture-process-view.md`, `.specify/memory/architecture-development-view.md`

**Purpose**: Derive deployment, hosting, external system, fact-source, observability, and operational boundaries from process and development views.

## Architecture Intent

The physical architecture must distinguish proven local runtime operation from draft cloud/SaaS ambitions, while preserving configuration, security, storage, native listener, and evidence boundaries across environments.

## Core Tensions

| Tension | Current Tradeoff Direction | Physical Consequence |
|---------|----------------------------|----------------------|
| Local RC support vs. cloud ambition | Local/on-prem single-node is supported; cloud is draft/deferred. | Physical view must not claim production cloud readiness. |
| HTTP/browser access vs. native protocol | Both are deployment surfaces with different host capabilities. | Native validation requires native listener/client or desktop bridge. |
| Durable storage claims vs. configured storage | Configured storage/WAL settings are observable; zero-loss needs evidence. | Storage configuration is necessary but not sufficient proof. |
| Multi-node/failover ambition vs. scaffold evidence | Runtime exposes cluster/failover controls; production topology not proven. | Multi-node operational claims remain review triggers. |

## Stable Boundaries

| Boundary | Must Remain Stable Because | Explicitly Does Not Carry |
|----------|----------------------------|---------------------------|
| Local runtime host | It is the proven installation/development path. | Production cloud/SaaS guarantees. |
| Native listener host capability | Native protocol depends on runtime/client capabilities beyond browser fetch. | Browser-only connection validation. |
| Data/config boundary | Storage, SQL engine, data directory, WAL fsync, TLS, and log settings are environment-controlled. | UI schema truth. |
| Evidence artifact boundary | Gate files and CI outputs are physical fact sources for completion. | Architecture claims without execution. |

## Change Axes

| Expected Change | Isolated By | Physical Impact |
|-----------------|-------------|-----------------|
| Cloud profile hardening | Deployment profile boundary | Moves AWS/Azure/GCP/Helm from draft to evidence-backed hosting units. |
| Native desktop validation | Native listener and client boundary | Clarifies Studio desktop vs. browser behavior. |
| Durable recovery hardening | Data/config and evidence boundaries | Adds recovery artifacts for latest-transaction claims. |
| Multi-node failover maturity | Runtime cluster and deployment boundaries | Adds topology and RTO/RPO evidence. |

## Invariants

| Invariant | Source Deployment / External / Fact Boundary | Risk If Violated |
|-----------|----------------------------------------------|------------------|
| Production-facing security settings must not rely on plaintext secrets or missing TLS decisions. | Security checklist; constitution; local guide | Credential exposure and insecure deployment. |
| Data directory and WAL configuration are part of the runtime deployment contract. | Sample runtime config; local guide | Persistence behavior becomes environment-dependent and opaque. |
| Cloud assets remain draft until tested. | Cloud README | False SaaS readiness. |
| Evidence artifacts remain observable and current for release claims. | Gate scripts and workflows | Unverifiable release status. |

## Non-goals / Anti-patterns

| Non-goal / Anti-pattern | Why It Is Out of Scope or Harmful |
|-------------------------|-----------------------------------|
| Describing exact cloud infrastructure as production-ready | Repo evidence explicitly says cloud is deferred/draft. |
| Treating browser inability to test native as removal of native protocol | Native is documented as optional listener and driver surface. |
| Assuming WAL route presence proves crash recovery | Physical recovery needs executed evidence. |
| Using sample data resources without bootstrap provenance | It confuses physical sample seed source with user database state. |

## Deployment and Hosting Boundaries

| Runtime / Hosting Unit | Carries | Boundary | Depends On | Release / Migration Impact |
|------------------------|---------|----------|------------|----------------------------|
| Local runtime process | HTTP API, optional native listener, config/env-driven storage and logging | Local/on-prem execution | Rust build, runtime config, data directory, optional TLS/native settings | Current local baseline; migrations must preserve config compatibility. |
| Studio desktop/web host | Human database-management interface | Client host and desktop/browser capability | Runtime contract, UI package, optional desktop shell | Must distinguish HTTP browser paths from native-capable desktop paths. |
| Driver host application | Programmatic client connectivity | Application process boundary | Runtime endpoint, credentials, driver contract | Driver changes require conformance and compatibility review. |
| Local/development deployment profiles | Repeatable local execution profile | Developer/operator environment | Runtime config and local volumes | Useful for local testing, not proof of cloud production. |
| Draft cloud/Kubernetes profiles | Future cloud/SaaS deployment | Provider/profile boundary | Runtime image/config, provider storage/network/security assumptions | Cannot be release-ready until smoke/load/failover evidence exists. |
| Evidence execution environment | CI, scripts, local live server, result artifacts | Validation boundary | Runtime/client/deploy availability | Status claims follow artifact freshness. |

## External System Collaboration

| External System | Purpose | Exchanged Content | Authoritative Fact | Failure Impact | Isolation / Substitute Boundary |
|-----------------|---------|-------------------|--------------------|----------------|---------------------------------|
| Cloud object/storage providers | Planned connector ingestion and future cold/cloud storage profiles | Files, streams, object data, credentials | Connector/plugin policy and deployment profile | Ingest or storage profile unavailable | Connector plugin boundary. |
| Enterprise source protocols | Planned import/export connector ecosystem | Source/sink records and checkpoints | Connector capability and audit evidence | Ingest/export degradation | Connector plugin boundary. |
| KMS/TLS infrastructure | Security, encryption, and transport protection | Certificates, keys, key status | Security runtime and deployment config | Security operations blocked/degraded | Security boundary. |
| IDE environments | Human/operator management surfaces | Commands, credentials, connection settings, audit-safe output | IDE extension/client contract | Management UX unavailable | Runtime API and Studio can substitute. |
| CI/gate runner | Validation and release evidence | Test outputs and artifacts | Gate artifact status | Cannot prove completion | Manual/local gate run may substitute if artifact path is captured. |

## Fact Sources and Observability

| Fact / Event | Authoritative Source | Observable Location | Consumers | Traceability Requirement |
|--------------|----------------------|---------------------|-----------|--------------------------|
| Runtime health and metrics | Runtime service | Local runtime probes and metrics surface | Operators, gates, Studio | Must be current for live-smoke claims. |
| Database/schema/catalog state | Runtime/catalog boundary | Admin/schema/database surfaces and persisted state | Studio, drivers, users | Must be scoped to selected database. |
| WAL/recovery status | Durable state boundary | Runtime durability/status surfaces and gate artifacts | Operators, release team | Must be tied to recovery evidence for zero-loss claims. |
| HTAP route/freshness status | Query routing boundary | Query route/status and HTAP status surfaces | Users, drivers, Studio | Must accompany automatic routing claims. |
| Security posture | Security boundary | TLS/KMS/audit/security checks and checklist artifacts | Operators, release team | Must identify incomplete security items. |
| Workstream completion | Evidence boundary | Gate result artifacts and CI outputs | Trackers, release reviews | Must reconcile with tracker text. |

## Operations and Release Boundaries

| Operational Concern | Responsible Boundary | Trigger | Affected Views | Architecture Consequence |
|---------------------|----------------------|---------|----------------|--------------------------|
| Local service startup | Local runtime host | Developer/operator starts service | Scenario, Process, Physical | Local behavior is the supported baseline. |
| Connection/database provisioning | Runtime plus Studio/client contract | User creates/selects database | Scenario, Logical, Process | Must be explicit and scoped. |
| Security and credential operation | Security boundary | Protected action or deployment config | All views | Blocks unsafe operations. |
| Failover/cluster operation | Runtime cluster boundary | Node/leader/failure event | Scenario, Process, Physical | Needs evidence before production HA claims. |
| Cloud release | Deployment profile boundary | Provider deployment readiness | Physical, Development | Deferred until profiles are tested. |
| Release readiness | Evidence boundary | Gate/test execution | All views | Artifact truth controls status. |

## Physical View Gaps

| Gap | Affected Deployment / External Boundary | Why It Matters |
|-----|-----------------------------------------|----------------|
| Cloud deployment is explicitly deferred and draft. | Draft cloud/Kubernetes profiles | Blocks production SaaS topology and cloud operational conclusions. |
| Native protocol validation path in Studio is unclear. | Studio host; Native listener | Blocks coherent physical client behavior for native connections. |
| Latest-transaction crash recovery requires executed evidence. | Data/config boundary | Blocks zero-data-loss physical claim. |
| Multi-node failover production topology is not proven. | Runtime cluster/deployment boundary | Blocks HA/RTO/RPO conclusions. |
| Extreme scale and latency claims are not proven. | Deployment and evidence boundaries | Blocks physical sizing/performance conclusions. |

## Prohibited Content

Do not write Kubernetes YAML, cloud resource manifests, machine sizes, service SKUs, deployment scripts, runbooks, or concrete infrastructure configuration here.
