"use strict";
/**
 * DriverAdapter: self-contained HTTP transport for the VoltNueronGrid extension.
 *
 * Inlines the request-building and HTTP-execution logic from the TS driver so
 * the extension can be packaged as a .vsix without bundling an external package.
 *
 * Native transport is handled by NativeClient (dynamic import, graceful fallback).
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.VoltNueronGridDriver = exports.DriverError = void 0;
exports.executeDriverRequest = executeDriverRequest;
exports.connectionToDriverConfig = connectionToDriverConfig;
exports.makeVngDriver = makeVngDriver;
// ── DriverError ───────────────────────────────────────────────────────────────
class DriverError extends Error {
    constructor(kind, message, statusCode, requestId) {
        super(message);
        this.kind = kind;
        this.statusCode = statusCode;
        this.requestId = requestId;
        this.name = "DriverError";
        Object.setPrototypeOf(this, new.target.prototype);
    }
    static validation(msg) { return new DriverError("validation", msg); }
    static transport(msg) { return new DriverError("transport", msg); }
    static httpStatus(status, msg, rid) { return new DriverError("http_status", msg, status, rid); }
    static timeout(msg) { return new DriverError("timeout", msg); }
    static cancelled(msg) { return new DriverError("cancelled", msg); }
}
exports.DriverError = DriverError;
// ── HTTP helpers ──────────────────────────────────────────────────────────────
const DEFAULT_TIMEOUT_MS = 30000;
const DEFAULT_MAX_RETRIES = 2;
const RETRYABLE_STATUSES = new Set([408, 425, 429, 500, 502, 503, 504]);
function sleepMs(ms) {
    return new Promise((r) => setTimeout(r, ms));
}
function effectiveBaseUrl(config) {
    const b = config.baseUrl.trim();
    if (b.toLowerCase().startsWith("vng://")) {
        throw new DriverError("validation", "baseUrl uses vng:// — use nativeEndpoint for native transport and set baseUrl to http://…");
    }
    return b.replace(/\/$/, "");
}
function buildHeaders(config) {
    const h = {
        "content-type": "application/json",
        "x-vng-session-id": config.sessionId,
    };
    if ((config.mode === "admin" || config.mode === "operator") && config.adminApiKey) {
        h["x-vng-admin-key"] = config.adminApiKey;
    }
    if (config.mode === "operator" && config.operatorId) {
        h["x-vng-operator-id"] = config.operatorId;
    }
    if (config.mode === "tenant" && config.tenantId) {
        h["x-vng-tenant-id"] = config.tenantId;
    }
    if (config.mode === "tenant" && config.userId) {
        h["x-vng-user-id"] = config.userId;
    }
    if (config.routeHint) {
        h["x-vng-route-hint"] = config.routeHint;
    }
    return h;
}
// ── VoltNueronGridDriver ──────────────────────────────────────────────────────
class VoltNueronGridDriver {
    constructor(config) {
        this.config = config;
        if (!config.baseUrl.trim()) {
            throw DriverError.validation("baseUrl must not be empty");
        }
        if (!config.sessionId.trim()) {
            throw DriverError.validation("sessionId must not be empty");
        }
        if (config.mode === "admin" && !config.adminApiKey?.trim()) {
            throw DriverError.validation("admin mode requires adminApiKey");
        }
    }
    buildHealthRequest() {
        return { method: "GET", url: `${effectiveBaseUrl(this.config)}/health`, headers: buildHeaders(this.config) };
    }
    buildSqlExecuteRequest(sqlBatch) {
        return {
            method: "POST",
            url: `${effectiveBaseUrl(this.config)}/api/v1/sql/execute`,
            headers: buildHeaders(this.config),
            bodyJson: JSON.stringify({ sql_batch: sqlBatch }),
        };
    }
    buildSchemaRegistryRequest() {
        return {
            method: "GET",
            url: `${effectiveBaseUrl(this.config)}/api/v1/admin/schema/tree`,
            headers: buildHeaders(this.config),
        };
    }
}
exports.VoltNueronGridDriver = VoltNueronGridDriver;
// ── performDriverHttpRequest ──────────────────────────────────────────────────
async function executeDriverRequest(req, opts) {
    const augmented = {
        ...req,
        headers: { ...req.headers, "user-agent": "VoltNueronGrid-VSCode/0.3.2" },
    };
    return performDriverHttpRequest(augmented, opts);
}
async function performDriverHttpRequest(req, opts) {
    const timeoutMs = opts?.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    const maxRetries = opts?.maxRetries ?? DEFAULT_MAX_RETRIES;
    const outerAbort = opts?.abortSignal;
    let lastError;
    for (let attempt = 0; attempt <= maxRetries; attempt++) {
        if (outerAbort?.aborted) {
            throw DriverError.cancelled("request aborted");
        }
        const controller = new AbortController();
        const onOuterAbort = () => controller.abort();
        if (outerAbort) {
            outerAbort.addEventListener("abort", onOuterAbort, { once: true });
        }
        const timer = setTimeout(() => controller.abort(), timeoutMs);
        try {
            const res = await fetch(req.url, {
                method: req.method,
                headers: req.headers,
                body: req.method === "POST" && req.bodyJson !== undefined ? req.bodyJson : undefined,
                signal: controller.signal,
            });
            clearTimeout(timer);
            if (outerAbort) {
                outerAbort.removeEventListener("abort", onOuterAbort);
            }
            const bodyText = await res.text();
            if (RETRYABLE_STATUSES.has(res.status) && attempt < maxRetries) {
                await sleepMs(Math.min(250 * 2 ** attempt, 2000));
                lastError = DriverError.httpStatus(res.status, `retryable HTTP ${res.status}`);
                continue;
            }
            return { status: res.status, bodyText };
        }
        catch (err) {
            clearTimeout(timer);
            if (outerAbort) {
                outerAbort.removeEventListener("abort", onOuterAbort);
            }
            if (err instanceof Error && err.name === "AbortError") {
                if (outerAbort?.aborted) {
                    throw DriverError.cancelled("request aborted");
                }
                throw DriverError.timeout(`request timed out after ${timeoutMs}ms`);
            }
            lastError = err;
            if (attempt < maxRetries) {
                await sleepMs(Math.min(250 * 2 ** attempt, 2000));
            }
        }
    }
    if (lastError instanceof DriverError) {
        throw lastError;
    }
    const msg = lastError instanceof Error ? lastError.message : "network error";
    throw DriverError.transport(msg);
}
// ── connectionToDriverConfig ──────────────────────────────────────────────────
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
    return config;
}
function makeVngDriver(connection) {
    return new VoltNueronGridDriver(connectionToDriverConfig(connection));
}
//# sourceMappingURL=DriverAdapter.js.map