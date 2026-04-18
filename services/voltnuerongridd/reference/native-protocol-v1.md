# VoltNueronGrid Native Protocol v1

**Status:** Draft skeleton (execution-ready for `NT-S2-*`)  
**Owners:** Architecture + Runtime + Driver Platform + QA  
**Last updated:** 2026-04-17  
**Related tracker:** `status-tracker-v3.md` (`NT-S2-001`..`NT-S2-004`)

---

## 0) Purpose and Scope

Define a first-party native data-plane protocol for VoltNueronGrid while retaining HTTP as a supported parallel transport.

This document is intentionally structured to map 1:1 with:

- `NT-S2-001` (protocol spec)
- `NT-S2-002` (runtime native listener scaffold)
- `NT-S2-003` (shared runtime transport abstraction)
- `NT-S2-004` (dual-transport conformance fixture schema)

---

## 1) NT-S2-001 — Native Protocol Spec (Frame, Handshake, Auth, Errors, Streaming)

### 1.1 Protocol Goals

- Low-latency binary-friendly data-plane transport.
- Explicit session lifecycle and long-lived channel support.
- Consistent request/response semantics with HTTP parity.
- Cross-language codec viability (Rust, TypeScript, Python).

### 1.2 Versioning and Compatibility

- Protocol identifier: `vng-native`
- Protocol version: `v1`
- Capability negotiation required at handshake.
- Backward compatibility policy:
  - `v1.x` additive only
  - breaking change requires `v2`

### 1.3 Connection and Session Lifecycle

States:

1. `socket_open`
2. `hello_sent`
3. `auth_pending`
4. `ready`
5. `in_transaction` (optional state)
6. `closing`
7. `closed`

### 1.4 Frame Envelope (Skeleton)

> Final binary layout TBD in implementation design review.

Mandatory envelope fields:

- `frameType` (enum)
- `protocolVersion`
- `requestId`
- `sessionId` (optional pre-auth, required post-auth)
- `flags` (bitfield)
- `payloadLength`
- `payload` (opaque bytes; JSON or binary body per command)

### 1.5 Frame Types (Initial)

- `HELLO`
- `HELLO_ACK`
- `AUTH`
- `AUTH_ACK`
- `COMMAND`
- `RESULT`
- `ERROR`
- `PING`
- `PONG`
- `STREAM_CHUNK`
- `STREAM_END`
- `CANCEL`
- `GOODBYE`

### 1.6 Auth and Identity Model

Auth modes (aligned to existing contract):

- `admin`
- `operator`
- `tenant`

Auth payload carries:

- credential material (token/key; redacted in logs)
- role mode
- optional tenant/user identifiers
- optional route hint

### 1.7 Command Surface (v1 Baseline)

Commands required for parity:

- `health`
- `sql.analyze`
- `sql.route`
- `sql.execute`
- `sql.transaction`
- `schema.registry`

### 1.8 Error Envelope and Mapping

Server `ERROR` frame minimum fields:

- `kind` (`validation|transport|http_status|serialization|timeout|cancelled|auth|protocol`)
- `message` (safe diagnostic text)
- `statusCode` (optional; for parity bridge)
- `requestId`
- `retryable` (bool)

Client/driver requirement:

- map protocol errors to existing `DriverErrorKind` and `DriverError` model.

### 1.9 Streaming Semantics

- Large results may be returned as `STREAM_CHUNK` frames.
- Stream contract:
  - ordered delivery per `requestId`
  - explicit `STREAM_END`
  - cancel support via `CANCEL`
- Backpressure controls:
  - receiver window hint
  - max in-flight stream frames

### 1.10 Timeout / Retry / Cancel

- Driver-side request timeout mandatory.
- Retry only for idempotent commands unless transaction policy explicitly allows.
- Cancel request must target `requestId` and return terminal response.

### 1.11 Keepalive and Health

- `PING/PONG` heartbeats.
- Configurable heartbeat interval + idle timeout.
- Driver should surface degraded channel diagnostics before hard failure.

### 1.12 Security Baseline

- TLS required in non-local environments.
- mTLS optional capability flag.
- No secret echo in server or client logs.
- Minimum cipher/TLS profile policy to be linked from security checklist.

### 1.13 Decision Closure Log (S2 v1 Defaults)

These decisions are now **closed for v1 scaffold execution** and can only change through a recorded protocol update.

1. **Payload encoding model**
   - Decision: `hybrid` (`json` default, `binary` optional by capability negotiation).
   - Rationale: preserves fast implementation path while allowing native efficiency upgrade path.

2. **Compression policy**
   - Decision: optional frame compression; default `disabled`.
   - Activation threshold: payloads `>= 4096` bytes when both client and server advertise compression capability.
   - Rationale: avoid small-frame overhead while enabling large-result efficiency.

3. **Frame size and fragmentation**
   - Decision: default max unfragmented frame payload `1 MiB`.
   - Fragmentation: required beyond threshold using continuation semantics tied to same `requestId`.
   - Rationale: predictable memory bound for listener and codec implementations.

4. **Session resume semantics**
   - Decision: no cross-socket session resume in v1.
   - Requirement: reconnect requires fresh handshake+auth; idempotent retry policy handles transient errors.
   - Rationale: reduces protocol complexity in S2 while maintaining correctness.

### 1.14 NT-S2-001 Acceptance Checklist

- [ ] Spec reviewed by Architecture, Runtime, Driver leads.
- [x] All required v1 commands documented.
- [x] Error and streaming contracts documented.
- [x] Versioning and compatibility policy documented.

### 1.15 v1 Constant Baselines

- `max_unfragmented_payload_bytes`: `1048576`
- `compression_threshold_bytes`: `4096`
- `heartbeat_interval_ms` (recommended default): `15000`
- `idle_timeout_ms` (recommended default): `60000`
- `handshake_timeout_ms` (recommended default): `5000`
- `protocol_capabilities` baseline:
  - `json_payload`
  - `binary_payload` (optional)
  - `compression` (optional)
  - `streaming`
  - `cancel`

---

## 2) NT-S2-002 — Runtime `db-native-listener` Scaffold

### 2.1 Runtime Config Skeleton

```yaml
native_listener:
  enabled: false
  host: "127.0.0.1"
  port: 7542
  tls_enabled: false
  max_connections: 2048
  idle_timeout_ms: 60000
  handshake_timeout_ms: 5000
```

### 2.2 Feature Gate Plan

- Gate key: `runtime.native_listener.enabled`
- Default: `false`
- Stage policy:
  1. disabled by default in all envs
  2. enabled in integration env
  3. controlled rollout by canary

### 2.3 Listener Scaffold Responsibilities

- accept socket connections
- perform hello/auth handshake
- route validated commands to shared handler layer
- emit protocol errors with request correlation
- expose listener metrics

### 2.4 Initial Observability Hooks

- active native connections
- handshake failures
- auth failures by mode
- command latency p50/p95/p99 by command
- stream cancel count

### 2.5 NT-S2-002 Acceptance Checklist

- [ ] Listener starts/stops behind feature gate.
- [ ] No regression in existing HTTP path.
- [ ] Basic health command over native path works in integration test.
- [ ] Metrics emitted for connection and handshake lifecycle.

---

## 3) NT-S2-003 — Shared Runtime Transport Abstraction

### 3.1 Design Objective

Prevent business logic drift between HTTP and native paths by sharing command handlers and result models.

### 3.2 Proposed Abstraction (Skeleton)

- `TransportGateway` (protocol adapter boundary)
- `CommandDispatcher` (shared routing)
- `ExecutionHandlers` (shared command implementations)
- `ResponseMapper` (transport-specific envelope serialization)

### 3.3 Adapter Responsibilities

HTTP adapter:

- map HTTP request to canonical command
- map canonical response/errors to HTTP responses

Native adapter:

- map native frames to canonical command
- map canonical response/errors to native frames

### 3.4 Canonical Command Model (Draft)

- `requestId`
- `sessionContext`
- `commandName`
- `payload`
- `transportMetadata` (`http|native`)

### 3.5 NT-S2-003 Acceptance Checklist

- [x] Shared handlers execute identical logic for HTTP and native (S2 scope). *(HTTP handlers route through canonical gateway/dispatcher; native `dispatch_frame` routes health/sql.analyze/sql.route/sql.execute/sql.transaction through same canonical dispatch path for scaffolded S2 command set.)*
- [x] Contract tests prove equivalent behavior for key commands. *(HTTP adapter boundary proofs and native adapter command roundtrip proofs landed for current S2 command set.)*
- [x] Transport-specific serialization confined to adapter layer. *(Canonical envelope/success/error wrappers now terminate at HTTP handler boundary.)*
- [x] Error mapping parity validated across both adapters (S2 scope). *(`dispatch_frame` parity matrix now proves protocol + serialization errors normalize into native `ERROR` frames for S2 command set while HTTP handler responses remain unchanged.)*

### 3.6 NT-S2-003 Progress Evidence (Current)

- Runtime scaffold implemented in `services/voltnuerongridd/src/main.rs`:
  - `TransportGateway` command methods (`health`, `sql.analyze`, `sql.route`, `sql.execute` route decision, transaction context)
  - `CommandDispatcher` delegate shell for gateway dispatch
  - canonical envelope helpers (`build_http_envelope`, `extract_request_id`)
  - canonical wrappers (`CanonicalSuccess`, `CanonicalError`)
  - native adapter scaffolding (`NativeFrame`, `NativeFrameType`, `NativeAdapter` mapping helpers)
  - native command router entrypoint (`NativeAdapter::dispatch_frame`) for `NativeCommandKind`-based dispatch with uniform canonical/native error frame conversion
  - native health proof dispatch (`NativeAdapter::dispatch_health_frame` -> canonical dispatch -> native result frame)
  - native sql.analyze proof dispatch (`NativeAdapter::dispatch_sql_analyze_frame` -> canonical dispatch -> native result frame)
  - native sql.route proof dispatch (`NativeAdapter::dispatch_sql_route_frame` -> canonical dispatch -> native result frame)
  - native sql.execute route-decision proof dispatch (`NativeAdapter::dispatch_sql_execute_route_decision_frame` -> canonical dispatch -> native result frame)
  - native sql.transaction context proof dispatch (`NativeAdapter::dispatch_sql_transaction_context_frame` -> canonical dispatch -> native result frame)
- Proof-of-path endpoint wiring:
  - `health` -> dispatcher/gateway
  - `sql.analyze` -> canonical envelope + canonical success mapping
  - `sql.route` -> canonical envelope + canonical success mapping
  - `sql.execute` -> canonical envelope + canonical success route-decision mapping + canonical error usage in blocked UDF branch
  - `sql.transaction` -> canonical transaction context extraction + canonical error usage in write-write conflict branch
- Targeted parity tests:
  - `tests::nt_s2_003_sql_analyze_gateway_wrapper_preserves_http_payload`
  - `tests::nt_s2_003_sql_route_gateway_wrapper_preserves_http_payload`
  - `tests::nt_s2_003_sql_execute_route_decision_wrapper_preserves_routing_result`
  - `tests::nt_s2_003_sql_transaction_context_wrapper_preserves_payload`
  - `tests::nt_s2_003_native_adapter_maps_command_frame_to_canonical_envelope`
  - `tests::nt_s2_003_native_adapter_maps_canonical_error_to_error_frame`
  - `tests::nt_s2_003_native_health_dispatch_roundtrip_produces_result_frame`
  - `tests::nt_s2_003_native_sql_analyze_dispatch_roundtrip_produces_result_frame`
  - `tests::nt_s2_003_native_sql_analyze_dispatch_rejects_missing_payload`
  - `tests::nt_s2_003_native_sql_route_dispatch_roundtrip_produces_result_frame`
  - `tests::nt_s2_003_native_sql_route_dispatch_rejects_invalid_payload`
  - `tests::nt_s2_003_native_sql_execute_route_decision_dispatch_roundtrip_produces_result_frame`
  - `tests::nt_s2_003_native_sql_execute_route_decision_dispatch_rejects_invalid_payload`
  - `tests::nt_s2_003_native_sql_transaction_context_dispatch_roundtrip_produces_result_frame`
  - `tests::nt_s2_003_native_sql_transaction_context_dispatch_rejects_invalid_payload`
  - `tests::nt_s2_003_native_dispatch_frame_rejects_missing_command_with_error_frame`
  - `tests::nt_s2_003_native_dispatch_frame_rejects_unknown_command_with_error_frame`
  - `tests::nt_s2_003_native_dispatch_frame_rejects_non_command_frame_with_error_frame`
  - `tests::nt_s2_003_native_dispatch_frame_routes_health_to_result_frame`
  - `tests::nt_s2_003_native_dispatch_frame_routes_sql_analyze_to_result_frame`
  - `tests::nt_s2_003_native_dispatch_frame_normalizes_handler_serialization_error`
  - `tests::nt_s2_003_native_dispatch_frame_routes_sql_route_to_result_frame`
  - `tests::nt_s2_003_native_dispatch_frame_routes_sql_execute_to_result_frame`
  - `tests::nt_s2_003_native_dispatch_frame_routes_sql_transaction_to_result_frame`
  - `tests::nt_s2_003_native_dispatch_frame_normalizes_sql_route_protocol_error`
  - `tests::nt_s2_003_native_dispatch_frame_normalizes_sql_execute_serialization_error`
  - `tests::nt_s2_003_native_dispatch_frame_normalizes_sql_transaction_protocol_error`
- Local validation:
  - `cargo test -p voltnuerongridd nt_s2_003_` -> 27 passed
  - `cargo test -p voltnuerongridd nt_s2_003_native_` -> 23 passed
  - `cargo check -p voltnuerongridd` -> pass

### 3.7 NT-S2-003 Readiness Checkpoint

- Current decision: **Ready for Validation (S2 scope)**.
- Reason: acceptance checklist items are satisfied for the scoped S2 command surface and adapter-boundary behavior.
- Completed hardening in this increment: `dispatch_frame` success routing proofs for all S2 commands plus protocol/serialization error-normalization parity matrix.

---

## 4) NT-S2-004 — Dual-Transport Conformance Fixture Schema v1

### 4.1 Fixture Goals

- Single semantic expectation set across HTTP and native transports.
- Detect protocol drift early.
- Enable language-agnostic driver parity checks.

### 4.2 Fixture Schema Extension (Draft)

Add fields:

- `transportMode`: `http|native|auto`
- `capabilities`: optional required capabilities list
- `expectFallback`: optional fallback expectation when `auto`
- `expectParityWith`: optional reference case id

### 4.3 Fixture Categories

1. config validation (mode/auth)
2. request building and command mapping
3. error mapping parity
4. timeout/cancel behavior
5. streaming chunk assembly and end-of-stream handling

### 4.4 CI Execution Matrix Target

- Rust driver: HTTP + native lanes
- TS driver: HTTP + native lanes
- Python driver: HTTP + native lanes

### 4.5 NT-S2-004 Acceptance Checklist

- [x] Fixture schema updated and documented.
- [x] At least one driver consumes transport-aware fixtures.
- [ ] CI lane executes and reports transport-specific outcomes. *(Deferred for cloud validation due to runner policy on current repo.)*
- [x] Parity report generated for core commands. *(Initial local/baseline artifact committed; CI-generated per-language artifacts deferred to final cloud validation.)*

### 4.6 NT-S2-004 Progress Evidence (Current)

- Fixture source:
  - `drivers/conformance/fixtures/transport-mode-cases.json`
- CI report generator:
  - `drivers/conformance/scripts/transport_ci_report.py`
- Baseline parity report artifact (committed):
  - `drivers/conformance/reports/nt-s2-004-parity-report-baseline.md`
- CI workflow transport reporting integration:
  - `.github/workflows/drivers-ci.yml`
  - Rust lane uploads `rust-transport-conformance` artifact:
    - `drivers/conformance/reports/rust-transport-outcomes.json`
    - `drivers/conformance/reports/rust-parity-report.md`
  - TypeScript lane uploads `typescript-transport-conformance` artifact:
    - `drivers/conformance/reports/typescript-transport-outcomes.json`
    - `drivers/conformance/reports/typescript-parity-report.md`
  - Python lane uploads `python-transport-conformance` artifact:
    - `drivers/conformance/reports/python-transport-outcomes.json`
    - `drivers/conformance/reports/python-parity-report.md`
- Remote run verification:
  - Workflow run URL: `https://github.com/Pavan-Pvj_ghub/polap-db/actions/runs/24568429948`
  - Status: `failure` before lane step execution
  - Root cause annotation: `GitHub Actions hosted runners are disabled for this repository`
  - Consequence: no transport artifacts available to download from this run
  - Resolution mode: `deferred-for-cloud-validation` (continue local execution; re-run CI artifact collection at final cloud-validation phase)

---

## 5) Execution Plan (S2 Immediate Start)

### Week 1

1. Close open decisions in Section 1.13.
2. Freeze frame envelope + command and error model drafts.
3. Scaffold native listener and feature gate wiring.

### Week 2

1. Land `TransportGateway` skeleton and shared dispatcher path.
2. Add first dual-transport fixtures (`health`, `sql.execute`, one error case).
3. Produce first parity evidence report (HTTP vs native for implemented subset).

---

## 6) Out of Scope for This Document

- Full v2 protocol evolution.
- Language-specific transport implementation details beyond contract hooks.
- Production rollout policy beyond S2 scaffolding.

---

## 7) Change Log

- 2026-04-17: Initial skeleton created and aligned to `NT-S2-001..004`.
- 2026-04-17: Closed v1 protocol defaults for payload model, compression threshold, frame size/fragmentation, and session resume semantics.
- 2026-04-17: Added NT-S2-003 native adapter frame-mapping scaffold and targeted wrapper parity tests (6-pass local evidence).
- 2026-04-17: Added NT-S2-003 native health dispatch roundtrip proof (`NativeAdapter -> CommandDispatcher -> TransportGateway -> NativeFrame::Result`).
- 2026-04-17: Added NT-S2-003 native sql.analyze dispatch roundtrip and missing-payload error-path tests.
- 2026-04-17: Added NT-S2-003 native sql.route dispatch roundtrip and invalid-payload error-path tests.
- 2026-04-17: Added NT-S2-003 native sql.execute route-decision dispatch roundtrip and invalid-payload error-path tests.
- 2026-04-17: Added NT-S2-004 CI transport outcome/parity artifact generation (`transport_ci_report.py`) and workflow uploads for Rust/TS/Python lanes.
- 2026-04-17: Triggered remote `voltnuerongrid-drivers-ci` run and captured runner-policy blocker (`hosted runners disabled`), preventing artifact production.
- 2026-04-17: Marked NT-S2-004 cloud artifact collection as `deferred-for-cloud-validation` per local-first execution decision.
- 2026-04-17: Started NT-S3-001 Rust native driver scaffold with frame codec + HELLO/AUTH handshake helpers and compile-backed unit tests.
- 2026-04-17: Added NT-S3-001.3 mock native transport health COMMAND/RESULT roundtrip with explicit `transportMode=native` opt-in gate and validation tests.
- 2026-04-17: Added first pluggable non-network loopback native transport adapter skeleton (`NativeFrameResponder` + `LoopbackNativeTransport`) to exercise encode/decode boundaries before socket transport implementation.
- 2026-04-17: Added socket-backed native transport stub (`SocketNativeTransport`) with `vng://` endpoint parsing and typed transport errors to prepare NT-S3 socket/codec implementation seam.
- 2026-04-17: Upgraded `SocketNativeTransport` to real TCP connect/send/recv using length-prefixed framed codec and added local loopback TCP roundtrip tests; kept explicit `transportMode=native` opt-in gate.
- 2026-04-17: Added socket failure error-kind mapping (`timeout/refused/reset/interrupted`) into structured driver errors and introduced initial `sql.execute` native COMMAND framing + socket roundtrip helper/tests.
- 2026-04-17: Added `sql.analyze` native COMMAND framing + socket roundtrip helper/tests and introduced shared driver helper for native command execution (`transportMode` gate + RESULT validation) across health/execute/analyze.
- 2026-04-17: Added `sql.route` native COMMAND framing + socket roundtrip helper/tests and expanded shared native execution helper coverage to health/execute/analyze/route.
- 2026-04-17: Consolidated native command execution through one shared helper path (`execute_native_command_roundtrip`) for health/sql.execute/sql.analyze/sql.route before persistent-session handshake/auth layering.
- 2026-04-17: Introduced persistent socket session layer (`PersistentNativeSession`) with HELLO/AUTH bootstrap and multi-command reuse over a single connection; routed health/execute/analyze/route through `*_in_session` command helpers.
- 2026-04-17: Added optional-session reuse wrappers for socket roundtrip helpers so callers can reuse an existing `PersistentNativeSession` (or fallback to one-shot socket execution) for health/sql.execute/sql.analyze/sql.route.
- 2026-04-18: Upgraded runtime native listener beyond accept-only scaffold: length-prefixed JSON frames, HELLO/HelloAck + AUTH/AuthAck (admin key gate when configured), COMMAND dispatch via `NativeAdapter::dispatch_frame` for the S2 command set; dual-transport selector (`resolve_transport_mode` / scheme-based auto) in Rust/TS/Python drivers; VS Code workspace settings for transport injection; CI matrix scaffolding for http vs native lanes (cloud evidence still deferred).
- 2026-04-18: Dual-endpoint auto resolution: `http_fallback_url` / `httpFallbackUrl` with `resolve_auto_transport` / `resolveAutoTransport` + `TransportCapabilities` (native-first); REST builders use `http_rest_base_url` / `httpRestBaseUrl` when `base_url` is `vng://`; optional TCP probe helpers in Rust (`probe_tcp_connect`, `infer_transport_capabilities_tcp`).

