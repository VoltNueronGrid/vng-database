---
name: security-audit
description: "Audit the security posture of VoltNueronGrid DB code and endpoints. Use when: reviewing auth logic, RBAC enforcement, checking for auth bypass, auditing endpoint security, reviewing KMS/TLS/plugin manifest security, checking tenant isolation, running WS5 security gate."
argument-hint: "Scope of audit, e.g. 'new endpoint in main.rs' or 'full WS5 gate'"
---
# Security Audit Skill

Review VoltNueronGrid security posture against RBAC contracts, tenant isolation, KMS rules, and plugin security.

## When to Use
- Before merging any new endpoint handler
- When reviewing auth/RBAC code changes
- When running or evaluating the WS5 security gate
- When checking for tenant data leakage

## Procedure

### Step 1 — Auth Enforcement Check
For every new or modified endpoint handler, verify this order:

```
✅ 1. Admin gate:    env::var("VNG_ADMIN_API_KEY")  ←→  headers["x-vng-admin-key"]
✅ 2. Operator gate: headers["x-vng-operator-id"]   +  role binding in AppState
✅ 3. Tenant gate:   headers["x-vng-tenant-id"]     +  headers["x-vng-user-id"]
```

Checks:
- [ ] Returns `401` when credential header is missing
- [ ] Returns `403` when credential is present but role/privilege is insufficient
- [ ] Does NOT return `200` for unauthenticated requests on protected paths
- [ ] No `skip_auth` flag or bypass path

### Step 2 — Tenant Isolation Check
- [ ] Database queries/record reads filter by `tenant_id`
- [ ] No cross-tenant data returned in the same response
- [ ] Audit records emitted by tenant operations carry the tenant's `x-vng-tenant-id`
- [ ] Autonomous action records include `tenant_scope` and support tenant-filtered reads

### Step 3 — Secrets / Logging Check
- [ ] API key header values are never logged
- [ ] KMS key IDs resolve from `VNG_KMS_*` env vars — no hardcoded key IDs
- [ ] No raw secrets in error response bodies

### Step 4 — Plugin Security Check (if applicable)
- [ ] Plugin manifest signature is verified before load
- [ ] Unsigned manifests return `403`
- [ ] Revocation policy hook is invoked before trust

### Step 5 — Run WS5 Security Gate
Start the server, then:
```powershell
pwsh ./tests/kpi/scripts/run-ws5-gate.ps1 `
    -IncludeRuntimeSmokes `
    -BaseUrl "http://127.0.0.1:8080" `
    -OutputPath "./tests/kpi/results/ws5/ws5-gate-summary.json"
```
Evaluate: `(Get-Content ./tests/kpi/results/ws5/ws5-gate-summary.json | ConvertFrom-Json).status`

### Step 6 — Report Findings
For each finding, report:
- **File + line** where the issue exists
- **Severity**: Critical / High / Medium / Low
- **Issue type**: auth-bypass / data-leak / secret-log / hardcoded-key / unsigned-plugin
- **Fix**: specific code change needed
