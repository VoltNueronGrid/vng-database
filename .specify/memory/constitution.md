<!--
Sync Impact Report
Version change: N/A -> 1.0.0
Modified principles: Initial constitution creation from repository prompts, design docs, and engineering instructions
Added sections: Product and Architecture Constraints; Delivery Workflow and Quality Gates; Governance
Removed sections: None
Templates requiring updates:
- ✅ .specify/templates/plan-template.md
- ✅ .specify/templates/spec-template.md
- ✅ .specify/templates/tasks-template.md
- ✅ .specify/templates/commands/*.md (not present in root Speckit templates)
- ✅ .specify/extensions/*/commands/*.md (reviewed; no constitution conflicts requiring root updates)
Follow-up TODOs: None
-->
# VoltNueronGrid DB Constitution

## Core Principles

### I. Durable HTAP Correctness Is Non-Negotiable
VoltNueronGrid DB MUST protect persisted database state across restarts, crashes, and
node failures. Features that create, mutate, replicate, ingest, query, or compact data
MUST define the durable write path, recovery behavior, transaction boundaries, and HTAP
freshness semantics before implementation. OLTP and OLAP paths MUST preserve database
boundaries, object uniqueness, constraints, indexes, catalog metadata, and ACID behavior
for the supported scope. Any fast path that can lose acknowledged data, bypass catalog
integrity, or return stale data outside its declared freshness tier is not compliant.

Rationale: The product exists to replace in-memory MDAP execution with a persistent,
enterprise-grade HTAP database. Performance claims are valuable only when correctness and
recovery survive real failure modes.

### II. Security, RBAC, And Tenant Isolation Come First
Protected endpoints, drivers, MCP tools, UI actions, autonomous agents, and plugin flows
MUST enforce authorization before domain work. The required order is admin key,
operator identity, then tenant/user scoping where applicable. Missing credentials MUST
return 401, insufficient privilege MUST return 403, and tenant-facing reads or writes
MUST never cross tenant or database boundaries. Secrets, keys, KMS references, tokens,
and credentials MUST NOT be logged, hardcoded, returned in responses, or stored in
plaintext. TLS, encryption-at-rest, signed plugin manifests, and audit trails MUST be
included for production-facing security work.

Rationale: The system targets enterprise and SaaS operation where one bypass can expose
customer data or invalidate the trust model.

### III. Performance Claims Require Reproducible Evidence
Work that claims blazing ingestion, low-latency retrieval, high concurrency, failover,
autoscaling, trillion-row readiness, native-driver performance, or minimal memory usage
MUST include measurable targets and reproducible evidence. Plans MUST name benchmarks,
smoke tests, soak tests, or gate artifacts that prove the claim for the implemented
scope. Performance work MUST preserve durability, RBAC, tenant isolation, and recovery
semantics; a faster path that weakens those guarantees is rejected unless explicitly
classified as experimental and blocked from production profiles.

Rationale: The roadmap depends on exceptional performance, but the repository must avoid
unproven marketing claims and measure each increment honestly.

### IV. Modular Rust-First Architecture With Reuse
Core database behavior MUST be implemented in Rust-first, modular crates with explicit
trait contracts and clear ownership boundaries. New code MUST reuse existing crates,
helpers, services, and gate patterns before adding new abstractions. SOLID principles
apply to handlers, services, drivers, storage, execution, auth, audit, ingest, plugins,
MCP, UI integration, and test helpers. Crate folders use `voltnuerongrid-{name}` and
Rust imports use `voltnuerongrid_{name}`. Service tests MUST use the established
`state_with_key()` helper when constructing `AppState`.

Rationale: The product spans database internals, drivers, tools, UI, and cloud
operations; maintainability depends on stable contracts rather than monolithic growth.

### V. Native Interfaces And Tooling Are Product Surface
HTTP APIs, native protocol, MCP tools, Studio UI, IDE extensions, and language drivers
are first-class product surfaces. Features that affect database behavior MUST consider
the relevant client contracts, connection pooling, failover behavior, error response
shape, and compatibility impact. Native drivers for prioritized languages, especially
Rust, TypeScript/JavaScript, and Python, MUST not be replaced by thin API-only stories
when the requirement calls for native connectivity. UI and IDE workflows MUST use the
same authorization and validation rules as runtime endpoints.

Rationale: The prompts require a database platform, not only a server. User trust comes
from consistent behavior across drivers, tools, and operational surfaces.

### VI. Autonomous And Plugin Actions Must Be Governed
AI/autonomous operations, MCP administration, plugin loading, connector execution, UDFs,
and generated database-object actions MUST run under explicit policy, capability, and
audit controls. High-impact actions MUST use plan, simulate, apply, and record behavior
where feasible. Plugins and UDFs MUST declare permissions, resource limits, provenance,
and signing or trust status before execution. Autonomous agents MUST not bypass RBAC,
tenant isolation, human approval requirements, or production safety gates.

Rationale: Autonomous database operations are a differentiator only if operators can
understand, constrain, reverse, and audit what the system does.

### VII. Evidence-Backed Delivery And Tracker Truth
No requirement, workstream, sprint, release, or gap may be marked complete without current
evidence. Changed logic MUST have focused unit tests and integration coverage where the
behavior crosses module, endpoint, driver, storage, security, or user-facing boundaries.
Gate scripts MUST derive pass/fail from emitted JSON `status` fields, not stale shell
exit codes. Status trackers, sprint trackers, release summaries, and reference docs MUST
match artifact reality. Coverage for impacted modules SHOULD meet the repository's 90%
target where tooling supports it; any shortfall MUST be explained with follow-up work.

Rationale: This repository is managed through workstreams and gates. Reliable status is a
feature of the engineering system, not paperwork.

## Product and Architecture Constraints

VoltNueronGrid DB is a Rust workspace for a distributed HTAP database engine with a
service binary, modular crates, drivers, MCP integration, UI tooling, deployment assets,
and evidence-producing tests. New Speckit work MUST identify which surfaces it changes:
`crates/`, `services/voltnuerongridd/`, `drivers/`, `ui/`, `tests/`, `deploy/`, `docs/`,
or tracker files.

Durable storage work MUST state whether it uses the configured RocksDB-backed path, a
future VNG storage selector, WAL/checkpoint recovery, or a documented non-production
stub. SQL work MUST state whether it uses DataFusion/sqlparser-backed execution, native
VNG execution, or an explicitly unsupported selector. Multi-database work MUST maintain
separation between connections, databases, schemas, users, roles, tables, columns, views,
functions, triggers, events, and metadata schemas.

Security-sensitive work MUST follow the repository RBAC order and update security tests
or gates. Ingest, connector, plugin, UDF, cloud, and autonomous work MUST define secret
handling, audit events, resource limits, and failure behavior. Driver and UI work MUST
include native connection, pooling, pagination, failover, and authorization behavior for
the affected surface.

## Delivery Workflow and Quality Gates

Every feature specification MUST map user stories to prompt requirements, workstreams,
or tracker identifiers when they exist. Specs MUST include security, durability,
performance, compatibility, and observability requirements when the feature touches those
areas. Ambiguous production claims MUST be marked for clarification or scoped as
experimental.

Every implementation plan MUST complete a Constitution Check before design work and
again after design. The check MUST cover durable correctness, RBAC/tenant isolation,
performance evidence, modular reuse, native interfaces, autonomous/plugin governance,
and tracker/evidence updates. Any violation MUST be listed with a concrete reason, a
safer alternative considered, and an approval or follow-up path.

Every task list MUST include required tests for changed logic, security checks for
protected surfaces, artifact updates for gate-driven work, and documentation/tracker
updates when claims change. Live HTTP smoke packs require a running server and MUST emit
JSON artifacts under `tests/kpi/results/`. PowerShell gate scripts MUST use the `$packs`
pattern and derive status from artifact JSON.

## Governance

This constitution supersedes conflicting local practices for Speckit planning and
delivery in this repository. Repository-specific GitHub instructions, skills, and agent
files provide operational detail and are subordinate implementation guidance unless they
impose stricter security, testing, or evidence requirements.

Amendments require a pull request or documented change set that includes the rationale,
version bump type, updated Sync Impact Report, and review of dependent Speckit templates
and runtime guidance. Backward-incompatible principle removals or redefinitions require
a MAJOR version bump. New principles, new mandatory sections, or materially expanded
governance require a MINOR bump. Clarifications and wording-only updates require a PATCH
bump.

Compliance review is required for every Speckit-generated plan and before work is marked
complete. Reviewers MUST block completion when durability, RBAC, tenant isolation,
secret handling, critical tests, or evidence artifacts are missing. Release readiness
MUST reconcile tracker statements with current test and gate artifacts.

**Version**: 1.0.0 | **Ratified**: 2026-06-22 | **Last Amended**: 2026-06-22
