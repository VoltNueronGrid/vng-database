# Local Installation Guide — VoltNueronGrid (voltnuerongridd)

**Document:** S11-004  
**Version:** 0.1.0 RC  
**Last updated:** 2026-04-22

---

## Prerequisites

| Tool | Minimum version | Check |
|------|----------------|-------|
| Rust toolchain | 1.75 | `rustc --version` |
| Cargo | 1.75 (ships with Rust) | `cargo --version` |
| Node.js | 20 LTS | `node --version` |
| npm | 10 | `npm --version` |
| Python | 3.11 | `python3 --version` |
| curl | any recent | `curl --version` |

Install Rust via [rustup](https://rustup.rs/):
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

---

## 1. Clone the Repository

```bash
git clone https://github.com/your-org/polap-db.git
cd polap-db
```

---

## 2. Configure Environment

Copy the environment template and edit as needed:

```bash
cp deploy/local/vng.env.example .env
# Edit .env to set VNG_ADMIN_API_KEY and other required values
source .env
```

See [vng.env.example](./vng.env.example) for full documentation of all variables.

---

## 3. Build the Server Binary

```bash
cargo build --release -p voltnuerongridd
```

The output binary is at `target/release/voltnuerongridd`.

To build the entire workspace (all crates and drivers):

```bash
cargo build --release
```

---

## 4. Run the Server

### Minimum configuration (local dev, HTTP only)

```bash
./target/release/voltnuerongridd
```

Default ports:
- HTTP API: `http://localhost:8080`
- Native listener: `vng://localhost:9090` (disabled unless `VNG_NATIVE_ENABLED=true`)

### With native listener enabled

```bash
VNG_NATIVE_ENABLED=true \
VNG_NATIVE_PORT=9090 \
VNG_ADMIN_API_KEY=changeme \
./target/release/voltnuerongridd
```

### With TLS on native listener

```bash
VNG_NATIVE_ENABLED=true \
VNG_NATIVE_TLS_CERT=/path/to/server.crt \
VNG_NATIVE_TLS_KEY=/path/to/server.key \
./target/release/voltnuerongridd
```

### Full environment variable reference

See [vng.env.example](./vng.env.example).

---

## 5. Verify the Server

```bash
curl http://localhost:8080/health
```

Expected response:
```json
{"status":"ok"}
```

Additional smoke checks:
```bash
# SQL analyze
curl -s -X POST http://localhost:8080/api/v1/sql/analyze \
  -H "Content-Type: application/json" \
  -d '{"sql":"SELECT 1"}' | jq .

# Schema registry
curl -s http://localhost:8080/api/v1/ingest/schema/registry | jq .
```

---

## 6. Install the VSCode Extension

### From a .vsix package

1. Build or obtain the `.vsix` file from `ui/ide-extensions/vscode-cursor/`:
   ```bash
   cd ui/ide-extensions/vscode-cursor
   npm ci
   npm run package
   ```
   The `.vsix` file appears in the `ui/ide-extensions/vscode-cursor/` directory.

2. Install in VS Code (or Cursor):
   ```bash
   code --install-extension vscode-cursor-0.3.2.vsix
   ```
   Or via the VS Code UI: **Extensions → ... → Install from VSIX**.

3. Configure in VS Code `settings.json`:
   ```json
   {
     "voltnuerongrid.serverUrl": "http://localhost:8080",
     "voltnuerongrid.transportMode": "auto",
     "voltnuerongrid.nativeUrl": "vng://localhost:9090"
   }
   ```

---

## 7. Quick-Start Script

For an automated setup, use the provided install script:

```bash
bash deploy/local/install.sh
```

The script:
1. Checks all prerequisites.
2. Copies `vng.env.example` to `.env` if not present.
3. Builds the release binary.
4. Starts the server in the background.
5. Prints the health check URL.

---

## 8. Stopping the Server

If started via `install.sh`, find and stop the process:

```bash
pkill -f voltnuerongridd
```

Or use `VNG_PID_FILE` if configured:
```bash
kill $(cat /tmp/voltnuerongridd.pid)
```

---

## 9. Logs

Default log level is `info`. Adjust with `VNG_LOG_LEVEL`:

```bash
VNG_LOG_LEVEL=debug ./target/release/voltnuerongridd
```

For JSON-structured logs (recommended for production):
```bash
VNG_LOG_FORMAT=json ./target/release/voltnuerongridd
```

---

## 10. Troubleshooting

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| `address already in use` on port 8080 | Another process on 8080 | Set `VNG_HTTP_PORT=8081` |
| `rustc not found` | Rust not installed | Install via rustup |
| Health check returns 503 | Server still starting | Wait 2–3 seconds, retry |
| Native listener not responding | `VNG_NATIVE_ENABLED` not set | Set `VNG_NATIVE_ENABLED=true` |

---

## 11. Cloud Deployment

Cloud deployment is documented in [deploy/cloud/README.md](../cloud/README.md).
It is deferred for v0.1.0 RC; the local guide above covers all supported configurations.
