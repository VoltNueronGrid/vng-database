# Versioned Compatibility Matrix (S11-002) - v1

| Runtime | Rust Driver | TS/Node Driver | Python Driver | Java Driver | VSCode Extension | Notes |
|---|---|---|---|---|---|---|
| `v3.0.0-local-rc1` | `0.1.x` | `0.1.x` | `0.1.x` | `0.1.0-baseline` | `0.1.x` | Local validation baseline for S11 |
| `v3.0.0-local-rc1` | `native/http` | `native/http` | `native/http` | `http (baseline)` | `http` (native in progress) | Transport parity still maturing for non-Rust |

## Notes

- Java driver is baseline-only in this phase.
- Deno adapter is provided on top of TS driver surface.
- Cloud matrix rows are deferred until hosted validation is available.
