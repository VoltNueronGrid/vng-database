# Runtime Native Listener Checklist (NT-S2-002 / NT-S2-003)

**Status:** Execution checklist  
**Scope:** Runtime scaffolding tasks for dual-transport baseline  
**Linked tasks:** `NT-S2-002`, `NT-S2-003`  
**Reference spec:** `native-protocol-v1.md`

---

## A) NT-S2-002 — `db-native-listener` Scaffold

### A1. Config and Feature Gate

- [ ] Add `native_listener` config section with defaults:
  - [ ] `enabled` (default `false`)
  - [ ] `host`
  - [ ] `port`
  - [ ] `tls_enabled`
  - [ ] `max_connections`
  - [ ] `idle_timeout_ms`
  - [ ] `handshake_timeout_ms`
- [ ] Add runtime startup guard:
  - [ ] listener not started when `enabled=false`
  - [ ] listener startup failure does not crash HTTP startup path in scaffold stage
- [ ] Add startup logs with redaction-safe config summary.

### A2. Listener Skeleton

- [ ] Create native listener module/package (`db_native_listener` or equivalent).
- [ ] Add connection accept loop with graceful shutdown hook.
- [ ] Add placeholder frame decoder/encoder boundaries.
- [ ] Add handshake skeleton (`HELLO` -> `HELLO_ACK`).
- [ ] Add auth skeleton (`AUTH` -> `AUTH_ACK` / `ERROR`).

### A3. Operational Safety

- [ ] Add per-connection idle timeout enforcement.
- [ ] Add max-connections guard with explicit rejection behavior.
- [ ] Add heartbeat baseline (`PING/PONG`) plumbing stubs.
- [ ] Add structured error path with `requestId` correlation.

### A4. Metrics/Telemetry Baseline

- [ ] `native_connections_active`
- [ ] `native_handshake_failures_total`
- [ ] `native_auth_failures_total`
- [ ] `native_command_latency_ms` (histogram, command-tagged)
- [ ] `native_stream_cancellations_total`

### A5. NT-S2-002 Exit Checks

- [ ] Runtime boots with listener disabled by default.
- [ ] Runtime boots with listener enabled in integration env.
- [ ] Basic native `health` command roundtrip returns expected response.
- [ ] HTTP behavior unchanged in regression smoke test.

---

## B) NT-S2-003 — Shared Transport Abstraction

### B1. Canonical Command Model

- [ ] Define canonical command envelope:
  - [ ] `requestId`
  - [ ] `sessionContext`
  - [ ] `commandName`
  - [ ] `payload`
  - [ ] `transportMetadata` (`http|native`)
- [ ] Define canonical response envelope:
  - [ ] success payload
  - [ ] error payload
  - [ ] transport-agnostic metadata

### B2. Dispatcher and Handler Wiring

- [ ] Create `CommandDispatcher` entrypoint independent of HTTP/native adapters.
- [ ] Extract/reuse existing HTTP command logic into shared handlers.
- [ ] Route native decoded commands into the same shared handlers.
- [ ] Ensure side-effect operations preserve existing transaction semantics.

### B3. Adapter Layer Boundaries

- [ ] HTTP adapter responsibilities isolated to:
  - [ ] request parsing
  - [ ] response serialization
  - [ ] HTTP status mapping
- [ ] Native adapter responsibilities isolated to:
  - [ ] frame parsing
  - [ ] frame serialization
  - [ ] native error envelope mapping

### B4. Error and Parity Guarantees

- [ ] Map canonical errors to transport-specific envelopes without message drift.
- [ ] Add parity tests for:
  - [ ] `health`
  - [ ] `sql.execute`
  - [ ] one auth failure case
  - [ ] one validation failure case

### B5. NT-S2-003 Exit Checks

- [ ] Same command handler path is used for HTTP and native for covered commands.
- [ ] Contract/parity tests pass for both adapters.
- [ ] No duplicated business logic between HTTP and native command execution paths.

---

## C) Ownership and Handoff

- **Runtime owner:** listener and adapter scaffold
- **Driver owner:** client transport assumptions and capability negotiation hooks
- **QA owner:** parity fixture execution and report generation
- **DX owner:** confirm downstream extension compatibility assumptions (`auto` mode readiness)

---

## D) Evidence Links (fill during execution)

- [ ] PR/commit link for listener scaffold
- [ ] PR/commit link for transport abstraction
- [ ] Test report link for HTTP/native parity
- [ ] Metrics snapshot link from integration run

