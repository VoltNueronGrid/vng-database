# VoltNueronGrid TypeScript driver

First-party driver for VoltNueronGrid DB (Sprint V3-S2 complete). Responsibilities match `services/voltnuerongridd/reference/driver-core-contract-v1.md`.

## Features

- **Config validation** — `validateConfig` for admin / operator / tenant modes; optional `requestTimeoutMs` (≥ 100) and `maxRetries` (0–20).
- **Request builders** — SQL analyze, route, execute, transaction; `GET /health`; `GET /api/v1/ingest/schema/registry`.
- **Transport selection** — `native` / `http` / `auto`, dual-endpoint resolution, TCP probes, HTTP discovery port (`VNG_HTTP_DISCOVERY_PORT`).
- **HTTP execution hooks** — `performDriverHttpRequest` applies per-attempt timeout, retries on transient HTTP statuses, and maps `AbortSignal` to typed `DriverError` (`timeout` vs `cancelled`).
- **Native wire helpers** — `nativeWire` / `nativeSession` modules for framed JSON over TCP (see tests and `DRIVER_TRANSPORT_LANE` in CI).

## Local usage

```bash
cd drivers/voltnuerongrid-driver-typescript
npm ci
npm test
```

Build artifacts emit to `dist/`. Import from the package entry (`main` / `types` in `package.json`).

## Typical flow (HTTP)

1. Construct `DriverConfig` with `baseUrl` (`http://…` or `vng://…` plus `httpFallbackUrl` when needed).
2. `new VoltNueronGridDriver(config)` and call `buildSqlExecuteRequest(sql, maxRows)` (or other builders).
3. Pass the resulting `DriverRequest` to `performDriverHttpRequest(req, { timeoutMs, maxRetries, abortSignal })` using global `fetch` or inject `fetchFn` for tests.

**Cloud / TLS:** use `https://` in `baseUrl` or `httpFallbackUrl`; TLS is handled by the runtime and the platform fetch implementation.
