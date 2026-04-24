# VoltNueronGrid Deno Driver

Deno adapter wrapping the TypeScript driver. Uses Deno's globally available `fetch` — no polyfills or npm packages required.

## Quick Start

```typescript
import { VoltNueronGridDriver, performDriverHttpRequest } from "./mod.ts";

const driver = new VoltNueronGridDriver({
  baseUrl: "http://localhost:8080",
  sessionId: "my-session",
  mode: "admin",
  adminApiKey: "my-secret-key",
});

const req = driver.buildSqlExecuteRequest("SELECT 1");
const result = await performDriverHttpRequest(req, { timeoutMs: 10_000 });

console.log(result.status);    // 200
console.log(result.bodyText);  // JSON string
```

## From deno.land/x (once published)

```typescript
import { VoltNueronGridDriver } from "https://deno.land/x/voltnuerongrid_driver@0.1.0/mod.ts";
```

## API

All exports are re-exported from `voltnuerongrid-driver-typescript`:

- `VoltNueronGridDriver` — request builder class
- `validateConfig(config)` — returns error string or null
- `performDriverHttpRequest(req, opts?)` — executes via `fetch`
- `isRetryableHttpStatus(status)` — true for 408/425/429/500/502/503/504
- `DriverError` — typed error class

See the TypeScript driver README for full API documentation.

## Tests

```bash
deno test --allow-net test/driver_test.ts
```

## Requirements

- Deno 1.40+ (for `deno.json` v3 + standard `fetch`)
- No npm packages or Node compatibility layer needed
