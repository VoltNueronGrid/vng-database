import test from "node:test";
import assert from "node:assert/strict";
import {
  DriverError,
  performDriverHttpRequest,
  isRetryableHttpStatus,
  DEFAULT_HTTP_MAX_RETRIES
} from "../httpExecution.js";

test("isRetryableHttpStatus matches Rust policy", () => {
  assert.equal(isRetryableHttpStatus(503), true);
  assert.equal(isRetryableHttpStatus(404), false);
});

test("validateConfig rejects invalid timeout and retries (via index)", async () => {
  const { validateConfig } = await import("../index.js");
  assert.equal(
    validateConfig({
      baseUrl: "http://127.0.0.1:8080",
      sessionId: "s",
      mode: "admin",
      adminApiKey: "k",
      requestTimeoutMs: 50
    }),
    "requestTimeoutMs must be >= 100 when set"
  );
  assert.equal(
    validateConfig({
      baseUrl: "http://127.0.0.1:8080",
      sessionId: "s",
      mode: "admin",
      adminApiKey: "k",
      maxRetries: 21
    }),
    "maxRetries must be an integer from 0 to 20 when set"
  );
});

test("performDriverHttpRequest retries then succeeds on 503 -> 200", async () => {
  let calls = 0;
  const fetchFn: typeof fetch = async () => {
    calls += 1;
    if (calls === 1) {
      return new Response("busy", { status: 503 });
    }
    return new Response("ok", { status: 200 });
  };

  const result = await performDriverHttpRequest(
    {
      method: "GET",
      url: "http://example.test/health",
      headers: { "content-type": "application/json", "x-vng-session-id": "s" }
    },
    { maxRetries: DEFAULT_HTTP_MAX_RETRIES, timeoutMs: 5000, fetchFn }
  );
  assert.equal(result.status, 200);
  assert.equal(result.bodyText, "ok");
  assert.equal(calls, 2);
});

test("performDriverHttpRequest throws timeout when fetch ignores completion", async () => {
  /** Honors AbortSignal like real fetch (tests timeout path). */
  const fetchFn: typeof fetch = (_input, init) =>
    new Promise<Response>((_resolve, reject) => {
      const sig = init?.signal;
      if (!sig) {
        reject(new Error("expected signal"));
        return;
      }
      const onAbort = (): void => {
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

  await assert.rejects(
    () =>
      performDriverHttpRequest(
        {
          method: "GET",
          url: "http://example.test/health",
          headers: { "content-type": "application/json", "x-vng-session-id": "s" }
        },
        { timeoutMs: 40, maxRetries: 0, fetchFn }
      ),
    (err: unknown) => err instanceof DriverError && (err as DriverError).kind === "timeout"
  );
});

test("performDriverHttpRequest respects external abort", async () => {
  const ac = new AbortController();
  const fetchFn: typeof fetch = (_input, init) =>
    new Promise<Response>((_resolve, reject) => {
      const sig = init?.signal;
      if (!sig) {
        reject(new Error("expected signal"));
        return;
      }
      const onAbort = (): void => {
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
  await assert.rejects(
    () =>
      performDriverHttpRequest(
        {
          method: "GET",
          url: "http://example.test/health",
          headers: { "content-type": "application/json", "x-vng-session-id": "s" }
        },
        {
          timeoutMs: 60_000,
          maxRetries: 0,
          abortSignal: ac.signal,
          fetchFn
        }
      ),
    (err: unknown) => err instanceof DriverError && (err as DriverError).kind === "cancelled"
  );
});
