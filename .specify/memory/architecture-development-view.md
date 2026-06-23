# Development View

**Input**: `.specify/memory/architecture-logical-view.md`, `.specify/memory/architecture-process-view.md`

**Purpose**: Derive architecture-level components, package boundary intent, contract/artifact semantics, and dependency rules.

## Architecture Intent

Development boundaries must keep runtime authority, database capabilities, client interfaces, deployment packaging, and evidence production independently evolvable while preserving common security and database semantics.

## Core Tensions

| Tension | Current Tradeoff Direction | Development Consequence |
|---------|----------------------------|-------------------------|
| Large runtime surface vs. modular ownership | Runtime composes focused capability packages. | New behavior should land in the owning boundary rather than duplicated clients. |
| Studio/driver convenience vs. contract consistency | Shared interface contracts govern clients. | UI and drivers must not invent separate database lifecycle semantics. |
| Planned cloud/tooling breadth vs. stable core | Core runtime and contracts are stable; draft deploy surfaces mature behind profiles. | Cloud templates do not force core semantics without evidence. |
| Gate evidence vs. implementation speed | Evidence artifacts are first-class outputs. | Tasks include validation and tracker reconciliation for changed claims. |

## Stable Boundaries

| Boundary | Must Remain Stable Because | Explicitly Must Not Own |
|----------|----------------------------|-------------------------|
| Runtime service component | Coordinates user-visible capabilities and applies runtime authority. | Language-specific client UX or cloud provider ownership. |
| Database capability packages | Own database semantics such as SQL, storage, ingest, auth, audit, failover, plugins, and autonomous governance. | Studio mock state or driver-only behavior. |
| Client interface packages | Own language/UI ergonomics and transport adaptation. | Database catalog truth or security decisions. |
| Deployment packages | Own environment profiles and runtime configuration presentation. | Database semantics. |
| Evidence packages | Own gate scripts, conformance fixtures, and artifact production. | Product behavior itself. |

## Change Axes

| Expected Change | Isolated By | Development Impact |
|-----------------|-------------|--------------------|
| Fix connection/database lifecycle | Interface contract plus runtime database lifecycle boundaries | Studio and runtime contract must align without duplicating catalog state. |
| Expand native drivers | Driver contract boundary | New language packages consume shared semantics. |
| Mature HTAP routing and durability | Database capability packages | Runtime surfaces remain stable while internals evolve. |
| Mature cloud deployment | Deployment package boundary | Provider profile changes avoid leaking into core semantics. |

## Invariants

| Invariant | Source Boundary / Contract / Dependency Rule | Risk If Violated |
|-----------|----------------------------------------------|------------------|
| Clients depend on runtime contracts for database truth. | Interface Contract and Database Lifecycle | UI/driver schema drift and phantom resources. |
| Capability packages expose focused responsibilities. | Modular Rust-first constitution; workspace manifest facts | Monolithic runtime behavior and duplicated logic. |
| Security rules apply across packages. | Security authority and constitution | Inconsistent RBAC or secret handling. |
| Evidence artifacts are produced by validation packages, not authored as claims. | Evidence boundary | Tracker/release false positives. |

## Non-goals / Anti-patterns

| Non-goal / Anti-pattern | Why It Is Out of Scope or Harmful |
|-------------------------|-----------------------------------|
| Encoding database lifecycle only in Studio state | It bypasses runtime authority and breaks other clients. |
| Treating HTTP-only driver contract as final native parity | Repo facts show native protocol is required and driver breadth is a gap. |
| Copying provider-specific deployment concerns into database capability packages | It couples core semantics to draft cloud assets. |
| Updating trackers without current gate artifacts | It violates evidence-boundary ownership. |

## Architecture-Level Components

| Component / Capability Package | Responsibility | Input / Output Boundary | Collaborators | Explicitly Must Not Own | Source View Evidence |
|--------------------------------|----------------|-------------------------|---------------|--------------------------|----------------------|
| Runtime Service | Coordinate external requests, route to database/admin/security/failover/audit/automation capabilities, expose observability. | Interface request to authorized runtime outcome. | Database capabilities, client interfaces, evidence packages. | Client UI state or provider-specific deployment. | Logical/process views: runtime authority and handoffs. |
| Database Core Capabilities | Own SQL, query routing, storage, metadata, ingest, plugins, auth, audit, failover, and autonomous policies. | Scoped database operation to durable/query/admin result. | Runtime Service, Evidence, Deployment. | Studio rendering and language-specific ergonomics. | Logical capability boundaries. |
| Client Interface Capabilities | Provide Studio, drivers, MCP/IDE, and operator UX over runtime contracts. | User/program request to normalized runtime interaction. | Runtime Service, Security, Evidence. | Catalog authority or durable storage. | Scenario/process views. |
| Deployment and Configuration Capabilities | Package local, Helm, and cloud profiles with runtime configuration and operational expectations. | Environment profile to runnable runtime boundary. | Runtime Service, Security, Evidence. | Database object semantics. | Physical facts and process view. |
| Evidence and Governance Capabilities | Produce and interpret gate/test artifacts, release checks, traceability, and architecture gaps. | Validation run to current status evidence. | All capability packages. | Runtime behavior itself. | Process view: evidence closure. |

## Package Boundary Intent

| Package / Boundary | Abstraction Level | Owned Concepts | May Depend On | Must Not Depend On | Evolution Rule |
|--------------------|-------------------|----------------|---------------|--------------------|----------------|
| Runtime Service | Application coordination | Routing, request coordination, runtime orchestration | Database core, security, audit, driver contracts | Studio-local schema truth | Add surfaces by delegating to owned capability boundaries. |
| Database Core | Domain capability | SQL, storage, metadata, ingest, failover, plugin, audit, AI/autonomous governance | Shared contracts and configuration | UI frameworks or provider manifests | Preserve database invariants before optimizing. |
| Drivers | Interface adaptation | Connection config, auth headers, retries, errors, native/HTTP transport | Runtime contract and conformance fixtures | Runtime internals or UI state | New languages conform to shared contract version. |
| Studio and IDE Tooling | Human operation interface | Connection UX, schema display, editor/workspace, dashboards | Runtime contract and desktop/web shell | Durable database state | UI state is cached/derived, not authoritative. |
| Deployment Profiles | Physical packaging | Local/cloud/Helm profile semantics, runtime env/config | Runtime config contract | Database catalog semantics | Draft profiles cannot be cited as production readiness. |
| Gates and Conformance | Evidence | Workstream, release, driver, UI, cloud, security validation artifacts | Runtime/client/deploy surfaces | Product claims without execution | Evidence changes must be current and traceable. |

## Contracts and Artifacts

| Contract / Artifact | Semantics | Producer | Consumer | Lifecycle | Architecture Consequence |
|---------------------|-----------|----------|----------|-----------|--------------------------|
| Runtime interface contract | Shared runtime behavior for SQL, admin, security, operational, and evidence surfaces. | Runtime Service | Drivers, Studio, MCP, gates | Evolves with compatibility review. | Keeps clients from inventing semantics. |
| Driver core contract | Cross-language config, auth, errors, retry, and transport expectations. | Architecture/DX boundary | Driver packages and conformance gates | Versioned contract. | Enables native interface parity. |
| Runtime configuration contract | Storage/SQL engine selector and operational knobs. | Configuration capability | Runtime and deployment profiles | Versioned by release/config changes. | Separates implementation choice from user contract. |
| Gate artifacts | Current proof for workstream/release claims. | Evidence packages | Trackers, release reviews, architecture gaps | Invalidated by relevant changes. | Prevents unproven claims. |
| Sample database scripts | Optional bootstrap source for demo/default resources. | Sample-data boundary | Runtime/Studio bootstrap scenarios | Ordered sample lifecycle. | Gives provenance to default database resources. |

## Dependency Rules

| Rule | Allowed Direction | Forbidden Direction | Reason | Risk If Violated |
|------|-------------------|---------------------|--------|------------------|
| Client-to-runtime authority | Clients call runtime contracts for database truth. | Clients creating hidden authoritative database state. | Database isolation must be shared across interfaces. | Phantom connections/resources. |
| Runtime-to-capability delegation | Runtime coordinates and delegates to database capability owners. | Capability logic duplicated in multiple unrelated surfaces. | Preserve SOLID/reuse boundaries. | Divergent behavior. |
| Security-before-domain | Interfaces and runtime invoke security authority before protected operations. | Protected domain mutation before RBAC/tenant check. | Enterprise trust and constitution. | Security incident. |
| Evidence-before-completion | Trackers and release status consume current gate artifacts. | Narrative completion without artifact proof. | Evidence-backed delivery. | False release readiness. |
| Deployment isolation | Deployment profiles configure runtime; core semantics remain provider-neutral. | Provider profile dictates database semantics. | Cloud is draft/deferred and must not destabilize core. | Portability loss. |

## Development View Gaps

| Gap | Affected Component / Boundary | Why It Matters |
|-----|-------------------------------|----------------|
| No repository-first dependency matrix or invocation spec exists. | Dependency governance | Detailed first-party dependency rules need a repository-first pass. |
| Studio/runtime connection contract needs proof and likely implementation alignment. | Client Interface; Runtime Service; Database Lifecycle | Directly affects reported issues. |
| Full driver parity is a critical gap in traceability. | Drivers | Native-first product surface is incomplete without conformance evidence. |
| Cloud profiles are draft/not tested. | Deployment Profiles | Cannot claim production SaaS deployment. |

## Prohibited Content

Do not write source file paths, concrete package trees, classes, functions, implementation tasks, framework-specific wiring, or code generation notes here.
