# NT-S2-004 Transport Parity Report Baseline

- Scope: initial artifact baseline for dual-transport CI reporting
- Fixture: `drivers/conformance/fixtures/transport-mode-cases.json`
- Status: `In Progress` (S2 uplift)

## Baseline Snapshot

- Total fixture cases: 5
- Modes represented: `http`, `native`, `auto`
- Expected active transports represented: `http`, `native`
- Fallback expectation coverage: present
- Error expectation coverage: present

## CI Artifact Plan

- Rust lane emits:
  - `drivers/conformance/reports/rust-transport-outcomes.json`
  - `drivers/conformance/reports/rust-parity-report.md`
- TypeScript lane emits:
  - `drivers/conformance/reports/typescript-transport-outcomes.json`
  - `drivers/conformance/reports/typescript-parity-report.md`
- Python lane emits:
  - `drivers/conformance/reports/python-transport-outcomes.json`
  - `drivers/conformance/reports/python-parity-report.md`

## Notes

- This baseline artifact is committed for traceability and review.
- CI-generated reports are authoritative for per-run outcomes.
