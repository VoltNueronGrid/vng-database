---
description: "Use for code review, gap analysis, tracker updates, security, testing quality gates, multi-agent execution, SOLID, and reuse standards across the repository."
applyTo: ["crates/**/*.rs", "services/**/*.rs", "drivers/**/*.rs", "tests/**/*.rs", "tests/kpi/scripts/**", "status_tracker.md", "status-tracker-sprintwise-v1.md"]
---
# VoltNueronGrid Engineering Quality Instructions

## Required Delivery Outcomes
- Close requirement/workstream/release gaps before declaring completion.
- Prefer reusable abstractions and shared helpers over duplicated logic.
- Apply SOLID principles in new and modified code.
- Use multiple agents for discovery, validation, and execution whenever work is multi-step.

## SOLID and Reuse Rules
- Single Responsibility: keep handlers, domain logic, and persistence concerns separated.
- Open/Closed: extend behavior through traits/interfaces and adapters, avoid editing stable code paths when extension points exist.
- Liskov Substitution: new implementations must preserve existing contracts and test expectations.
- Interface Segregation: expose focused traits; avoid large all-in-one interfaces.
- Dependency Inversion: depend on traits or abstractions, not concrete implementations.
- Reuse first: check existing crates/modules/helpers before creating new code.
- Eliminate duplication: if a pattern appears twice, extract shared function/module.

## Multi-Agent Execution Rules
- For substantial tasks, run at least two agents in parallel for read-only discovery.
- Suggested split:
  - Agent A: locate impacted files/symbols/gaps.
  - Agent B: propose low-risk implementation/test plan.
  - Optional Agent C: validate security/testing implications.
- Reconcile findings and implement only evidence-backed changes.

## Code Review Rules
- Prioritize correctness, security, behavior regressions, and test gaps.
- Include severity and concrete fix path for each finding.
- Block approval when:
  - auth/RBAC checks are bypassed,
  - tenant isolation is violated,
  - secret handling is unsafe,
  - tests are missing for changed critical paths.

## Gap Analysis Rules
- Compare tracker claims with artifact truth (`tests/kpi/results/**`).
- Treat artifact JSON status as canonical over script exit codes.
- Any mismatch must be corrected in tracker files in the same change set.

## Tracker Update Rules
- Keep `status_tracker.md` and `status-tracker-sprintwise-v1.md` synchronized.
- Update timestamps, session references, and gate evidence paths together.
- Do not add speculative claims; every status statement must map to current artifacts.

## Security Rules (in addition to security-rbac instructions)
- Maintain admin -> operator -> tenant auth order.
- Never log keys/secrets; never hardcode KMS identifiers.
- Validate tenant boundaries for all tenant-facing reads/writes.

## Testing Quality Gates
- New/changed logic requires unit tests and at least one integration path where applicable.
- Required test dimensions:
  - unit tests,
  - integration tests,
  - mutation testing (where feasible),
  - code coverage.
- Coverage target: minimum 90% for changed modules and overall gate reports where supported.
- Recommended commands:
  - `cargo test -p <crate>`
  - `cargo llvm-cov --workspace --lcov --output-path target/coverage.lcov`
  - `cargo llvm-cov --workspace --summary-only`
  - `cargo mutants --timeout 300 --output target/mutants`

## Definition of Complete
- Functional gaps closed and tracker synced.
- Security and review checks passed.
- Tests pass and coverage >= 90% target for impacted scope.
- Evidence artifacts and tracker narrative agree.
