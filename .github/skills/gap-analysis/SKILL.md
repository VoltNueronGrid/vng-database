---
name: gap-analysis
description: "Find and close tracker/evidence gaps. Use when: validating status claims against artifacts, preparing release readiness updates, and reconciling inconsistencies across tracker files."
argument-hint: "Scope, e.g. 'R2 and Sprint 8' or 'full tracker mismatch scan'"
---
# Gap Analysis Skill

## Canonical Data
- Use `tests/kpi/results/**` artifacts as source of truth.
- Do not infer state from command exit codes when JSON artifact exists.

## Steps
1. Compare tracker statements to artifact fields (`status`, `release_readiness`, timestamps).
2. List mismatches with exact replacement text.
3. Apply synchronized fixes to both tracker files.
4. Re-verify no stale references remain.

## Output
- Mismatch table with `location`, `current`, `expected`, `replacement`.
- Final confirmation that trackers and artifacts are aligned.
