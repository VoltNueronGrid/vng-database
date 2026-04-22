"use strict";
/**
 * DriverAdapter: bridges ConnectionSettings → DriverConfig and executes DriverRequests.
 *
 * S3-001: replaces ad-hoc fetch() calls with the TS driver abstraction so the extension
 * no longer constructs HTTP calls, auth headers, or retry logic directly.
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.DriverError = void 0;
exports.connectionToDriverConfig = connectionToDriverConfig;
exports.makeVngDriver = makeVngDriver;
exports.executeDriverRequest = executeDriverRequest;
const driver_typescript_1 = require("@voltnuerongrid/driver-typescript");
Object.defineProperty(exports, "DriverError", { enumerable: true, get: function () { return driver_typescript_1.DriverError; } });
/** Extension version injected as User-Agent for observability on the server side. */
const EXTENSION_USER_AGENT = "VoltNueronGrid-VSCode/0.3.2";
/**
 * Maps a VS Code extension Connection to a DriverConfig.
 *
 * - `sessionId` uses the connection's stable id so per-connection tracing works.
 * - `requestTimeoutMs` inherits the advanced connectionTimeout when set.
 * - `adminKey` → `adminApiKey` rename follows the driver-core-contract v1 field name.
 */
function connectionToDriverConfig(connection) {
    const s = connection.settings;
    const config = {
        baseUrl: s.baseUrl,
        sessionId: s.id,
        mode: s.mode,
    };
    if (s.adminKey) {
        config.adminApiKey = s.adminKey;
    }
    if (s.operatorId) {
        config.operatorId = s.operatorId;
    }
    if (s.tenantId) {
        config.tenantId = s.tenantId;
    }
    if (s.userId) {
        config.userId = s.userId;
    }
    if (s.advanced?.connectionTimeout && s.advanced.connectionTimeout > 0) {
        config.requestTimeoutMs = s.advanced.connectionTimeout;
    }
    // Preserve native endpoint when the connection specifies one (dual-transport path).
    if (s.nativeEndpoint?.trim()) {
        config.httpFallbackUrl = s.baseUrl;
        config.baseUrl = s.nativeEndpoint.trim();
    }
    return config;
}
/**
 * Creates a VoltNueronGridDriver for the given connection.
 * Throws a DriverError (validation) if the connection settings are insufficient
 * for the selected auth mode (e.g., admin mode without an adminKey).
 */
function makeVngDriver(connection) {
    return new driver_typescript_1.VoltNueronGridDriver(connectionToDriverConfig(connection));
}
/**
 * Executes a DriverRequest through `performDriverHttpRequest`, adding the
 * extension User-Agent header before dispatch.
 */
async function executeDriverRequest(req, opts) {
    const augmented = {
        ...req,
        headers: {
            ...req.headers,
            "user-agent": EXTENSION_USER_AGENT,
        },
    };
    return (0, driver_typescript_1.performDriverHttpRequest)(augmented, opts);
}
//# sourceMappingURL=DriverAdapter.js.map