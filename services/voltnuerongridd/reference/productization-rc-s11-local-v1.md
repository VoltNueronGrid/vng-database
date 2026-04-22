# Productization and RC Pack (S11) - Local Validation v1

## S11-001 End-to-end scenario pack

- Scenario pack root: `tests/kpi/scenarios/s11-e2e-pack/`
- Covers prompt-aligned workflows:
  - connection and auth
  - ingest and schema registry
  - query execute/analyze/route
  - plugin/security control checkpoints

## S11-002 Compatibility matrix

- Matrix artifact:
  - `services/voltnuerongridd/reference/versioned-compatibility-matrix-s11-v1.md`
- Scope:
  - runtime version x driver version x extension version
  - notes on transport coverage and known caveats

## S11-003 Security/compliance checklist closure

- Checklist artifact:
  - `services/voltnuerongridd/reference/security-compliance-closure-s11-v1.md`
- Includes:
  - RBAC header enforcement checks
  - key/secret hygiene checks
  - plugin signature and policy checks

## S11-004 RC packaging + installation guides

- RC packaging and install docs:
  - `services/voltnuerongridd/reference/rc-packaging-installation-guides-s11-v1.md`
- Local install path documented as primary.
- Cloud install path documented as deferred validation lane.

## Cloud Defer Note

Cloud runtime deployment and hosted install validation remain deferred.
S11 v1 is accepted on local packability and local install reproducibility.
