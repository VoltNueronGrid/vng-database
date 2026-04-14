# VoltNueronGrid MCP Quick Start (One Pager)

This one-pager gets you from zero to first successful MCP tool calls in minutes.

For full details, see [README-MCP.md](README-MCP.md).

## 1. What You Get

VoltNueronGrid MCP currently exposes:
- `tools/health`
- `tools/schema`
- `tools/query`
- `tools/benchmark` (admin only)

Auth model:
1. Admin (`x-vng-admin-key`)
2. Operator (`x-vng-operator-id`)
3. Tenant (`x-vng-tenant-id` + `x-vng-user-id`)

Status semantics:
- `401` missing/invalid credentials
- `403` insufficient privilege or guardrail block

## 2. Fast Validation (Repo Root)

```powershell
cargo check -p voltnuerongrid-mcp
cargo test -p voltnuerongrid-mcp
cargo build -p voltnuerongrid-mcp --release
```

Expected: all tests pass.

## 3. Pick a Runtime Pattern

Use one of these:
- Stdio MCP host wrapper command that calls the crate logic
- HTTP adapter route in your service that forwards JSON-RPC envelopes

The templates in [mcp-config-pack/README.md](mcp-config-pack/README.md) work for either pattern.

## 4. Configure an MCP Client

Copy one template from [mcp-config-pack](mcp-config-pack):
- VSCode: [mcp-config-pack/vscode.settings.template.json](mcp-config-pack/vscode.settings.template.json)
- Cursor: [mcp-config-pack/cursor.mcp.template.json](mcp-config-pack/cursor.mcp.template.json)
- Claude Desktop: [mcp-config-pack/claude_desktop_config.template.json](mcp-config-pack/claude_desktop_config.template.json)

Set env values from [mcp-config-pack/.env.example](mcp-config-pack/.env.example).

Replace `<YOUR_MCP_HOST_COMMAND_HERE>` with your actual MCP adapter command.

## 5. First 3 Calls (Recommended)

1. Health check
2. Schema check
3. Safe read-only query

Use payload samples from:
- [mcp-config-pack/http-jsonrpc-samples/health.operator.json](mcp-config-pack/http-jsonrpc-samples/health.operator.json)
- [mcp-config-pack/http-jsonrpc-samples/schema.operator.json](mcp-config-pack/http-jsonrpc-samples/schema.operator.json)
- [mcp-config-pack/http-jsonrpc-samples/query.operator.json](mcp-config-pack/http-jsonrpc-samples/query.operator.json)

## 6. PowerShell Smoke Calls (HTTP Adapter)

```powershell
# health
$health = Get-Content ./mcp-config-pack/http-jsonrpc-samples/health.operator.json -Raw
Invoke-RestMethod -Method POST -Uri "http://127.0.0.1:8080/api/v1/mcp" -ContentType "application/json" -Body $health

# schema
$schema = Get-Content ./mcp-config-pack/http-jsonrpc-samples/schema.operator.json -Raw
Invoke-RestMethod -Method POST -Uri "http://127.0.0.1:8080/api/v1/mcp" -ContentType "application/json" -Body $schema

# query
$query = Get-Content ./mcp-config-pack/http-jsonrpc-samples/query.operator.json -Raw
Invoke-RestMethod -Method POST -Uri "http://127.0.0.1:8080/api/v1/mcp" -ContentType "application/json" -Body $query
```

## 7. Privilege Boundary Check

- Run benchmark with operator payload: expect `403`
- Run benchmark with admin payload: expect success

Samples:
- [mcp-config-pack/http-jsonrpc-samples/benchmark.operator.forbidden.json](mcp-config-pack/http-jsonrpc-samples/benchmark.operator.forbidden.json)
- [mcp-config-pack/http-jsonrpc-samples/benchmark.admin.json](mcp-config-pack/http-jsonrpc-samples/benchmark.admin.json)

## 8. Common Fixes

If client does not discover server:
- Validate JSON config syntax
- Ensure command/args are valid in your shell
- Restart client after config changes

If you get `401`:
- Confirm required auth header fields are present in payload

If you get `403`:
- Check privilege level for the tool
- Check query guardrails (mutating SQL is blocked)

## 9. Done Criteria

You are done when:
- Health call succeeds
- Schema call succeeds
- Query call succeeds
- Operator benchmark is forbidden (`403`)
- Admin benchmark succeeds
