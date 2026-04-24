"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.DriverError = exports.DEFAULT_HTTP_MAX_RETRIES = exports.DEFAULT_HTTP_REQUEST_TIMEOUT_MS = void 0;
exports.isRetryableHttpStatus = isRetryableHttpStatus;
exports.performDriverHttpRequest = performDriverHttpRequest;
/** Aligns with Rust `DEFAULT_HTTP_REQUEST_TIMEOUT_MS` / driver-core-contract v1. */
exports.DEFAULT_HTTP_REQUEST_TIMEOUT_MS = 30_000;
/** Aligns with Rust `DEFAULT_HTTP_MAX_RETRIES` / driver-core-contract v1. */
exports.DEFAULT_HTTP_MAX_RETRIES = 2;
/** Typed error envelope (driver-core-contract §6). */
class DriverError extends Error {
    kind;
    statusCode;
    requestId;
    constructor(kind, message, statusCode, requestId) {
        super(message);
        this.kind = kind;
        this.statusCode = statusCode;
        this.requestId = requestId;
        this.name = "DriverError";
        Object.setPrototypeOf(this, new.target.prototype);
    }
    static validation(message) {
        return new DriverError("validation", message);
    }
    static transport(message) {
        return new DriverError("transport", message);
    }
    static httpStatus(statusCode, message, requestId) {
        return new DriverError("http_status", message, statusCode, requestId);
    }
    static timeout(message) {
        return new DriverError("timeout", message);
    }
    static cancelled(message) {
        return new DriverError("cancelled", message);
    }
}
exports.DriverError = DriverError;
/** Matches Rust `is_retryable_http_status`. */
function isRetryableHttpStatus(status) {
    return [408, 425, 429, 500, 502, 503, 504].includes(status);
}
function sleepMs(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
/**
 * Executes a built [`DriverRequest`] over HTTP with timeout + retry hooks (driver-core-contract §5 hooks).
 * Idempotent GET-style calls are safe; for POST bodies ensure callers only enable retries when appropriate.
 */
async function performDriverHttpRequest(req, opts) {
    const timeoutMs = opts?.timeoutMs ?? exports.DEFAULT_HTTP_REQUEST_TIMEOUT_MS;
    const maxRetries = opts?.maxRetries ?? exports.DEFAULT_HTTP_MAX_RETRIES;
    const fetchFn = opts?.fetchFn ?? fetch;
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
            const res = await fetchFn(req.url, {
                method: req.method,
                headers: req.headers,
                body: req.method === "POST" && req.bodyJson !== undefined ? req.bodyJson : undefined,
                signal: controller.signal
            });
            clearTimeout(timer);
            if (outerAbort) {
                outerAbort.removeEventListener("abort", onOuterAbort);
            }
            const bodyText = await res.text();
            if (isRetryableHttpStatus(res.status) && attempt < maxRetries) {
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
            const msg = err instanceof Error ? err.message : String(err);
            if (attempt < maxRetries) {
                await sleepMs(Math.min(250 * 2 ** attempt, 2000));
                continue;
            }
            throw DriverError.transport(`HTTP request failed: ${msg}`);
        }
    }
    if (lastError instanceof DriverError) {
        throw lastError;
    }
    throw DriverError.transport("exhausted HTTP retries");
}
