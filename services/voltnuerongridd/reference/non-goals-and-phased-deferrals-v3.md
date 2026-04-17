# Non-Goals and Phased Deferrals v3

**Status:** Proposed for stakeholder validation (S0-004)  
**Date:** 2026-04-17  
**Owners:** PM + Architecture

---

## Purpose

Document intentional deferrals so no prompt requirement is silently dropped, while preserving focus on P0 outcomes (native drivers + IDE driver integration + reliability baseline).

---

## Current Non-Goals (Phase-limited)

These are not removed from scope; they are deferred with target sprints:

1. **Full language driver matrix beyond Rust/TS/Python**  
   - Deferred to: `S10`+
   - Reason: reduce early fragmentation and close P0 drivers first.

2. **Plugin marketplace-level ecosystem (vector/geospatial/search/multimodel/cache)**  
   - Deferred to: `S10-S11`
   - Reason: requires stabilized extension SPI and security model first.

3. **Trillion-row proof claims in release messaging**  
   - Deferred to: `S9-S11`
   - Reason: claim must be evidence-backed with benchmark protocol and reproducible runs.

4. **Full trigger + queue sink parity**  
   - Deferred to: `S7`
   - Reason: depends on foundational query/runtime hardening and event contract design.

5. **Cloud elastic HA production guarantees**  
   - Deferred to: `S9-S11`
   - Reason: requires reliability and failure-injection evidence before hard commitments.

---

## Guardrails

1. Deferred items remain mapped to explicit `R-*` requirements.
2. Deferrals must include owner, rationale, and target sprint.
3. No release notes may claim completion for a deferred item.
4. Every sprint review must re-confirm deferral validity.

---

## Decision Log

| Decision ID | Decision | R-* coverage impacted | Target sprint | Status |
|---|---|---|---|---|
| DFR-V3-001 | Sequence drivers as Rust/TS/Python first, then expand | R-17 | S1 then S10 | Proposed |
| DFR-V3-002 | Postpone plugin breadth until SPI hardening | R-09 | S10-S11 | Proposed |
| DFR-V3-003 | Require benchmark evidence before scale claims | R-10, R-13 | S9-S11 | Proposed |
| DFR-V3-004 | Stage trigger and queue event parity after core runtime closure | R-12 | S7 | Proposed |
| DFR-V3-005 | Keep cloud HA guarantees behind soak/failure gates | R-04, R-08 | S9-S11 | Proposed |

---

## Stakeholder Approval Checklist

- [ ] PM approval
- [ ] Architecture approval
- [ ] Runtime lead approval
- [ ] DX lead approval

