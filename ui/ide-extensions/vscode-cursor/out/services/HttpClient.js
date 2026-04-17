"use strict";
/**
 * HttpClient: HTTP communication with auth headers and error handling
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.HttpClient = void 0;
exports.createHttpClient = createHttpClient;
class HttpClient {
    constructor() {
        this.requestTimeout = 30000; // 30 seconds
    }
    /**
     * Execute a query against the server
     */
    async executeQuery(connection, query, options) {
        return this.post(connection, "/api/v1/sql/execute", {
            sql_batch: [query],
            request_id: options?.requestId ?? `ide-query-${Date.now()}`,
        }, {
            timeoutMs: options?.timeoutMs,
            signal: options?.signal,
        });
    }
    /**
     * Get schema registry
     */
    async getSchemaRegistry(connection) {
        return this.get(connection, "/api/v1/ingest/schema/registry");
    }
    /**
     * Health check
     */
    async healthCheck(connection) {
        return this.get(connection, "/health");
    }
    /**
     * Test connection
     */
    async testConnection(connection) {
        try {
            const response = await this.healthCheck(connection);
            if (response.status === 200) {
                return { isHealthy: true, message: "Connection successful" };
            }
            else {
                return { isHealthy: false, message: `Server returned status ${response.status}` };
            }
        }
        catch (error) {
            const message = error instanceof Error ? error.message : "Unknown error";
            return { isHealthy: false, message };
        }
    }
    /**
     * Generic GET request
     */
    async get(connection, path) {
        const url = `${connection.settings.baseUrl}${path}`;
        const headers = this.buildHeaders(connection, "GET");
        try {
            const response = await this.fetchWithTimeout(url, {
                method: "GET",
                headers,
            });
            const data = await this.parseResponse(response);
            return {
                status: response.status,
                data,
                headers: this.extractHeaders(response),
            };
        }
        catch (error) {
            return {
                status: 0,
                error: error instanceof Error ? error.message : "Unknown error",
                headers: {},
            };
        }
    }
    /**
     * Generic POST request
     */
    async post(connection, path, body, requestOptions) {
        const url = `${connection.settings.baseUrl}${path}`;
        const headers = this.buildHeaders(connection, "POST");
        try {
            const response = await this.fetchWithTimeout(url, {
                method: "POST",
                headers,
                body: JSON.stringify(body),
            }, requestOptions?.timeoutMs, requestOptions?.signal);
            const data = await this.parseResponse(response);
            return {
                status: response.status,
                data,
                headers: this.extractHeaders(response),
            };
        }
        catch (error) {
            return {
                status: 0,
                error: error instanceof Error ? error.message : "Unknown error",
                headers: {},
            };
        }
    }
    /**
     * Build request headers with auth
     */
    buildHeaders(connection, method) {
        const headers = {
            "Content-Type": "application/json",
            "User-Agent": "VoltNueronGrid-VSCode/0.3.2",
        };
        const { mode, adminKey, operatorId, tenantId, userId } = connection.settings;
        // Admin or operator mode: include admin key
        if ((mode === "admin" || mode === "operator") && adminKey) {
            headers["x-vng-admin-key"] = adminKey;
        }
        // Operator mode: include operator ID
        if (mode === "operator" && operatorId) {
            headers["x-vng-operator-id"] = operatorId;
        }
        // Tenant mode: include tenant and user IDs
        if (mode === "tenant") {
            if (tenantId)
                headers["x-vng-tenant-id"] = tenantId;
            if (userId)
                headers["x-vng-user-id"] = userId;
        }
        return headers;
    }
    /**
     * Parse response based on content-type
     */
    async parseResponse(response) {
        const contentType = response.headers.get("content-type") || "";
        if (contentType.includes("application/json")) {
            try {
                return await response.json();
            }
            catch {
                return null;
            }
        }
        if (contentType.includes("text/")) {
            return await response.text();
        }
        return null;
    }
    /**
     * Extract response headers
     */
    extractHeaders(response) {
        const headers = {};
        response.headers.forEach((value, key) => {
            headers[key] = value;
        });
        return headers;
    }
    /**
     * Fetch with timeout
     */
    fetchWithTimeout(url, options, timeout = this.requestTimeout, upstreamSignal) {
        const controller = new AbortController();
        let upstreamAbortHandler;
        if (upstreamSignal) {
            if (upstreamSignal.aborted) {
                controller.abort();
            }
            else {
                upstreamAbortHandler = () => controller.abort();
                upstreamSignal.addEventListener("abort", upstreamAbortHandler, { once: true });
            }
        }
        const timeoutId = setTimeout(() => controller.abort(new Error(`Request timeout after ${timeout}ms`)), timeout);
        return fetch(url, {
            ...options,
            signal: controller.signal,
        }).finally(() => {
            clearTimeout(timeoutId);
            if (upstreamSignal && upstreamAbortHandler) {
                upstreamSignal.removeEventListener("abort", upstreamAbortHandler);
            }
        });
    }
}
exports.HttpClient = HttpClient;
function createHttpClient() {
    return new HttpClient();
}
//# sourceMappingURL=HttpClient.js.map