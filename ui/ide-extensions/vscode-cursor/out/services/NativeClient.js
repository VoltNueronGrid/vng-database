"use strict";
/**
 * NativeClient: native socket transport for the VoltNueronGrid VSCode extension.
 *
 * Wraps the TypeScript driver's native session helpers so the extension can
 * route queries, health checks, and schema discovery through the low-latency
 * native wire protocol instead of HTTP when transportMode is set to "native"
 * or resolves to native under "auto".
 *
 * Uses a dynamic import so the extension activates even when the driver package
 * is not bundled (e.g. when installed via .vsix with --no-dependencies). In that
 * case all native calls return a descriptive error and HTTP transport is used.
 */
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.NativeClient = void 0;
exports.createNativeClient = createNativeClient;
/** Default native listener port (matches VNG_NATIVE_BIND default in the server). */
const DEFAULT_NATIVE_PORT = 7542;
const DEFAULT_CONNECT_TIMEOUT_MS = 5000;
/**
 * Parse `vng://host:port` or `host:port` into host + port.
 * Falls back to DEFAULT_NATIVE_PORT when no port is present.
 */
function parseNativeEndpoint(endpoint) {
    const raw = endpoint.trim().replace(/^vng:\/\//, "").split("/")[0] ?? "";
    // IPv6 bracketed: [::1]:7542
    if (raw.startsWith("[")) {
        const close = raw.indexOf("]");
        if (close > 0) {
            const host = raw.slice(1, close);
            const rest = raw.slice(close + 1);
            const port = rest.startsWith(":") ? Number.parseInt(rest.slice(1), 10) : DEFAULT_NATIVE_PORT;
            return { host, port: Number.isFinite(port) ? port : DEFAULT_NATIVE_PORT };
        }
    }
    const lastColon = raw.lastIndexOf(":");
    if (lastColon > 0) {
        const maybePort = Number.parseInt(raw.slice(lastColon + 1), 10);
        if (Number.isInteger(maybePort) && maybePort > 0 && maybePort <= 65535) {
            return { host: raw.slice(0, lastColon), port: maybePort };
        }
    }
    return { host: raw || "127.0.0.1", port: DEFAULT_NATIVE_PORT };
}
/** Build NativeCommandSessionOptions from a Connection. */
function toNativeOpts(connection, connectTimeoutMs) {
    const s = connection.settings;
    // Prefer explicit nativeEndpoint, else derive from host + default port.
    const rawEndpoint = s.nativeEndpoint?.trim() ||
        `${s.host || "127.0.0.1"}:${DEFAULT_NATIVE_PORT}`;
    const { host, port } = parseNativeEndpoint(rawEndpoint);
    return {
        host,
        port,
        sessionId: s.id,
        adminApiKey: s.adminKey,
        connectTimeoutMs: connectTimeoutMs ?? DEFAULT_CONNECT_TIMEOUT_MS,
        requestIdPrefix: `vscode-native-${s.id.slice(0, 8)}`,
    };
}
/** Wrap any thrown error into an HttpResponse error shape. */
function nativeError(err) {
    const message = err instanceof Error ? err.message : "Native transport error";
    return { status: 0, error: message, headers: {} };
}
const DRIVER_NOT_AVAILABLE = {
    status: 0,
    error: "Native driver not available — reinstall extension with bundled dependencies or use HTTP transport.",
    headers: {},
};
let driverCache = undefined; // undefined = not tried yet, null = unavailable
async function loadDriver() {
    if (driverCache !== undefined) {
        return driverCache;
    }
    try {
        // Dynamic import prevents a missing module from crashing extension activation.
        driverCache = await Promise.resolve().then(() => __importStar(require("@voltnuerongrid/driver-typescript")));
    }
    catch {
        driverCache = null;
    }
    return driverCache;
}
class NativeClient {
    /** Execute a SQL batch over the native socket. */
    async executeQuery(connection, query, options) {
        const driver = await loadDriver();
        if (!driver) {
            return DRIVER_NOT_AVAILABLE;
        }
        try {
            const opts = toNativeOpts(connection, options?.timeoutMs);
            const result = await driver.nativeSqlExecuteCommandRoundtrip(opts, {
                sql_batch: query,
            });
            return { status: 200, data: result, headers: {} };
        }
        catch (err) {
            return nativeError(err);
        }
    }
    /** Health check over the native socket. */
    async healthCheck(connection) {
        const driver = await loadDriver();
        if (!driver) {
            return DRIVER_NOT_AVAILABLE;
        }
        try {
            const opts = toNativeOpts(connection);
            const result = await driver.nativeHealthCommandRoundtrip(opts);
            return { status: 200, data: result, headers: {} };
        }
        catch (err) {
            return nativeError(err);
        }
    }
    /** Schema registry introspection over the native socket. */
    async getSchemaRegistry(connection) {
        const driver = await loadDriver();
        if (!driver) {
            return DRIVER_NOT_AVAILABLE;
        }
        try {
            const opts = toNativeOpts(connection);
            const result = await driver.nativeSchemaRegistryCommandRoundtrip(opts);
            return { status: 200, data: result, headers: {} };
        }
        catch (err) {
            return nativeError(err);
        }
    }
    /** Structured test-connection result (mirrors HttpClient.testConnection). */
    async testConnection(connection) {
        const response = await this.healthCheck(connection);
        if (response.status === 200) {
            return { isHealthy: true, message: "Native connection successful" };
        }
        return {
            isHealthy: false,
            message: response.error ?? "Native transport unavailable",
        };
    }
}
exports.NativeClient = NativeClient;
function createNativeClient() {
    return new NativeClient();
}
//# sourceMappingURL=NativeClient.js.map