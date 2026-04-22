# Multi-language Driver Expansion (S10) - Baseline Evidence v1

## S10-001 Java driver baseline

- Workspace: `drivers/voltnuerongrid-driver-java`
- Includes:
  - minimal Java client config/request builder contract
  - Maven build scaffold
  - baseline unit tests for request construction

## S10-002 JavaScript (Node) driver baseline

- Reuses TypeScript package at `drivers/voltnuerongrid-driver-typescript`
- Node baseline acceptance:
  - package compiles to `dist/`
  - Node test runner path remains green
  - HTTP request builders and transport helpers available from package entrypoint

## S10-003 C/C++ FFI strategy + PoC

- Strategy:
  - expose a stable C ABI facade over a Rust implementation layer
  - keep ownership and buffer lifecycle explicit in C API
  - isolate ABI to a small surface (`init`, `execute`, `free`)
- PoC artifact:
  - `drivers/voltnuerongrid-driver-cffi-poc/README.md`

## S10-004 Deno adapter on TS driver

- Adapter module:
  - `drivers/voltnuerongrid-driver-typescript/src/denoAdapter.ts`
- Goal:
  - provide fetch-based execution helper with Deno-compatible runtime assumptions

## S10-005 Perl feasibility report

- Feasibility artifact:
  - `services/voltnuerongridd/reference/perl-binding-feasibility-s10-v1.md`
- Decision posture:
  - proceed only if C ABI remains stable after S10-003

## Cloud Defer Note

All cloud endpoint validation for new drivers is deferred.
This sprint captures local build/test feasibility and interface baselines.
