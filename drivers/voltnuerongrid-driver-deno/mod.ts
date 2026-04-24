/**
 * VoltNueronGrid Deno adapter.
 *
 * Re-exports the full TypeScript driver API surface with Deno-compatible
 * imports. The `performDriverHttpRequest` function is re-exported from
 * httpExecution.ts, which uses `fetch` — available globally in Deno without
 * any polyfill.
 *
 * Usage:
 *   import { VoltNueronGridDriver, validateConfig } from "./mod.ts";
 *
 * Or from a URL (once published to deno.land/x):
 *   import { VoltNueronGridDriver } from "https://deno.land/x/voltnuerongrid_driver/mod.ts";
 */

// Re-export everything from the TypeScript driver.
// Deno resolves .ts imports natively; no compilation step needed.
export {
  // Config & validation
  validateConfig,
  httpRestBaseUrl,

  // Transport helpers
  selectTransportFromBaseUrl,
  parseVngHostForDiscovery,
  inferHttpBaseUrlFromVngUrl,
  parseDiscoveryHttpPortStr,
  discoveryHttpPortFromEnv,
  resolveAutoTransport,
  resolveAutoTransportWithDiscovery,
  inferTransportCapabilitiesTcp,
  inferTransportCapabilitiesTcpWithDiscovery,
  probeTcpConnect,
  parseHostPort,

  // Driver class
  VoltNueronGridDriver,

  // Constants
  DEFAULT_HTTP_DISCOVERY_PORT,
} from "../voltnuerongrid-driver-typescript/src/index.ts";

export type {
  DriverMode,
  DriverTransportMode,
  DriverConfig,
  DriverRequest,
  TransportCapabilities,
  TransportResolution,
  AutoTransportResolution,
} from "../voltnuerongrid-driver-typescript/src/index.ts";

export {
  // HTTP execution
  performDriverHttpRequest,
  isRetryableHttpStatus,
  DriverError,

  // Constants
  DEFAULT_HTTP_REQUEST_TIMEOUT_MS,
  DEFAULT_HTTP_MAX_RETRIES,
} from "../voltnuerongrid-driver-typescript/src/httpExecution.ts";

export type {
  DriverErrorKind,
  HttpDriverRequest,
  HttpExecutionOptions,
  HttpExecutionResult,
} from "../voltnuerongrid-driver-typescript/src/httpExecution.ts";
