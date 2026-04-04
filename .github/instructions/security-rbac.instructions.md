---
description: "Use when working on auth, RBAC, security, encryption, KMS, TLS, operator identity, tenant isolation, or access-control logic. Covers required header checks, privilege enforcement, and forbidden bypass patterns."
---
# VoltNueronGrid DB — Security & RBAC Instructions

## Auth Enforcement Order (MANDATORY)

Every protected endpoint must check in this exact order:

```
1. Admin gate:    VNG_ADMIN_API_KEY env var  ←→  x-vng-admin-key header
2. Operator gate: x-vng-operator-id header   +  registered role binding
3. Tenant gate:   x-vng-tenant-id header     +  x-vng-user-id header
```

Mixed operator-or-tenant surfaces (e.g. ingest) check `x-vng-admin-key` OR `x-vng-operator-id` first, then fall through to tenant headers.

## Response Codes
- `401` — missing header or invalid key
- `403` — header present but role/privilege insufficient

## Tenant Isolation Rules
- **Never** return data belonging to a different tenant
- Queries/records scoped to a tenant must filter by `tenant_id`
- Audit records emitted from tenant operations must carry the emitting tenant's ID
- Autonomous action records must include optional `tenant_scope` metadata and support tenant-filtered reads

## Operator Identity
- Operators must have a registered role binding in addition to presenting `x-vng-admin-key`
- The `x-vng-operator-id` header is required for operator-scoped control-plane paths
- Role bindings are stored in `AppState`; validate against the in-memory registry

## KMS & Encryption
- KMS key IDs are **never hardcoded** — resolve via `VNG_KMS_PRIMARY_KEY_ID`, `VNG_KMS_FAILOVER_KEY_ID`, etc.
- Provider awareness: local → generic in-memory refs; AWS/Azure/GCP → CLI-backed refs when env vars are set
- Log only key reference names, never key material or secret values

## Plugin Security
- Plugin manifests must be signed; reject any unsigned manifest with `403`
- Signed provenance endpoint: `POST /api/v1/security/plugins/provenance/register`
- Keyring trust/revocation policy hooks must be checked before loading plugin

## TLS / Encryption-at-Rest
- TLS and mTLS configuration lives in security contract validated by WS5
- Adding new endpoints requires adding them to the WS5 security contract
- Encryption-at-rest enforced via KMS adapter; never write plaintext sensitive fields

## Forbidden
- Do NOT add a bypass flag (e.g. `skip_auth: bool`) to any handler
- Do NOT return `200` for an unauthenticated request on a protected path
- Do NOT log API key headers even for debugging
