"use strict";
/**
 * HttpClient: transport-aware communication for the VoltNueronGrid extension.
 *
 * S3-001: HTTP path backed by VoltNueronGridDriver request builders.
 * NT-S4-001 extension: routes to NativeClient when connection transportMode
 * is "native", or when "auto" and a nativeEndpoint is configured.
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.HttpClient = void 0;
exports.createHttpClient = createHttpClient;
const DriverAdapter_1 = require("./DriverAdapter");
const NativeClient_1 = require("./NativeClient");
class HttpClient {
    constructor() {
        this.native = new NativeClient_1.NativeClient();
    }
    /**
     * Resolve the effective transport mode for a connection.
     * "native" → always native socket.
     * "auto"   → native when nativeEndpoint is configured, else HTTP.
     * "http"   → always HTTP (default).
     */
    effectiveTransport(connection) {
        const mode = connection.settings.transportMode ?? "http";
        if (mode === "native") {
            return "native";
        }
        if (mode === "auto" && connection.settings.nativeEndpoint?.trim()) {
            return "native";
        }
        return "http";
    }
    /**
     * Execute a query against the server.
     * Routes to native socket transport when transportMode is "native" or
     * "auto" with a configured nativeEndpoint; otherwise uses HTTP.
     */
    async executeQuery(connection, query, options) {
        if (this.effectiveTransport(connection) === "native") {
            return this.native.executeQuery(connection, query, { timeoutMs: options?.timeoutMs });
        }
        try {
            const driver = (0, DriverAdapter_1.makeVngDriver)(connection);
            const req = driver.buildSqlExecuteRequest(query);
            const execOpts = {};
            if (options?.timeoutMs !== undefined) {
                execOpts.timeoutMs = options.timeoutMs;
            }
            if (options?.signal !== undefined) {
                execOpts.abortSignal = options.signal;
            }
            const result = await (0, DriverAdapter_1.executeDriverRequest)(req, execOpts);
            return this.toHttpResponse(result);
        }
        catch (err) {
            return this.toErrorResponse(err);
        }
    }
    /**
     * Get schema registry.
     * Routes to native socket when transportMode is "native" or effective-auto-native.
     */
    async getSchemaRegistry(connection) {
        // Always use HTTP /api/v1/admin/schema/tree. The native protocol's
        // ingest.schema.registry returns ingest-connector metadata, not the
        // database table tree, so it's not a substitute. For native-mode
        // connections, baseUrl is the HTTP companion URL set at create-time.
        try {
            const driver = (0, DriverAdapter_1.makeVngDriver)(connection);
            const req = driver.buildSchemaRegistryRequest();
            const result = await (0, DriverAdapter_1.executeDriverRequest)(req);
            return this.toHttpResponse(result);
        }
        catch (err) {
            return this.toErrorResponse(err);
        }
    }
    /**
     * Health check.
     * Routes to native socket when transportMode is "native" or effective-auto-native;
     * otherwise HTTP with retries suppressed for fast deterministic probes.
     */
    async healthCheck(connection) {
        if (this.effectiveTransport(connection) === "native") {
            const result = await this.native.healthCheck(connection);
            if (result.status !== 0) {
                return result;
            }
        }
        try {
            const driver = (0, DriverAdapter_1.makeVngDriver)(connection);
            const req = driver.buildHealthRequest();
            const result = await (0, DriverAdapter_1.executeDriverRequest)(req, { maxRetries: 0 });
            return this.toHttpResponse(result);
        }
        catch (err) {
            return this.toErrorResponse(err);
        }
    }
    /**
     * Test connection: returns structured result for UI display.
     * Delegates to NativeClient.testConnection when on native transport.
     */
    async testConnection(connection) {
        try {
            const response = await this.healthCheck(connection);
            if (response.status === 200) {
                return { isHealthy: true, message: "Connection successful" };
            }
            if (response.error) {
                return { isHealthy: false, message: response.error };
            }
            return { isHealthy: false, message: `Server returned status ${response.status}` };
        }
        catch (error) {
            const message = error instanceof Error ? error.message : "Unknown error";
            return { isHealthy: false, message };
        }
    }
    /**
     * Converts a driver HttpExecutionResult to the extension's HttpResponse shape.
     * Response headers are not available from the driver layer; callers that need
     * specific headers should migrate to direct driver calls.
     */
    toHttpResponse(result) {
        let data;
        if (result.bodyText) {
            try {
                data = JSON.parse(result.bodyText);
            }
            catch {
                data = result.bodyText;
            }
        }
        return { status: result.status, data, headers: {} };
    }
    /**
     * Wraps any thrown error (DriverError or otherwise) into an HttpResponse error shape.
     */
    toErrorResponse(err) {
        if (err instanceof DriverAdapter_1.DriverError) {
            return { status: err.statusCode ?? 0, error: err.message, headers: {} };
        }
        const message = err instanceof Error ? err.message : "Unknown error";
        return { status: 0, error: message, headers: {} };
    }
}
exports.HttpClient = HttpClient;
function createHttpClient() {
    return new HttpClient();
}
//# sourceMappingURL=HttpClient.js.map