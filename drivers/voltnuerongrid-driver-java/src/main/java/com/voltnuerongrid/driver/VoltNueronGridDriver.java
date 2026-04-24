package com.voltnuerongrid.driver;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.stream.Collectors;

/**
 * Request builder for the VoltNueronGrid HTTP API.
 *
 * <p>This driver only constructs {@link DriverRequest} objects; it does not
 * perform I/O. Pass the returned requests to your preferred HTTP client.
 *
 * <pre>{@code
 * DriverConfig cfg = new DriverConfig.Builder()
 *     .baseUrl("http://localhost:8080")
 *     .sessionId("s1")
 *     .mode("admin")
 *     .adminApiKey("secret")
 *     .build();
 * VoltNueronGridDriver driver = new VoltNueronGridDriver(cfg);
 * DriverRequest health = driver.buildHealthRequest();
 * }</pre>
 */
public final class VoltNueronGridDriver {

    private final DriverConfig config;

    /**
     * Creates a driver for the given configuration.
     *
     * @throws DriverError with kind {@link DriverError.Kind#VALIDATION} if config is invalid
     */
    public VoltNueronGridDriver(DriverConfig config) {
        if (config == null) {
            throw DriverError.validation("config must not be null");
        }
        config.validateOrThrow();
        this.config = config;
    }

    // -------------------------------------------------------------------------
    // Request builders
    // -------------------------------------------------------------------------

    /**
     * Builds a {@code GET /health} request.
     */
    public DriverRequest buildHealthRequest() {
        return new DriverRequest("GET", baseUrl() + "/health", buildHeaders(), null);
    }

    /**
     * Builds a {@code POST /api/v1/sql/execute} request.
     *
     * @param sqlBatch SQL text to execute
     */
    public DriverRequest buildSqlExecuteRequest(String sqlBatch) {
        requireNonEmpty(sqlBatch, "sqlBatch");
        String body = "{\"sql_batch\":" + jsonString(sqlBatch) + "}";
        return new DriverRequest("POST", baseUrl() + "/api/v1/sql/execute", buildHeaders(), body);
    }

    /**
     * Builds a {@code POST /api/v1/sql/analyze} request.
     *
     * @param sqlBatch SQL text to analyze
     */
    public DriverRequest buildSqlAnalyzeRequest(String sqlBatch) {
        requireNonEmpty(sqlBatch, "sqlBatch");
        String body = "{\"sql_batch\":" + jsonString(sqlBatch) + "}";
        return new DriverRequest("POST", baseUrl() + "/api/v1/sql/analyze", buildHeaders(), body);
    }

    /**
     * Builds a {@code POST /api/v1/sql/route} request.
     *
     * @param sqlBatch SQL text for routing decision
     */
    public DriverRequest buildSqlRouteRequest(String sqlBatch) {
        requireNonEmpty(sqlBatch, "sqlBatch");
        String body = "{\"sql_batch\":" + jsonString(sqlBatch) + "}";
        return new DriverRequest("POST", baseUrl() + "/api/v1/sql/route", buildHeaders(), body);
    }

    /**
     * Builds a {@code POST /api/v1/sql/transaction} request for a list of statements.
     *
     * @param statements ordered list of SQL statements in the transaction
     */
    public DriverRequest buildSqlTransactionRequest(List<String> statements) {
        if (statements == null || statements.isEmpty()) {
            throw DriverError.validation("statements must not be null or empty");
        }
        String arrayBody = statements.stream()
                .map(this::jsonString)
                .collect(Collectors.joining(",", "[", "]"));
        String body = "{\"statements\":" + arrayBody + "}";
        return new DriverRequest("POST", baseUrl() + "/api/v1/sql/transaction", buildHeaders(), body);
    }

    /**
     * Builds a {@code GET /api/v1/ingest/schema/registry} request.
     */
    public DriverRequest buildSchemaRegistryRequest() {
        return new DriverRequest("GET", baseUrl() + "/api/v1/ingest/schema/registry", buildHeaders(), null);
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    private String baseUrl() {
        return config.baseUrl.trim().replaceAll("/$", "");
    }

    private Map<String, String> buildHeaders() {
        Map<String, String> headers = new LinkedHashMap<>();
        headers.put("content-type", "application/json");
        headers.put("x-vng-session-id", config.sessionId);

        if (("admin".equals(config.mode) || "operator".equals(config.mode))
                && config.adminApiKey != null && !config.adminApiKey.trim().isEmpty()) {
            headers.put("x-vng-admin-key", config.adminApiKey);
        }
        if ("operator".equals(config.mode)
                && config.operatorId != null && !config.operatorId.trim().isEmpty()) {
            headers.put("x-vng-operator-id", config.operatorId);
        }
        if ("tenant".equals(config.mode)
                && config.tenantId != null && !config.tenantId.trim().isEmpty()) {
            headers.put("x-vng-tenant-id", config.tenantId);
        }
        if ("tenant".equals(config.mode)
                && config.userId != null && !config.userId.trim().isEmpty()) {
            headers.put("x-vng-user-id", config.userId);
        }
        if (config.routeHint != null && !config.routeHint.trim().isEmpty()) {
            headers.put("x-vng-route-hint", config.routeHint);
        }
        return headers;
    }

    /**
     * Minimal JSON string encoding (handles common escapes; suitable for SQL batch strings).
     */
    private String jsonString(String value) {
        if (value == null) {
            return "null";
        }
        StringBuilder sb = new StringBuilder("\"");
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '"':  sb.append("\\\""); break;
                case '\\': sb.append("\\\\"); break;
                case '\b': sb.append("\\b");  break;
                case '\f': sb.append("\\f");  break;
                case '\n': sb.append("\\n");  break;
                case '\r': sb.append("\\r");  break;
                case '\t': sb.append("\\t");  break;
                default:
                    if (c < 0x20) {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
            }
        }
        sb.append("\"");
        return sb.toString();
    }

    private void requireNonEmpty(String value, String name) {
        if (value == null || value.trim().isEmpty()) {
            throw DriverError.validation(name + " must not be null or empty");
        }
    }
}
