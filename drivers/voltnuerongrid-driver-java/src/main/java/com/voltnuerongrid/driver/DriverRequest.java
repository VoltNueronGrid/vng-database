package com.voltnuerongrid.driver;

import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * Represents an HTTP request built by the driver, ready for execution by the
 * caller's HTTP client.
 */
public final class DriverRequest {

    /** HTTP method, e.g. {@code "GET"} or {@code "POST"}. */
    public final String method;

    /** Full URL including base and path. */
    public final String url;

    /** HTTP headers (immutable). */
    public final Map<String, String> headers;

    /**
     * JSON-encoded request body, or {@code null} for GET requests.
     */
    public final String bodyJson;

    public DriverRequest(String method, String url, Map<String, String> headers, String bodyJson) {
        this.method = method;
        this.url = url;
        this.headers = Collections.unmodifiableMap(new LinkedHashMap<>(headers));
        this.bodyJson = bodyJson;
    }

    @Override
    public String toString() {
        return "DriverRequest{method='" + method + "', url='" + url + "', bodyJson=" +
               (bodyJson == null ? "null" : "'" + bodyJson.substring(0, Math.min(80, bodyJson.length())) + "...'") + "}";
    }
}
