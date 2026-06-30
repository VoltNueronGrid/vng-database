package io.voltnuerongrid.jdbc;

import com.voltnuerongrid.driver.DriverConfig;
import com.voltnuerongrid.driver.VoltNueronGridDriver;

import java.sql.Connection;
import java.sql.Driver;
import java.sql.DriverManager;
import java.sql.DriverPropertyInfo;
import java.sql.SQLException;
import java.sql.SQLFeatureNotSupportedException;
import java.util.Properties;
import java.util.logging.Logger;

/**
 * D-2: {@code java.sql.Driver} implementation for VoltNueronGrid.
 *
 * <p>Registers itself with {@link DriverManager} on class load (and via the
 * {@code META-INF/services/java.sql.Driver} service file) so that
 * {@code DriverManager.getConnection("jdbc:voltnuerongrid://host:port/db?...")}
 * returns a working {@link VngConnection}.
 *
 * <p>URL format:
 * <pre>jdbc:voltnuerongrid://HOST:PORT/DATABASE?adminKey=KEY&amp;operatorId=ID&amp;mode=operator</pre>
 * Query parameters and {@link Properties} entries (adminKey, operatorId,
 * tenantId, userId, mode, sessionId) configure the underlying HTTP driver.
 */
public final class VngJdbcDriver implements Driver {

    /** JDBC sub-protocol prefix this driver accepts. */
    public static final String URL_PREFIX = "jdbc:voltnuerongrid:";

    static {
        try {
            DriverManager.registerDriver(new VngJdbcDriver());
        } catch (SQLException e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    @Override
    public boolean acceptsURL(String url) {
        return url != null && url.startsWith(URL_PREFIX);
    }

    @Override
    public Connection connect(String url, Properties info) throws SQLException {
        if (!acceptsURL(url)) {
            return null; // Per JDBC spec: return null when the URL is not ours.
        }
        ParsedUrl parsed = parseUrl(url, info);
        VoltNueronGridDriver httpDriver = new VoltNueronGridDriver(parsed.config);
        return new VngConnection(httpDriver, parsed.config, parsed.database);
    }

    /** Parsed JDBC URL: a {@link DriverConfig} plus the optional database name. */
    static final class ParsedUrl {
        final DriverConfig config;
        final String database;

        ParsedUrl(DriverConfig config, String database) {
            this.config = config;
            this.database = database;
        }
    }

    /**
     * Parse a {@code jdbc:voltnuerongrid://host:port/db?k=v} URL plus optional
     * {@link Properties} into a {@link DriverConfig} and database name.
     */
    static ParsedUrl parseUrl(String url, Properties info) throws SQLException {
        String rest = url.substring(URL_PREFIX.length());
        if (rest.startsWith("//")) {
            rest = rest.substring(2);
        }
        String query = "";
        int qIdx = rest.indexOf('?');
        if (qIdx >= 0) {
            query = rest.substring(qIdx + 1);
            rest = rest.substring(0, qIdx);
        }
        String hostPort = rest;
        String database = "";
        int slash = rest.indexOf('/');
        if (slash >= 0) {
            hostPort = rest.substring(0, slash);
            database = rest.substring(slash + 1);
        }
        String host = hostPort;
        int port = 8080;
        int colon = hostPort.indexOf(':');
        if (colon >= 0) {
            host = hostPort.substring(0, colon);
            try {
                port = Integer.parseInt(hostPort.substring(colon + 1).trim());
            } catch (NumberFormatException e) {
                throw new SQLException("invalid port in JDBC URL: " + url);
            }
        }
        if (host.isEmpty()) {
            host = "127.0.0.1";
        }

        Properties params = new Properties();
        if (info != null) {
            params.putAll(info);
        }
        for (String pair : query.split("&")) {
            if (pair.isEmpty()) {
                continue;
            }
            int eq = pair.indexOf('=');
            if (eq >= 0) {
                params.setProperty(pair.substring(0, eq), pair.substring(eq + 1));
            }
        }

        // Default to operator mode so both the admin key and an operator id are
        // sent (the server's SQL runtime RBAC requires a bound operator identity).
        String mode = params.getProperty("mode", "operator");
        DriverConfig.Builder builder = new DriverConfig.Builder()
                .baseUrl("http://" + host + ":" + port)
                .sessionId(params.getProperty("sessionId", "jdbc-" + System.nanoTime()))
                .mode(mode)
                .operatorId(params.getProperty("operatorId", "admin"));
        if (params.getProperty("adminKey") != null) {
            builder.adminApiKey(params.getProperty("adminKey"));
        }
        if (params.getProperty("tenantId") != null) {
            builder.tenantId(params.getProperty("tenantId"));
        }
        if (params.getProperty("userId") != null) {
            builder.userId(params.getProperty("userId"));
        }
        DriverConfig config = builder.build();
        return new ParsedUrl(config, database.isEmpty() ? null : database);
    }

    @Override
    public DriverPropertyInfo[] getPropertyInfo(String url, Properties info) {
        return new DriverPropertyInfo[0];
    }

    @Override
    public int getMajorVersion() {
        return 0;
    }

    @Override
    public int getMinorVersion() {
        return 1;
    }

    @Override
    public boolean jdbcCompliant() {
        // Not fully JDBC-compliant (simple-query subset over HTTP).
        return false;
    }

    @Override
    public Logger getParentLogger() throws SQLFeatureNotSupportedException {
        throw new SQLFeatureNotSupportedException("no java.util.logging parent logger");
    }
}
