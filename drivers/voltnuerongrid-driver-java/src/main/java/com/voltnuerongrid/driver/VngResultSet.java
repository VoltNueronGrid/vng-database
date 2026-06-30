package com.voltnuerongrid.driver;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * A forward-only, JDBC-style cursor over a VoltNueronGrid SQL result set.
 *
 * <p>Parsed from a {@code /api/v1/sql/execute} JSON response. Use
 * {@link #next()} to advance and the typed getters to read columns of the
 * current row.
 *
 * <pre>{@code
 * VngResultSet rs = driver.executeQuery("SELECT id, name FROM users");
 * while (rs.next()) {
 *     long id = rs.getLong("id");
 *     String name = rs.getString("name");
 * }
 * }</pre>
 */
public final class VngResultSet {

    private final List<String> columns;
    private final List<List<String>> rows;
    private int cursor = -1;

    VngResultSet(List<String> columns, List<List<String>> rows) {
        this.columns = columns;
        this.rows = rows;
    }

    /**
     * Builds a result set from a raw {@code sql/execute} JSON response body.
     *
     * <p>Handles both columnar rows ({@code rows:[["1","x"],...]}) and object
     * rows ({@code rows:[{"id":"1",...},...]}); scalar cells are stringified.
     */
    @SuppressWarnings("unchecked")
    static VngResultSet fromJson(String body) {
        Object root = Json.parse(body);
        if (!(root instanceof Map)) {
            return new VngResultSet(Collections.emptyList(), Collections.emptyList());
        }
        Map<String, Object> obj = (Map<String, Object>) root;

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
                } else {
                    rows.add(Collections.singletonList(scalar(row)));
                }
            }
        }
        return new VngResultSet(columns, rows);
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

    /**
     * Resolve a column name from a {@code columns[]} entry. The server may send
     * either a bare string ({@code "id"}) or an object carrying a {@code name}
     * field ({@code {"name":"id","data_type":"integer"}}); both yield the name.
     */
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

    /** Advances to the next row. Returns {@code true} if a row is now current. */
    public boolean next() {
        if (cursor + 1 < rows.size()) {
            cursor++;
            return true;
        }
        return false;
    }

    /** Number of rows in the result set. */
    public int rowCount() {
        return rows.size();
    }

    /** Ordered column names. */
    public List<String> columns() {
        return Collections.unmodifiableList(columns);
    }

    /** Returns the value of column {@code index} (0-based) in the current row. */
    public String getString(int index) {
        checkRow();
        List<String> row = rows.get(cursor);
        if (index < 0 || index >= row.size()) {
            throw DriverError.validation("column index out of range: " + index);
        }
        return row.get(index);
    }

    /** Returns the value of the named column in the current row. */
    public String getString(String column) {
        int idx = columns.indexOf(column);
        if (idx < 0) {
            throw DriverError.validation("unknown column: " + column);
        }
        return getString(idx);
    }

    /** Returns the named column parsed as an {@code int} (0 when null/blank). */
    public int getInt(String column) {
        String v = getString(column);
        return (v == null || v.isEmpty()) ? 0 : Integer.parseInt(v.trim());
    }

    /** Returns the named column parsed as a {@code long} (0 when null/blank). */
    public long getLong(String column) {
        String v = getString(column);
        return (v == null || v.isEmpty()) ? 0L : Long.parseLong(v.trim());
    }

    /** Returns the named column parsed as a {@code double} (0 when null/blank). */
    public double getDouble(String column) {
        String v = getString(column);
        return (v == null || v.isEmpty()) ? 0.0 : Double.parseDouble(v.trim());
    }

    /** Returns the current row as an ordered column→value map. */
    public Map<String, String> rowAsMap() {
        checkRow();
        Map<String, String> m = new LinkedHashMap<>();
        List<String> row = rows.get(cursor);
        for (int i = 0; i < columns.size() && i < row.size(); i++) {
            m.put(columns.get(i), row.get(i));
        }
        return m;
    }

    private void checkRow() {
        if (cursor < 0 || cursor >= rows.size()) {
            throw DriverError.validation("no current row — call next() first");
        }
    }
}
