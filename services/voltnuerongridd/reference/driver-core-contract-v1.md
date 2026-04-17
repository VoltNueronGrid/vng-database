# VoltNueronGrid Driver Core Contract v1

**Status:** Draft v1 (Sprint V3-S0 scaffold)  
**Owner:** Architecture + DX  
**Last updated:** 2026-04-17

---

## 1) Purpose

Define a single cross-language driver contract for first-party drivers:

- Rust
- TypeScript
- Python

This contract intentionally keeps transport HTTP-based for now, while preserving a future path to native protocol support.

---

## 2) Driver Responsibilities

Every first-party driver MUST provide:

1. Connection/session configuration validation.
2. Request builders for SQL execute/analyze/route/transaction.
3. Health/status request helpers.
4. Uniform auth header handling for admin/operator/tenant.
5. Timeout/cancellation/retry strategy hooks.
6. Typed error model with redaction-safe messages.

---

## 3) Canonical Config Model

```json
{
  "baseUrl": "http://127.0.0.1:8080",
  "sessionId": "session-123",
  "mode": "admin|operator|tenant",
  "adminApiKey": "secret-optional",
  "operatorId": "optional-for-operator",
  "tenantId": "optional-for-tenant",
  "userId": "optional-for-tenant",
  "routeHint": "optional",
  "requestTimeoutMs": 30000,
  "maxRetries": 2
}
```

Validation rules:

- `baseUrl` and `sessionId` are required.
- `mode=admin` requires `adminApiKey`.
- `mode=operator` requires `adminApiKey` and `operatorId`.
- `mode=tenant` requires `tenantId`; `userId` optional.

---

## 4) Header Contract

Mandatory headers:

- `x-vng-session-id`
- `content-type: application/json`

Conditional headers:

- `x-vng-admin-key` (admin/operator)
- `x-vng-operator-id` (operator)
- `x-vng-tenant-id` (tenant)
- `x-vng-user-id` (tenant; optional)
- `x-vng-route-hint` (optional)

---

## 5) Endpoint Contract (v1)

Drivers must provide helpers for:

- `GET /health`
- `POST /api/v1/sql/analyze`
- `POST /api/v1/sql/route`
- `POST /api/v1/sql/execute`
- `POST /api/v1/sql/transaction`
- `GET /api/v1/ingest/schema/registry`

---

## 6) Error Contract

Drivers should normalize errors into a language-native type with:

- `kind`: validation | transport | http_status | serialization | timeout | cancelled
- `message`: safe diagnostic message (no secret leakage)
- `statusCode`: optional integer
- `requestId`: optional for trace correlation

---

## 7) Conformance Requirements

Each driver must pass the same conformance fixture set:

1. Config validation matrix by mode.
2. Header generation matrix.
3. Request path/body correctness.
4. Timeout/cancellation behavior.
5. Error mapping behavior.

Fixture location (planned):

- `drivers/conformance/fixtures/*.json`

---

## 8) Versioning

- Contract version: `driver-core/v1`.
- Runtime compatibility matrix maintained in release docs.
- Breaking changes require `v2` contract document and migration notes.

