---
name: code-review-plus
description: "Advanced code review workflow. Use when: reviewing PRs, validating SOLID/reuse adherence, checking regressions, auditing tests and coverage thresholds, and producing actionable findings."
argument-hint: "Scope, e.g. 'services main.rs changes' or 'WS6 failover PR'"
---
# Code Review Plus Skill

## Objectives
- Detect correctness/security regressions quickly.
- Enforce SOLID and reusable design patterns.
- Verify test completeness and coverage quality.

## Review Flow
1. Discover changed files and impacted symbols.
2. Validate behavior and failure modes.
3. Check SOLID/reuse violations and duplication.
4. Verify unit/integration tests and coverage evidence.
5. Report findings by severity with precise fixes.

## Severity Model
- Critical: auth bypass, data leak, incorrect transaction/consistency behavior.
- High: regression risk, missing error handling, missing critical tests.
- Medium: maintainability problems, SOLID violations, fragile APIs.
- Low: style consistency and minor refactors.

## Required Output
- Findings list with `file`, `severity`, `impact`, `fix`.
- Explicit `APPROVE` or `REQUEST CHANGES` verdict.
