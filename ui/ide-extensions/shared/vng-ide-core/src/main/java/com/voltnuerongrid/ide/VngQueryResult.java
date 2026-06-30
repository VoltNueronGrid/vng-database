package com.voltnuerongrid.ide;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * D-5: SDK-free query result for IDE extensions. Holds the column names and the
 * decoded rows (each a column→value map) parsed from a {@code /api/v1/sql/execute}
 * response, so any IDE (Eclipse/JetBrains) can render results without pulling in a
 * JSON library or the server's types.
 */
public final class VngQueryResult {

    private final String status;
    private final String routePath;
    private final List<String> columns;
    private final List<List<String>> rows;
    private final String error;

    VngQueryResult(String status, String routePath, List<String> columns,
                   List<List<String>> rows, String error) {
        this.status = status;
        this.routePath = routePath;
        this.columns = columns;
        this.rows = rows;
        this.error = error;
    }

    public String status() {
        return status;
    }

    public String routePath() {
        return routePath;
    }

    public List<String> columns() {
        return columns;
    }

    public List<List<String>> rows() {
        return rows;
    }

    public int rowCount() {
        return rows.size();
    }

    public boolean isError() {
        return error != null;
    }

    public String error() {
        return error;
    }

    /** Current row as an ordered column→value map. */
    public Map<String, String> rowAsMap(int rowIndex) {
        Map<String, String> m = new LinkedHashMap<>();
        if (rowIndex < 0 || rowIndex >= rows.size()) {
            return m;
        }
        List<String> row = rows.get(rowIndex);
        for (int i = 0; i < columns.size() && i < row.size(); i++) {
            m.put(columns.get(i), row.get(i));
        }
        return m;
    }

    /** Build an error result. */
    static VngQueryResult error(String message) {
        return new VngQueryResult("error", "", new ArrayList<>(), new ArrayList<>(), message);
    }
}
