/**
 * DriverAdapter: bridges ConnectionSettings → DriverConfig and executes DriverRequests.
 *
 * S3-001: replaces ad-hoc fetch() calls with the TS driver abstraction so the extension
 * no longer constructs HTTP calls, auth headers, or retry logic directly.
 */

import {
  DriverConfig,
  DriverMode,
  VoltNueronGridDriver,
  performDriverHttpRequest,
  DriverError,
} from "@voltnuerongrid/driver-typescript";
import type {
  HttpDriverRequest,
  HttpExecutionOptions,
  HttpExecutionResult,
} from "@voltnuerongrid/driver-typescript";
import { Connection } from "../models/Connection";

/** Extension version injected as User-Agent for observability on the server side. */
const EXTENSION_USER_AGENT = "VoltNueronGrid-VSCode/0.3.2";

/**
 * Maps a VS Code extension Connection to a DriverConfig.
 *
 * - `sessionId` uses the connection's stable id so per-connection tracing works.
 * - `requestTimeoutMs` inherits the advanced connectionTimeout when set.
 * - `adminKey` → `adminApiKey` rename follows the driver-core-contract v1 field name.
 */
export function connectionToDriverConfig(connection: Connection): DriverConfig {
  const s = connection.settings;
  const config: DriverConfig = {
    baseUrl: s.baseUrl,
    sessionId: s.id,
    mode: s.mode as DriverMode,
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
  // Preserve native endpoint when the connection specifies one (dual-transport path).
  if (s.nativeEndpoint?.trim()) {
    config.httpFallbackUrl = s.baseUrl;
    config.baseUrl = s.nativeEndpoint.trim();
  }
  return config;
}

/**
 * Creates a VoltNueronGridDriver for the given connection.
 * Throws a DriverError (validation) if the connection settings are insufficient
 * for the selected auth mode (e.g., admin mode without an adminKey).
 */
export function makeVngDriver(connection: Connection): VoltNueronGridDriver {
  return new VoltNueronGridDriver(connectionToDriverConfig(connection));
}

/**
 * Executes a DriverRequest through `performDriverHttpRequest`, adding the
 * extension User-Agent header before dispatch.
 */
export async function executeDriverRequest(
  req: HttpDriverRequest,
  opts?: HttpExecutionOptions
): Promise<HttpExecutionResult> {
  const augmented: HttpDriverRequest = {
    ...req,
    headers: {
      ...req.headers,
      "user-agent": EXTENSION_USER_AGENT,
    },
  };
  return performDriverHttpRequest(augmented, opts);
}

export { DriverError };
export type { HttpExecutionOptions };
