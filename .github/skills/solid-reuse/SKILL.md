---
name: solid-reuse
description: "Design and refactor with SOLID and high reuse. Use when: adding new features, refactoring handlers/services, reducing duplication, and designing traits/interfaces."
argument-hint: "Scope, e.g. 'new endpoint + store integration'"
---
# SOLID and Reuse Skill

## Design Checklist
- Single Responsibility: one reason to change per module/class/function.
- Open/Closed: extension via traits/adapters, avoid modifying stable core paths.
- Liskov Substitution: implementation behavior preserves interface contract.
- Interface Segregation: smaller focused traits over wide contracts.
- Dependency Inversion: handlers depend on abstractions, not concrete stores.

## Reuse Checklist
- Search existing helper/util/trait before adding new code.
- Extract duplicates into shared functions/modules.
- Keep APIs composable and testable.

## Output
- Proposed abstraction map and reuse opportunities.
- Refactor suggestions with minimal regression risk.
