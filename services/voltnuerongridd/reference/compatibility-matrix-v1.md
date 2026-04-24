# Compatibility Matrix v1

**Document:** S11-002  
**Status:** Released (v0.1 baseline)  
**Last updated:** 2026-04-22

---

## 1. Component Version Matrix

| Component | Version | Runtime Min | Runtime Max | Notes |
|-----------|---------|-------------|-------------|-------|
| Rust driver | 0.1.0 | 0.1.0 | * | Native + HTTP transport (dual-transport GA) |
| TypeScript driver | 0.1.0 | 0.1.0 | * | HTTP + Native (S4+ runtime required for native) |
| Python driver | 0.1.0 | 0.1.0 | * | HTTP + Native (S4+ runtime required for native) |
| VSCode extension | 0.3.2 | 0.1.0 | * | transportMode: auto; requires VS Code ≥ 1.85 |
| Java driver | 0.1.0 | 0.1.0 | * | HTTP only (native transport: roadmap, post-v3) |
| Node driver | 0.1.0 | 0.1.0 | * | HTTP only (native: roadmap, post-v3) |
| C/C++ FFI layer | 0.1.0 | 0.1.0 | * | Thin wrapper over Rust driver via FFI |
| Deno adapter | 0.1.0 | 0.1.0 | * | Adapter over TypeScript driver |
| Native protocol | v1 | 0.1.0 | * | Frame format frozen; v2 requires governance vote |
| HTTP API | v1 | 0.1.0 | * | REST at `/api/v1/`; no deprecation before v4 |

`*` = no maximum; forward-compatible until a breaking change governance vote.

---

## 2. Feature Support Matrix per Transport

### 2.1 HTTP Transport

| Feature | Rust | TypeScript | Python | Java | Node | C/C++ FFI | Deno |
|---------|------|-----------|--------|------|------|-----------|------|
| health check | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| sql.execute | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| sql.analyze | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| sql.route | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| sql.transaction | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| ingest.schema.registry | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| object history | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| admin server-status | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| bearer token auth | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| TLS | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

### 2.2 Native Transport (`vng://`)

| Feature | Rust | TypeScript | Python | Java | Node | C/C++ FFI | Deno |
|---------|------|-----------|--------|------|------|-----------|------|
| health check | ✅ | ✅ | ✅ | ⏳ | ⏳ | ✅ (via Rust) | ✅ (via TS) |
| sql.execute | ✅ | ✅ | ✅ | ⏳ | ⏳ | ✅ (via Rust) | ✅ (via TS) |
| sql.analyze | ✅ | ✅ | ✅ | ⏳ | ⏳ | ✅ (via Rust) | ✅ (via TS) |
| sql.route | ✅ | ✅ | ✅ | ⏳ | ⏳ | ✅ (via Rust) | ✅ (via TS) |
| sql.transaction | ✅ | ✅ | ✅ | ⏳ | ⏳ | ✅ (via Rust) | ✅ (via TS) |
| ingest.schema.registry | ✅ | ✅ | ✅ | ⏳ | ⏳ | ✅ (via Rust) | ✅ (via TS) |
| object history | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| admin server-status | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| bearer token auth | ✅ | ✅ | ✅ | ⏳ | ⏳ | ✅ (via Rust) | ✅ (via TS) |
| mTLS | ✅ | ✅ | ✅ | ⏳ | ⏳ | ✅ (via Rust) | ✅ (via TS) |

**Legend:**
- ✅ Implemented and tested
- ⏳ Planned (roadmap item, not yet implemented)
- ❌ Will not implement (see Known Incompatibilities)

---

## 3. Dual-Transport Auto-Resolution

When `DriverConfig.base_url` uses the `vng://` scheme and `http_fallback_url` is set,
the Rust driver (and TS/Python wrappers) support `DriverTransportMode::Auto`:

1. Probe native endpoint (`vng://host:port`).
2. On failure, fall back to HTTP (`http_fallback_url`).
3. Cache transport selection for the session lifetime.

Java and Node drivers do **not** support auto-resolution in v0.1.0; they use HTTP only.

---

## 4. Known Incompatibilities

| Incompatibility | Affected Components | Notes |
|-----------------|--------------------|----|
| Native protocol v1 is TCP-only; no UDP or QUIC | All native drivers | QUIC transport is post-v3 roadmap |
| `object history` endpoint not available on native transport | All drivers | Admin plane stays HTTP; no timeline for native |
| `admin server-status` not available on native transport | All drivers | By design; admin plane is HTTP-only |
| Java/Node native transport not implemented | Java driver, Node driver | Planned post-v3; HTTP is fully supported |
| Perl FFI has no native transport | Perl driver | Via C/C++ FFI layer only; native is roadmap |
| Runtime versions below S4 do not support TS/Python native | Runtime | Upgrade runtime to ≥ v0.1.0 S4 baseline |

---

## 5. Upgrade Path Notes

### 0.0.x → 0.1.0 (current)
- All drivers: update `base_url` scheme; `http://` URLs continue to work unchanged.
- Rust driver: `http_fallback_url` is new; set it if using `vng://` with REST fallback.
- VSCode extension: set `transportMode: "auto"` in settings for dual-transport behaviour.
- No wire-format breaking changes in native protocol v1.

### 0.1.x → future 0.2.x
- Native protocol v2 (if approved by governance) will be negotiated during handshake.
- Drivers will remain backward-compatible with v1 runtime for at least one minor release cycle.
- Java/Node native transport additions will be additive; existing HTTP configs unaffected.

---

## 6. Tested Runtime Environments

| Environment | Version | Status |
|-------------|---------|--------|
| macOS 14 (Apple Silicon) | Rust 1.77+ | Tested |
| Ubuntu 22.04 LTS | Rust 1.75+ | Tested |
| Windows 11 (x64) | Rust 1.77+ | Tested |
| Node 20 LTS | npm 10 | Tested (Node driver + TS driver) |
| Python 3.11 | pip 23+ | Tested (Python driver) |
| JDK 21 LTS | Maven 3.9 | Tested (Java driver) |
| VS Code 1.85+ | Extension host | Tested |
