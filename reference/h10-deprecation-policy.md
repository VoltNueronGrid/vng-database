# H-10 Deprecation Policy

Status: draft for ARB review
Owner: Chief Architect + Release Engineering
Release Target: R4
Last Updated: 2026-04-03

## Policy Goals
- Keep API and configuration evolution predictable.
- Minimize breaking-change risk for drivers, IDEs, and runtime clients.
- Provide explicit migration guidance before removals.

## Deprecation Lifecycle
1. Proposal
- Open a deprecation proposal with rationale, impact, and alternatives.
- Tag affected areas: API, config, script, runtime behavior, contract.

2. ARB Review
- Architecture Review Board approves or rejects proposal.
- Decision is recorded in the deprecation registry.

3. Announcement
- Mark item as deprecated in docs and release notes.
- Provide migration path and timeline.

4. Grace Period
- Default grace period is two releases unless ARB overrides.
- During grace period, telemetry and warnings are collected.

5. Removal
- Removal occurs only after grace period completion and ARB sign-off.
- Removal PR must reference registry entry and migration evidence.

## Required Metadata
Each deprecated item must include:
- Identifier
- Owner
- First deprecated release
- Planned removal release
- Impact scope
- Migration instructions
- ARB decision reference

## Exceptions
Emergency removals are allowed only for security or data-integrity incidents and require post-incident ARB retrospective documentation.

## Acceptance Criteria
- Registry entry exists for every deprecated item.
- Release notes include all active deprecations.
- No removal merges without ARB sign-off reference.
