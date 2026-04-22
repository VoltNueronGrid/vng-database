# Perl Binding Feasibility Report (S10-005) - v1

## Summary

Perl binding is feasible via FFI once the C ABI surface is stabilized.
Current recommendation: defer implementation until after C/C++ PoC interface freeze.

## Options Considered

1. `FFI::Platypus` over shared library exported from Rust C ABI
2. XS native extension wrapping C ABI
3. Pure HTTP client (no native FFI)

## Recommended Path

- Phase 1: `FFI::Platypus` against stable C ABI for fastest iteration.
- Phase 2: evaluate XS only if performance or distribution constraints require it.

## Risks

- ABI churn causes downstream breakage.
- Cross-platform packaging burden (macOS/Linux/Windows).
- Memory ownership bugs if allocation/free contract is unclear.

## Acceptance for Feasibility

- C ABI docs and sample headers exist.
- One end-to-end request/response path can be called from Perl using local test harness.

## Cloud Defer Note

Cloud-hosted validation for Perl binding is deferred.
Local feasibility is the acceptance source for S10-005.
