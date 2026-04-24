"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.runConnectivityChecks = runConnectivityChecks;
exports.executeSql = executeSql;
exports.analyzeSql = analyzeSql;
exports.getSchemaRegistry = getSchemaRegistry;
exports.requestRuntime = requestRuntime;
exports.toPermissionMessage = toPermissionMessage;
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
    const url = `${connection.settings.baseUrl}${options.path}`;
    const headers = {
        "content-type": "application/json",
    };
    if (connection.settings.mode === "admin" || connection.settings.mode === "operator") {
        if (connection.adminApiKey) {
            headers["x-vng-admin-key"] = connection.adminApiKey;
        }
    }
    if (connection.settings.mode === "operator" && connection.settings.operatorId) {
        headers["x-vng-operator-id"] = connection.settings.operatorId;
    }
    if (connection.settings.mode === "tenant") {
        if (connection.settings.tenantId) {
            headers["x-vng-tenant-id"] = connection.settings.tenantId;
        }
        if (connection.settings.userId) {
            headers["x-vng-user-id"] = connection.settings.userId;
        }
    }
    const response = await fetch(url, {
        method: options.method,
        headers,
        body: options.body ? JSON.stringify(options.body) : undefined,
    });
    const bodyText = await response.text();
    return {
        status: response.status,
        bodyText,
    };
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
//# sourceMappingURL=client.js.map