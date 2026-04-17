# Native Listener Config Contract v1 (NT-S2-002 Input)

**Status:** Draft for implementation handoff  
**Linked tasks:** `NT-S2-002`, `NT-S2-003`  
**Related protocol:** `native-protocol-v1.md`

---

## 1) Environment Variables (v1)

| Variable | Type | Default | Required | Description |
|---|---|---|---|---|
| `VNG_NATIVE_LISTENER_ENABLED` | bool | `false` | no | Global gate for native listener startup |
| `VNG_NATIVE_BIND` | string | `127.0.0.1:7542` | no | Host:port bind for native listener |
| `VNG_NATIVE_TLS_ENABLED` | bool | `false` | no | Enable TLS for native listener |
| `VNG_NATIVE_MAX_CONNECTIONS` | integer | `2048` | no | Hard cap on active native connections |
| `VNG_NATIVE_IDLE_TIMEOUT_MS` | integer | `60000` | no | Idle connection timeout |
| `VNG_NATIVE_HANDSHAKE_TIMEOUT_MS` | integer | `5000` | no | Max time for HELLO+AUTH completion |
| `VNG_NATIVE_HEARTBEAT_INTERVAL_MS` | integer | `15000` | no | Server heartbeat interval |
| `VNG_NATIVE_MAX_FRAME_BYTES` | integer | `1048576` | no | Max unfragmented frame payload bytes |
| `VNG_NATIVE_COMPRESSION_ENABLED` | bool | `false` | no | Enables negotiated frame compression |
| `VNG_NATIVE_COMPRESSION_THRESHOLD_BYTES` | integer | `4096` | no | Compression threshold for payload size |

---

## 2) Validation Rules

1. If `VNG_NATIVE_LISTENER_ENABLED=false`, runtime must skip listener startup.
2. `VNG_NATIVE_MAX_CONNECTIONS >= 1`
3. `VNG_NATIVE_IDLE_TIMEOUT_MS >= 1000`
4. `VNG_NATIVE_HANDSHAKE_TIMEOUT_MS >= 100`
5. `VNG_NATIVE_HEARTBEAT_INTERVAL_MS >= 1000`
6. `VNG_NATIVE_MAX_FRAME_BYTES >= 65536`
7. `VNG_NATIVE_COMPRESSION_THRESHOLD_BYTES <= VNG_NATIVE_MAX_FRAME_BYTES`

---

## 3) Startup Behavior Contract

1. Parse native config at runtime boot.
2. Emit one structured startup log with sanitized native config.
3. If native listener bind fails while enabled:
   - return startup error in integration/prod modes
   - allow explicit dev override only if approved later (not part of v1)
4. HTTP listener startup path remains unchanged when native is disabled.

---

## 4) Telemetry Keys (NT-S2-002)

- `native_listener_enabled` (gauge/flag)
- `native_listener_bind_failures_total`
- `native_connections_active`
- `native_handshake_failures_total`
- `native_auth_failures_total`
- `native_commands_total` (tagged by command)
- `native_command_latency_ms` (histogram)

---

## 5) Handoff Checklist

- [ ] Runtime config parser supports all variables in Section 1.
- [ ] Validation rules enforced with explicit startup errors.
- [ ] Startup logs and telemetry keys present.
- [ ] Feature-gated listener lifecycle connected to runtime boot sequence.

