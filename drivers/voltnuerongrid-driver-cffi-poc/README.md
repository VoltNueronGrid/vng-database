# VoltNueronGrid C/C++ FFI PoC (S10-003)

This folder captures the strategy-level PoC boundary for a C ABI facade over the Rust driver.

## Proposed ABI surface (v0)

- `vng_driver_init(config_json)`
- `vng_driver_execute(request_json)`
- `vng_driver_free_string(ptr)`

## PoC goals

1. Keep ABI narrow and stable.
2. Keep ownership explicit (caller frees returned buffers via exported free function).
3. Avoid exposing Rust internals directly across the ABI boundary.

## Acceptance

- Strategy documented for C and C++ consumers.
- Perl feasibility can build on this ABI direction.

Cloud execution validation is deferred for this phase.
