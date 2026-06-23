# Architecture Synthesis: VoltNueronGrid DB

**Input Views**:
- Scenario: `.specify/memory/architecture-scenario-view.md`
- Logical: `.specify/memory/architecture-logical-view.md`
- Process: `.specify/memory/architecture-process-view.md`
- Development: `.specify/memory/architecture-development-view.md`
- Physical: `.specify/memory/architecture-physical-view.md`

**Note**: This synthesis normalizes the 4+1 design results into the project architecture SSOT.

## View Index

| View | File | Purpose | Current Status |
|------|------|---------|----------------|
| Scenario | `.specify/memory/architecture-scenario-view.md` | UC-producing actor, use case, path, branch, and acceptance semantics | Generated project SSOT |
| Logical | `.specify/memory/architecture-logical-view.md` | Capability boundaries, domain objects, states, and invariants | Generated from scenario view |
| Process | `.specify/memory/architecture-process-view.md` | Runtime links, handoffs, approvals, receipts, failure closure | Generated from scenario and logical views |
| Development | `.specify/memory/architecture-development-view.md` | Architecture-level components, package boundaries, contracts, dependencies | Generated from logical and process views |
| Physical | `.specify/memory/architecture-physical-view.md` | Deployment, external systems, fact sources, observability, operations | Generated from process and development views |

## Architecture Intent

VoltNueronGrid DB is a Rust-first HTAP database platform whose architecture must keep database state durable and isolated while exposing consistent behavior through runtime APIs, native/HTTP drivers, Studio, MCP/admin tooling, autonomous operations, and evidence-producing gates. The stable architecture shape is a runtime-authoritative database core with separate client, deployment, and evidence boundaries.

## Central Design Forces

The central force is the difference between a database product and a demo UI. A valid connection must resolve to an authorized database; database resources must come from the selected database or an explicit sample/default bootstrap; and all clients must obey the same runtime authority. Performance, native protocol, autonomous operations, cloud deployment, and scale goals are important, but they become architecture conclusions only when evidence exists.

## Primary Tradeoffs

| Tradeoff | Chosen Direction | Consequence | Revisit When |
|----------|------------------|-------------|--------------|
| UI-first connection convenience vs. database validity | Database validity wins. | Studio must prompt/create/select instead of opening phantom connections. | Studio/runtime lifecycle evidence is complete. |
| HTTP/browser simplicity vs. native protocol requirement | Preserve native as first-class and separate browser limitations. | Native validation needs a native-capable path or desktop bridge. | Native Studio/driver conformance is proven. |
| Planned cloud/SaaS ambition vs. proven deployment | Local/on-prem baseline is proven; cloud stays draft/deferred. | Physical view does not invent production cloud topology. | Cloud smoke/load/failover artifacts exist. |
| Performance claims vs. correctness/evidence | Correctness and evidence win. | Scale/latency claims remain gaps until benchmarked. | Reproducible benchmark artifacts support claims. |

## Stable Boundaries

| Boundary | Affected Views | Must Remain Stable Because | Forbidden Crossing |
|----------|----------------|----------------------------|--------------------|
| Database lifecycle | Scenario, Logical, Process, Development | Owns existence, isolation, roles, resources, persistence, and bootstrap provenance. | Connection/UI state cannot create hidden database truth. |
| Runtime authority | Scenario, Process, Development, Physical | Coordinates SQL, admin, security, storage, failover, audit, automation, and observability. | Clients cannot bypass runtime authorization/catalog decisions. |
| Security and governance | Scenario, Logical, Process, Physical | Protects admin/operator/tenant/database boundaries and autonomous/plugin actions. | Domain mutation before privilege/policy decision. |
| Interface contract | Scenario, Logical, Development, Physical | Keeps drivers, Studio, MCP, and runtime behavior coherent. | Native/HTTP/UI surfaces diverging on database validity or errors. |
| Evidence boundary | Scenario, Process, Development, Physical | Determines completion, confidence, and release status. | Tracker or architecture claim without current artifact proof. |

## Change Axes

| Expected Change | Isolated By | Affected Views | Architecture Consequence |
|-----------------|-------------|----------------|--------------------------|
| Connection/database lifecycle fixes | Database lifecycle and interface contract | Scenario, Logical, Process, Development | Resolves phantom connection and resource provenance gaps. |
| HTAP routing/durability hardening | Query and durable-state capabilities | Scenario, Logical, Process, Physical | Moves ingest-once/query-intelligently and recovery claims from gaps to conclusions. |
| Native driver and Studio maturity | Interface contract boundary | Scenario, Development, Physical | Makes native protocol coherent across programmatic and UI surfaces. |
| Cloud/SaaS maturity | Deployment profile boundary | Physical, Process, Development | Converts draft cloud assets into deployable topology only with evidence. |
| Autonomous/plugin expansion | Governance boundary | Scenario, Logical, Process, Development | Adds capabilities without bypassing RBAC, policy, resource limits, or audit. |

## Anti-patterns

| Anti-pattern | Why It Violates Intent | Affected Views |
|--------------|------------------------|----------------|
| Creating an active empty connection before database validation | A connection is not a database and cannot be valid without scope. | Scenario, Logical, Process |
| Displaying tables/views/triggers without explicit selected database or bootstrap provenance | Breaks database isolation and user trust. | Scenario, Logical |
| Treating native protocol as a browser-test field only | Confuses physical transport capability with web UI limitations. | Scenario, Process, Physical |
| Claiming cloud, zero-loss, or trillion-row readiness from docs/routes alone | Architecture conclusions require evidence. | Scenario, Physical, Process |
| Letting clients duplicate catalog/security behavior | Divergent client behavior undermines runtime authority. | Development, Logical |

## Cross-View Architecture Model

| Architecture Concept | Scenario Meaning | Logical Interpretation | Runtime Role | Development Boundary | Physical Constraint | Architecture Constraint |
|----------------------|------------------|------------------------|--------------|----------------------|---------------------|---------------------------|
| Database | User-selected or explicitly created working scope. | Isolated persistence and metadata namespace. | Validated before active sessions and protected operations. | Database core capability. | Backed by configured data/storage profile. | No phantom or cross-boundary resources. |
| Connection | User/client access path. | Scoped session to one database under credentials. | Validated and authorized before workspace/use. | Client interface plus runtime contract. | Native or HTTP capability differs by host. | Cannot be active without valid database scope. |
| Query Workload | User asks for SQL/HTAP result. | Transactional, analytical, or hybrid intent. | Routed under freshness and correctness semantics. | SQL/query capability. | Depends on runtime and storage availability. | Route decisions cannot weaken correctness. |
| Governed Action | Admin/agent requests high-impact operation. | Policy-constrained operation on database/runtime state. | Approved, applied, rejected, and audited. | Security/autonomous/plugin capability. | Requires secure config and observable audit. | No RBAC or audit bypass. |
| Evidence Artifact | Proof that behavior/status is current. | Status fact with lifecycle. | Produced by validation execution. | Gate/conformance boundary. | Requires CI/local runtime environment. | Completion follows artifact truth. |

## Key Architecture Conclusions

| Conclusion | Affected Views | Boundary/Owner | Consequence |
|------------|----------------|----------------|-------------|
| Runtime authority must be the source of database, schema, auth, and operational truth. | All views | Runtime service and database core | UI/drivers must derive state from runtime contracts. |
| Connection and database lifecycle need explicit architecture treatment. | Scenario, Logical, Process, Development | Database lifecycle and interface contract | The selected user issues are architecture-significant, not cosmetic. |
| Native protocol remains first-class but requires host-aware validation. | Scenario, Process, Physical | Interface and native listener boundaries | Browser-only warnings are not enough for a native product surface. |
| Cloud and extreme-scale claims are open architecture gaps. | Physical, Process | Deployment and evidence boundaries | Do not assert production SaaS/trillion-row readiness until evidence exists. |
| Evidence artifacts control architecture confidence. | All views | Evidence boundary | Trackers and release statements must stay synchronized with gates. |

## Cross-Cutting Constraints

| Constraint | Source | Affected Views | Scope | Architecture Consequence |
|------------|--------|----------------|-------|--------------------------|
| Durable HTAP correctness first | Constitution; README; design docs | Scenario, Logical, Process, Physical | Database state, routing, storage | Correctness precedes performance and UX. |
| Admin/operator/tenant/database security | Constitution; security checklist | All views | Protected operations and interfaces | Auth/policy checks precede state work. |
| Modular Rust-first core and contract-driven clients | Workspace manifest; constitution; driver contract | Development, Logical | Package/component boundaries | Preserve focused ownership and compatibility. |
| Evidence-backed delivery | Constitution; KPI scripts; workflows | All views | Release/status claims | Artifact status is the proof boundary. |
| Cloud deferred until proven | Cloud README | Physical, Process | Deployment readiness | Keep cloud as draft/deferred architecture gap. |

## Open Risks and Review Triggers

| Risk or Trigger | Missing Evidence / Change Condition | Affected Views | Required Architecture Review |
|-----------------|-------------------------------------|----------------|------------------------------|
| Studio connection/database flow remains incoherent. | Need proof that nonexistent DB, empty DB, sample DB, existing DB, and native protocol paths behave consistently. | Scenario, Logical, Process, Development | Review database lifecycle and interface contract before UI fixes are marked complete. |
| Database-level roles and isolation incomplete. | Need end-to-end evidence across runtime, Studio, and drivers. | Scenario, Logical, Process | Security/RBAC architecture review. |
| Crash recovery to latest transaction unproven. | Need durability/recovery gate evidence for full database resources. | Logical, Process, Physical | Storage/recovery architecture review. |
| HTAP automatic routing incomplete. | Need route/freshness evidence across representative query shapes. | Scenario, Logical, Process | Query architecture review. |
| Native driver breadth incomplete. | Traceability matrix marks R-17 critical gap. | Scenario, Development, Physical | Driver contract and conformance review. |
| Cloud production readiness asserted. | Cloud README says deferred/draft. | Physical | Deployment architecture review with evidence. |
