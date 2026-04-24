# Security and Compliance Checklist v1

**Document:** S11-003  
**Status:** Baseline closed (v0.1 release candidate)  
**Last updated:** 2026-04-22  
**Owner:** VoltNueronGrid Security DRI

---

## 1. Secret Management

- [x] **Admin key stored in VSCode SecretStorage (not plaintext)**
  - Implementation: `ui/ide-extensions/vscode-cursor/` uses `vscode.SecretStorage` API
  - Evidence: No admin key appears in `globalState` or `workspaceState` writes
  - Status: ✅ Complete

- [x] **No secrets logged in output channels**
  - Implementation: `redactSecrets()` applied to all output channel writes
  - Evidence: Audit pass over extension log calls; no raw key/token strings appear
  - Status: ✅ Complete

- [x] **`VNG_ADMIN_API_KEY` not written to log files**
  - Implementation: `voltnuerongrid-audit` crate redacts known secret env var patterns
  - Evidence: `voltnuerongrid-audit` unit tests verify redaction
  - Status: ✅ Complete

---

## 2. Transport Security

- [x] **TLS on native listener (optional, env-configured)**
  - Config: `VNG_NATIVE_TLS_CERT` / `VNG_NATIVE_TLS_KEY` env vars
  - Implementation: `tokio-rustls` integration in `services/voltnuerongridd/src/main.rs`
  - Status: ✅ Complete (optional; disabled by default for local dev)

- [x] **mTLS client certificate verification**
  - Config: `VNG_NATIVE_MTLS_CA` env var; enables client cert requirement
  - Implementation: `rustls` `ClientCertVerifier` plugged into native listener
  - Evidence: `runtime-native-listener-checklist-nt-s2-002-003.md` sign-off
  - Status: ✅ Complete

- [x] **Bearer token authentication on native listener**
  - Config: `VNG_NATIVE_BEARER_TOKEN`
  - Implementation: Frame-level auth in native protocol v1 handshake
  - Evidence: `native-protocol-v1.md` §4 (auth frame)
  - Status: ✅ Complete

- [x] **HTTPS enforced on HTTP API listener in production profiles**
  - Config: `VNG_HTTP_TLS_CERT` / `VNG_HTTP_TLS_KEY`; production env template documents this
  - Status: ✅ Complete (optional in dev, documented as required for production)

---

## 3. Authorization

- [x] **RBAC guard on IDE commands (admin / operator / tenant)**
  - Implementation: `voltnuerongrid-auth` crate; `RbacGuard::check()` called in command handlers
  - Evidence: Auth crate unit tests cover all three role levels
  - Status: ✅ Complete

- [x] **Tenant isolation — cross-tenant data access blocked**
  - Implementation: `tenant_id` scoping enforced in query routing
  - Evidence: Auth integration tests in `voltnuerongrid-auth`
  - Status: ✅ Complete

---

## 4. Data Integrity

- [ ] **SQL injection: parameterized query enforcement (runtime)**
  - Implementation: Parameterized query API exists in drivers; enforcement at runtime level is in progress
  - Gap: Runtime does not yet reject raw string interpolation paths
  - Status: ⏳ In Progress — target S12
  - Owner: Runtime team

- [x] **Input validation on all ingest endpoints**
  - Implementation: Schema validation before write in `voltnuerongrid-ingest`
  - Status: ✅ Complete

---

## 5. Audit and Observability

- [ ] **Audit log for admin operations**
  - Implementation: `voltnuerongrid-audit` crate scaffolded; log schema defined
  - Gap: Persistent audit log sink (file/syslog) not yet wired in service startup
  - Status: ⏳ In Progress — `voltnuerongrid-audit` crate; sink wiring in S12
  - Owner: Observability DRI

- [x] **Structured log format (JSON) available**
  - Config: `VNG_LOG_FORMAT=json`
  - Status: ✅ Complete

- [x] **No PII in default log output**
  - Implementation: `redactSecrets()` + structured log fields reviewed in audit pass
  - Status: ✅ Complete

---

## 6. Dependency and Supply Chain

- [ ] **Dependency vulnerability scan**
  - Implementation: `cargo audit` not yet integrated into CI pipeline
  - Gap: CI pipeline not yet established for this repository
  - Status: ⏳ Deferred — requires CI (S12+)
  - Workaround: Run `cargo audit` manually before each release

- [ ] **SBOM (Software Bill of Materials) generation**
  - Status: ⏳ Deferred (S12+)

- [x] **No unapproved native code dependencies**
  - All `unsafe` blocks reviewed as part of S9 hardening pass
  - Status: ✅ Complete

---

## 7. Rate Limiting and Availability

- [ ] **Rate limiting on ingest endpoints**
  - Status: ⏳ Deferred — S12+
  - Rationale: Not required for v0.1 RC (single-tenant local + small-team cloud)

- [ ] **Request timeout enforcement on all HTTP handlers**
  - Status: ⏳ In Progress — Axum layer-level timeout partially configured

---

## 8. Summary

| Category | ✅ Complete | ⏳ In Progress / Deferred |
|----------|------------|--------------------------|
| Secret management | 3/3 | 0 |
| Transport security | 4/4 | 0 |
| Authorization | 2/2 | 0 |
| Data integrity | 1/2 | 1 (SQL injection enforcement) |
| Audit and observability | 3/5 | 2 (audit sink, SBOM) |
| Dependency / supply chain | 1/3 | 2 (vuln scan, SBOM) |
| Rate limiting / availability | 0/2 | 2 |

**Overall RC gate:** Items marked ✅ are sufficient for v0.1 RC release. All ⏳ items are
tracked in the S12+ backlog and do not block the RC release.

---

## 9. Sign-off

| Role | Name | Date |
|------|------|------|
| Security DRI | (placeholder) | — |
| Release DRI | (placeholder) | — |
| Architecture DRI | (placeholder) | — |
