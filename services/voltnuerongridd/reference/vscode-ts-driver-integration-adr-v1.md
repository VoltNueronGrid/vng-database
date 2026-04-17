# ADR: VSCode Integration via TypeScript Driver Adapter (v1)

**Status:** Accepted draft (S0-003)  
**Date:** 2026-04-17  
**Owners:** DX + Driver Platform  
**Related:** `services/voltnuerongridd/reference/driver-core-contract-v1.md`

---

## Context

The VS Code extension currently contains runtime-facing HTTP behavior in extension-side code paths.  
Program v3 requires a driver-first strategy where IDE integrations consume native drivers rather than duplicating transport and auth behavior.

Primary objective: reduce drift and enforce one contract implementation path for request building, validation, and error mapping.

---

## Decision

The VS Code extension will integrate through a TypeScript driver adapter layer.

### Architecture

1. Extension UI/commands call a **Connection Service** (existing extension boundary).
2. Connection Service calls a **TS Driver Adapter** (new boundary inside extension).
3. TS Driver Adapter consumes `voltnuerongrid-driver-typescript`.
4. Driver package owns:
   - config validation
   - header construction
   - request building
   - error normalization to contract kinds

### Mandatory rule

No direct runtime HTTP request construction in extension feature code once migration is complete.

---

## Migration Plan

1. Introduce adapter interface (`DriverClient`) with methods:
   - `health()`
   - `sqlAnalyze()`
   - `sqlRoute()`
   - `sqlExecute()`
   - `sqlTransaction()`
   - `schemaRegistry()`
2. Implement `TsDriverClient` backed by `voltnuerongrid-driver-typescript`.
3. Replace extension direct request builders with adapter usage in phases:
   - connection verification and health
   - schema browse flows
   - query execution flows
4. Keep legacy direct calls behind temporary feature flag for one sprint.
5. Remove feature flag after integration and scenario tests pass.

---

## Consequences

### Positive

- Single source of truth for protocol/auth behavior.
- Better parity across Rust/TS/Python through shared fixtures.
- Cleaner extension code and lower maintenance cost.

### Trade-offs

- Additional adapter abstraction in extension.
- Short-term dual path complexity during migration sprint.

---

## Verification Criteria

1. Extension compiles with adapter enabled by default.
2. Smoke flow passes: create -> connect -> verify -> browse -> query -> disconnect.
3. No direct request-building code remains in extension feature paths.
4. Driver conformance fixtures remain green for TS/Rust/Python.

