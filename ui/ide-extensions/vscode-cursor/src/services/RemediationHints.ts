/**
 * RemediationHints: produce actionable error messages for connectivity failures.
 */

import { Connection } from "../models/Connection";

/**
 * Build a human-readable remediation hint based on the failed endpoint, HTTP
 * status code, and optional error string returned from the probe.
 *
 * @param endpoint  - The URL that was probed (e.g. "http://127.0.0.1:8080/health")
 * @param status    - HTTP status code, or 0 for network-level failures
 * @param error     - Optional error string (e.g. "ECONNREFUSED", "timeout")
 * @param connection - Optional managed connection for context-aware hints
 */
export function buildRemediationHint(
  endpoint: string,
  status: number,
  error?: string,
  connection?: Connection
): string {
  // Network-level failures (status 0) — check error string for specifics
  if (status === 0 || status === undefined) {
    const lowerError = (error ?? "").toLowerCase();
    if (lowerError.includes("timeout") || lowerError.includes("timed out")) {
      return (
        `Connection timed out. Check firewall rules or increase timeout in connection Advanced settings.`
      );
    }
    // ECONNREFUSED, connection refused, etc.
    const baseUrl = connection?.settings.baseUrl ?? endpoint;
    return (
      `Server not running at ${baseUrl}. Start VoltNueronGrid with \`cargo run -p voltnuerongridd\` or check the port.`
    );
  }

  switch (status) {
    case 401:
      return `Check Admin Key in connection settings (Settings \u2192 Edit Connection)`;
    case 403:
      return `The current role lacks permission. Check role configuration.`;
    case 408:
    case 504: {
      return `Connection timed out. Check firewall rules or increase timeout in connection Advanced settings.`;
    }
    default: {
      // Detect refused / unavailable from error text even when a status code is present
      const lowerError = (error ?? "").toLowerCase();
      if (
        lowerError.includes("refused") ||
        lowerError.includes("econnrefused") ||
        lowerError.includes("not running")
      ) {
        const baseUrl = connection?.settings.baseUrl ?? endpoint;
        return `Server not running at ${baseUrl}. Start VoltNueronGrid with \`cargo run -p voltnuerongridd\` or check the port.`;
      }
      if (lowerError.includes("timeout") || lowerError.includes("timed out")) {
        return `Connection timed out. Check firewall rules or increase timeout in connection Advanced settings.`;
      }
      return `HTTP ${status} from ${endpoint}. See the VoltNueronGrid output channel for details.`;
    }
  }
}
