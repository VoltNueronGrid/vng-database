---
name: quality-testing
description: "Testing workflow for unit, integration, mutation, and coverage gates. Use when: validating feature quality, enforcing >=90% coverage, and preventing regressions."
argument-hint: "Scope, e.g. 'WS3 router changes' or 'full workspace quality gate'"
---
# Quality Testing Skill

## Mandatory Test Types
- Unit tests for changed modules.
- Integration tests for API/runtime behavior.
- Mutation testing for critical logic paths.
- Coverage reporting with 90% minimum target.

## Commands
```bash
cargo test -p <crate>
cargo test -p voltnuerongridd <prefix>_
cargo llvm-cov --workspace --summary-only
cargo llvm-cov --workspace --lcov --output-path target/coverage.lcov
cargo mutants --timeout 300 --output target/mutants
```

## Gate Criteria
- No failing tests.
- No critical mutation survivors in changed critical paths.
- Coverage >= 90% on impacted modules; if below, add tests before completion.

## Output
- Test summary by type.
- Coverage percentage and target comparison.
- Mutation results with surviving mutants and fixes.
