# H-09 Cross-IDE Parity And Safety Test Matrix

Status: draft
Owner: DX Team
Release Target: R4
Last Updated: 2026-04-03

## Scope
This matrix defines parity and safety checks across supported IDE integrations:
- Visual Studio
- Cursor
- Antigravity
- JetBrains
- Eclipse

## Capability Matrix
| Capability | VS | Cursor | Antigravity | JetBrains | Eclipse | Notes |
|---|---|---|---|---|---|---|
| Connect/Disconnect | planned | planned | planned | planned | planned | Verify clean session lifecycle |
| SQL Analyze | planned | planned | planned | planned | planned | `/api/v1/sql/analyze` parity |
| SQL Route | planned | planned | planned | planned | planned | `/api/v1/sql/route` parity |
| SQL Execute | planned | planned | planned | planned | planned | `/api/v1/sql/execute` parity |
| Tenant Header Propagation | planned | planned | planned | planned | planned | `x-vng-tenant-id` and `x-vng-user-id` |
| Auth Failure Handling | planned | planned | planned | planned | planned | 401/403 consistency and sanitized messages |
| Permission Boundary Enforcement | planned | planned | planned | planned | planned | No over-privileged calls |
| Audit Trace Presence | planned | planned | planned | planned | planned | Verify operation IDs and tenant scoping |

## Safety Checks
| Safety Control | Validation Rule | Acceptance |
|---|---|---|
| Permission boundary | IDE action must map to allowed API capability set | No forbidden endpoint usage |
| Tenant isolation | Requests must include tenant/user identity where required | No cross-tenant access |
| Error sanitization | Extension surfaces user-safe errors only | No secret leakage in UI logs |
| Request throttling | Burst behavior must respect configured limits | No unbounded retry storms |
| Audit linkage | Actions must emit correlatable audit evidence | Traceable end-to-end events |

## Execution Plan
1. Add `run-h09-ide-parity-matrix.ps1` to execute matrix checks.
2. Add result artifact `tests/kpi/results/h09/h09-ide-parity-matrix.json`.
3. Add gate aggregator `tests/kpi/scripts/run-h09-gate.ps1`.
4. Integrate H-09 scripts into CI workflow.

## Evidence Targets
- `tests/kpi/results/h09/h09-ide-parity-matrix.json`
- `tests/kpi/results/h09/h09-gate-summary.json`
- `tests/kpi/results/gates/h09-release-readiness.json`
