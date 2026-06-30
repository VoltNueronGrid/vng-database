package io.voltnuerongrid.jdbc;

import com.voltnuerongrid.driver.VngResultSet;

import java.sql.Connection;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.SQLFeatureNotSupportedException;
import java.sql.SQLWarning;
import java.sql.Statement;

/**
 * D-2: {@code java.sql.Statement} that executes SQL through the HTTP driver and
 * returns a {@link VngJdbcResultSet} wrapping the engine's {@link VngResultSet}.
 */
public final class VngStatement implements Statement {

    private final VngConnection connection;
    private VngJdbcResultSet currentResultSet;
    private int updateCount = -1;
    private boolean closed = false;
    private int maxRows = 0;
    private int queryTimeoutSeconds = 0;

    VngStatement(VngConnection connection) {
        this.connection = connection;
    }

    private void ensureOpen() throws SQLException {
        if (closed) {
            throw new SQLException("statement is closed");
        }
    }

    @Override
    public ResultSet executeQuery(String sql) throws SQLException {
        ensureOpen();
        try {
            VngResultSet rs = connection.httpDriver().executeQuery(sql);
            currentResultSet = new VngJdbcResultSet(this, rs);
            updateCount = -1;
            return currentResultSet;
        } catch (RuntimeException e) {
            throw new SQLException("executeQuery failed: " + e.getMessage(), e);
        }
    }

    @Override
    public int executeUpdate(String sql) throws SQLException {
        ensureOpen();
        try {
            VngResultSet rs = connection.httpDriver().executeQuery(sql);
            updateCount = rs.rowCount();
            currentResultSet = null;
            return updateCount;
        } catch (RuntimeException e) {
            throw new SQLException("executeUpdate failed: " + e.getMessage(), e);
        }
    }

    @Override
    public boolean execute(String sql) throws SQLException {
        ensureOpen();
        VngResultSet rs;
        try {
            rs = connection.httpDriver().executeQuery(sql);
        } catch (RuntimeException e) {
            throw new SQLException("execute failed: " + e.getMessage(), e);
        }
        boolean isQuery = sql.trim().toUpperCase().startsWith("SELECT");
        if (isQuery) {
            currentResultSet = new VngJdbcResultSet(this, rs);
            updateCount = -1;
            return true;
        }
        currentResultSet = null;
        updateCount = rs.rowCount();
        return false;
    }

    @Override
    public ResultSet getResultSet() {
        return currentResultSet;
    }

    @Override
    public int getUpdateCount() {
        return updateCount;
    }

    @Override
    public boolean getMoreResults() {
        currentResultSet = null;
        return false;
    }

    @Override
    public boolean getMoreResults(int current) {
        return false;
    }

    @Override
    public Connection getConnection() {
        return connection;
    }

    @Override
    public void close() {
        closed = true;
        currentResultSet = null;
    }

    @Override
    public boolean isClosed() {
        return closed;
    }

    @Override
    public int getMaxRows() {
        return maxRows;
    }

    @Override
    public void setMaxRows(int max) {
        this.maxRows = max;
    }

    @Override
    public int getQueryTimeout() {
        return queryTimeoutSeconds;
    }

    @Override
    public void setQueryTimeout(int seconds) {
        this.queryTimeoutSeconds = seconds;
    }

    @Override
    public SQLWarning getWarnings() {
        return null;
    }

    @Override
    public void clearWarnings() {
    }

    @Override
    public int getResultSetType() {
        return ResultSet.TYPE_FORWARD_ONLY;
    }

    @Override
    public int getResultSetConcurrency() {
        return ResultSet.CONCUR_READ_ONLY;
    }

    @Override
    public int getResultSetHoldability() {
        return ResultSet.CLOSE_CURSORS_AT_COMMIT;
    }

    @Override
    public int getFetchDirection() {
        return ResultSet.FETCH_FORWARD;
    }

    @Override
    public void setFetchDirection(int direction) {
    }

    @Override
    public int getFetchSize() {
        return 0;
    }

    @Override
    public void setFetchSize(int rows) {
    }

    @Override
    public void setEscapeProcessing(boolean enable) {
    }

    @Override
    public void setCursorName(String name) {
    }

    @Override
    public void setPoolable(boolean poolable) {
    }

    @Override
    public boolean isPoolable() {
        return false;
    }

    @Override
    public void closeOnCompletion() {
    }

    @Override
    public boolean isCloseOnCompletion() {
        return false;
    }

    @Override
    public int getMaxFieldSize() {
        return 0;
    }

    @Override
    public void setMaxFieldSize(int max) {
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

    // ── Unsupported batch / generated-keys features ─────────────────────────

    private static SQLFeatureNotSupportedException unsupported(String f) {
        return new SQLFeatureNotSupportedException(f + " is not supported");
    }

    @Override
    public void cancel() throws SQLException {
        throw unsupported("cancel");
    }

    @Override
    public void addBatch(String sql) throws SQLException {
        throw unsupported("addBatch");
    }

    @Override
    public void clearBatch() throws SQLException {
        throw unsupported("clearBatch");
    }

    @Override
    public int[] executeBatch() throws SQLException {
        throw unsupported("executeBatch");
    }

    @Override
    public ResultSet getGeneratedKeys() throws SQLException {
        throw unsupported("getGeneratedKeys");
    }

    @Override
    public int executeUpdate(String sql, int autoGeneratedKeys) throws SQLException {
        throw unsupported("executeUpdate(autoGeneratedKeys)");
    }

    @Override
    public int executeUpdate(String sql, int[] columnIndexes) throws SQLException {
        throw unsupported("executeUpdate(columnIndexes)");
    }

    @Override
    public int executeUpdate(String sql, String[] columnNames) throws SQLException {
        throw unsupported("executeUpdate(columnNames)");
    }

    @Override
    public boolean execute(String sql, int autoGeneratedKeys) throws SQLException {
        throw unsupported("execute(autoGeneratedKeys)");
    }

    @Override
    public boolean execute(String sql, int[] columnIndexes) throws SQLException {
        throw unsupported("execute(columnIndexes)");
    }

    @Override
    public boolean execute(String sql, String[] columnNames) throws SQLException {
        throw unsupported("execute(columnNames)");
    }
}
