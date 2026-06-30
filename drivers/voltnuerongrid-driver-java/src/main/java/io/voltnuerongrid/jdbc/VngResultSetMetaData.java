package io.voltnuerongrid.jdbc;

import java.sql.ResultSetMetaData;
import java.sql.SQLException;
import java.sql.Types;
import java.util.List;

/**
 * D-2: minimal {@code java.sql.ResultSetMetaData} reporting the column names of
 * a {@link com.voltnuerongrid.driver.VngResultSet}. All columns are advertised
 * as nullable VARCHAR (the engine returns text-encoded cells).
 */
public final class VngResultSetMetaData implements ResultSetMetaData {

    private final List<String> columns;

    VngResultSetMetaData(List<String> columns) {
        this.columns = columns;
    }

    private void check(int column) throws SQLException {
        if (column < 1 || column > columns.size()) {
            throw new SQLException("column index out of range: " + column);
        }
    }

    @Override
    public int getColumnCount() {
        return columns.size();
    }

    @Override
    public String getColumnName(int column) throws SQLException {
        check(column);
        return columns.get(column - 1);
    }

    @Override
    public String getColumnLabel(int column) throws SQLException {
        return getColumnName(column);
    }

    @Override
    public int getColumnType(int column) throws SQLException {
        check(column);
        return Types.VARCHAR;
    }

    @Override
    public String getColumnTypeName(int column) throws SQLException {
        check(column);
        return "VARCHAR";
    }

    @Override
    public String getColumnClassName(int column) throws SQLException {
        check(column);
        return String.class.getName();
    }

    @Override
    public int isNullable(int column) throws SQLException {
        check(column);
        return ResultSetMetaData.columnNullable;
    }

    @Override
    public boolean isCaseSensitive(int column) {
        return true;
    }

    @Override
    public boolean isSearchable(int column) {
        return true;
    }

    @Override
    public boolean isCurrency(int column) {
        return false;
    }

    @Override
    public boolean isSigned(int column) {
        return false;
    }

    @Override
    public int getColumnDisplaySize(int column) {
        return 255;
    }

    @Override
    public int getPrecision(int column) {
        return 0;
    }

    @Override
    public int getScale(int column) {
        return 0;
    }

    @Override
    public boolean isAutoIncrement(int column) {
        return false;
    }

    @Override
    public boolean isReadOnly(int column) {
        return true;
    }

    @Override
    public boolean isWritable(int column) {
        return false;
    }

    @Override
    public boolean isDefinitelyWritable(int column) {
        return false;
    }

    @Override
    public String getSchemaName(int column) {
        return "";
    }

    @Override
    public String getTableName(int column) {
        return "";
    }

    @Override
    public String getCatalogName(int column) {
        return "";
    }

    @Override
    public <T> T unwrap(Class<T> iface) throws SQLException {
        if (iface.isInstance(this)) {
            return iface.cast(this);
        }
        throw new SQLException("not a wrapper for " + iface);
    }

    @Override
    public boolean isWrapperFor(Class<?> iface) {
        return iface.isInstance(this);
    }
}
