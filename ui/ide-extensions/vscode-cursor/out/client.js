"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.runConnectivityChecks = runConnectivityChecks;
exports.executeSql = executeSql;
exports.analyzeSql = analyzeSql;
exports.getSchemaRegistry = getSchemaRegistry;
exports.requestRuntime = requestRuntime;
exports.toPermissionMessage = toPermissionMessage;
const DriverAdapter_1 = require("./services/DriverAdapter");
async function runConnectivityChecks(connection) {
    const health = await requestRuntime(connection, {
        method: "GET",
        path: "/health",
    });
    const sql = await requestRuntime(connection, {
        method: "POST",
        path: "/api/v1/sql/execute",
        body: {
            sql_batch: ["SELECT 1;"],
            request_id: "ide-connectivity-check",
        },
    });
    const schema = await requestRuntime(connection, {
        method: "GET",
        path: "/api/v1/ingest/schema/registry",
    });
    return [
        toResult("GET", "/health", health.status, health.bodyText),
        toResult("POST", "/api/v1/sql/execute", sql.status, sql.bodyText),
        toResult("GET", "/api/v1/ingest/schema/registry", schema.status, schema.bodyText),
    ];
}
async function executeSql(connection, sql) {
    return requestRuntime(connection, {
        method: "POST",
        path: "/api/v1/sql/execute",
        body: {
            sql_batch: [sql],
            request_id: "ide-query-runner",
        },
    });
}
async function analyzeSql(connection, sql) {
    return requestRuntime(connection, {
        method: "POST",
        path: "/api/v1/sql/analyze",
        body: {
            sql,
            request_id: "ide-query-diagnostics",
        },
    });
}
async function getSchemaRegistry(connection) {
    return requestRuntime(connection, {
        method: "GET",
        path: "/api/v1/ingest/schema/registry",
    });
}
async function requestRuntime(connection, options) {
    try {
        const driver = (0, DriverAdapter_1.makeVngDriver)(runtimeToManagedConnection(connection));
        const req = {
            method: options.method,
            url: `${connection.settings.baseUrl}${options.path}`,
            headers: {
                "content-type": "application/json",
            },
            body: options.body,
        };
        // Keep checks deterministic and fast by disabling retries in ad-hoc probes.
        const result = await (0, DriverAdapter_1.executeDriverRequest)(req, { maxRetries: 0 });
        return {
            status: result.status,
            bodyText: result.bodyText,
        };
    }
    catch (error) {
        if (error instanceof DriverAdapter_1.DriverError) {
            return {
                status: error.statusCode ?? 0,
                bodyText: error.message,
            };
        }
        return {
            status: 0,
            bodyText: error instanceof Error ? error.message : "Unknown request error",
        };
    }
}
function toPermissionMessage(status, mode) {
    if (status === 401) {
        if (mode === "tenant") {
            return "Authentication failed. Verify tenant and user headers.";
        }
        return "Authentication failed. Verify admin key and operator headers.";
    }
    if (status === 403) {
        return "Permission denied for the selected identity and operation.";
    }
    return undefined;
}
function toResult(method, endpoint, status, bodyText) {
    const success = endpoint === "/health" ? status === 200 : status === 200 || status === 401 || status === 403;
    const detail = summarizeBody(bodyText);
    return {
        endpoint,
        method,
        ok: success,
        status,
        detail,
    };
}
function summarizeBody(bodyText) {
    if (!bodyText) {
        return "(empty response)";
    }
    if (bodyText.length <= 200) {
        return bodyText;
    }
    return `${bodyText.slice(0, 200)}...`;
}
function runtimeToManagedConnection(connection) {
    return {
        id: `runtime-${connection.settings.mode}-${connection.settings.baseUrl}`,
        settings: {
            id: `runtime-${connection.settings.mode}-${connection.settings.baseUrl}`,
            name: "Runtime",
            serverType: "voltnuerongrid",
            runtimeTarget: connection.settings.runtimeTarget,
            baseUrl: connection.settings.baseUrl,
            host: "127.0.0.1",
            port: 8080,
            mode: connection.settings.mode,
            adminKey: connection.adminApiKey,
            operatorId: connection.settings.operatorId,
            tenantId: connection.settings.tenantId,
            userId: connection.settings.userId,
            ssl: {
                enabled: false,
            },
            advanced: {
                connectionTimeout: 5000,
            },
            createdAt: Date.now(),
        },
        isActive: true,
        isConnected: false,
        state: "active",
        diagnostics: {},
    };
}
//# sourceMappingURL=client.js.map