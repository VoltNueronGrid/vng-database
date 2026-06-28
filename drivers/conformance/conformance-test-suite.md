# VoltNueronGrid Driver Conformance Test Suite

> **Status:** Active — expanded in P9.  
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
6. Deserializes SQL execute responses correctly.
7. Applies per-database scoping via the x-vng-database header.

---

## Test Categories

### C1 — Configuration Validation

Tests that driver configuration is validated on construction.  
Fixture: `fixtures/config-validation-cases.json`

| # | Case | Expected |
|---|------|----------|
| 1 | admin mode without adminApiKey | error: "admin mode requires adminApiKey" |
| 2 | operator mode without operatorId | error: "operator mode requires adminApiKey and operatorId" |
| 3 | tenant mode without tenantId | error: "tenant mode requires tenantId" |
| 4 | valid admin config | no error |
| 5 | valid tenant config | no error |
| 6 | empty baseUrl | error: "baseUrl is required" |
| 7 | baseUrl with trailing slash normalised | trailing slash stripped |

### C2 — Transport Modes

Tests that the driver selects the correct transport (HTTP/HTTPS, WebSocket) based on config.  
Fixture: `fixtures/transport-mode-cases.json`

| # | Case | Expected |
|---|------|----------|
| 8  | http baseUrl → plain HTTP transport | transport_type = "http" |
| 9  | https baseUrl → TLS transport | transport_type = "https" |
| 10 | missing baseUrl → error | error on construction |

### C3 — Request Building

Tests that the driver produces correct HTTP request structure.  
Fixture: `fixtures/request-building-cases.json`

| # | Case | Expected |
|---|------|----------|
| 11 | admin query | x-vng-admin-key header set |
| 12 | tenant query | x-vng-tenant-id + x-vng-user-id headers set |
| 13 | operator query | x-vng-operator-id + x-vng-admin-key headers set |
| 14 | sql_batch body | Content-Type: application/json, body has sql_batch key |
| 15 | database-scoped query | x-vng-database header present when database is set in config |
| 16 | no database in config | x-vng-database header absent |

### C4 — Connection Pool

Tests connection pool lifecycle.  
Fixture: `fixtures/pool-cases.json`

| # | Case | Expected |
|---|------|----------|
| 17 | pool respects minConnections | at least min connections created |
| 18 | pool respects maxConnections | at most max connections created |
| 19 | pool releases on Drop / close | connections returned on session end |

### C5 — Error Propagation

Tests that server error responses are surfaced to callers without data corruption.

| # | Case | Expected |
|---|------|----------|
| 20 | 401 Unauthorized | error kind = AuthError |
| 21 | 403 Forbidden | error kind = PermissionError |
| 22 | 503 Service Unavailable | error kind = ServerUnavailable |
| 23 | malformed JSON response | error kind = ProtocolError |

### C6 — SQL Response Deserialisation

Tests that the driver correctly parses server SQL execute responses.  
Fixture: `fixtures/sql-response-cases.json`

| # | Case | Expected |
|---|------|----------|
| 24 | SELECT response with rows | columns + rows arrays populated |
| 25 | DML INSERT response | transaction field present, status = "ok" |
| 26 | empty SELECT result set | rows = [] (no error) |
| 27 | DDL response (CREATE TABLE) | touches_catalog = true |
| 28 | OLAP response | olap field present, route_path = "olap" |

### C7 — Retry Semantics

Tests that the driver applies correct retry logic for transient failures.

| # | Case | Expected |
|---|------|----------|
| 29 | 503 response → driver retries up to maxRetries | final error after N retries |
| 30 | connection refused → exponential backoff | at least 2 retry attempts logged |

---

## Drivers in Scope

| Driver | Language | Conformance Status |
|--------|----------|--------------------|
| voltnuerongrid-driver-rust | Rust | C1–C3 fixture-level + unit tests |
| voltnuerongrid-driver-python | Python | Skeleton added (P9) |
| voltnuerongrid-driver-node | Node.js | Not started |
| voltnuerongrid-driver-java | Java | Not started |
| voltnuerongrid-driver-typescript | TypeScript | Skeleton added (P9) |
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

- [X] All C1 config-validation fixture cases have corresponding Rust driver unit tests.
- [X] C2 transport-mode fixture passes gate validation.
- [X] C3 request-building fixture passes gate validation.
- [X] Gate script emits `"status": "passed"` artifact.
- [X] At least one non-Rust driver adds a conformance test for C1 (Python + TypeScript skeletons).
- [X] Test case count reaches ≥ 20 (currently 30).
- [ ] `drivers/conformance/reports/` contains a baseline conformance report.
