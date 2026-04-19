# VoltNueronGrid Python driver

First-party driver (through **Sprint V3-S2**). Matches `services/voltnuerongridd/reference/driver-core-contract-v1.md`.

## Capabilities

- **Config validation** — modes and headers; `request_timeout_ms` must be ≥ 100; `max_retries` in 0..20.
- **Request builders** — `VoltNueronGridDriver`: SQL analyze, route, execute, transaction; `GET /health`; `GET /api/v1/ingest/schema/registry`.
- **Transport** — `resolve_auto_transport`, TCP probes, optional HTTP discovery port (`VNG_HTTP_DISCOVERY_PORT`).
- **HTTP execution** — `perform_driver_http_request(request, config)` uses `urllib` with per-attempt timeout, retries on transient statuses (`is_retryable_http_status`), and `DriverError` for failures.
- **Native wire** — `native_wire` / `native_session` helpers for framed JSON over TCP.

## Local install and tests

```bash
cd drivers/voltnuerongrid-driver-python
python -m pip install -e .
python -m unittest discover -s tests
```

**Cloud / TLS:** pass `https://` URLs in `base_url` or `http_fallback_url`; TLS is enforced by the Python SSL stack when using HTTPS URLs.

