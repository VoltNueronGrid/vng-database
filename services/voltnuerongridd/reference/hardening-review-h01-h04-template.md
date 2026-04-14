# Hardening Review Template (H-01 to H-04)

Use this template for the first architecture hardening review cycle.

## Review Metadata

- Review Date:
- Facilitator:
- Attendees:
- Scope: H-01, H-02, H-03, H-04
- Release Impact: R2

## Gate Inputs

- `reference/autonomous-guardrails-api.md`
- `tests/kpi/scripts/run-autonomous-guardrail-smoke.ps1`
- `tests/kpi/results/20260305-h01/autonomous-guardrail-smoke.json`
- `tests/kpi/results/20260304-pr007/reports-real/final-gate-report.md`

## Checklist

### H-01 Autonomous Blast Radius Controls
- [ ] Emergency-stop endpoint contract reviewed
- [ ] Policy matrix reviewed (allow/deny coverage)
- [ ] Smoke evidence reviewed and signed
- [ ] Follow-up gaps assigned with owners

### H-02 HTAP Sync Correctness
- [ ] Consistency fault model documented
- [ ] Test harness scope approved
- [ ] Failure-mode matrix reviewed

### H-03 Control-Plane Resilience
- [ ] Chaos test plan reviewed
- [ ] Control-plane failover criteria approved
- [ ] Recovery SLA criteria captured

### H-04 Event Durability
- [ ] Outbox and replay invariants reviewed
- [ ] Exactly-once claim boundaries documented
- [ ] Replay validation plan approved

## Decisions

- Decision 1:
- Decision 2:

## Action Items

| Action | Owner | Due Date | Evidence Required |
|---|---|---|---|
|  |  |  |  |
|  |  |  |  |

## Exit Criteria

- [ ] Review minutes published
- [ ] Action owners confirmed
- [ ] Next evidence milestone dates updated in `status_tracker.md`
