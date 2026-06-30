package com.voltnuerongrid.driver;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** D-1: result-set parsing and typed accessors (no live server required). */
class VngResultSetTest {

    @Test
    void parsesColumnarResponseAndIterates() {
        String body = "{\"status\":\"ok\",\"columns\":[\"id\",\"name\"],"
                + "\"rows\":[[\"1\",\"alice\"],[\"2\",\"bob\"]]}";
        VngResultSet rs = VngResultSet.fromJson(body);

        assertEquals(2, rs.rowCount());
        assertEquals(2, rs.columns().size());

        assertTrue(rs.next());
        assertEquals(1L, rs.getLong("id"));
        assertEquals("alice", rs.getString("name"));
        assertEquals("alice", rs.getString(1));

        assertTrue(rs.next());
        assertEquals(2, rs.getInt("id"));
        assertEquals("bob", rs.getString("name"));

        assertFalse(rs.next());
    }

    @Test
    void parsesObjectRowsInferringColumns() {
        String body = "{\"rows\":[{\"id\":\"1\",\"name\":\"alice\"},{\"id\":\"2\",\"name\":\"bob\"}]}";
        VngResultSet rs = VngResultSet.fromJson(body);
        assertEquals(2, rs.rowCount());
        assertTrue(rs.columns().contains("id"));
        assertTrue(rs.columns().contains("name"));
        assertTrue(rs.next());
        assertEquals("alice", rs.getString("name"));
    }

    @Test
    void stringifiesNumericAndNullScalars() {
        String body = "{\"columns\":[\"n\"],\"rows\":[[1],[2.5],[null]]}";
        VngResultSet rs = VngResultSet.fromJson(body);
        assertTrue(rs.next());
        assertEquals("1", rs.getString("n"));
        assertTrue(rs.next());
        assertEquals("2.5", rs.getString("n"));
        assertTrue(rs.next());
        // null cell → empty/zero
        assertEquals(0L, rs.getLong("n"));
    }

    @Test
    void unknownColumnThrows() {
        VngResultSet rs = VngResultSet.fromJson("{\"columns\":[\"a\"],\"rows\":[[\"x\"]]}");
        assertTrue(rs.next());
        assertThrows(DriverError.class, () -> rs.getString("missing"));
    }

    @Test
    void noCurrentRowThrows() {
        VngResultSet rs = VngResultSet.fromJson("{\"columns\":[\"a\"],\"rows\":[[\"x\"]]}");
        assertThrows(DriverError.class, () -> rs.getString("a"));
    }

    @Test
    void jsonParserHandlesNestedAndEscapes() {
        Object v = Json.parse("{\"a\":[1,2,{\"b\":\"q\\\"x\"}],\"c\":true,\"d\":null}");
        assertTrue(v instanceof java.util.Map);
    }
}
