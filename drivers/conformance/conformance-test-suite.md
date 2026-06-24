# VoltNueronGrid Driver Conformance Test Suite

> **Status:** Skeleton — P9 implementation target.  
> **Gate script:** `tests/kpi/scripts/run-driver-conformance-gate.ps1`  
> **Fixtures:** `drivers/conformance/fixtures/`  
> **Reports:** `drivers/conformance/reports/`

---

## Purpose

This document defines the driver conformance test suite for all VoltNueronGrid
client drivers. Conformance validates that each driver correctly:

1. Authenticates in admin, operator, and tenant modes.
2. Builds well-formed HTTP requests with the correct headers.
3. Respects connection pool min/max configuration.
4. Handles transport errors and retries correctly.
5. Propagates server error responses without data corruption.

---

## Test Categories

### C1 — Configuration Validation

Tests that driver configuration is validated on construction.  
Fixture: `fixtures/config-validation-cases.json`

| Case | Expected |
|------|----------|
| admin mode without adminApiKey | error: "admin mode requires adminApiKey" |
| operator mode without operatorId | error: "operator mode requires adminApiKey and operatorId" |
| tenant mode without tenantId | error: "tenant mode requires tenantId" |
| valid admin config | no error |
| valid tenant config | no error |

### C2 — Transport Modes

Tests that the driver selects the correct transport (HTTP/HTTPS, WebSocket) based on config.  
Fixture: `fixtures/transport-mode-cases.json`

| Case | Expected |
|------|----------|
| http baseUrl → plain HTTP transport | transport_type = "http" |
| https baseUrl → TLS transport | transport_type = "https" |
| missing baseUrl → error | error on construction |

### C3 — Request Building

Tests that the driver produces correct HTTP request structure.  
Fixture: `fixtures/request-building-cases.json`

| Case | Expected |
|------|----------|
| admin query | x-vng-admin-key header set |
| tenant query | x-vng-tenant-id + x-vng-user-id headers set |
| operator query | x-vng-operator-id + x-vng-admin-key headers set |
| sql_batch body | Content-Type: application/json, body has sql_batch key |

### C4 — Connection Pool

Tests connection pool lifecycle.  
(No fixture yet — to be defined when pool is testable.)

| Case | Expected |
|------|----------|
| pool respects minConnections | at least min connections created |
| pool respects maxConnections | at most max connections created |
| pool releases on Drop | connections returned on session end |

### C5 — Error Propagation

Tests that server error responses are surfaced to callers without data corruption.

| Case | Expected |
|------|----------|
| 401 Unauthorized | error kind = AuthError |
| 403 Forbidden | error kind = PermissionError |
| 503 Service Unavailable | error kind = ServerUnavailable |
| malformed JSON response | error kind = ProtocolError |

---

## Drivers in Scope

| Driver | Language | Conformance Status |
|--------|----------|--------------------|
| voltnuerongrid-driver-rust | Rust | Partial (C1, C2, C3 fixtures defined) |
| voltnuerongrid-driver-python | Python | Not started |
| voltnuerongrid-driver-node | Node.js | Not started |
| voltnuerongrid-driver-java | Java | Not started |
| voltnuerongrid-driver-typescript | TypeScript | Not started |
| voltnuerongrid-driver-deno | Deno | Not started |
| voltnuerongrid-driver-perl | Perl | Not started |
| voltnuerongrid-driver-c | C | Not started |

---

## Running the Gate

```bash
# No live server needed — runs unit tests + fixture validation only.
pwsh ./tests/kpi/scripts/run-driver-conformance-gate.ps1

# Output artifact:
tests/kpi/results/ws10/driver-conformance-gate.json
```

---

## Completion Criteria for P9

- [ ] All C1 config-validation fixture cases have corresponding Rust driver unit tests.
- [ ] C2 transport-mode fixture passes gate validation.
- [ ] C3 request-building fixture passes gate validation.
- [ ] Gate script emits `"status": "passed"` artifact.
- [ ] At least one non-Rust driver adds a conformance test for C1.
- [ ] `drivers/conformance/reports/` contains a baseline conformance report.
