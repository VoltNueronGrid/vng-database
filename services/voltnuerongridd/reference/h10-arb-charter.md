# H-10 Architecture Review Board Charter

Status: draft
Owner: Chief Architect
Release Target: R4
Last Updated: 2026-04-03

## Mission
Provide architecture governance for maintainability-critical decisions including deprecations, dependency policy changes, and release-readiness exceptions.

## Membership
- Chief Architect (chair)
- Release Engineering lead
- Security lead
- SQL/Query lead
- Storage/Distributed Systems lead
- Platform/SRE lead

## Cadence
- Regular cadence: bi-weekly
- Emergency session: within 48 hours for security/data-integrity exceptions

## Decision Areas
- Deprecation approvals and timeline exceptions
- Dependency policy updates
- Breaking change approvals
- Gate exception approvals for R4 promotions

## Voting Rules
- Standard decisions: simple majority
- Breaking removals: two-thirds majority and chair approval
- Emergency actions: security lead + chair minimum, with retrospective vote required

## Records
Each meeting must produce:
- Agenda
- Decisions and rationale
- Action items with owners/dates
- Registry updates (if applicable)

## Success Criteria
- All H-10 deprecations tracked in registry
- All removals linked to ARB decisions
- No R4 release promotion without ARB checklist completion
