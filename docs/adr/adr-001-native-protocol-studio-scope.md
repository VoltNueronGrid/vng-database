# ADR-001: Native Protocol Is Driver-Only — Studio Uses HTTP Exclusively

**Status**: Accepted  
**Date**: 2026-06-26  
**Deciders**: VoltNueronGrid Architecture Team  
**Related Tasks**: tasks-v4.md C3, P9  
**Closes**: Architecture Physical View gap — "Native protocol validation path in Studio is unclear"

---

## Context

VoltNueronGrid DB exposes two client connectivity surfaces:
1. **HTTP API** (`/api/v1/…`) — accessible from any HTTP client, including browser fetch
2. **Native TCP protocol** — a binary framing protocol for high-performance language driver connectivity

The architecture physical view (`.specify/memory/architecture-physical-view.md`) carried an open gap:
> "Native protocol validation path in Studio is unclear — blocks coherent physical client behavior for native connections."

Studio is a browser-based management console (`ui/voltnuerongrid-studio/`). Browser JavaScript runs in a sandboxed environment and cannot open raw TCP sockets. The `fetch()` API can only connect to HTTP/HTTPS endpoints. Therefore:

- Studio **cannot** reach the native TCP listener from a browser context
- There is no "Tauri bridge" or desktop wrapper in the current Studio implementation
- Any native protocol option shown in Studio's connection form would be non-functional in a standard browser

---

## Decision

**Studio uses HTTP exclusively for all server communication.**

The native TCP protocol is **driver-only** — it is intended for:
- Rust driver (`drivers/voltnuerongrid-driver-rust/`)
- TypeScript/Node driver (`drivers/voltnuerongrid-driver-typescript/`)
- Python driver (`drivers/voltnuerongrid-driver-python/`)
- Java, Perl, Deno, and other language drivers

Studio's connection form should:
1. Show **HTTP** as the only connection protocol option
2. Remove or hide the native protocol option from the connection UI
3. Display an informational tooltip if the native protocol option is shown: *"Native TCP protocol is available in language drivers only — not accessible from browser-based Studio."*

---

## Consequences

### Positive

- Eliminates the physical view gap "native protocol validation path in Studio is unclear"
- Prevents user confusion from non-functional connection options
- Simplifies Studio's connection state machine (no TCP socket management needed)
- Driver conformance gate (`tests/kpi/scripts/run-driver-conformance-gate.ps1`) is the correct evidence artifact for native protocol validation

### Negative

- Studio users who need native protocol performance must use a language driver directly
- A future desktop-native Studio (e.g., Tauri-based) would need this ADR revisited

### Neutral

- No server-side changes required
- Native protocol server implementation continues as planned (P9)
- Driver conformance testing covers native protocol correctness independently

---

## Scope Boundary

| Surface | Protocol | Rationale |
|---------|----------|-----------|
| Studio (browser) | HTTP only | Browser security model prevents raw TCP |
| Rust driver | HTTP or Native TCP | Configurable via `TransportKind` enum |
| Python driver | HTTP or Native TCP | Configurable at connection construction |
| TypeScript driver | HTTP or Native TCP | Configurable via transport option |
| Java driver | HTTP or Native TCP | Configurable at data source level |
| MCP tools | HTTP only | MCP protocol uses JSON-RPC over HTTP/SSE |

---

## Evidence

- Architecture physical view gap: `.specify/memory/architecture-physical-view.md` — marked closed
- Driver conformance gate artifact: `tests/kpi/results/ws10/driver-conformance-gate.json` — status: passed
- ADR location: `docs/adr/adr-001-native-protocol-studio-scope.md`

---

## References

- tasks-v4.md C3: "No Gate or Scope Boundary for Native Protocol Studio Validation"
- tasks-v4.md P9: "Native Driver Conformance Gate" (separate evidence for driver-side native protocol)
- Constitution Principle V: "Native drivers for prioritized languages MUST not be replaced by thin API-only stories when the requirement calls for native connectivity."
- `.specify/memory/architecture-physical-view.md` — Physical View gap for native protocol Studio
