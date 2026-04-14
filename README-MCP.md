# VoltNueronGrid MCP Guide

## 1. Overview

This document is the comprehensive guide for Model Context Protocol (MCP) usage in VoltNueronGrid.

It covers:
- What MCP is and why it matters for this project
- What is implemented right now in this repository
- Security and auth behavior
- Tool schemas and expected responses
- Local setup and validation
- Client configuration and run steps for:
  - VSCode
  - Cursor
  - Claude Desktop
  - CLI workflows
- End-to-end examples and troubleshooting

---

## 2. What MCP Is

MCP (Model Context Protocol) is a standard way to expose structured tools, resources, and actions to AI clients.

In VoltNueronGrid, MCP is used to expose safe operational capabilities of the database plane to agent clients:
- Query execution (read-only)
- Schema inspection
- Health inspection
- Controlled benchmark execution

MCP allows clients to call these capabilities with strict permission boundaries and predictable JSON-RPC payloads.

---

## 3. Current Repository State

### 3.1 Implemented MCP crate

The MCP implementation is in the crate:
- `crates/voltnuerongrid-mcp`

Main modules:
- `src/lib.rs` - MCP request/response envelope + router
- `src/auth.rs` - auth context and permission checks
- `src/tools.rs` - tool request/response schemas
- `src/guardrails.rs` - safety checks for queries and limits
- `src/integration.rs` - integration-facing helper layer

### 3.2 Test status

Validated in this repository:
- Unit tests: 28 passing
- Integration tests: 12 passing
- Total: 40 passing

### 3.3 Important note about transport

This repository currently provides a production-grade MCP core library. Depending on your environment, you can run it through:
- A host process that maps stdio MCP traffic into this crate
- A service route adapter that forwards JSON-RPC requests to this crate

If you already have an MCP host wrapper in your environment, use the client configuration sections in this document as-is.

---

## 4. Security and RBAC Model

VoltNueronGrid enforces auth order for protected surfaces:
1. Admin gate
2. Operator gate
3. Tenant gate

### 4.1 Auth headers

Admin:
- `x-vng-admin-key`

Operator:
- `x-vng-operator-id`

Tenant:
- `x-vng-tenant-id`
- `x-vng-user-id`

### 4.2 Response semantics

- `401`: missing or invalid credentials
- `403`: credentials present but insufficient privilege

### 4.3 Permission intent by tool

- Query tool: operator or admin
- Schema tool: operator or admin
- Health tool: operator or admin
- Benchmark tool: admin only
- DDL create tool: admin + additional DDL key
- DDL drop tool: admin + additional DDL key
- ERD tool: operator or admin
- Data transfer tool (import/export): admin + additional transfer key

### 4.4 Additional key gates for dangerous operations

In addition to standard admin header auth (`x-vng-admin-key`), sensitive write and transfer operations require a second explicit key in the request body:
- `tools/ddl_create` and `tools/ddl_drop` require `ddl_admin_key`
- `tools/data_transfer` requires `transfer_admin_key`

Optional environment hardening:
- `VNG_MCP_DDL_KEY` (if set, must match `ddl_admin_key`)
- `VNG_MCP_TRANSFER_KEY` (if set, must match `transfer_admin_key`)

### 4.5 Guardrails

Query guardrails enforce:
- Max query payload size
- Max result size estimate
- Timeout upper bound
- Prohibited mutating keywords
- Suspicious pattern rejection (including multi-statement style abuse)

---

## 5. MCP Tool Catalog

## 5.1 tools/query

Purpose:
- Execute read-only SQL queries in a controlled way

Request shape:
```json
{
  "sql_query": "SELECT id, name FROM users LIMIT 10",
  "timeout_ms": 5000,
  "tenant_id": "tenant-a",
  "max_rows": 10
}
```

Typical response:
```json
{
  "columns": ["id", "name"],
  "rows": [["1", "alice"], ["2", "bob"]],
  "execution_time_ms": 14,
  "rowcount": 2
}
```

Rejected example (mutating SQL):
```json
{
  "error": {
    "code": 403,
    "message": "Guardrail violation: Query contains prohibited keywords: DELETE"
  }
}
```

## 5.2 tools/schema

Purpose:
- Inspect tables, columns, and indexes

Request shape:
```json
{
  "schema_filter": "public",
  "table_filter": "users"
}
```

Typical response:
```json
{
  "tables": [
    {
      "name": "users",
      "columns": [
        {"name": "id", "data_type": "INT", "nullable": false, "primary_key": true},
        {"name": "name", "data_type": "TEXT", "nullable": false, "primary_key": false}
      ],
      "indexes": [
        {"name": "idx_users_name", "columns": ["name"], "unique": false}
      ]
    }
  ],
  "total_tables": 1
}
```

## 5.3 tools/health

Purpose:
- Return runtime service health and related metrics

Request shape:
```json
{
  "detailed": true
}
```

Typical response:
```json
{
  "status": "healthy",
  "version": "0.1.0",
  "uptime_ms": 123456,
  "node_count": 1,
  "replication_lag_ms": 0,
  "detailed_metrics": {
    "active_connections": 8,
    "memory_usage_mb": 256,
    "disk_usage_mb": 1024,
    "query_cache_hit_rate": 0.93,
    "transactions_per_sec": 412.7
  }
}
```

## 5.4 tools/benchmark

Purpose:
- Run controlled benchmark operations

Request shape:
```json
{
  "benchmark_name": "query_select_all",
  "params": {"dataset": "small"},
  "iterations": 100
}
```

## 5.5 tools/ddl_create

Purpose:
- Create database objects (table, view, function, index, materialized_view, schema)

Request shape:
```json
{
  "object_type": "table",
  "object_name": "users",
  "create_sql": "CREATE TABLE users(id INT PRIMARY KEY)",
  "ddl_admin_key": "extra-ddl-key"
}
```

Auth:
- Admin header required
- Additional `ddl_admin_key` required

## 5.6 tools/ddl_drop

Purpose:
- Drop database objects with explicit second-factor key

Request shape:
```json
{
  "object_type": "view",
  "object_name": "v_users",
  "drop_sql": "DROP VIEW v_users",
  "ddl_admin_key": "extra-ddl-key"
}
```

## 5.7 tools/erd

Purpose:
- Generate ERD diagram text for given schema/tables

Request shape:
```json
{
  "schema_filter": "public",
  "table_names": ["users", "orders"],
  "output_format": "mermaid"
}
```

Typical response:
```json
{
  "format": "mermaid",
  "diagram": "erDiagram\\n    USERS ||--o{ ORDERS : places"
}
```

## 5.8 tools/data_transfer

Purpose:
- Import/export table data from/to CSV or Parquet via Blob/WebDAV/FTP/local file endpoint

Request shape:
```json
{
  "direction": "import",
  "format": "csv",
  "endpoint": "blob",
  "location": "blob://sample/container/users.csv",
  "table_name": "users",
  "transfer_admin_key": "xfer-key",
  "options": {"delimiter": ","}
}
```

Auth:
- Admin header required
- Additional `transfer_admin_key` required

Typical response:
```json
{
  "name": "query_select_all",
  "duration_ms": 1000,
  "ops_per_sec": 1000.0,
  "latency_p50_ms": 0.5,
  "latency_p99_ms": 3.7,
  "latency_p999_ms": 9.1,
  "throughput_bytes_per_sec": 2097152
}
```

Privilege failure response for non-admin:
```json
{
  "error": {
    "code": 403,
    "message": "Authentication failed: Insufficient privileges for this operation"
  }
}
```

---

## 6. JSON-RPC Envelope

MCP requests in this implementation use JSON-RPC 2.0 framing.

Request:
```json
{
  "jsonrpc": "2.0",
  "id": "req-001",
  "method": "tools/query",
  "params": {
    "sql_query": "SELECT 1"
  },
  "headers": {
    "x_vng_operator_id": "operator-1"
  }
}
```

Success response:
```json
{
  "jsonrpc": "2.0",
  "id": "req-001",
  "result": {
    "columns": ["value"],
    "rows": [[1]],
    "execution_time_ms": 1,
    "rowcount": 1
  },
  "error": null
}
```

Error response:
```json
{
  "jsonrpc": "2.0",
  "id": "req-001",
  "result": null,
  "error": {
    "code": 401,
    "message": "Authentication failed: Missing required authentication header",
    "data": null
  }
}
```

---

## 7. Local Build, Validate, and Test

## 7.1 Prerequisites

- Rust toolchain installed
- Cargo available on PATH
- PowerShell for Windows command examples
- Admin key for protected operations where needed

## 7.2 Build and test commands

From repository root:

```powershell
cargo check -p voltnuerongrid-mcp
cargo test -p voltnuerongrid-mcp
cargo build -p voltnuerongrid-mcp --release
```

## 7.3 Expected test summary

- 28 unit tests passing
- 12 integration tests passing
- No failures

---

## 8. Running With a Host Adapter

Most MCP clients expect a stdio MCP server command.

If your environment already includes an MCP host adapter process that calls this crate, configure that command in your client.

If not, use one of these patterns:

- Pattern A: internal service route adapter (HTTP endpoint in your service)
- Pattern B: lightweight stdio wrapper binary in your local environment that forwards calls to this crate logic

This guide includes client configuration templates for both stdio and HTTP-bridge style usage.

---

## 9. VSCode Configuration

VSCode MCP support can vary by extension version. Use the schema expected by your installed MCP-capable extension.

Common pattern is a `mcpServers` object with command + args.

### 9.1 Using the stdio server binary

The recommended approach is to use the `mcp-stdio-server` binary included in this repository.

Option A: **Direct cargo run** (development):
```json
{
  "mcpServers": {
    "voltnuerongrid": {
      "command": "cargo",
      "args": [
        "run",
        "-p",
        "voltnuerongrid-mcp",
        "--bin",
        "mcp-stdio-server",
        "--release"
      ],
      "env": {
        "VNG_ADMIN_API_KEY": "your-admin-key-here",
        "VNG_OPERATOR_ID": "operator-1"
      }
    }
  }
}
```

Option B: **Pre-built binary** (production):
After `cargo build -p voltnuerongrid-mcp --bin mcp-stdio-server --release`, the binary is at:
- Windows: `target/release/mcp-stdio-server.exe`
- Linux/macOS: `target/release/mcp-stdio-server`

```json
{
  "mcpServers": {
    "voltnuerongrid": {
      "command": "/path/to/target/release/mcp-stdio-server",
      "args": [],
      "env": {
        "VNG_ADMIN_API_KEY": "your-admin-key-here",
        "VNG_OPERATOR_ID": "operator-1"
      }
    }
  }
}
```

### 9.2 Suggested VSCode setup steps

1. Clone or navigate to the repository
2. Run `cargo build -p voltnuerongrid-mcp --bin mcp-stdio-server --release` once
3. Open VSCode settings JSON (Cmd/Ctrl+Shift+P → "Open User Settings (JSON)")
4. Find or create `mcpServers` section
5. Add the configuration above (use Option A for development, Option B for pre-built)
6. Set environment variables appropriate to your auth level
7. Reload VSCode window
8. Open MCP tools panel
9. Verify tool discovery shows: query, schema, health, benchmark

### 9.3 Validation sequence in VSCode

1. `tools/health` with operator/admin auth
2. `tools/schema` with operator auth
3. `tools/query` with a simple read-only query
4. `tools/benchmark` using admin key

For containerized and hosted deployment patterns, see Section 11.3.

---

## 10. Cursor Configuration

Cursor commonly uses an MCP config file with `mcpServers`.

### 10.1 Using the stdio server binary

After building the server, reference it in your Cursor MCP config:

```json
{
  "mcpServers": {
    "voltnuerongrid": {
      "command": "cargo",
      "args": [
        "run",
        "-p",
        "voltnuerongrid-mcp",
        "--bin",
        "mcp-stdio-server",
        "--release"
      ],
      "env": {
        "VNG_ADMIN_API_KEY": "your-admin-key-here",
        "VNG_OPERATOR_ID": "operator-1"
      }
    }
  }
}
```

Or with a pre-built binary:
```json
{
  "mcpServers": {
    "voltnuerongrid": {
      "command": "/path/to/target/release/mcp-stdio-server",
      "env": {
        "VNG_ADMIN_API_KEY": "your-admin-key-here",
        "VNG_OPERATOR_ID": "operator-1"
      }
    }
  }
}
```

### 10.2 Suggested steps in Cursor

1. Navigate to Cursor settings
2. Find or create MCP configuration section
3. Add server entry named `voltnuerongrid` using config above
4. Save and restart Cursor
5. Confirm listed tools: query, schema, health, benchmark
6. Run smoke flow: health -> schema -> query

For containerized and hosted deployment patterns, see Section 11.3.

Cursor example prompts once connected:
- "Call tools/health and summarize node status"
- "List tables using tools/schema"
- "Run tools/query with SELECT COUNT(*) FROM users"

---

## 11. Claude Desktop Configuration

Claude Desktop MCP config typically uses `mcpServers` and command execution.

### 11.1 Configuration with stdio server

Create or edit `claude_desktop_config.json` in your Claude Desktop config directory:
- **macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`
- **Windows**: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "voltnuerongrid": {
      "command": "cargo",
      "args": [
        "run",
        "-p",
        "voltnuerongrid-mcp",
        "--bin",
        "mcp-stdio-server",
        "--release"
      ],
      "env": {
        "VNG_ADMIN_API_KEY": "your-admin-key-here",
        "VNG_OPERATOR_ID": "operator-1"
      }
    }
  }
}
```

Or with pre-built binary:
```json
{
  "mcpServers": {
    "voltnuerongrid": {
      "command": "/path/to/target/release/mcp-stdio-server",
      "env": {
        "VNG_ADMIN_API_KEY": "your-admin-key-here",
        "VNG_OPERATOR_ID": "operator-1"
      }
    }
  }
}
```

### 11.2 Bring-up steps

1. Build: `cargo build -p voltnuerongrid-mcp --bin mcp-stdio-server --release`
2. Edit `claude_desktop_config.json` using config above
3. Restart Claude Desktop
4. Verify server appears connected in Claude's MCP panel
5. Start with health tool call: "Call tools/health"
6. Progress to schema: "Call tools/schema"
7. Try query: "Call tools/query with SELECT 1"
8. Test admin boundary: "Call tools/benchmark" (should confirm restricted for non-admin)

### 11.3 Deployment Topologies: Docker Local, Docker Cloud, Hosted MCP, Cloud DB

Use this section when your MCP process is not running directly on your laptop.

#### Case A: Docker container on local machine (recommended for local dev)

Run MCP server in a local container and let the IDE start it.

VSCode/Cursor config example:
```json
{
  "mcpServers": {
    "voltnuerongrid": {
      "command": "docker",
      "args": [
        "run",
        "--rm",
        "-i",
        "--name",
        "vng-mcp",
        "-e",
        "VNG_ADMIN_API_KEY=your-admin-key-here",
        "-e",
        "VNG_OPERATOR_ID=operator-1",
        "ghcr.io/pavan-pvj_ghub/voltnuerongrid-mcp:latest"
      ]
    }
  }
}
```

Notes:
- `-i` is required for stdio MCP.
- Image entrypoint must start `mcp-stdio-server`.
- If DB is another local container, connect through Docker network and set DB env vars in the MCP container.

#### Case B: Docker container hosted on cloud VM/Kubernetes, IDE runs locally

Best pattern is local stdio + remote HTTP bridge:
1. Keep IDE MCP config using a local command.
2. Local command runs a bridge script that forwards JSON-RPC to cloud MCP endpoint over HTTPS.
3. Cloud endpoint routes to MCP container.

VSCode/Cursor config example:
```json
{
  "mcpServers": {
    "voltnuerongrid": {
      "command": "pwsh",
      "args": [
        "-NoProfile",
        "-File",
        "./mcp-config-pack/bridge/stdio-to-http.ps1"
      ],
      "env": {
        "MCP_REMOTE_URL": "https://mcp.your-domain.com/api/v1/mcp",
        "MCP_REMOTE_TOKEN": "replace-with-token"
      }
    }
  }
}
```

Bridge behavior:
- Read one JSON-RPC request from stdin.
- POST to `MCP_REMOTE_URL` with auth header.
- Write JSON-RPC response to stdout.

#### Case C: Fully hosted MCP server (managed service)

If your IDE extension supports URL-based MCP servers, use direct hosted URL:
```json
{
  "mcpServers": {
    "voltnuerongrid": {
      "url": "https://mcp.your-domain.com/mcp",
      "headers": {
        "Authorization": "Bearer <token>",
        "x-vng-operator-id": "operator-1"
      }
    }
  }
}
```

If your IDE extension only supports command-based MCP, use Case B with local bridge script.

#### Case D: DB runs in cloud, MCP runs local

You can run MCP locally and point it to cloud DB connection settings.

VSCode/Cursor config example:
```json
{
  "mcpServers": {
    "voltnuerongrid": {
      "command": "cargo",
      "args": [
        "run",
        "-p",
        "voltnuerongrid-mcp",
        "--bin",
        "mcp-stdio-server",
        "--release"
      ],
      "env": {
        "VNG_ADMIN_API_KEY": "your-admin-key-here",
        "VNG_OPERATOR_ID": "operator-1",
        "VNG_DB_URL": "postgres://user:pass@cloud-db.example.com:5432/vng",
        "VNG_DB_SSLMODE": "require"
      }
    }
  }
}
```

Security notes for cloud DB:
- Enforce TLS (`require` or stronger).
- Restrict DB inbound IPs to MCP host(s).
- Use least-privilege DB role for MCP query path.
- Keep benchmark admin-only.

#### Connectivity validation for all topologies

After configuring any topology:
1. Start/restart IDE.
2. Call `tools/health`.
3. Call `tools/schema`.
4. Call `tools/query` with `SELECT 1`.
5. Confirm non-admin `tools/benchmark` returns `403`.
6. Confirm admin `tools/benchmark` succeeds.

---

## 12. CLI Workflows

You can validate MCP payload behavior from CLI using JSON-RPC-like bodies through stdin/stdout.

### 12.1 Testing with the stdio server directly

Start the server in one terminal:
```powershell
cargo run -p voltnuerongrid-mcp --bin mcp-stdio-server --release
```

In another terminal, send JSON-RPC requests to stdin:
```powershell
# PowerShell
@{
  jsonrpc = "2.0"
  id = "cli-test-1"
  method = "tools/health"
  params = @{ detailed = $true }
  headers = @{ x_vng_operator_id = "operator-1" }
} | ConvertTo-Json -Depth 10 | &amp; { $input | cargo run -p voltnuerongrid-mcp --bin mcp-stdio-server --release }
```

Or create a test input file and pipe it:
```powershell
$body = @{
  jsonrpc = "2.0"
  id = "cli-1"
  method = "tools/health"
  params = @{ detailed = $true }
  headers = @{ x_vng_operator_id = "operator-1" }
} | ConvertTo-Json -Depth 10

$body | echo $_ | cargo run -p voltnuerongrid-mcp --bin mcp-stdio-server --release
```

### 12.2 PowerShell example (via stdin to stdio server)

Save this to `test-query.ps1`:
```powershell
$body = @{
  jsonrpc = "2.0"
  id = "pw-001"
  method = "tools/query"
  params = @{ sql_query = "SELECT 1"; timeout_ms = 5000 }
  headers = @{ x_vng_operator_id = "operator-1" }
} | ConvertTo-Json -Depth 10

# Run server and pipe request
$process = Start-Process -FilePath "cargo" -ArgumentList ('run', '-p', 'voltnuerongrid-mcp', '--bin', 'mcp-stdio-server', '--release') -NoNewWindow -PassThru
$process.StandardInput.WriteLine($body)
```

### 12.3 Alternative: HTTP adapter bridge

If you prefer HTTP to stdio, create a wrapper that runs the stdio server and forwards HTTP requests:

**12.3.1 Python requests example** (to HTTP adapter):

```python
import requests

payload = {
    "jsonrpc": "2.0",
    "id": "py-1",
    "method": "tools/schema",
    "params": {"schema_filter": "public"},
    "headers": {"x_vng_operator_id": "operator-1"},
}

# Assumes HTTP adapter is listening on localhost:8080
resp = requests.post("http://127.0.0.1:8080/api/v1/mcp", json=payload, timeout=10)
print(resp.status_code)
print(resp.json())
```

**12.3.2 curl example** (to HTTP adapter):

```bash
curl -X POST http://127.0.0.1:8080/api/v1/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "id":"curl-001",
    "method":"tools/query",
    "params":{"sql_query":"SELECT 1","timeout_ms":1000},
    "headers":{"x_vng_operator_id":"operator-1"}
  }'
```

**12.3.3 Node.js example** (to HTTP adapter):

```javascript
const payload = {
  jsonrpc: "2.0",
  id: "node-1",
  method: "tools/benchmark",
  params: {
    benchmark_name: "query_select_all",
    params: { dataset: "small" },
    iterations: 50
  },
  headers: {
    x_vng_admin_key: "secret"
  }
};

// Assumes HTTP adapter is listening on localhost:8080
fetch("http://127.0.0.1:8080/api/v1/mcp", {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify(payload)
})
  .then(r => r.json())
  .then(data => {
    console.log("Response:", data);
    if (data.error) {
      console.log("Error code:", data.error.code, "Message:", data.error.message);
    } else {
      console.log("Result:", data.result);
    }
  })
  .catch(err => console.error("Request failed:", err));
```

---

## 13. End-to-End Example Flows

## 13.1 Connectivity smoke flow (stdio server)

1. Start server: `cargo run -p voltnuerongrid-mcp --bin mcp-stdio-server --release`
2. Send JSON-RPC request for `tools/health` with operator header
3. Receive response: `status` is `healthy`
4. Send `tools/schema` request
5. Receive schema with tables list
6. Send `tools/query` with `SELECT 1`
7. Receive rows result

## 13.2 Auth boundary flow (stdio server)

1. Send `tools/benchmark` with operator header
2. Expect error response with code `403`
3. Repeat with admin header
4. Expect success response with benchmark metrics

## 13.3 Guardrail flow (stdio server)

1. Send `tools/query` with SQL: `DELETE FROM users`
2. Expect guardrail rejection (error code `403`)
3. Send `tools/query` with pure `SELECT 1`
4. Expect success

## 13.4 Tenant isolation flow (stdio server)

1. Authenticate as tenant A
2. Query tenant-scoped data
3. Attempt tenant B scoped read
4. Expect rejection (error code `403` or `401`)

---

## 14. Troubleshooting

**14.1 Server not discovered in client**

Check:
- MCP config file is valid JSON
- Command path resolves in shell
- Restart client after config update
- Env vars are set in MCP server config
- (For pre-built binary) binary exists at specified path

**14.2 "command not found" or "cannot run program"**

Check:
- Cargo is installed: `cargo --version`
- You're in the repository directory with Cargo.toml
- Binary built with: `cargo build -p voltnuerongrid-mcp --bin mcp-stdio-server --release`
- Pre-built path is correct: `./target/release/mcp-stdio-server` (Windows) or `./target/release/mcp-stdio-server` (Linux/macOS)

**14.3 401 errors**

Check:
- Required auth headers exist in JSON-RPC `headers` field
- Header names exactly match expected schema (underscores, not hyphens)
- Admin key is present when using admin operations
- Operator ID is set for operator-level operations

**14.4 403 errors**

Check:
- Principal has enough privilege for requested tool
- Benchmark uses admin key
- Query does not violate guardrail policy
- Tenant ID matches user's permitted scope

**14.5 Invalid request errors**

Check:
- `jsonrpc` must be `"2.0"` (string)
- `method` must match a known tool: `tools/query`, `tools/schema`, `tools/health`, `tools/benchmark`
- `params` object must match tool schema
- `id` is a string

**14.6 Guardrail blocks legitimate query**

Check:
- Query is single-statement (not multiple with `;`)
- No mutating keywords: `DELETE`, `INSERT`, `UPDATE`, `DROP`, `ALTER`, `TRUNCATE`
- Timeout does not exceed max (typically 30000ms)
- Request size is below configured limits

---

## 15. Hardening Recommendations

- Keep benchmark tool admin-only
- Enforce strict result size limits in guardrails
- Log audit metadata without sensitive header values
- Add rate limiting around benchmark and expensive query patterns
- Maintain explicit deny list for mutating SQL operations in MCP query path
- Validate tenant scope on every tenant-facing read
- Use HTTPS/TLS for any HTTP adapter bridges in production
- Rotate admin keys regularly
- Monitor for suspicious query patterns in logs

---

## 16. Operational Checklist

Before enabling for user-facing AI clients:
1. Build: `cargo build -p voltnuerongrid-mcp --bin mcp-stdio-server --release`
2. Run: `cargo run -p voltnuerongrid-mcp --bin mcp-stdio-server --release`
3. Validate connectivity: send `tools/health` request
4. Validate auth boundary: attempt `tools/benchmark` with non-admin, expect 403
5. Validate guardrails: send mutating SQL, expect rejection
6. Validate tenant isolation: test cross-tenant read, expect rejection
7. Verify logs do not contain secrets
8. Configure client (VSCode/Cursor/Claude) with proper mcp-stdio-server command
9. Test tool discovery in client
10. Smoke test: health -> schema -> query -> benchmark (with appropriate auth)

---

## 17. Quick Reference

### 17.1 Build and run

```powershell
# One-time build
cargo build -p voltnuerongrid-mcp --bin mcp-stdio-server --release

# Run for client connection
cargo run -p voltnuerongrid-mcp --bin mcp-stdio-server --release

# Or use pre-built
./target/release/mcp-stdio-server
```

### 17.2 Methods
- `tools/query`
- `tools/schema`
- `tools/health`
- `tools/benchmark`
- `tools/ddl_create`
- `tools/ddl_drop`
- `tools/erd`
- `tools/data_transfer`

### 17.3 Auth header keys (in JSON-RPC `headers` object)
- `x_vng_admin_key` - API key for admin operations
- `x_vng_operator_id` - Operator identity for operator-level access
- `x_vng_tenant_id` - Tenant ID for tenant-scoped operations
- `x_vng_user_id` - User ID for tenant-facing paths

### 17.4 Response codes
- `200` (implied in JSON-RPC success): request succeeded
- `401`: missing or invalid authentication
- `403`: credentials present but insufficient privilege
- `400`: invalid request structure or params
- `500`: internal server error

---

## 18. FAQ

**Q: Is this implementation read-only for query execution?**
A: Query guardrails are designed to enforce safe read-oriented behavior and reject mutating SQL patterns (DELETE, INSERT, UPDATE, DROP, ALTER, TRUNCATE).

**Q: Why do I get 403 on benchmark tool?**
A: Benchmark is admin-only by design. Use `x_vng_admin_key` header for admin operations.

**Q: Can I use this from VSCode/Cursor/Claude directly?**
A: Yes. Configure the MCP server command to point to the stdio server binary:
- `cargo run -p voltnuerongrid-mcp --bin mcp-stdio-server --release` (development)
- `/path/to/target/release/mcp-stdio-server` (production pre-built)

**Q: What's the difference between stdio server and HTTP adapter?**
A: Stdio server reads from stdin and writes to stdout (standard MCP protocol). HTTP adapter wraps it to accept HTTP requests. Most MCP clients use stdio server directly.

**Q: Can I validate without a UI client?**
A: Yes. Start the stdio server and send JSON-RPC requests via Python, curl, Node, or PowerShell. See Section 12 for examples.

**Q: How do I know if the server is healthy?**
A: Run `tools/health` with valid operator credentials. Success response indicates server is ready.

**Q: Can I benchmark from CLI?**
A: Yes, but only with an admin key in the `headers` object. Non-admin attempts return 403.

---

## 19. Change Log for This Guide

- **v1.1**: Added stdio server binary (`mcp-stdio-server`), updated all client configurations to reference the binary
- **v1.0**: Initial comprehensive MCP guide with 19 sections
