package io.voltnuerongrid.jdbc;

import com.voltnuerongrid.driver.DriverConfig;
import com.voltnuerongrid.driver.VoltNueronGridDriver;

import java.sql.Array;
import java.sql.Blob;
import java.sql.CallableStatement;
import java.sql.Clob;
import java.sql.Connection;
import java.sql.DatabaseMetaData;
import java.sql.NClob;
import java.sql.PreparedStatement;
import java.sql.SQLClientInfoException;
import java.sql.SQLException;
import java.sql.SQLFeatureNotSupportedException;
import java.sql.SQLWarning;
import java.sql.SQLXML;
import java.sql.Savepoint;
import java.sql.Statement;
import java.sql.Struct;
import java.util.Map;
import java.util.Properties;
import java.util.concurrent.Executor;

/**
 * D-2: {@code java.sql.Connection} over the VoltNueronGrid HTTP driver.
 *
 * <p>Supports the core surface used by tools and JDBC clients:
 * {@link #createStatement()}, transaction no-ops, metadata, and lifecycle.
 * Unsupported advanced features throw {@link SQLFeatureNotSupportedException}.
 */
public final class VngConnection implements Connection {

    private final VoltNueronGridDriver driver;
    private final DriverConfig config;
    private String catalog;
    private boolean closed = false;
    private boolean autoCommit = true;

    VngConnection(VoltNueronGridDriver driver, DriverConfig config, String database) {
        this.driver = driver;
        this.config = config;
        this.catalog = database;
    }

    /** Package-visible accessor for statements to reach the HTTP driver. */
    VoltNueronGridDriver httpDriver() {
        return driver;
    }

    private void ensureOpen() throws SQLException {
        if (closed) {
            throw new SQLException("connection is closed");
        }
    }

    @Override
    public Statement createStatement() throws SQLException {
        ensureOpen();
        return new VngStatement(this);
    }

    @Override
    public Statement createStatement(int resultSetType, int resultSetConcurrency) throws SQLException {
        return createStatement();
    }

    @Override
    public Statement createStatement(int rt, int rc, int rh) throws SQLException {
        return createStatement();
    }

    @Override
    public boolean isClosed() {
        return closed;
    }

    @Override
    public void close() {
        closed = true;
    }

    @Override
    public boolean isValid(int timeout) {
        return !closed;
    }

    @Override
    public void setAutoCommit(boolean autoCommit) {
        this.autoCommit = autoCommit;
    }

    @Override
    public boolean getAutoCommit() {
        return autoCommit;
    }

    @Override
    public void commit() {
        // Transactions are expressed as SQL batches (BEGIN..COMMIT); no-op here.
    }

    @Override
    public void rollback() {
        // No client-side rollback state to discard.
    }

    @Override
    public String getCatalog() {
        return catalog;
    }

    @Override
    public void setCatalog(String catalog) {
        this.catalog = catalog;
    }

    @Override
    public DatabaseMetaData getMetaData() throws SQLException {
        throw unsupported("getMetaData");
    }

    @Override
    public String getSchema() {
        return catalog;
    }

    @Override
    public void setSchema(String schema) {
        this.catalog = schema;
    }

    @Override
    public SQLWarning getWarnings() {
        return null;
    }

    @Override
    public void clearWarnings() {
    }

    @Override
    public void setReadOnly(boolean readOnly) {
    }

    @Override
    public boolean isReadOnly() {
        return false;
    }

    @Override
    public void setTransactionIsolation(int level) {
    }

    @Override
    public int getTransactionIsolation() {
        return Connection.TRANSACTION_READ_COMMITTED;
    }

    @Override
    public Properties getClientInfo() {
        return new Properties();
    }

    @Override
    public String getClientInfo(String name) {
        return null;
    }

    @Override
    public void setClientInfo(String name, String value) {
    }

    @Override
    public void setClientInfo(Properties properties) {
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

    @Override
    public void setNetworkTimeout(Executor executor, int milliseconds) {
    }

    @Override
    public int getNetworkTimeout() {
        return config.requestTimeoutMs();
    }

    @Override
    public void abort(Executor executor) {
        close();
    }

    @Override
    public void setHoldability(int holdability) {
    }

    @Override
    public int getHoldability() {
        return java.sql.ResultSet.CLOSE_CURSORS_AT_COMMIT;
    }

    @Override
    public String nativeSQL(String sql) {
        return sql;
    }

    // ── Unsupported advanced JDBC features ──────────────────────────────────

    private static SQLFeatureNotSupportedException unsupported(String feature) {
        return new SQLFeatureNotSupportedException(feature + " is not supported");
    }

    @Override
    public PreparedStatement prepareStatement(String sql) throws SQLException {
        throw unsupported("prepareStatement");
    }

    @Override
    public PreparedStatement prepareStatement(String sql, int a, int b) throws SQLException {
        throw unsupported("prepareStatement");
    }

    @Override
    public PreparedStatement prepareStatement(String sql, int a, int b, int c) throws SQLException {
        throw unsupported("prepareStatement");
    }

    @Override
    public PreparedStatement prepareStatement(String sql, int autoGeneratedKeys) throws SQLException {
        throw unsupported("prepareStatement");
    }

    @Override
    public PreparedStatement prepareStatement(String sql, int[] columnIndexes) throws SQLException {
        throw unsupported("prepareStatement");
    }

    @Override
    public PreparedStatement prepareStatement(String sql, String[] columnNames) throws SQLException {
        throw unsupported("prepareStatement");
    }

    @Override
    public CallableStatement prepareCall(String sql) throws SQLException {
        throw unsupported("prepareCall");
    }

    @Override
    public CallableStatement prepareCall(String sql, int a, int b) throws SQLException {
        throw unsupported("prepareCall");
    }

    @Override
    public CallableStatement prepareCall(String sql, int a, int b, int c) throws SQLException {
        throw unsupported("prepareCall");
    }

    @Override
    public Map<String, Class<?>> getTypeMap() throws SQLException {
        throw unsupported("getTypeMap");
    }

    @Override
    public void setTypeMap(Map<String, Class<?>> map) throws SQLException {
        throw unsupported("setTypeMap");
    }

    @Override
    public Savepoint setSavepoint() throws SQLException {
        throw unsupported("setSavepoint");
    }

    @Override
    public Savepoint setSavepoint(String name) throws SQLException {
        throw unsupported("setSavepoint");
    }

    @Override
    public void rollback(Savepoint savepoint) throws SQLException {
        throw unsupported("rollback(Savepoint)");
    }

    @Override
    public void releaseSavepoint(Savepoint savepoint) throws SQLException {
        throw unsupported("releaseSavepoint");
    }

    @Override
    public Clob createClob() throws SQLException {
        throw unsupported("createClob");
    }

    @Override
    public Blob createBlob() throws SQLException {
        throw unsupported("createBlob");
    }

    @Override
    public NClob createNClob() throws SQLException {
        throw unsupported("createNClob");
    }

    @Override
    public SQLXML createSQLXML() throws SQLException {
        throw unsupported("createSQLXML");
    }

    @Override
    public Array createArrayOf(String typeName, Object[] elements) throws SQLException {
        throw unsupported("createArrayOf");
    }

    @Override
    public Struct createStruct(String typeName, Object[] attributes) throws SQLException {
        throw unsupported("createStruct");
    }
}
