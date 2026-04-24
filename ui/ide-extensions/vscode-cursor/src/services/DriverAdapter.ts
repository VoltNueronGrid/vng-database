/**
 * DriverAdapter: self-contained HTTP transport for the VoltNueronGrid extension.
 *
 * Inlines the request-building and HTTP-execution logic from the TS driver so
 * the extension can be packaged as a .vsix without bundling an external package.
 *
 * Native transport is handled by NativeClient (dynamic import, graceful fallback).
 */

import { Connection } from "../models/Connection";

// ── Types ────────────────────────────────────────────────────────────────────

export interface DriverConfig {
  baseUrl: string;
  sessionId: string;
  mode: DriverMode;
  adminApiKey?: string;
  operatorId?: string;
  tenantId?: string;
  userId?: string;
  routeHint?: string;
  requestTimeoutMs?: number;
  maxRetries?: number;
}

export type DriverMode = "admin" | "operator" | "tenant";

export interface HttpDriverRequest {
  method: "GET" | "POST";
  url: string;
  headers: Record<string, string>;
  bodyJson?: string;
}

export interface HttpExecutionOptions {
  timeoutMs?: number;
  maxRetries?: number;
  abortSignal?: AbortSignal;
}

export interface HttpExecutionResult {
  status: number;
  bodyText: string;
}

// ── DriverError ───────────────────────────────────────────────────────────────

export class DriverError extends Error {
  readonly kind: "validation" | "transport" | "http_status" | "timeout" | "cancelled";
  readonly statusCode?: number;
  readonly requestId?: string;

  constructor(
    kind: "validation" | "transport" | "http_status" | "timeout" | "cancelled",
    message: string,
    statusCode?: number,
    requestId?: string
  ) {
    super(message);
    this.kind = kind;
    this.statusCode = statusCode;
    this.requestId = requestId;
    this.name = "DriverError";
    Object.setPrototypeOf(this, new.target.prototype);
  }

  static validation(msg: string) { return new DriverError("validation", msg); }
  static transport(msg: string) { return new DriverError("transport", msg); }
  static httpStatus(status: number, msg: string, rid?: string) { return new DriverError("http_status", msg, status, rid); }
  static timeout(msg: string) { return new DriverError("timeout", msg); }
  static cancelled(msg: string) { return new DriverError("cancelled", msg); }
}

// ── HTTP helpers ──────────────────────────────────────────────────────────────

const DEFAULT_TIMEOUT_MS = 30_000;
const DEFAULT_MAX_RETRIES = 2;
const RETRYABLE_STATUSES = new Set([408, 425, 429, 500, 502, 503, 504]);

function sleepMs(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

function effectiveBaseUrl(config: DriverConfig): string {
  const b = config.baseUrl.trim();
  if (b.toLowerCase().startsWith("vng://")) {
    throw new DriverError("validation", "baseUrl uses vng:// — use nativeEndpoint for native transport and set baseUrl to http://…");
  }
  return b.replace(/\/$/, "");
}

function buildHeaders(config: DriverConfig): Record<string, string> {
  const h: Record<string, string> = {
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

export class VoltNueronGridDriver {
  constructor(private readonly config: DriverConfig) {
    if (!config.baseUrl.trim()) { throw DriverError.validation("baseUrl must not be empty"); }
    if (!config.sessionId.trim()) { throw DriverError.validation("sessionId must not be empty"); }
    if (config.mode === "admin" && !config.adminApiKey?.trim()) {
      throw DriverError.validation("admin mode requires adminApiKey");
    }
  }

  buildHealthRequest(): HttpDriverRequest {
    return { method: "GET", url: `${effectiveBaseUrl(this.config)}/health`, headers: buildHeaders(this.config) };
  }

  buildSqlExecuteRequest(sqlBatch: string): HttpDriverRequest {
    return {
      method: "POST",
      url: `${effectiveBaseUrl(this.config)}/api/v1/sql/execute`,
      headers: buildHeaders(this.config),
      bodyJson: JSON.stringify({ sql_batch: sqlBatch }),
    };
  }

  buildSchemaRegistryRequest(): HttpDriverRequest {
    return {
      method: "GET",
      url: `${effectiveBaseUrl(this.config)}/api/v1/admin/schema/tree`,
      headers: buildHeaders(this.config),
    };
  }
}

// ── performDriverHttpRequest ──────────────────────────────────────────────────

export async function executeDriverRequest(
  req: HttpDriverRequest,
  opts?: HttpExecutionOptions
): Promise<HttpExecutionResult> {
  const augmented: HttpDriverRequest = {
    ...req,
    headers: { ...req.headers, "user-agent": "VoltNueronGrid-VSCode/0.3.2" },
  };
  return performDriverHttpRequest(augmented, opts);
}

async function performDriverHttpRequest(
  req: HttpDriverRequest,
  opts?: HttpExecutionOptions
): Promise<HttpExecutionResult> {
  const timeoutMs = opts?.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const maxRetries = opts?.maxRetries ?? DEFAULT_MAX_RETRIES;
  const outerAbort = opts?.abortSignal;

  let lastError: unknown;

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    if (outerAbort?.aborted) { throw DriverError.cancelled("request aborted"); }

    const controller = new AbortController();
    const onOuterAbort = () => controller.abort();
    if (outerAbort) { outerAbort.addEventListener("abort", onOuterAbort, { once: true }); }
    const timer = setTimeout(() => controller.abort(), timeoutMs);

    try {
      const res = await fetch(req.url, {
        method: req.method,
        headers: req.headers,
        body: req.method === "POST" && req.bodyJson !== undefined ? req.bodyJson : undefined,
        signal: controller.signal,
      });
      clearTimeout(timer);
      if (outerAbort) { outerAbort.removeEventListener("abort", onOuterAbort); }

      const bodyText = await res.text();
      if (RETRYABLE_STATUSES.has(res.status) && attempt < maxRetries) {
        await sleepMs(Math.min(250 * 2 ** attempt, 2_000));
        lastError = DriverError.httpStatus(res.status, `retryable HTTP ${res.status}`);
        continue;
      }
      return { status: res.status, bodyText };
    } catch (err) {
      clearTimeout(timer);
      if (outerAbort) { outerAbort.removeEventListener("abort", onOuterAbort); }
      if (err instanceof Error && err.name === "AbortError") {
        if (outerAbort?.aborted) { throw DriverError.cancelled("request aborted"); }
        throw DriverError.timeout(`request timed out after ${timeoutMs}ms`);
      }
      lastError = err;
      if (attempt < maxRetries) {
        await sleepMs(Math.min(250 * 2 ** attempt, 2_000));
      }
    }
  }

  if (lastError instanceof DriverError) { throw lastError; }
  const msg = lastError instanceof Error ? lastError.message : "network error";
  throw DriverError.transport(msg);
}

// ── connectionToDriverConfig ──────────────────────────────────────────────────

export function connectionToDriverConfig(connection: Connection): DriverConfig {
  const s = connection.settings;
  const config: DriverConfig = {
    baseUrl: s.baseUrl,
    sessionId: s.id,
    mode: s.mode as DriverMode,
  };
  if (s.adminKey) { config.adminApiKey = s.adminKey; }
  if (s.operatorId) { config.operatorId = s.operatorId; }
  if (s.tenantId) { config.tenantId = s.tenantId; }
  if (s.userId) { config.userId = s.userId; }
  if (s.advanced?.connectionTimeout && s.advanced.connectionTimeout > 0) {
    config.requestTimeoutMs = s.advanced.connectionTimeout;
  }
  return config;
}

export function makeVngDriver(connection: Connection): VoltNueronGridDriver {
  return new VoltNueronGridDriver(connectionToDriverConfig(connection));
}

