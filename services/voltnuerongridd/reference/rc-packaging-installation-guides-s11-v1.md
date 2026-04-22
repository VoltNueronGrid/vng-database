# RC Packaging and Installation Guides (S11-004) - v1

## Runtime Packaging (Local)

1. Build runtime:
   - `cargo build -p voltnuerongridd --release`
2. Package binary and default configs:
   - `target/release/voltnuerongridd`
   - required environment contract references under `services/voltnuerongridd/reference/`
3. Validate launch:
   - start service locally
   - verify `GET /health`

## Driver Packaging (Local)

- Rust driver: crate packaging flow from `drivers/voltnuerongrid-driver-rust`
- TS/Node driver: `npm ci && npm run build` from `drivers/voltnuerongrid-driver-typescript`
- Python driver: package baseline from `drivers/voltnuerongrid-driver-python`
- Java driver baseline: Maven package from `drivers/voltnuerongrid-driver-java`

## Installation Quickstart (Local)

1. Start local runtime on `http://127.0.0.1:8080`.
2. Use one of the driver baselines to call `GET /health`.
3. Execute one SQL analyze and one SQL execute request.
4. Run S8/S9/S10/S11 local gate scripts to collect evidence.

## Cloud Guide Status

Cloud packaging/install guides are defined as deferred validation items in this phase.
They are intentionally excluded from closure criteria for this local-first increment.
