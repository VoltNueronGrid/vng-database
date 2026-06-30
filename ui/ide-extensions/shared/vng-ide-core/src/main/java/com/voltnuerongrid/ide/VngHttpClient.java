package com.voltnuerongrid.ide;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/**
 * D-5: SDK-free VoltNueronGrid query runner shared by the Eclipse and JetBrains
 * IDE extensions. Uses only {@link HttpURLConnection} (no OkHttp/Gson) so each
 * IDE module carries no third-party HTTP/JSON dependency.
 *
 * <p>Authentication mirrors the server's RBAC: an admin key plus an operator id
 * (default {@code admin}) are sent so the SQL-runtime principal resolves.
 */
public final class VngHttpClient {

    private final String baseUrl;
    private final String adminKey;
    private final String operatorId;
    private final String database;
    private final int timeoutMs;

    public VngHttpClient(String host, int port, String adminKey) {
        this(host, port, adminKey, "admin", null, 30_000);
    }

    public VngHttpClient(String host, int port, String adminKey, String operatorId,
                         String database, int timeoutMs) {
        this.baseUrl = "http://" + host + ":" + port;
        this.adminKey = adminKey;
        this.operatorId = (operatorId == null || operatorId.isEmpty()) ? "admin" : operatorId;
        this.database = database;
        this.timeoutMs = timeoutMs > 0 ? timeoutMs : 30_000;
    }

    /** Base URL this client targets (e.g. {@code http://127.0.0.1:8080}). */
    public String baseUrl() {
        return baseUrl;
    }

    /** Build the request headers that would be sent (exposed for testing). */
    public java.util.Map<String, String> authHeaders() {
        java.util.Map<String, String> h = new java.util.LinkedHashMap<>();
        h.put("Content-Type", "application/json");
        if (adminKey != null && !adminKey.isEmpty()) {
            h.put("x-vng-admin-key", adminKey);
            h.put("x-vng-operator-id", operatorId);
        }
        if (database != null && !database.isEmpty()) {
            h.put("x-vng-database", database);
        }
        return h;
    }

    /** GET {@code /health}. Returns true on HTTP 200. */
    public boolean health() {
        try {
            HttpURLConnection conn = open("/health", "GET");
            int code = conn.getResponseCode();
            drain(conn);
            return code == 200;
        } catch (IOException e) {
            return false;
        }
    }

    /** POST a SQL batch to {@code /api/v1/sql/execute} and parse the result. */
    public VngQueryResult executeSql(String sql) {
        try {
            HttpURLConnection conn = open("/api/v1/sql/execute", "POST");
            String body = "{\"sql_batch\":" + jsonString(sql) + "}";
            conn.setDoOutput(true);
            try (OutputStream os = conn.getOutputStream()) {
                os.write(body.getBytes(StandardCharsets.UTF_8));
            }
            int code = conn.getResponseCode();
            String responseBody = readBody(conn, code);
            if (code != 200) {
                return VngQueryResult.error("HTTP " + code + ": " + responseBody);
            }
            return parseExecuteResponse(responseBody);
        } catch (IOException e) {
            return VngQueryResult.error("transport error: " + e.getMessage());
        }
    }

    private HttpURLConnection open(String path, String method) throws IOException {
        HttpURLConnection conn = (HttpURLConnection) URI.create(baseUrl + path).toURL().openConnection();
        conn.setRequestMethod(method);
        conn.setConnectTimeout(timeoutMs);
        conn.setReadTimeout(timeoutMs);
        for (Map.Entry<String, String> e : authHeaders().entrySet()) {
            conn.setRequestProperty(e.getKey(), e.getValue());
        }
        return conn;
    }

    private static String readBody(HttpURLConnection conn, int code) throws IOException {
        InputStream is = (code >= 200 && code < 400) ? conn.getInputStream() : conn.getErrorStream();
        if (is == null) {
            return "";
        }
        return new String(is.readAllBytes(), StandardCharsets.UTF_8);
    }

    private static void drain(HttpURLConnection conn) {
        try (InputStream is = conn.getInputStream()) {
            if (is != null) {
                is.readAllBytes();
            }
        } catch (IOException ignored) {
            // best-effort
        }
    }

    /**
     * Parse a {@code /api/v1/sql/execute} JSON body into a {@link VngQueryResult}.
     * Handles the canonical shapes: object columns ({@code {"name":..}}) or bare
     * string columns, and array rows or object rows.
     */
    @SuppressWarnings("unchecked")
    static VngQueryResult parseExecuteResponse(String body) {
        Object root = MiniJson.parse(body);
        if (!(root instanceof Map)) {
            return VngQueryResult.error("malformed response");
        }
        Map<String, Object> obj = (Map<String, Object>) root;
        String status = String.valueOf(obj.getOrDefault("status", "ok"));
        String routePath = String.valueOf(obj.getOrDefault("route_path", ""));

        List<String> columns = new ArrayList<>();
        Object colsVal = obj.get("columns");
        if (colsVal instanceof List) {
            for (Object c : (List<Object>) colsVal) {
                columns.add(columnName(c));
            }
        }

        List<List<String>> rows = new ArrayList<>();
        Object rowsVal = obj.get("rows");
        if (rowsVal instanceof List) {
            for (Object row : (List<Object>) rowsVal) {
                if (row instanceof List) {
                    List<String> cells = new ArrayList<>();
                    for (Object cell : (List<Object>) row) {
                        cells.add(scalar(cell));
                    }
                    if (columns.isEmpty()) {
                        for (int i = 0; i < cells.size(); i++) {
                            columns.add("column" + i);
                        }
                    }
                    rows.add(cells);
                } else if (row instanceof Map) {
                    Map<String, Object> m = (Map<String, Object>) row;
                    if (columns.isEmpty()) {
                        columns.addAll(m.keySet());
                    }
                    List<String> cells = new ArrayList<>();
                    for (String col : columns) {
                        cells.add(scalar(m.get(col)));
                    }
                    rows.add(cells);
                }
            }
        }
        return new VngQueryResult(status, routePath, columns, rows, null);
    }

    @SuppressWarnings("unchecked")
    private static String columnName(Object c) {
        if (c instanceof Map) {
            Object name = ((Map<String, Object>) c).get("name");
            if (name != null) {
                return scalar(name);
            }
        }
        return scalar(c);
    }

    private static String scalar(Object v) {
        if (v == null) {
            return null;
        }
        if (v instanceof Double) {
            double d = (Double) v;
            if (d == Math.rint(d) && !Double.isInfinite(d)) {
                return Long.toString((long) d);
            }
            return Double.toString(d);
        }
        return v.toString();
    }

    /** Minimal JSON string escaping for the SQL batch payload. */
    static String jsonString(String value) {
        if (value == null) {
            return "null";
        }
        StringBuilder sb = new StringBuilder("\"");
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '"': sb.append("\\\""); break;
                case '\\': sb.append("\\\\"); break;
                case '\b': sb.append("\\b"); break;
                case '\f': sb.append("\\f"); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '\t': sb.append("\\t"); break;
                default:
                    if (c < 0x20) {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
            }
        }
        return sb.append('"').toString();
    }
}
