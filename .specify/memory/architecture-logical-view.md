# Logical View

**Input**: `.specify/memory/architecture-scenario-view.md`

**Purpose**: Derive capability boundaries, domain objects, states, relationships, and invariants from the scenario view.

## Architecture Intent

The logical architecture separates database identity, connection/session access, query execution, durable state, security authority, tooling contracts, autonomous governance, and evidence status so no surface can invent database state or bypass runtime authority.

## Core Tensions

| Tension | Current Tradeoff Direction | Logical Consequence |
|---------|----------------------------|---------------------|
| Connection convenience vs. database authority | Database existence and role authorization own validity. | Connection is not a database and cannot manufacture hidden resources. |
| Unified HTAP experience vs. separate execution concerns | Query intent is unified, transactional and analytical responsibilities remain distinct. | Routing must preserve freshness and durability invariants. |
| Many interfaces vs. one security model | Interfaces are separate; authorization semantics are shared. | Studio, drivers, MCP, and runtime admin use common authority boundaries. |
| Planned breadth vs. proven domain | Gaps stay explicit. | Scale, trigger/event breadth, and cloud maturity do not become logical facts without evidence. |

## Stable Boundaries

| Boundary | Must Remain Stable Because | Explicitly Does Not Own |
|----------|----------------------------|-------------------------|
| Database | Owns resources, roles, metadata, persistence, and isolation. | UI connection convenience or driver retry policy. |
| Connection/session | Grants scoped access to a selected database under credentials and limits. | Database creation unless explicitly requested and authorized. |
| Security authority | Owns credential validation, privilege decisions, tenant/database isolation, and secret handling. | Query planning and storage layout. |
| Query routing | Owns workload classification and freshness-aware route choice. | Storage durability and role grants. |
| Durable storage | Owns persisted state and recovery semantics. | UI schema display and autonomous policy. |
| Evidence status | Owns completion truth and confidence. | Runtime behavior itself. |

## Change Axes

| Expected Change | Isolated By | Logical Impact |
|-----------------|-------------|----------------|
| New database bootstrap modes | Database and connection lifecycle objects | Adds explicit states without weakening connection validity. |
| New driver languages | Interface contract object | Adds clients without changing database/security meaning. |
| New plugin/autonomous capabilities | Governed action object | Adds actions under policy and audit lifecycle. |
| Cloud deployment maturity | Deployment profile object | Adds physical hosting states without changing database semantics. |

## Invariants

| Invariant | Source Scenario / Object / State | Risk If Violated |
|-----------|----------------------------------|------------------|
| A connection references exactly one active database scope at a time. | Existing database connection scenario; Database and Connection objects | Cross-database access and resource confusion. |
| A database resource belongs to one database namespace and has explicit provenance. | New database bootstrap scenario; Database Resource object | Sample/default objects appear without explanation. |
| Security authorization precedes protected state changes. | Admin operations and autonomous action scenarios | Unauthorized mutation or tenant leakage. |
| Analytical results have a declared freshness relation to transactional data. | HTAP query scenario | Stale or inconsistent query answers. |
| Evidence state cannot be inferred from planned design alone. | Release validation scenario | False completion and unearned architecture confidence. |

## Non-goals / Anti-patterns

| Non-goal / Anti-pattern | Why It Is Out of Scope or Harmful |
|-------------------------|-----------------------------------|
| Treating connection records as database records | It breaks the database boundary and leads to phantom connections. |
| Treating Studio state as authoritative schema state | Runtime/catalog must own resource truth. |
| Treating route existence as capability completion | Logical capability requires behavior evidence. |
| Letting autonomous actions mutate resources outside policy | It violates governance and audit invariants. |

## Capability Boundaries

| Capability / Boundary | Responsibility | Input | Output | Explicitly Does Not Own | Scenario Source |
|-----------------------|----------------|-------|--------|--------------------------|-----------------|
| Database Lifecycle | Create, select, drop, isolate, bootstrap, and expose database-owned resources. | Authorized database intent and optional bootstrap choice. | Scoped database state and resource provenance. | UI rendering or driver transport. | Existing connection; new database bootstrap. |
| Connection and Session Access | Validate endpoint, protocol, credentials, selected database, and scoped session. | Connection profile and credentials. | Authorized active session or rejection. | Database object ownership. | Existing database connection. |
| Authorization and Governance | Decide privileges, tenant/database scope, policy approvals, and secret safety. | Credentials, actor, action, resource scope. | Allow/deny decision and audit obligation. | Query planning and storage layout. | Admin operation; autonomous action. |
| HTAP Query Capability | Classify query intent and coordinate transactional/analytical result semantics. | SQL/query request and data freshness constraints. | Result with route/freshness meaning. | WAL recovery and database role ownership. | HTAP query. |
| Durable State Capability | Preserve data, metadata, WAL/checkpoints, and recovery state. | Committed mutations and configuration. | Recoverable persisted state. | Client connection UX. | Existing/new database; HTAP query. |
| Interface Contract Capability | Maintain consistency across drivers, Studio, MCP, and runtime APIs. | User/tool request over supported interface. | Normalized behavior and errors. | Database catalog authority. | Programmatic access; Studio operation. |
| Evidence Capability | Capture proof for workstream, release, and architecture confidence. | Test/gate execution and artifacts. | Current status and unresolved gaps. | Runtime mutation. | Release validation. |

## Domain Objects and Relationships

| Object | Meaning | Owning Capability | Key Relationships | Fact Source | Invariants |
|--------|---------|-------------------|-------------------|-------------|------------|
| Database | Isolated persistence and metadata namespace. | Database Lifecycle | Contains resources, roles, metadata, and connections. | Scenario view | Unique namespace, explicit bootstrap/provenance, durable state. |
| Connection | Authorized access path to a database through a protocol. | Connection and Session Access | References endpoint, credentials, protocol, selected database, limits. | Scenario view | Cannot be valid without authorized database scope. |
| Role/Privilege | Authority relation between actor and database/resource. | Authorization and Governance | Grants or denies access to database actions. | Scenario view | Authorization precedes protected operations. |
| Query Workload | User intent requiring transactional, analytical, or hybrid execution. | HTAP Query Capability | Reads database state and route/freshness facts. | Scenario view | Routing cannot weaken correctness. |
| Persisted State | Durable data and metadata after committed work. | Durable State Capability | Backed by storage/recovery policy and recovered by runtime. | Scenario view | Acknowledged committed data must be recoverable for claimed scope. |
| Interface Contract | Shared semantics across client surfaces. | Interface Contract Capability | Binds drivers, Studio, MCP, and runtime behavior. | Scenario view | Interfaces cannot bypass auth or database rules. |
| Evidence Artifact | Proof of behavior or release status. | Evidence Capability | Supports tracker and architecture confidence. | Scenario view | Status follows artifact truth. |

## State and Lifecycle

| Object / Flow | State | Entered When | Exited When | Forbidden Transition | Responsible Boundary |
|---------------|-------|--------------|-------------|----------------------|----------------------|
| Database | Absent | No catalog/persisted database exists. | Authorized create succeeds. | Absent directly to connected workspace without create/select decision. | Database Lifecycle |
| Database | Empty | Created without sample/default resources. | Resources are created or database is dropped. | Empty shown with unexplained tables/views/triggers. | Database Lifecycle |
| Database | Sample-backed | Created with documented sample/default bootstrap. | User mutates resources or drops database. | Sample resources shown without explicit bootstrap source. | Database Lifecycle |
| Connection | Draft | User supplies endpoint/protocol/credentials/database intent. | Validation succeeds or fails. | Draft treated as active without database validation. | Connection and Session Access |
| Connection | Active | Runtime authorizes database scope. | User disconnects, auth expires, or DB is unavailable. | Active crosses into another DB scope silently. | Connection and Session Access |
| Query Workload | Routed | Runtime classifies transactional/analytical/hybrid intent. | Result is returned or safe error/degradation occurs. | Routed result without freshness semantics for analytical path. | HTAP Query Capability |
| Evidence Artifact | Current | Gate/test artifact supports claim. | Code, claim, or requirement changes. | Stale artifact used as current proof. | Evidence Capability |

## Logical Decisions

| Decision | Scope | Owner / Boundary | Affected Objects or Flows | Consequence |
|----------|-------|------------------|---------------------------|-------------|
| Database validity is separate from connection profile validity. | Studio, drivers, runtime | Database Lifecycle and Connection Access | Database, Connection | Fixes phantom connection and nonexistent DB scenarios. |
| Sample/default resources require explicit bootstrap provenance. | Studio and database creation | Database Lifecycle | Database Resource | Prevents unexplained schema display. |
| Native protocol remains outside pure browser testability unless bridged. | Studio and drivers | Interface Contract | Connection | Studio must provide a native-capable path or scope the option. |
| Evidence gaps remain architecture gaps until proven. | All views | Evidence Capability | Evidence Artifact | Prevents route/config/design docs from becoming unproven conclusions. |

## Logical Gaps

| Gap | Affected Capability / Object | Why It Matters |
|-----|------------------------------|----------------|
| Studio database bootstrap and connection validation semantics are not proven. | Database Lifecycle; Connection | Blocks coherent UI connection architecture. |
| Per-database roles for all lifecycle paths need stronger evidence. | Authorization; Database | Blocks full database isolation conclusion. |
| Latest-transaction crash recovery across all DB resources needs gate proof. | Durable State | Blocks zero-data-loss conclusion. |
| Full trigger/event and extreme-scale domains are open in traceability. | Database Resource; Query Workload | Blocks complete product domain closure. |

## Prohibited Content

Do not write classes, DTOs, database tables, fields, method names, endpoints, schemas, or implementation data structures here.
