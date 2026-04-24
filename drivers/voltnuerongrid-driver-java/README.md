# VoltNueronGrid Java Driver

Pure stdlib Java driver (Java 11+) for the VoltNueronGrid database.
No external runtime dependencies — only JUnit 5 is required for tests.

## Quick Start

```java
import com.voltnuerongrid.driver.*;

// 1. Build config
DriverConfig config = new DriverConfig.Builder()
    .baseUrl("http://localhost:8080")
    .sessionId("my-session-id")
    .mode("admin")
    .adminApiKey("my-secret-key")
    .requestTimeoutMs(30000)
    .maxRetries(2)
    .build();

// 2. Create driver
VoltNueronGridDriver driver = new VoltNueronGridDriver(config);

// 3. Build requests
DriverRequest healthReq  = driver.buildHealthRequest();
DriverRequest executeReq = driver.buildSqlExecuteRequest("SELECT 1");
DriverRequest analyzeReq = driver.buildSqlAnalyzeRequest("SELECT id FROM users");
DriverRequest routeReq   = driver.buildSqlRouteRequest("SELECT * FROM orders");
DriverRequest txnReq     = driver.buildSqlTransactionRequest(
    List.of("INSERT INTO t VALUES(1)", "INSERT INTO t VALUES(2)"));
DriverRequest schemaReq  = driver.buildSchemaRegistryRequest();

// 4. Execute with your preferred HTTP client (e.g. HttpClient from Java 11+)
HttpClient http = HttpClient.newBuilder()
    .connectTimeout(Duration.ofMillis(config.requestTimeoutMs))
    .build();

HttpRequest httpRequest = HttpRequest.newBuilder()
    .uri(URI.create(healthReq.url))
    .GET()
    .headers(flattenHeaders(healthReq.headers))
    .build();

HttpResponse<String> response = http.send(httpRequest, HttpResponse.BodyHandlers.ofString());
System.out.println("Status: " + response.statusCode());
System.out.println("Body:   " + response.body());
```

## Modes

| Mode       | Required fields              |
|------------|------------------------------|
| `admin`    | `adminApiKey`                |
| `operator` | `adminApiKey`, `operatorId`  |
| `tenant`   | `tenantId`                   |

## Error Handling

All driver errors are thrown as `DriverError` (a `RuntimeException`):

```java
try {
    DriverRequest req = driver.buildSqlExecuteRequest(sql);
    // ... execute req ...
} catch (DriverError e) {
    switch (e.getKind()) {
        case VALIDATION:   // bad config or params
        case TRANSPORT:    // network-level failure
        case HTTP_STATUS:  // non-2xx response (e.getStatusCode())
        case TIMEOUT:      // request exceeded timeout
        case CANCELLED:    // caller cancelled
    }
}
```

## Build & Test

```bash
mvn test
```

## Retryable HTTP statuses

408, 425, 429, 500, 502, 503, 504 — same list as the Rust and TypeScript drivers.
