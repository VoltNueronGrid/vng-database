# VoltNueronGrid Node.js Driver

Pure ESM Node.js driver for VoltNueronGrid. Zero external dependencies — uses only Node's built-in `http`/`https` modules.

Requires Node.js >= 18.

## Quick Start

```js
import { VoltNueronGridDriver, performDriverHttpRequest } from "@voltnuerongrid/driver-node";

const driver = new VoltNueronGridDriver({
  baseUrl: "http://localhost:8080",
  sessionId: "my-session",
  mode: "admin",
  adminApiKey: "my-secret-key",
});

// Build a request
const req = driver.buildSqlExecuteRequest("SELECT 1");

// Execute it
const result = await performDriverHttpRequest(req, {
  timeoutMs: 10_000,
  maxRetries: 2,
});

console.log(result.status);    // 200
console.log(result.bodyText);  // JSON string
```

## Config Modes

| mode       | required fields               |
|------------|-------------------------------|
| `admin`    | `adminApiKey`                 |
| `operator` | `adminApiKey`, `operatorId`   |
| `tenant`   | `tenantId`                    |

## API

### `validateConfig(config) -> string | null`

Validates a config object. Returns an error message string, or `null` if valid.

### `class VoltNueronGridDriver`

Request builders:

- `buildHealthRequest()`
- `buildSqlExecuteRequest(sqlBatch)`
- `buildSqlAnalyzeRequest(sqlBatch)`
- `buildSqlRouteRequest(sqlBatch)`
- `buildSqlTransactionRequest(statements[])`
- `buildSchemaRegistryRequest()`

### `performDriverHttpRequest(req, opts?) -> Promise<{status, bodyText}>`

Executes a `DriverRequest` with timeout and exponential-backoff retry.

Options:
- `timeoutMs` — default 30000
- `maxRetries` — default 2
- `abortSignal` — optional `AbortSignal`

### `isRetryableHttpStatus(status) -> boolean`

Returns `true` for: 408, 425, 429, 500, 502, 503, 504.

### `class DriverError extends Error`

- `.kind`: `"validation" | "transport" | "http_status" | "timeout" | "cancelled"`
- `.statusCode`: HTTP status (http_status kind only)

## Tests

```bash
node --test test/driver.test.js
```
