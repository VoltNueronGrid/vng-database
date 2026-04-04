---
description: "Use when writing, reviewing, or editing Rust code in this project. Covers crate naming, AppState patterns, endpoint handlers, error responses, and trait conventions."
applyTo: ["crates/**/*.rs", "services/**/*.rs", "drivers/**/*.rs"]
---
# VoltNueronGrid DB — Rust Conventions

## Crate Naming
- Folder names use hyphens: `voltnuerongrid-{name}`
- Rust imports use underscores: `use voltnuerongrid_{name}::TraitName`
- New crates must explicitly `pub use` traits so the service can import them
- Do NOT add logic to the three stub crates (`voltnuerongrid-core`, `voltnuerongrid-failover`, `voltnuerongrid-meta`)

## AppState

- Test helper `state_with_key()` must be updated whenever new fields are added to `AppState`
- Never construct `AppState` directly in tests — always use `state_with_key()`
- Fields backed by `Arc<Mutex<...>>` for shared mutable state; `Arc<RwLock<...>>` for read-heavy catalogs

## Endpoint Handler Pattern

```rust
async fn handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<RequestType>,
) -> impl IntoResponse {
    // 1. Auth check (admin → operator → tenant)
    // 2. Business logic
    // 3. Return typed JSON response
}
```

## HTTP Status Codes
- `401 Unauthorized` — missing/invalid credentials
- `403 Forbidden` — insufficient privilege for the resource
- `404 Not Found` — resource not found
- `409 Conflict` — e.g. deadlock risk, duplicate key
- `408 Request Timeout` — lock wait timeout

## Forbidden Patterns
- Do NOT use `unwrap()` on `Result`/`Option` in handler paths — use `?` or explicit error responses
- Do NOT log header values that may contain API keys or secrets
- Do NOT hardcode KMS key IDs — resolve via `VNG_KMS_*` env vars
- Do NOT `panic!` in handler code; return a `500` response instead

## Test Conventions
- Prefix: `ws{N}_{feature}` (workstream tests), `h{NN}_{feature}` (hardening), `operator_auth_*`, `sql_runtime_*`, `ingest_*`
- Use `#[tokio::test]` for async tests
- Test modules go in `#[cfg(test)]` blocks at the bottom of the file they test
