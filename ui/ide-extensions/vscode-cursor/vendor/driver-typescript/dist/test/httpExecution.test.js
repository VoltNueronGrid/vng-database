"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const node_test_1 = __importDefault(require("node:test"));
const strict_1 = __importDefault(require("node:assert/strict"));
const httpExecution_js_1 = require("../httpExecution.js");
(0, node_test_1.default)("isRetryableHttpStatus matches Rust policy", () => {
    strict_1.default.equal((0, httpExecution_js_1.isRetryableHttpStatus)(503), true);
    strict_1.default.equal((0, httpExecution_js_1.isRetryableHttpStatus)(404), false);
});
(0, node_test_1.default)("validateConfig rejects invalid timeout and retries (via index)", async () => {
    const { validateConfig } = await import("../index.js");
    strict_1.default.equal(validateConfig({
        baseUrl: "http://127.0.0.1:8080",
        sessionId: "s",
        mode: "admin",
        adminApiKey: "k",
        requestTimeoutMs: 50
    }), "requestTimeoutMs must be >= 100 when set");
    strict_1.default.equal(validateConfig({
        baseUrl: "http://127.0.0.1:8080",
        sessionId: "s",
        mode: "admin",
        adminApiKey: "k",
        maxRetries: 21
    }), "maxRetries must be an integer from 0 to 20 when set");
});
(0, node_test_1.default)("performDriverHttpRequest retries then succeeds on 503 -> 200", async () => {
    let calls = 0;
    const fetchFn = async () => {
        calls += 1;
        if (calls === 1) {
            return new Response("busy", { status: 503 });
        }
        return new Response("ok", { status: 200 });
    };
    const result = await (0, httpExecution_js_1.performDriverHttpRequest)({
        method: "GET",
        url: "http://example.test/health",
        headers: { "content-type": "application/json", "x-vng-session-id": "s" }
    }, { maxRetries: httpExecution_js_1.DEFAULT_HTTP_MAX_RETRIES, timeoutMs: 5000, fetchFn });
    strict_1.default.equal(result.status, 200);
    strict_1.default.equal(result.bodyText, "ok");
    strict_1.default.equal(calls, 2);
});
(0, node_test_1.default)("performDriverHttpRequest throws timeout when fetch ignores completion", async () => {
    /** Honors AbortSignal like real fetch (tests timeout path). */
    const fetchFn = (_input, init) => new Promise((_resolve, reject) => {
        const sig = init?.signal;
        if (!sig) {
            reject(new Error("expected signal"));
            return;
        }
        const onAbort = () => {
            const err = new Error("Aborted");
            err.name = "AbortError";
            reject(err);
        };
        if (sig.aborted) {
            onAbort();
            return;
        }
        sig.addEventListener("abort", onAbort, { once: true });
    });
    await strict_1.default.rejects(() => (0, httpExecution_js_1.performDriverHttpRequest)({
        method: "GET",
        url: "http://example.test/health",
        headers: { "content-type": "application/json", "x-vng-session-id": "s" }
    }, { timeoutMs: 40, maxRetries: 0, fetchFn }), (err) => err instanceof httpExecution_js_1.DriverError && err.kind === "timeout");
});
(0, node_test_1.default)("performDriverHttpRequest respects external abort", async () => {
    const ac = new AbortController();
    const fetchFn = (_input, init) => new Promise((_resolve, reject) => {
        const sig = init?.signal;
        if (!sig) {
            reject(new Error("expected signal"));
            return;
        }
        const onAbort = () => {
            const err = new Error("Aborted");
            err.name = "AbortError";
            reject(err);
        };
        if (sig.aborted) {
            onAbort();
            return;
        }
        sig.addEventListener("abort", onAbort, { once: true });
    });
    queueMicrotask(() => ac.abort());
    await strict_1.default.rejects(() => (0, httpExecution_js_1.performDriverHttpRequest)({
        method: "GET",
        url: "http://example.test/health",
        headers: { "content-type": "application/json", "x-vng-session-id": "s" }
    }, {
        timeoutMs: 60_000,
        maxRetries: 0,
        abortSignal: ac.signal,
        fetchFn
    }), (err) => err instanceof httpExecution_js_1.DriverError && err.kind === "cancelled");
});
