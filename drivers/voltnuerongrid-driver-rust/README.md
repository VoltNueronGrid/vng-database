# VoltNueronGrid Rust driver

First-party driver crate (through **Sprint V3-S2**). See `services/voltnuerongridd/reference/driver-core-contract-v1.md`.

## Highlights

- **HTTP request builders** — `VoltNueronGridDriver::build_sql_*`, `build_health_request`, `build_schema_registry_request`, autonomous authorize, etc.
- **Retry policy** — `is_retryable_http_status`, `DEFAULT_HTTP_REQUEST_TIMEOUT_MS`, `DEFAULT_HTTP_MAX_RETRIES` (align with TS/Python).
- **Native protocol** — framed JSON over TCP, session pooling helpers, and conformance-driven transport tests (`DRIVER_TRANSPORT_LANE` in CI).

## Build and test

From the workspace root:

```bash
cargo test -p voltnuerongrid-driver-rust
```

TLS for HTTP URLs is the responsibility of the embedding application; this crate focuses on request construction and native wire I/O.
