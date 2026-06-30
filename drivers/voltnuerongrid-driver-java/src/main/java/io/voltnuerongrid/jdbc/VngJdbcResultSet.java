package io.voltnuerongrid.jdbc;

import com.voltnuerongrid.driver.VngResultSet;

import java.io.InputStream;
import java.io.Reader;
import java.math.BigDecimal;
import java.net.URL;
import java.sql.Array;
import java.sql.Blob;
import java.sql.Clob;
import java.sql.Date;
import java.sql.NClob;
import java.sql.Ref;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.RowId;
import java.sql.SQLException;
import java.sql.SQLFeatureNotSupportedException;
import java.sql.SQLWarning;
import java.sql.SQLXML;
import java.sql.Statement;
import java.sql.Time;
import java.sql.Timestamp;
import java.util.Calendar;
import java.util.Map;

/**
 * D-2: forward-only, read-only {@code java.sql.ResultSet} over the engine's
 * {@link VngResultSet}. All column values are stored as strings and coerced on
 * access; SQL NULL is reported via {@link #wasNull()}.
 */
public final class VngJdbcResultSet implements ResultSet {

    private final Statement statement;
    private final VngResultSet delegate;
    private boolean closed = false;
    private boolean lastWasNull = false;

    VngJdbcResultSet(Statement statement, VngResultSet delegate) {
        this.statement = statement;
        this.delegate = delegate;
    }

    private void ensureOpen() throws SQLException {
        if (closed) {
            throw new SQLException("result set is closed");
        }
    }

    @Override
    public boolean next() throws SQLException {
        ensureOpen();
        return delegate.next();
    }

    @Override
    public void close() {
        closed = true;
    }

    @Override
    public boolean isClosed() {
        return closed;
    }

    @Override
    public boolean wasNull() {
        return lastWasNull;
    }

    // ── String / primitive getters by 1-based index ─────────────────────────

    @Override
    public String getString(int columnIndex) throws SQLException {
        ensureOpen();
        String v = delegate.getString(columnIndex - 1);
        lastWasNull = (v == null);
        return v;
    }

    @Override
    public String getString(String columnLabel) throws SQLException {
        ensureOpen();
        String v = delegate.getString(columnLabel);
        lastWasNull = (v == null);
        return v;
    }

    @Override
    public boolean getBoolean(int columnIndex) throws SQLException {
        String v = getString(columnIndex);
        return v != null && (v.equalsIgnoreCase("true") || v.equals("1") || v.equalsIgnoreCase("t"));
    }

    @Override
    public boolean getBoolean(String columnLabel) throws SQLException {
        return getBoolean(findColumn(columnLabel));
    }

    @Override
    public byte getByte(int columnIndex) throws SQLException {
        String v = getString(columnIndex);
        return (v == null || v.isEmpty()) ? 0 : Byte.parseByte(v.trim());
    }

    @Override
    public byte getByte(String columnLabel) throws SQLException {
        return getByte(findColumn(columnLabel));
    }

    @Override
    public short getShort(int columnIndex) throws SQLException {
        String v = getString(columnIndex);
        return (v == null || v.isEmpty()) ? 0 : Short.parseShort(v.trim());
    }

    @Override
    public short getShort(String columnLabel) throws SQLException {
        return getShort(findColumn(columnLabel));
    }

    @Override
    public int getInt(int columnIndex) throws SQLException {
        String v = getString(columnIndex);
        return (v == null || v.isEmpty()) ? 0 : Integer.parseInt(v.trim());
    }

    @Override
    public int getInt(String columnLabel) throws SQLException {
        return getInt(findColumn(columnLabel));
    }

    @Override
    public long getLong(int columnIndex) throws SQLException {
        String v = getString(columnIndex);
        return (v == null || v.isEmpty()) ? 0L : Long.parseLong(v.trim());
    }

    @Override
    public long getLong(String columnLabel) throws SQLException {
        return getLong(findColumn(columnLabel));
    }

    @Override
    public float getFloat(int columnIndex) throws SQLException {
        String v = getString(columnIndex);
        return (v == null || v.isEmpty()) ? 0f : Float.parseFloat(v.trim());
    }

    @Override
    public float getFloat(String columnLabel) throws SQLException {
        return getFloat(findColumn(columnLabel));
    }

    @Override
    public double getDouble(int columnIndex) throws SQLException {
        String v = getString(columnIndex);
        return (v == null || v.isEmpty()) ? 0d : Double.parseDouble(v.trim());
    }

    @Override
    public double getDouble(String columnLabel) throws SQLException {
        return getDouble(findColumn(columnLabel));
    }

    @Override
    public BigDecimal getBigDecimal(int columnIndex) throws SQLException {
        String v = getString(columnIndex);
        return (v == null || v.isEmpty()) ? null : new BigDecimal(v.trim());
    }

    @Override
    public BigDecimal getBigDecimal(String columnLabel) throws SQLException {
        return getBigDecimal(findColumn(columnLabel));
    }

    @Override
    public Object getObject(int columnIndex) throws SQLException {
        return getString(columnIndex);
    }

    @Override
    public Object getObject(String columnLabel) throws SQLException {
        return getString(columnLabel);
    }

    @Override
    public int findColumn(String columnLabel) throws SQLException {
        int idx = delegate.columns().indexOf(columnLabel);
        if (idx < 0) {
            throw new SQLException("unknown column: " + columnLabel);
        }
        return idx + 1; // JDBC columns are 1-based.
    }

    @Override
    public ResultSetMetaData getMetaData() {
        return new VngResultSetMetaData(delegate.columns());
    }

    @Override
    public Statement getStatement() {
        return statement;
    }

    @Override
    public int getType() {
        return ResultSet.TYPE_FORWARD_ONLY;
    }

    @Override
    public int getConcurrency() {
        return ResultSet.CONCUR_READ_ONLY;
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
    public int getHoldability() {
        return ResultSet.CLOSE_CURSORS_AT_COMMIT;
    }

    @Override
    public SQLWarning getWarnings() {
        return null;
    }

    @Override
    public void clearWarnings() {
    }

    @Override
    public String getCursorName() {
        return null;
    }

    @Override
    public int getRow() {
        return 0;
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

    // ── Everything below is unsupported (scrollable cursors, LOBs, updaters) ─

    private static SQLFeatureNotSupportedException nope(String f) {
        return new SQLFeatureNotSupportedException(f + " is not supported");
    }

    @Override public byte[] getBytes(int c) throws SQLException { throw nope("getBytes"); }
    @Override public byte[] getBytes(String c) throws SQLException { throw nope("getBytes"); }
    @Override public Date getDate(int c) throws SQLException { throw nope("getDate"); }
    @Override public Date getDate(String c) throws SQLException { throw nope("getDate"); }
    @Override public Date getDate(int c, Calendar cal) throws SQLException { throw nope("getDate"); }
    @Override public Date getDate(String c, Calendar cal) throws SQLException { throw nope("getDate"); }
    @Override public Time getTime(int c) throws SQLException { throw nope("getTime"); }
    @Override public Time getTime(String c) throws SQLException { throw nope("getTime"); }
    @Override public Time getTime(int c, Calendar cal) throws SQLException { throw nope("getTime"); }
    @Override public Time getTime(String c, Calendar cal) throws SQLException { throw nope("getTime"); }
    @Override public Timestamp getTimestamp(int c) throws SQLException { throw nope("getTimestamp"); }
    @Override public Timestamp getTimestamp(String c) throws SQLException { throw nope("getTimestamp"); }
    @Override public Timestamp getTimestamp(int c, Calendar cal) throws SQLException { throw nope("getTimestamp"); }
    @Override public Timestamp getTimestamp(String c, Calendar cal) throws SQLException { throw nope("getTimestamp"); }
    @Override public InputStream getAsciiStream(int c) throws SQLException { throw nope("getAsciiStream"); }
    @Override public InputStream getAsciiStream(String c) throws SQLException { throw nope("getAsciiStream"); }
    @Override @Deprecated public InputStream getUnicodeStream(int c) throws SQLException { throw nope("getUnicodeStream"); }
    @Override @Deprecated public InputStream getUnicodeStream(String c) throws SQLException { throw nope("getUnicodeStream"); }
    @Override public InputStream getBinaryStream(int c) throws SQLException { throw nope("getBinaryStream"); }
    @Override public InputStream getBinaryStream(String c) throws SQLException { throw nope("getBinaryStream"); }
    @Override public Reader getCharacterStream(int c) throws SQLException { throw nope("getCharacterStream"); }
    @Override public Reader getCharacterStream(String c) throws SQLException { throw nope("getCharacterStream"); }
    @Override @Deprecated public BigDecimal getBigDecimal(int c, int s) throws SQLException { throw nope("getBigDecimal(scale)"); }
    @Override @Deprecated public BigDecimal getBigDecimal(String c, int s) throws SQLException { throw nope("getBigDecimal(scale)"); }
    @Override public Object getObject(int c, Map<String, Class<?>> m) throws SQLException { throw nope("getObject(map)"); }
    @Override public Object getObject(String c, Map<String, Class<?>> m) throws SQLException { throw nope("getObject(map)"); }
    @Override public <T> T getObject(int c, Class<T> t) throws SQLException { throw nope("getObject(type)"); }
    @Override public <T> T getObject(String c, Class<T> t) throws SQLException { throw nope("getObject(type)"); }
    @Override public Ref getRef(int c) throws SQLException { throw nope("getRef"); }
    @Override public Ref getRef(String c) throws SQLException { throw nope("getRef"); }
    @Override public Blob getBlob(int c) throws SQLException { throw nope("getBlob"); }
    @Override public Blob getBlob(String c) throws SQLException { throw nope("getBlob"); }
    @Override public Clob getClob(int c) throws SQLException { throw nope("getClob"); }
    @Override public Clob getClob(String c) throws SQLException { throw nope("getClob"); }
    @Override public Array getArray(int c) throws SQLException { throw nope("getArray"); }
    @Override public Array getArray(String c) throws SQLException { throw nope("getArray"); }
    @Override public URL getURL(int c) throws SQLException { throw nope("getURL"); }
    @Override public URL getURL(String c) throws SQLException { throw nope("getURL"); }
    @Override public RowId getRowId(int c) throws SQLException { throw nope("getRowId"); }
    @Override public RowId getRowId(String c) throws SQLException { throw nope("getRowId"); }
    @Override public NClob getNClob(int c) throws SQLException { throw nope("getNClob"); }
    @Override public NClob getNClob(String c) throws SQLException { throw nope("getNClob"); }
    @Override public SQLXML getSQLXML(int c) throws SQLException { throw nope("getSQLXML"); }
    @Override public SQLXML getSQLXML(String c) throws SQLException { throw nope("getSQLXML"); }
    @Override public String getNString(int c) throws SQLException { return getString(c); }
    @Override public String getNString(String c) throws SQLException { return getString(c); }
    @Override public Reader getNCharacterStream(int c) throws SQLException { throw nope("getNCharacterStream"); }
    @Override public Reader getNCharacterStream(String c) throws SQLException { throw nope("getNCharacterStream"); }

    @Override public boolean isBeforeFirst() throws SQLException { throw nope("isBeforeFirst"); }
    @Override public boolean isAfterLast() throws SQLException { throw nope("isAfterLast"); }
    @Override public boolean isFirst() throws SQLException { throw nope("isFirst"); }
    @Override public boolean isLast() throws SQLException { throw nope("isLast"); }
    @Override public void beforeFirst() throws SQLException { throw nope("beforeFirst"); }
    @Override public void afterLast() throws SQLException { throw nope("afterLast"); }
    @Override public boolean first() throws SQLException { throw nope("first"); }
    @Override public boolean last() throws SQLException { throw nope("last"); }
    @Override public boolean absolute(int row) throws SQLException { throw nope("absolute"); }
    @Override public boolean relative(int rows) throws SQLException { throw nope("relative"); }
    @Override public boolean previous() throws SQLException { throw nope("previous"); }
    @Override public boolean rowUpdated() throws SQLException { throw nope("rowUpdated"); }
    @Override public boolean rowInserted() throws SQLException { throw nope("rowInserted"); }
    @Override public boolean rowDeleted() throws SQLException { throw nope("rowDeleted"); }
    @Override public void insertRow() throws SQLException { throw nope("insertRow"); }
    @Override public void updateRow() throws SQLException { throw nope("updateRow"); }
    @Override public void deleteRow() throws SQLException { throw nope("deleteRow"); }
    @Override public void refreshRow() throws SQLException { throw nope("refreshRow"); }
    @Override public void cancelRowUpdates() throws SQLException { throw nope("cancelRowUpdates"); }
    @Override public void moveToInsertRow() throws SQLException { throw nope("moveToInsertRow"); }
    @Override public void moveToCurrentRow() throws SQLException { throw nope("moveToCurrentRow"); }

    // Updaters — read-only result set.
    @Override public void updateNull(int c) throws SQLException { throw nope("update"); }
    @Override public void updateNull(String c) throws SQLException { throw nope("update"); }
    @Override public void updateBoolean(int c, boolean x) throws SQLException { throw nope("update"); }
    @Override public void updateBoolean(String c, boolean x) throws SQLException { throw nope("update"); }
    @Override public void updateByte(int c, byte x) throws SQLException { throw nope("update"); }
    @Override public void updateByte(String c, byte x) throws SQLException { throw nope("update"); }
    @Override public void updateShort(int c, short x) throws SQLException { throw nope("update"); }
    @Override public void updateShort(String c, short x) throws SQLException { throw nope("update"); }
    @Override public void updateInt(int c, int x) throws SQLException { throw nope("update"); }
    @Override public void updateInt(String c, int x) throws SQLException { throw nope("update"); }
    @Override public void updateLong(int c, long x) throws SQLException { throw nope("update"); }
    @Override public void updateLong(String c, long x) throws SQLException { throw nope("update"); }
    @Override public void updateFloat(int c, float x) throws SQLException { throw nope("update"); }
    @Override public void updateFloat(String c, float x) throws SQLException { throw nope("update"); }
    @Override public void updateDouble(int c, double x) throws SQLException { throw nope("update"); }
    @Override public void updateDouble(String c, double x) throws SQLException { throw nope("update"); }
    @Override public void updateBigDecimal(int c, BigDecimal x) throws SQLException { throw nope("update"); }
    @Override public void updateBigDecimal(String c, BigDecimal x) throws SQLException { throw nope("update"); }
    @Override public void updateString(int c, String x) throws SQLException { throw nope("update"); }
    @Override public void updateString(String c, String x) throws SQLException { throw nope("update"); }
    @Override public void updateBytes(int c, byte[] x) throws SQLException { throw nope("update"); }
    @Override public void updateBytes(String c, byte[] x) throws SQLException { throw nope("update"); }
    @Override public void updateDate(int c, Date x) throws SQLException { throw nope("update"); }
    @Override public void updateDate(String c, Date x) throws SQLException { throw nope("update"); }
    @Override public void updateTime(int c, Time x) throws SQLException { throw nope("update"); }
    @Override public void updateTime(String c, Time x) throws SQLException { throw nope("update"); }
    @Override public void updateTimestamp(int c, Timestamp x) throws SQLException { throw nope("update"); }
    @Override public void updateTimestamp(String c, Timestamp x) throws SQLException { throw nope("update"); }
    @Override public void updateAsciiStream(int c, InputStream x, int l) throws SQLException { throw nope("update"); }
    @Override public void updateAsciiStream(String c, InputStream x, int l) throws SQLException { throw nope("update"); }
    @Override public void updateAsciiStream(int c, InputStream x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateAsciiStream(String c, InputStream x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateAsciiStream(int c, InputStream x) throws SQLException { throw nope("update"); }
    @Override public void updateAsciiStream(String c, InputStream x) throws SQLException { throw nope("update"); }
    @Override public void updateBinaryStream(int c, InputStream x, int l) throws SQLException { throw nope("update"); }
    @Override public void updateBinaryStream(String c, InputStream x, int l) throws SQLException { throw nope("update"); }
    @Override public void updateBinaryStream(int c, InputStream x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateBinaryStream(String c, InputStream x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateBinaryStream(int c, InputStream x) throws SQLException { throw nope("update"); }
    @Override public void updateBinaryStream(String c, InputStream x) throws SQLException { throw nope("update"); }
    @Override public void updateCharacterStream(int c, Reader x, int l) throws SQLException { throw nope("update"); }
    @Override public void updateCharacterStream(String c, Reader x, int l) throws SQLException { throw nope("update"); }
    @Override public void updateCharacterStream(int c, Reader x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateCharacterStream(String c, Reader x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateCharacterStream(int c, Reader x) throws SQLException { throw nope("update"); }
    @Override public void updateCharacterStream(String c, Reader x) throws SQLException { throw nope("update"); }
    @Override public void updateObject(int c, Object x, int s) throws SQLException { throw nope("update"); }
    @Override public void updateObject(String c, Object x, int s) throws SQLException { throw nope("update"); }
    @Override public void updateObject(int c, Object x) throws SQLException { throw nope("update"); }
    @Override public void updateObject(String c, Object x) throws SQLException { throw nope("update"); }
    @Override public void updateRef(int c, Ref x) throws SQLException { throw nope("update"); }
    @Override public void updateRef(String c, Ref x) throws SQLException { throw nope("update"); }
    @Override public void updateBlob(int c, Blob x) throws SQLException { throw nope("update"); }
    @Override public void updateBlob(String c, Blob x) throws SQLException { throw nope("update"); }
    @Override public void updateBlob(int c, InputStream x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateBlob(String c, InputStream x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateBlob(int c, InputStream x) throws SQLException { throw nope("update"); }
    @Override public void updateBlob(String c, InputStream x) throws SQLException { throw nope("update"); }
    @Override public void updateClob(int c, Clob x) throws SQLException { throw nope("update"); }
    @Override public void updateClob(String c, Clob x) throws SQLException { throw nope("update"); }
    @Override public void updateClob(int c, Reader x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateClob(String c, Reader x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateClob(int c, Reader x) throws SQLException { throw nope("update"); }
    @Override public void updateClob(String c, Reader x) throws SQLException { throw nope("update"); }
    @Override public void updateArray(int c, Array x) throws SQLException { throw nope("update"); }
    @Override public void updateArray(String c, Array x) throws SQLException { throw nope("update"); }
    @Override public void updateRowId(int c, RowId x) throws SQLException { throw nope("update"); }
    @Override public void updateRowId(String c, RowId x) throws SQLException { throw nope("update"); }
    @Override public void updateNString(int c, String x) throws SQLException { throw nope("update"); }
    @Override public void updateNString(String c, String x) throws SQLException { throw nope("update"); }
    @Override public void updateNClob(int c, NClob x) throws SQLException { throw nope("update"); }
    @Override public void updateNClob(String c, NClob x) throws SQLException { throw nope("update"); }
    @Override public void updateNClob(int c, Reader x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateNClob(String c, Reader x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateNClob(int c, Reader x) throws SQLException { throw nope("update"); }
    @Override public void updateNClob(String c, Reader x) throws SQLException { throw nope("update"); }
    @Override public void updateSQLXML(int c, SQLXML x) throws SQLException { throw nope("update"); }
    @Override public void updateSQLXML(String c, SQLXML x) throws SQLException { throw nope("update"); }
    @Override public void updateNCharacterStream(int c, Reader x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateNCharacterStream(String c, Reader x, long l) throws SQLException { throw nope("update"); }
    @Override public void updateNCharacterStream(int c, Reader x) throws SQLException { throw nope("update"); }
    @Override public void updateNCharacterStream(String c, Reader x) throws SQLException { throw nope("update"); }
}
