# Process View

**Input**: `.specify/memory/architecture-scenario-view.md`, `.specify/memory/architecture-logical-view.md`

**Purpose**: Derive runtime collaboration, handoffs, approvals, receipts, state advancement, and failure closure from scenario paths and logical boundaries.

## Architecture Intent

Runtime collaboration must ensure every user/tool request is authorized, scoped to a database, routed through the correct capability, and backed by receipts or evidence when it changes state or claims completion.

## Core Tensions

| Tension | Current Tradeoff Direction | Process Consequence |
|---------|----------------------------|---------------------|
| UI responsiveness vs. runtime authority | Runtime authorization/catalog state is authoritative. | Studio waits for database validation or creation before workspace activation. |
| Native protocol vs. web UI transport | Native validation uses native-capable client/bridge; browser HTTP validation remains distinct. | The process must not offer an untestable dead-end as success. |
| Single ingest vs. dual query paths | Ingested data advances durable state and analytical visibility through synchronization. | Query handoff requires freshness/status semantics. |
| Failure handling vs. optimistic state | Failures close through rejection, recovery, or evidence gap. | Connection/database failures must be visible and recoverable. |

## Stable Boundaries

| Boundary | Must Remain Stable Because | Explicitly Does Not Control |
|----------|----------------------------|-----------------------------|
| Connection validation handoff | Prevents phantom sessions. | Rendering of result panes. |
| Database bootstrap handoff | Prevents unexplained resources. | User-authored SQL content. |
| Authorization approval handoff | Protects tenant/database boundaries. | Query optimization details. |
| Evidence closure handoff | Keeps status reliable. | Runtime business logic. |

## Change Axes

| Expected Change | Isolated By | Process Impact |
|-----------------|-------------|----------------|
| Studio fixes for connection/database lifecycle | Connection validation and bootstrap handoffs | Adds prompt/select/create branch before active workspace. |
| Native protocol testability improvements | Interface validation handoff | Adds desktop/native bridge path or explicit separation from browser testing. |
| HTAP routing maturity | Query routing and sync handoff | Adds route/freshness branches and fallback/degradation handling. |
| Cloud deployment maturity | Operational deployment handoff | Adds production health, storage, and failover receipts. |

## Invariants

| Invariant | Source Scenario / Runtime Link | Risk If Violated |
|-----------|--------------------------------|------------------|
| Authorization runs before state mutation or protected read. | Admin operation and connection scenarios | Unauthorized state exposure or mutation. |
| Workspace activation follows database validation or creation. | Existing/new database scenarios | Empty connections and unexplained schema. |
| Native connection verification uses a capable runtime path. | Native interface scenario | False warning or false success. |
| Gate closure follows current artifact status. | Release validation scenario | Stale or false completion. |

## Non-goals / Anti-patterns

| Non-goal / Anti-pattern | Why It Is Out of Scope or Harmful |
|-------------------------|-----------------------------------|
| Optimistically creating a local UI connection before runtime validation | It breaks database authority and user trust. |
| Swallowing bootstrap failure and showing sample objects anyway | It hides provenance and recovery state. |
| Treating single-process failover scaffold as proven production failover | Repo facts explicitly limit that claim. |
| Running autonomous actions without audit receipts | It blocks enterprise review and rollback. |

## Main Runtime Links

| Runtime Link | Trigger | Source | Target | Transferred Content / Fact | Completion Condition |
|--------------|---------|--------|--------|----------------------------|----------------------|
| Connection validation | User/client attempts connection | Studio or driver | Runtime authority boundary | Endpoint, protocol, credentials, database intent | Authorized active session or explicit rejection/create prompt. |
| Database bootstrap | User chooses create empty or sample/default | Studio/admin surface | Database lifecycle boundary | Database name, bootstrap mode, actor privilege | Database exists with declared provenance. |
| SQL execution and routing | User/client submits SQL | Interface contract boundary | HTAP query capability | Query intent, database scope, actor scope | Result, route/freshness receipt, or safe error. |
| Durable mutation | Database-changing operation commits | Query/admin/ingest capability | Durable state capability | Committed mutation and metadata effect | Persisted/recoverable state for claimed scope. |
| Operational control | Admin/MCP/operator requests control action | Admin boundary | Cluster/failover/security/audit capability | Actor, action, target runtime scope | Authorized change/report plus audit/evidence. |
| Evidence closure | Developer/release workflow runs validation | Gate scripts/CI | Evidence boundary | Test/gate output and artifact status | Current artifact supports or rejects status claim. |

## Handoffs and Approvals

| Handoff / Approval | From | To | Meaning | Accepted Path | Rejected / Returned Path |
|--------------------|------|----|---------|---------------|--------------------------|
| Credential and role approval | Interface | Security authority | Actor may access database or operation. | Proceed to database/query/admin boundary. | Reject and expose no protected state. |
| Database existence approval | Connection/session | Database lifecycle | Target database exists or will be explicitly created. | Active scoped session or bootstrap flow. | Prompt/select/create branch or error. |
| Bootstrap approval | Studio/admin user | Database lifecycle | Empty or sample/default creation is intentional. | Database created with provenance. | Duplicate/invalid/unauthorized branch. |
| Autonomous action approval | Agent/operator | Governance boundary | High-impact action passes policy. | Plan/simulate/apply/audit path. | Denied action with receipt. |
| Release evidence approval | Gate artifacts | Tracker/release status | Claim can be promoted. | Status synchronized. | Gap remains open. |

## Receipts and User Participation

| Receipt / Participation Point | Sender | Receiver | Content | User Action | Architecture Consequence |
|-------------------------------|--------|----------|---------|-------------|--------------------------|
| Connection validation result | Runtime | Studio/client | Database scope and authorization result | Continue, correct credentials, or create/select database | Prevents phantom connection. |
| Bootstrap choice | Studio | User | Empty vs. sample/default database choice | Select mode | Makes resource provenance explicit. |
| Query route/freshness status | Runtime | User/client | Transactional/analytical/hybrid meaning and freshness | Accept result or inspect status | Preserves HTAP trust. |
| Audit/evidence record | Runtime/gate | Operator/release team | Action or validation outcome | Review or reconcile tracker | Supports governance and release confidence. |

## Failure, Degradation, and Closure

| Failure / Branch | Detection Boundary | Responsible Boundary | Degradation or Compensation | User-Visible Result | Closure Condition |
|------------------|--------------------|----------------------|-----------------------------|---------------------|-------------------|
| Database name does not exist during connection | Database lifecycle | Studio/runtime contract | Offer create empty/sample or select existing database. | Connection not active until resolved. | User creates/selects valid database or cancels. |
| Native protocol cannot be validated from current client | Interface contract | Studio/driver boundary | Use native-capable bridge/client or explain unsupported mode. | No false success or dead-end field. | Native validation path exists or UI scopes option correctly. |
| Unauthorized database or admin action | Security authority | Authorization boundary | Reject without protected mutation. | Credential/privilege error. | Correct credentials/role or no operation. |
| OLAP freshness lag | HTAP query capability | Query routing and sync boundary | Report freshness or route to transactional path where required. | Result includes trustable semantics or safe error. | Freshness restored or route changed. |
| Crash/restart recovery gap | Durable state capability | Storage/recovery boundary | Recovery process and evidence required. | Availability/recovery status visible. | Gate proves recovery for scope. |
| Gate failure/stale artifact | Evidence boundary | Release/development team | Keep tracker open and record gap. | Work not complete. | Current passing artifact or documented deferral. |

## Process Gaps

| Gap | Affected Runtime Link / Scenario | Why It Matters |
|-----|----------------------------------|----------------|
| Studio connection flow needs explicit validation/create/select process evidence. | Connection validation; database bootstrap | Directly affects the user's selected issues. |
| Native protocol browser/desktop validation process is unresolved. | Interface validation | Native is first-class but browser testing alone is insufficient. |
| End-to-end latest-transaction crash recovery evidence is missing. | Durable mutation | Blocks zero-data-loss conclusion. |
| Complete HTAP route/freshness behavior needs broader proof. | SQL execution and routing | Blocks ingest-once/query-intelligently conclusion. |
| Production cloud failover process is deferred. | Operational control | Blocks physical SaaS readiness. |

## Prohibited Content

Do not write call stacks, queue names, retry counts, thread/process details, endpoint sequences, workflow engine configuration, or orchestration code here.
