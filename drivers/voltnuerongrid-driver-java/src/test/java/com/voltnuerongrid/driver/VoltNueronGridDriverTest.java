package com.voltnuerongrid.driver;

import org.junit.jupiter.api.Test;

import java.util.Arrays;

import static org.junit.jupiter.api.Assertions.*;

/**
 * Unit tests for {@link VoltNueronGridDriver} and supporting classes.
 */
class VoltNueronGridDriverTest {

    // --- Helpers ---

    private DriverConfig adminConfig() {
        return new DriverConfig.Builder()
                .baseUrl("http://localhost:8080")
                .sessionId("test-session")
                .mode("admin")
                .adminApiKey("secret-key")
                .build();
    }

    private DriverConfig tenantConfig() {
        return new DriverConfig.Builder()
                .baseUrl("http://localhost:8080")
                .sessionId("test-session")
                .mode("tenant")
                .tenantId("tenant-1")
                .build();
    }

    // --- S10-001 required tests ---

    @Test
    void testBuildHealthRequestUrl() {
        VoltNueronGridDriver driver = new VoltNueronGridDriver(adminConfig());
        DriverRequest req = driver.buildHealthRequest();

        assertEquals("GET", req.method);
        assertEquals("http://localhost:8080/health", req.url);
    }

    @Test
    void testBuildSqlExecuteRequestBody() {
        VoltNueronGridDriver driver = new VoltNueronGridDriver(adminConfig());
        DriverRequest req = driver.buildSqlExecuteRequest("SELECT 1");

        assertEquals("POST", req.method);
        assertNotNull(req.bodyJson);
        assertTrue(req.bodyJson.contains("sql_batch"));
        assertTrue(req.bodyJson.contains("SELECT 1"));
        assertTrue(req.url.endsWith("/api/v1/sql/execute"));
    }

    @Test
    void testValidationRejectsEmptyBaseUrl() {
        DriverConfig.Builder builder = new DriverConfig.Builder()
                .baseUrl("")
                .sessionId("s1")
                .mode("admin")
                .adminApiKey("key");
        DriverConfig cfg = builder.build();

        DriverError err = assertThrows(DriverError.class, () -> new VoltNueronGridDriver(cfg));
        assertEquals(DriverError.Kind.VALIDATION, err.getKind());
    }

    @Test
    void testAdminModeRequiresApiKey() {
        DriverConfig cfg = new DriverConfig.Builder()
                .baseUrl("http://localhost:8080")
                .sessionId("s1")
                .mode("admin")
                // intentionally omit adminApiKey
                .build();

        DriverError err = assertThrows(DriverError.class, () -> new VoltNueronGridDriver(cfg));
        assertEquals(DriverError.Kind.VALIDATION, err.getKind());
        assertTrue(err.getMessage().contains("adminApiKey"));
    }

    @Test
    void testBuildSchemaRegistryRequestMethod() {
        VoltNueronGridDriver driver = new VoltNueronGridDriver(adminConfig());
        DriverRequest req = driver.buildSchemaRegistryRequest();

        assertEquals("GET", req.method);
        assertTrue(req.url.endsWith("/api/v1/ingest/schema/registry"));
    }

    // --- Additional coverage ---

    @Test
    void testBuildSqlAnalyzeRequest() {
        VoltNueronGridDriver driver = new VoltNueronGridDriver(adminConfig());
        DriverRequest req = driver.buildSqlAnalyzeRequest("SELECT 1");
        assertEquals("POST", req.method);
        assertTrue(req.url.endsWith("/api/v1/sql/analyze"));
    }

    @Test
    void testBuildSqlRouteRequest() {
        VoltNueronGridDriver driver = new VoltNueronGridDriver(adminConfig());
        DriverRequest req = driver.buildSqlRouteRequest("SELECT 1");
        assertEquals("POST", req.method);
        assertTrue(req.url.endsWith("/api/v1/sql/route"));
    }

    @Test
    void testBuildSqlTransactionRequest() {
        VoltNueronGridDriver driver = new VoltNueronGridDriver(adminConfig());
        DriverRequest req = driver.buildSqlTransactionRequest(Arrays.asList("INSERT INTO t VALUES(1)", "SELECT 1"));
        assertEquals("POST", req.method);
        assertTrue(req.url.endsWith("/api/v1/sql/transaction"));
        assertTrue(req.bodyJson.contains("statements"));
    }

    @Test
    void testAdminHeadersPresent() {
        VoltNueronGridDriver driver = new VoltNueronGridDriver(adminConfig());
        DriverRequest req = driver.buildHealthRequest();
        assertEquals("secret-key", req.headers.get("x-vng-admin-key"));
        assertEquals("test-session", req.headers.get("x-vng-session-id"));
    }

    @Test
    void testTenantHeadersPresent() {
        VoltNueronGridDriver driver = new VoltNueronGridDriver(tenantConfig());
        DriverRequest req = driver.buildHealthRequest();
        assertEquals("tenant-1", req.headers.get("x-vng-tenant-id"));
        assertFalse(req.headers.containsKey("x-vng-admin-key"));
    }

    @Test
    void testTrailingSlashStrippedFromBaseUrl() {
        DriverConfig cfg = new DriverConfig.Builder()
                .baseUrl("http://localhost:8080/")
                .sessionId("s1")
                .mode("admin")
                .adminApiKey("key")
                .build();
        VoltNueronGridDriver driver = new VoltNueronGridDriver(cfg);
        DriverRequest req = driver.buildHealthRequest();
        assertEquals("http://localhost:8080/health", req.url);
    }

    @Test
    void testTenantModeRequiresTenantId() {
        DriverConfig cfg = new DriverConfig.Builder()
                .baseUrl("http://localhost:8080")
                .sessionId("s1")
                .mode("tenant")
                // no tenantId
                .build();
        DriverError err = assertThrows(DriverError.class, () -> new VoltNueronGridDriver(cfg));
        assertEquals(DriverError.Kind.VALIDATION, err.getKind());
    }
}
