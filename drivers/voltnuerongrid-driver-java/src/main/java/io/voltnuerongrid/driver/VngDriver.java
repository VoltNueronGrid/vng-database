package io.voltnuerongrid.driver;

import java.util.LinkedHashMap;
import java.util.Map;

public final class VngDriver {
    private final String baseUrl;

    public VngDriver(String baseUrl) {
        if (baseUrl == null || baseUrl.isBlank()) {
            throw new IllegalArgumentException("baseUrl must not be empty");
        }
        this.baseUrl = baseUrl.endsWith("/") ? baseUrl.substring(0, baseUrl.length() - 1) : baseUrl;
    }

    public Request buildHealthRequest() {
        return new Request("GET", baseUrl + "/health", Map.of());
    }

    public Request buildSqlAnalyzeRequest(String sqlBatch) {
        if (sqlBatch == null || sqlBatch.isBlank()) {
            throw new IllegalArgumentException("sqlBatch must not be empty");
        }
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("sql_batch", sqlBatch);
        return new Request("POST", baseUrl + "/api/v1/sql/analyze", body);
    }

    public record Request(String method, String url, Map<String, Object> body) {}
}
