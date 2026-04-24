/**
 * VoltNueronGrid Node.js driver — pure ESM, zero external dependencies.
 * Uses Node's built-in `https`/`http` modules for I/O.
 *
 * API surface mirrors the TypeScript driver (voltnuerongrid-driver-typescript).
 */

import * as https from "node:https";
import * as http from "node:http";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

export const DEFAULT_HTTP_REQUEST_TIMEOUT_MS = 30_000;
export const DEFAULT_HTTP_MAX_RETRIES = 2;

// ---------------------------------------------------------------------------
// DriverError
// ---------------------------------------------------------------------------

/**
 * Typed error thrown / rejected by the VoltNueronGrid driver.
 *
 * @property {"validation"|"transport"|"http_status"|"timeout"|"cancelled"} kind
 * @property {number|undefined} statusCode  HTTP status (for http_status kind)
 */
export class DriverError extends Error {
  /**
   * @param {"validation"|"transport"|"http_status"|"timeout"|"cancelled"} kind
   * @param {string} message
   * @param {number|undefined} statusCode
   */
  constructor(kind, message, statusCode) {
    super(message);
    this.name = "DriverError";
    this.kind = kind;
    this.statusCode = statusCode;
  }

  static validation(message) {
    return new DriverError("validation", message, undefined);
  }

  static transport(message) {
    return new DriverError("transport", message, undefined);
  }

  static httpStatus(statusCode, message) {
    return new DriverError("http_status", message, statusCode);
  }

  static timeout(message) {
    return new DriverError("timeout", message, undefined);
  }

  static cancelled(message) {
    return new DriverError("cancelled", message, undefined);
  }
}

// ---------------------------------------------------------------------------
// Config validation
// ---------------------------------------------------------------------------

/**
 * Validates a driver config object.
 *
 * @param {import("./index.d.ts").DriverConfig} config
 * @returns {string|null}  error message or null if valid
 */
export function validateConfig(config) {
  if (!config || typeof config !== "object") {
    return "config must be an object";
  }
  if (typeof config.baseUrl !== "string" || !config.baseUrl.trim()) {
    return "baseUrl must not be empty";
  }
  if (typeof config.sessionId !== "string" || !config.sessionId.trim()) {
    return "sessionId must not be empty";
  }
  const mode = config.mode;
  if (mode === "admin") {
    if (!config.adminApiKey?.trim()) return "admin mode requires adminApiKey";
  } else if (mode === "operator") {
    if (!config.adminApiKey?.trim()) return "operator mode requires adminApiKey";
    if (!config.operatorId?.trim()) return "operator mode requires operatorId";
  } else if (mode === "tenant") {
    if (!config.tenantId?.trim()) return "tenant mode requires tenantId";
  } else {
    return `mode must be one of: admin, operator, tenant`;
  }
  if (config.requestTimeoutMs !== undefined) {
    if (!Number.isFinite(config.requestTimeoutMs) || config.requestTimeoutMs < 100) {
      return "requestTimeoutMs must be >= 100 when set";
    }
  }
  if (config.maxRetries !== undefined) {
    if (!Number.isInteger(config.maxRetries) || config.maxRetries < 0 || config.maxRetries > 20) {
      return "maxRetries must be an integer from 0 to 20 when set";
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function httpRestBaseUrl(config) {
  return config.baseUrl.trim().replace(/\/$/, "");
}

function buildHeaders(config) {
  /** @type {Record<string, string>} */
  const headers = {
    "content-type": "application/json",
    "x-vng-session-id": config.sessionId,
  };
  if ((config.mode === "admin" || config.mode === "operator") && config.adminApiKey?.trim()) {
    headers["x-vng-admin-key"] = config.adminApiKey;
  }
  if (config.mode === "operator" && config.operatorId?.trim()) {
    headers["x-vng-operator-id"] = config.operatorId;
  }
  if (config.mode === "tenant" && config.tenantId?.trim()) {
    headers["x-vng-tenant-id"] = config.tenantId;
  }
  if (config.mode === "tenant" && config.userId?.trim()) {
    headers["x-vng-user-id"] = config.userId;
  }
  if (config.routeHint?.trim()) {
    headers["x-vng-route-hint"] = config.routeHint;
  }
  return headers;
}

function buildPost(config, path, payload) {
  const base = httpRestBaseUrl(config);
  return {
    method: /** @type {"POST"} */ ("POST"),
    url: `${base}${path}`,
    headers: buildHeaders(config),
    bodyJson: JSON.stringify(payload),
  };
}

// ---------------------------------------------------------------------------
// VoltNueronGridDriver
// ---------------------------------------------------------------------------

/**
 * Request builder for the VoltNueronGrid HTTP API.
 * Does not perform I/O; returns {@link DriverRequest} objects.
 */
export class VoltNueronGridDriver {
  /**
   * @param {import("./index.d.ts").DriverConfig} config
   */
  constructor(config) {
    const err = validateConfig(config);
    if (err) throw DriverError.validation(err);
    this.config = config;
  }

  /** @returns {import("./index.d.ts").DriverRequest} */
  buildHealthRequest() {
    const base = httpRestBaseUrl(this.config);
    return {
      method: "GET",
      url: `${base}/health`,
      headers: buildHeaders(this.config),
      bodyJson: undefined,
    };
  }

  /**
   * @param {string} sqlBatch
   * @returns {import("./index.d.ts").DriverRequest}
   */
  buildSqlExecuteRequest(sqlBatch) {
    if (!sqlBatch?.trim()) throw DriverError.validation("sqlBatch must not be empty");
    return buildPost(this.config, "/api/v1/sql/execute", { sql_batch: sqlBatch });
  }

  /**
   * @param {string} sqlBatch
   * @returns {import("./index.d.ts").DriverRequest}
   */
  buildSqlAnalyzeRequest(sqlBatch) {
    if (!sqlBatch?.trim()) throw DriverError.validation("sqlBatch must not be empty");
    return buildPost(this.config, "/api/v1/sql/analyze", { sql_batch: sqlBatch });
  }

  /**
   * @param {string} sqlBatch
   * @returns {import("./index.d.ts").DriverRequest}
   */
  buildSqlRouteRequest(sqlBatch) {
    if (!sqlBatch?.trim()) throw DriverError.validation("sqlBatch must not be empty");
    return buildPost(this.config, "/api/v1/sql/route", { sql_batch: sqlBatch });
  }

  /**
   * @param {string[]} statements
   * @returns {import("./index.d.ts").DriverRequest}
   */
  buildSqlTransactionRequest(statements) {
    if (!Array.isArray(statements) || statements.length === 0) {
      throw DriverError.validation("statements must be a non-empty array");
    }
    return buildPost(this.config, "/api/v1/sql/transaction", { statements });
  }

  /** @returns {import("./index.d.ts").DriverRequest} */
  buildSchemaRegistryRequest() {
    const base = httpRestBaseUrl(this.config);
    return {
      method: "GET",
      url: `${base}/api/v1/ingest/schema/registry`,
      headers: buildHeaders(this.config),
      bodyJson: undefined,
    };
  }
}

// ---------------------------------------------------------------------------
// HTTP status helpers
// ---------------------------------------------------------------------------

const RETRYABLE_STATUSES = new Set([408, 425, 429, 500, 502, 503, 504]);

/**
 * Returns true if the HTTP status should be retried.
 * Mirrors Rust `is_retryable_http_status`.
 *
 * @param {number} status
 * @returns {boolean}
 */
export function isRetryableHttpStatus(status) {
  return RETRYABLE_STATUSES.has(status);
}

// ---------------------------------------------------------------------------
// performDriverHttpRequest — Node https/http stdlib implementation
// ---------------------------------------------------------------------------

/**
 * @param {number} ms
 * @returns {Promise<void>}
 */
function sleepMs(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Execute a {@link DriverRequest} over HTTP using Node's built-in modules.
 * No `fetch` dependency required.
 *
 * @param {import("./index.d.ts").DriverRequest} req
 * @param {import("./index.d.ts").HttpExecutionOptions} [opts]
 * @returns {Promise<import("./index.d.ts").HttpExecutionResult>}
 */
export async function performDriverHttpRequest(req, opts = {}) {
  const timeoutMs = opts.timeoutMs ?? DEFAULT_HTTP_REQUEST_TIMEOUT_MS;
  const maxRetries = opts.maxRetries ?? DEFAULT_HTTP_MAX_RETRIES;
  const abortSignal = opts.abortSignal;

  let lastError;

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    if (abortSignal?.aborted) {
      throw DriverError.cancelled("request aborted");
    }

    try {
      const result = await _doRequest(req, timeoutMs, abortSignal);
      if (isRetryableHttpStatus(result.status) && attempt < maxRetries) {
        await sleepMs(Math.min(250 * 2 ** attempt, 2000));
        lastError = DriverError.httpStatus(result.status, `retryable HTTP ${result.status}`);
        continue;
      }
      return result;
    } catch (err) {
      if (err instanceof DriverError && (err.kind === "cancelled" || err.kind === "timeout")) {
        throw err;
      }
      lastError = err;
      if (attempt < maxRetries) {
        await sleepMs(Math.min(250 * 2 ** attempt, 2000));
        continue;
      }
      const msg = err instanceof Error ? err.message : String(err);
      throw DriverError.transport(`HTTP request failed: ${msg}`);
    }
  }

  if (lastError instanceof DriverError) throw lastError;
  throw DriverError.transport("exhausted HTTP retries");
}

/**
 * Single HTTP attempt using Node http/https.
 *
 * @param {import("./index.d.ts").DriverRequest} req
 * @param {number} timeoutMs
 * @param {AbortSignal|undefined} abortSignal
 * @returns {Promise<import("./index.d.ts").HttpExecutionResult>}
 */
function _doRequest(req, timeoutMs, abortSignal) {
  return new Promise((resolve, reject) => {
    if (abortSignal?.aborted) {
      return reject(DriverError.cancelled("request aborted"));
    }

    const urlObj = new URL(req.url);
    const isHttps = urlObj.protocol === "https:";
    const transport = isHttps ? https : http;

    const bodyBuf =
      req.method === "POST" && req.bodyJson != null
        ? Buffer.from(req.bodyJson, "utf8")
        : null;

    const options = {
      hostname: urlObj.hostname,
      port: urlObj.port || (isHttps ? 443 : 80),
      path: urlObj.pathname + urlObj.search,
      method: req.method,
      headers: {
        ...req.headers,
        ...(bodyBuf ? { "content-length": String(bodyBuf.byteLength) } : {}),
      },
    };

    const nodeReq = transport.request(options, (res) => {
      const chunks = [];
      res.on("data", (chunk) => chunks.push(chunk));
      res.on("end", () => {
        clearTimeout(timer);
        resolve({
          status: res.statusCode ?? 0,
          bodyText: Buffer.concat(chunks).toString("utf8"),
        });
      });
      res.on("error", (err) => {
        clearTimeout(timer);
        reject(DriverError.transport(err.message));
      });
    });

    const timer = setTimeout(() => {
      nodeReq.destroy();
      reject(DriverError.timeout(`request timed out after ${timeoutMs}ms`));
    }, timeoutMs);

    const onAbort = () => {
      clearTimeout(timer);
      nodeReq.destroy();
      reject(DriverError.cancelled("request aborted"));
    };
    abortSignal?.addEventListener("abort", onAbort, { once: true });

    nodeReq.on("error", (err) => {
      clearTimeout(timer);
      abortSignal?.removeEventListener("abort", onAbort);
      reject(DriverError.transport(err.message));
    });

    if (bodyBuf) {
      nodeReq.write(bodyBuf);
    }
    nodeReq.end();
  });
}
