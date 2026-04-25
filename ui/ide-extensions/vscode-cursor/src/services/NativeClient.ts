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

import { Connection } from "../models/Connection";
import type { HttpResponse } from "./HttpClient";

/** Default native listener port (matches VNG_NATIVE_BIND default in the server). */
const DEFAULT_NATIVE_PORT = 7542;
const DEFAULT_CONNECT_TIMEOUT_MS = 5_000;

/**
 * Parse `vng://host:port` or `host:port` into host + port.
 * Falls back to DEFAULT_NATIVE_PORT when no port is present.
 */
function parseNativeEndpoint(endpoint: string): { host: string; port: number } {
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

interface NativeCommandSessionOptions {
  host: string;
  port: number;
  sessionId: string;
  adminApiKey: string | undefined;
  connectTimeoutMs: number;
  requestIdPrefix: string;
}

/** Build NativeCommandSessionOptions from a Connection. */
function toNativeOpts(
  connection: Connection,
  connectTimeoutMs?: number
): NativeCommandSessionOptions {
  const s = connection.settings;
  // Prefer explicit nativeEndpoint, else derive from host + default port.
  const rawEndpoint =
    s.nativeEndpoint?.trim() ||
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
function nativeError(err: unknown): HttpResponse {
  const message = err instanceof Error ? err.message : "Native transport error";
  return { status: 0, error: message, headers: {} };
}

const DRIVER_NOT_AVAILABLE: HttpResponse = {
  status: 0,
  error: "Native driver not available — reinstall extension with bundled dependencies or use HTTP transport.",
  headers: {},
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type DriverModule = any;

let driverCache: DriverModule | null | undefined = undefined; // undefined = not tried yet, null = unavailable

async function loadDriver(): Promise<DriverModule | null> {
  if (driverCache !== undefined) {
    return driverCache;
  }
  // Try the vendored driver first (bundled in the .vsix at vendor/driver-typescript).
  // Fall back to npm-resolved package for development workflows.
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const path = require("path") as { resolve: (...parts: string[]) => string };
  const candidates = [
    path.resolve(__dirname, "../../vendor/driver-typescript/dist/index.js"),
    "@voltnuerongrid/driver-typescript",
  ];
  for (const candidate of candidates) {
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      driverCache = require(candidate);
      if (driverCache && typeof driverCache.nativeHealthCommandRoundtrip === "function") {
        return driverCache;
      }
    } catch {
      // try next candidate
    }
  }
  driverCache = null;
  return driverCache;
}

export class NativeClient {
  /** Execute a SQL batch over the native socket. */
  async executeQuery(
    connection: Connection,
    query: string,
    options?: { timeoutMs?: number }
  ): Promise<HttpResponse> {
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
    } catch (err) {
      return nativeError(err);
    }
  }

  /** Health check over the native socket. */
  async healthCheck(connection: Connection): Promise<HttpResponse> {
    const driver = await loadDriver();
    if (!driver) {
      return DRIVER_NOT_AVAILABLE;
    }
    try {
      const opts = toNativeOpts(connection);
      const result = await driver.nativeHealthCommandRoundtrip(opts);
      return { status: 200, data: result, headers: {} };
    } catch (err) {
      return nativeError(err);
    }
  }

  /** Schema registry introspection over the native socket. */
  async getSchemaRegistry(connection: Connection): Promise<HttpResponse> {
    const driver = await loadDriver();
    if (!driver) {
      return DRIVER_NOT_AVAILABLE;
    }
    try {
      const opts = toNativeOpts(connection);
      const result = await driver.nativeSchemaRegistryCommandRoundtrip(opts);
      return { status: 200, data: result, headers: {} };
    } catch (err) {
      return nativeError(err);
    }
  }

  /** Structured test-connection result (mirrors HttpClient.testConnection). */
  async testConnection(
    connection: Connection
  ): Promise<{ isHealthy: boolean; message: string }> {
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

export function createNativeClient(): NativeClient {
  return new NativeClient();
}
