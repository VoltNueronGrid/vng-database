package io.voltnuerongrid.jdbc;

import com.voltnuerongrid.driver.VngResultSet;
import org.junit.jupiter.api.Test;

import java.lang.reflect.Method;
import java.sql.Connection;
import java.sql.Driver;
import java.sql.DriverManager;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.SQLException;
import java.sql.Statement;
import java.util.Properties;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * D-2: JDBC layer tests. The URL/driver/result-set tests run offline; the
 * end-to-end connect→query test runs only when VNG_JDBC_LIVE=1 and a server is
 * reachable (live validation otherwise tracked under E-5).
 */
class VngJdbcDriverTest {

    @Test
    void driverIsRegisteredWithDriverManager() throws SQLException {
        // Touch the class so its static registration block runs.
        Driver driver = new VngJdbcDriver();
        assertTrue(driver.acceptsURL("jdbc:voltnuerongrid://127.0.0.1:8080/acme"));
        assertFalse(driver.acceptsURL("jdbc:postgresql://localhost/db"));

        // DriverManager must be able to resolve our driver for the URL.
        Driver resolved = DriverManager.getDriver("jdbc:voltnuerongrid://127.0.0.1:8080/acme");
        assertNotNull(resolved);
        assertTrue(resolved instanceof VngJdbcDriver);
    }

    @Test
    void connectReturnsNullForForeignUrl() throws SQLException {
        Driver driver = new VngJdbcDriver();
        assertNull(driver.connect("jdbc:mysql://localhost/db", new Properties()));
    }

    @Test
    void parseUrlExtractsHostPortDatabaseAndParams() throws Exception {
        Properties info = new Properties();
        VngJdbcDriver.ParsedUrl parsed = VngJdbcDriver.parseUrl(
                "jdbc:voltnuerongrid://db.example.com:9099/sales?adminKey=secret&operatorId=admin",
                info);
        assertEquals("http://db.example.com:9099", parsed.config.baseUrl);
        assertEquals("sales", parsed.database);
        assertEquals("secret", parsed.config.adminApiKey);
        assertEquals("admin", parsed.config.operatorId);
        assertEquals("operator", parsed.config.mode);
    }

    @Test
    void parseUrlDefaultsHostPortAndOperator() throws Exception {
        VngJdbcDriver.ParsedUrl parsed = VngJdbcDriver.parseUrl(
                "jdbc:voltnuerongrid://:8080/?adminKey=k", new Properties());
        assertEquals("http://127.0.0.1:8080", parsed.config.baseUrl);
        assertNull(parsed.database);
        assertEquals("admin", parsed.config.operatorId);
    }

    @Test
    void resultSetIteratesOverEngineRows() throws Exception {
        // Build a VngResultSet from a sql/execute JSON body (no network), then
        // wrap it in the JDBC ResultSet and iterate via the java.sql API.
        String body = "{\"status\":\"ok\",\"columns\":[\"id\",\"name\"],"
                + "\"rows\":[[\"1\",\"alice\"],[\"2\",\"bob\"]]}";
        VngResultSet engineRs = invokeFromJson(body);
        VngJdbcResultSet rs = new VngJdbcResultSet(null, engineRs);

        ResultSetMetaData md = rs.getMetaData();
        assertEquals(2, md.getColumnCount());
        assertEquals("id", md.getColumnName(1));
        assertEquals("name", md.getColumnName(2));

        assertTrue(rs.next());
        assertEquals(1, rs.getInt("id"));
        assertEquals("alice", rs.getString("name"));
        assertEquals("alice", rs.getString(2));
        assertTrue(rs.next());
        assertEquals(2, rs.getInt(1));
        assertEquals("bob", rs.getString("name"));
        assertFalse(rs.next());
    }

    @Test
    void resultSetReportsSqlNull() throws Exception {
        String body = "{\"columns\":[\"id\",\"note\"],\"rows\":[[\"1\",null]]}";
        VngJdbcResultSet rs = new VngJdbcResultSet(null, invokeFromJson(body));
        assertTrue(rs.next());
        assertNull(rs.getString("note"));
        assertTrue(rs.wasNull());
        assertEquals("1", rs.getString("id"));
        assertFalse(rs.wasNull());
    }

    /** Live end-to-end test — only runs when VNG_JDBC_LIVE=1. */
    @Test
    void liveConnectQueryIterate() throws SQLException {
        if (!"1".equals(System.getenv("VNG_JDBC_LIVE"))) {
            return; // skipped unless explicitly enabled (E-5 live validation)
        }
        String key = System.getenv().getOrDefault("VNG_ADMIN_API_KEY", "secret");
        String url = "jdbc:voltnuerongrid://127.0.0.1:8080/jdbcdb?adminKey=" + key + "&operatorId=admin";
        String table = "jdbc_demo_" + System.nanoTime();
        try (Connection conn = DriverManager.getConnection(url);
             Statement st = conn.createStatement()) {
            st.execute("CREATE TABLE " + table + " (id INT PRIMARY KEY, name TEXT)");
            st.executeUpdate("INSERT INTO " + table + " (id, name) VALUES (1, 'alice')");
            try (ResultSet rs = st.executeQuery("SELECT id, name FROM " + table)) {
                assertTrue(rs.next());
                assertEquals("alice", rs.getString("name"));
                assertEquals(1, rs.getInt("id"));
            }
        }
    }

    /** Reflectively call the package-private {@code VngResultSet.fromJson}. */
    private static VngResultSet invokeFromJson(String body) throws Exception {
        Method m = VngResultSet.class.getDeclaredMethod("fromJson", String.class);
        m.setAccessible(true);
        return (VngResultSet) m.invoke(null, body);
    }
}
