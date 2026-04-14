# VoltNueronGrid MCP Operations Guide

## Quick Start

### Building

```bash
# Build the MCP crate as part of the workspace
cargo build -p voltnuerongrid-mcp

# Build with optimizations
cargo build -p voltnuerongrid-mcp --release
```

### Testing

```bash
# Unit tests
cargo test --lib -p voltnuerongrid-mcp

# Integration tests  
cargo test --test integration_tests -p voltnuerongrid-mcp

# All tests with output
cargo test -p voltnuerongrid-mcp -- --nocapture

# Specific test
cargo test -p voltnuerongrid-mcp mcp_001
```

### Checks

```bash
# Compile check only (fast)
cargo check -p voltnuerongrid-mcp

# Lint warnings  
cargo clippy -p voltnuerongrid-mcp

# Format check
cargo fmt --check -p voltnuerongrid-mcp
```

## Configuration

### Environment Variables

```bash
# Logging configuration
export VNG_MCP_LOG_LEVEL=info

# Request and response size limits (KB)
export VNG_MCP_MAX_QUERY_SIZE_KB=64
export VNG_MCP_MAX_RESULT_SIZE_KB=10

# Query timeout (milliseconds)
export VNG_MCP_QUERY_TIMEOUT_MS=300000

# Audit logging
export VNG_MCP_AUDIT_ENABLED=true
export VNG_MCP_AUDIT_LOG_PATH=/var/log/vng/mcp-audit.log
```

### Security Keys

```bash
# Admin API key (required for admin operations)
export VNG_ADMIN_API_KEY=<your-secret-key>

# KMS key references (for encryption)
export VNG_KMS_PRIMARY_KEY_ID=<key-id>
export VNG_KMS_FAILOVER_KEY_ID=<fallback-key-id>
```

## Deployment

### Embedded in voltnuerongridd

The MCP server is automatically available when running the main service:

```bash
export VNG_ADMIN_API_KEY=secret
cargo run -p voltnuerongridd
```

The server processes JSON-RPC requests via stdio or the integrated HTTP layer based on how the client connects.

### Request Headers (HTTP)

When accessed via HTTP, provide authentication headers:

```bash
# Admin request
curl -X POST http://localhost:8080/api/v1/mcp \
  -H "Content-Type: application/json" \
  -H "x-vng-admin-key: $VNG_ADMIN_API_KEY" \
  -d '{"jsonrpc":"2.0","id":"1","method":"tools/health","params":{}}'

# Operator request
curl -X POST http://localhost:8080/api/v1/mcp \
  -H "Content-Type: application/json" \
  -H "x-vng-operator-id: op-001" \
  -d '{"jsonrpc":"2.0","id":"2","method":"tools/schema","params":{}}'

# Tenant request
curl -X POST http://localhost:8080/api/v1/mcp \
  -H "Content-Type: application/json" \
  -H "x-vng-tenant-id: tenant-123" \
  -H "x-vng-user-id: user-456" \
  -d '{"jsonrpc":"2.0","id":"3","method":"tools/query","params":{"sql_query":"SELECT 1"}}'
```

## Monitoring

### Health Checks

```bash
# Quick health check
const_req='{"jsonrpc":"2.0","id":"1","method":"tools/health","params":{},"headers":{"x_vng_admin_key":"key"}}'
curl -X POST http://localhost:8080/api/v1/mcp \
  -H "Content-Type: application/json" \
  -H "x-vng-admin-key: $VNG_ADMIN_API_KEY" \
  -d "$const_req"
```

### Metrics

MCP emits structured logs with:
- Request ID and timestamp
- Operation type (tool name)
- Authenticated principal
- Execution duration
- Success/failure status

Expected log format:
```json
{
  "timestamp": "2026-04-14T10:30:45.123Z",
  "request_id": "req-123",
  "tool": "query",
  "principal": "op-001",
  "duration_ms": 42,
  "status": "success"
}
```

## Troubleshooting

### 401 Unauthorized

**Cause:** Missing or invalid authentication headers

**Solution:**
- Verify `x-vng-admin-key` header OR
- Verify `x-vng-operator-id` header OR
- Verify both `x-vng-tenant-id` AND `x-vng-user-id`

### 403 Forbidden

**Cause:** Insufficient permissions for the requested tool

**Solution:**
- Admin tools require admin key
- Operator tools require operator ID or admin key
- Tenant operations require tenant scope

### 400 Bad Request

**Cause:** Invalid request format

**Solution:**
- Verify JSON-RPC format: `{"jsonrpc":"2.0","id":"<string>","method":"<string>","params":<object>}`
- Verify method name is one of: `tools/query`, `tools/schema`, `tools/health`, `tools/benchmark`

### Query Rejected by Guardrails

**Cause:** Query violates safety constraints

**Common issues:**
- Contains DDL (DROP, CREATE, ALTER)
- Contains DML (INSERT, UPDATE, DELETE)
- Too large (> 64 KB)
- Multiple statements (semicolon-separated)
- Contains SQL comments

**Solution:** Use simple SELECT queries without modifications

## Upgrade Process

### Backward Compatibility

MCP v0.1.0 maintains best-effort compatibility:
- New tools added as new methods (existing methods unchanged)
- Response format fully backward compatible
- Auth model stable

### Rolling Update

1. Build new version
2. Run full test suite
3. Deploy alongside old service
4. Update routing to new service
5. Monitor error rates
6. Decommission old service if stable

### Rollback

If issues found:
1. Route traffic back to previous version
2. Investigate error logs
3. Fix and re-test
4. Resume update

## Performance Tuning

### Query Optimization

The MCP server delegates query execution to the main SQL engine. To optimize:

1. **Reduce result size**
   - Add LIMIT clause
   - Select only needed columns
   - Use WHERE filters

2. **Increase timeout for complex queries**
   - Set `timeout_ms` appropriately
   - Note: maximum is 5 minutes

3. **Use schema/index information**
   - Call `tools/schema` to understand structure
   - Design queries leveraging indexes

### Request Batching

For multiple operations:

1. **Sequential:** Multiple separate requests (simpler, slower)
2. **Batch:** Send as array (not yet supported, roadmap item)

## Audit and Compliance

### Audit Logging

All MCP operations are logged:

```bash
# View audit log
tail -f logs/mcp-audit.log

# Parse JSON logs
jq '.[] | select(.tool=="query")' logs/mcp-audit.log
```

### Data Retention

- Audit logs retained for 90 days by policy
- Query results not persisted beyond transaction
- No credentials logged

### Compliance Checks

```bash
# Check for sensitive data in logs
grep -i "password\|secret\|key" logs/mcp-audit.log

# Verify auth was enforced
jq '.[] | select(.principal==null)' logs/mcp-audit.log | wc -l
# Should return 0 (all requests have principal)
```

## Support & Debugging

### Enable Debug Logging

```bash
export VNG_MCP_LOG_LEVEL=debug
cargo run -p voltnuerongridd
```

### Capture Raw Requests/Responses

```bash
# With curl verbose output  
curl -v -X POST http://localhost:8080/api/v1/mcp \
  -H "Content-Type: application/json" \
  -H "x-vng-admin-key: $VNG_ADMIN_API_KEY" \
  -d '{"jsonrpc":"2.0","id":"1","method":"tools/health","params":{}}'
```

### Test Suite for Regression

```bash
# Full regression test
cargo test -p voltnuerongrid-mcp

# Filter by test name
cargo test -p voltnuerongrid-mcp mcp_003

# With backtrace
RUST_BACKTRACE=1 cargo test -p voltnuerongrid-mcp
```

## Related Documentation

- [README.md](./README.md) - Feature overview and usage examples
- [src/lib.rs](./src/lib.rs) - Core implementation
- [src/auth.rs](./src/auth.rs) - Authentication and authorization
- [src/tools.rs](./src/tools.rs) - Tool definitions
- [src/guardrails.rs](./src/guardrails.rs) - Safety constraints
