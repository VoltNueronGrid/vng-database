# MCP Client Config Pack

This folder contains ready-to-edit templates for connecting MCP clients to VoltNueronGrid MCP.

## Included Files

- [vscode.settings.template.json](vscode.settings.template.json)
- [cursor.mcp.template.json](cursor.mcp.template.json)
- [claude_desktop_config.template.json](claude_desktop_config.template.json)
- [.env.example](.env.example)
- [http-jsonrpc-samples](http-jsonrpc-samples)

## How to Use

1. Copy the template for your client.
2. Replace `<YOUR_MCP_HOST_COMMAND_HERE>` with your actual MCP host adapter command.
3. Set environment variables from `.env.example`.
4. Restart the client.
5. Run health, schema, and query smoke checks.

## Notes

- This repository provides a production MCP core crate.
- Client transport is typically via stdio host wrapper or HTTP bridge.
- The templates are transport-agnostic and use command + args style expected by MCP clients.

## Suggested Smoke Sequence

1. health -> should return `healthy`
2. schema -> should return visible tables
3. query -> should return rows for read-only SQL
4. benchmark with operator -> should return `403`
5. benchmark with admin -> should succeed
