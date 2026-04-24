"use strict";
/**
 * Connection model representing a database connection configuration
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.validateConnectionSettings = validateConnectionSettings;
exports.createDefaultConnection = createDefaultConnection;
/**
 * Validate connection settings
 */
function validateConnectionSettings(settings) {
    if (!settings.name || settings.name.trim().length === 0) {
        return "Connection name is required";
    }
    if (!settings.host || settings.host.trim().length === 0) {
        return "Host is required";
    }
    if (!settings.port || settings.port < 1 || settings.port > 65535) {
        return "Port must be between 1 and 65535";
    }
    if (!settings.baseUrl || settings.baseUrl.trim().length === 0) {
        return "Base URL is required";
    }
    if (settings.mode === "operator" && !settings.operatorId) {
        return "Operator ID required for operator mode";
    }
    if (settings.mode === "tenant" && !settings.tenantId) {
        return "Tenant ID required for tenant mode";
    }
    if (settings.ssl?.enabled) {
        const sslPaths = [settings.ssl.caPath, settings.ssl.certPath, settings.ssl.keyPath];
        if (sslPaths.some((path) => path !== undefined && path.trim().length === 0)) {
            return "SSL certificate paths cannot be empty";
        }
    }
    if (settings.advanced?.connectionTimeout !== undefined && settings.advanced.connectionTimeout <= 0) {
        return "Connection timeout must be greater than 0";
    }
    if (settings.advanced?.idleTimeout !== undefined && settings.advanced.idleTimeout <= 0) {
        return "Idle timeout must be greater than 0";
    }
    if (settings.advanced?.maxConnections !== undefined && settings.advanced.maxConnections <= 0) {
        return "Max connections must be greater than 0";
    }
    return null;
}
/**
 * Create a default connection template
 */
function createDefaultConnection(overrides) {
    const now = Date.now();
    return {
        id: `conn-${now}`,
        name: overrides?.name || "New Connection",
        serverType: overrides?.serverType || "voltnuerongrid",
        runtimeTarget: overrides?.runtimeTarget || "local",
        baseUrl: overrides?.baseUrl || "http://127.0.0.1:8080",
        host: overrides?.host || "127.0.0.1",
        port: overrides?.port || 8080,
        database: overrides?.database,
        username: overrides?.username,
        mode: overrides?.mode || "admin",
        operatorId: overrides?.operatorId,
        tenantId: overrides?.tenantId,
        userId: overrides?.userId,
        ssl: overrides?.ssl || {
            enabled: false,
        },
        advanced: overrides?.advanced || {
            connectionTimeout: 5000,
            idleTimeout: 300000,
            keepAlive: true,
            maxConnections: 10,
        },
        createdAt: now,
        ...overrides,
    };
}
//# sourceMappingURL=Connection.js.map