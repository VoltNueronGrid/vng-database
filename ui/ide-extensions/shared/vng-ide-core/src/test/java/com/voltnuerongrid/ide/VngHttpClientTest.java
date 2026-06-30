package com.voltnuerongrid.ide;

import org.junit.jupiter.api.Test;

import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * D-5: tests for the shared IDE query-runner core. URL/header/parse logic runs
 * offline; the live execute test runs only when VNG_IDE_LIVE=1 (live IDE-runtime
 * validation otherwise tracked under E-5).
 */
class VngHttpClientTest {

    @Test
    void buildsAuthHeadersWithOperatorAndDatabase() {
        VngHttpClient client = new VngHttpClient("db.host", 9099, "secret", "admin", "sales", 5000);
        assertEquals("http://db.host:9099", client.baseUrl());
        Map<String, String> h = client.authHeaders();
        assertEquals("secret", h.get("x-vng-admin-key"));
        assertEquals("admin", h.get("x-vng-operator-id"));
        assertEquals("sales", h.get("x-vng-database"));
        assertEquals("application/json", h.get("Content-Type"));
    }

    @Test
    void defaultsOperatorIdToAdmin() {
        VngHttpClient client = new VngHttpClient("127.0.0.1", 8080, "k");
        assertEquals("admin", client.authHeaders().get("x-vng-operator-id"));
    }

    @Test
    void omitsAuthHeadersWhenNoKey() {
        VngHttpClient client = new VngHttpClient("127.0.0.1", 8080, "");
        Map<String, String> h = client.authHeaders();
        assertFalse(h.containsKey("x-vng-admin-key"));
    }

    @Test
    void parsesObjectColumnsAndObjectRows() {
        // Mirrors the real server shape: columns as objects, rows as objects.
        String body = "{\"status\":\"ok\",\"route_path\":\"oltp\","
                + "\"columns\":[{\"name\":\"id\",\"data_type\":\"integer\"},{\"name\":\"name\",\"data_type\":\"text\"}],"
                + "\"rows\":[{\"id\":1,\"name\":\"alice\"},{\"id\":2,\"name\":\"bob\"}]}";
        VngQueryResult r = VngHttpClient.parseExecuteResponse(body);
        assertFalse(r.isError());
        assertEquals(List.of("id", "name"), r.columns());
        assertEquals(2, r.rowCount());
        assertEquals("1", r.rows().get(0).get(0));
        assertEquals("alice", r.rows().get(0).get(1));
        assertEquals("bob", r.rowAsMap(1).get("name"));
    }

    @Test
    void parsesArrayRows() {
        String body = "{\"columns\":[\"a\",\"b\"],\"rows\":[[\"1\",\"x\"],[\"2\",\"y\"]]}";
        VngQueryResult r = VngHttpClient.parseExecuteResponse(body);
        assertEquals(2, r.rowCount());
        assertEquals("x", r.rows().get(0).get(1));
    }

    @Test
    void jsonStringEscapesQuotesAndNewlines() {
        assertEquals("\"a\\\"b\"", VngHttpClient.jsonString("a\"b"));
        assertEquals("\"line1\\nline2\"", VngHttpClient.jsonString("line1\nline2"));
        assertEquals("null", VngHttpClient.jsonString(null));
    }

    @Test
    void miniJsonRoundTripsNestedStructures() {
        Object v = MiniJson.parse("{\"a\":[1,2,{\"b\":\"c\"}],\"d\":true,\"e\":null}");
        assertTrue(v instanceof Map);
        @SuppressWarnings("unchecked")
        Map<String, Object> m = (Map<String, Object>) v;
        assertTrue(m.get("a") instanceof List);
        assertEquals(Boolean.TRUE, m.get("d"));
    }

    /** Live end-to-end — only runs when VNG_IDE_LIVE=1. */
    @Test
    void liveExecuteAgainstServer() {
        if (!"1".equals(System.getenv("VNG_IDE_LIVE"))) {
            return; // skipped unless explicitly enabled (E-5 live validation)
        }
        String key = System.getenv().getOrDefault("VNG_ADMIN_API_KEY", "secret");
        VngHttpClient client = new VngHttpClient("127.0.0.1", 8080, key);
        assertTrue(client.health());
        String table = "ide_demo_" + System.nanoTime();
        client.executeSql("CREATE TABLE " + table + " (id INT PRIMARY KEY, name TEXT)");
        client.executeSql("INSERT INTO " + table + " (id, name) VALUES (1, 'alice')");
        VngQueryResult r = client.executeSql("SELECT id, name FROM " + table);
        assertFalse(r.isError());
        assertTrue(r.rowCount() >= 1);
    }
}
