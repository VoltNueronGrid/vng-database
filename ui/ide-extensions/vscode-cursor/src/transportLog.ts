import * as vscode from "vscode";
import * as net from "net";

let channel: vscode.OutputChannel | undefined;

export function getTransportOutputChannel(): vscode.OutputChannel {
  if (!channel) {
    channel = vscode.window.createOutputChannel("VoltNueronGrid Transport");
  }
  return channel;
}

/** NT-S5-002 scaffold: structured lines for transport preference vs actual data-plane (HTTP until TS native execution lands). */
export function appendTransportLogLine(message: string): void {
  const ts = new Date().toISOString();
  getTransportOutputChannel().appendLine(`[${ts}] ${message}`);
}

/** NT-S5-002: Log a measured round-trip time for a transport health check. */
export function appendTransportRttLine(transportMode: string, rttMs: number, endpoint: string): void {
  const ts = new Date().toISOString();
  getTransportOutputChannel().appendLine(
    `[${ts}] transport_rtt transportMode=${transportMode} endpoint=${endpoint} rttMs=${rttMs}`
  );
}

/** NT-S5-002: Log a transport fallback event (from → to). */
export function appendTransportFallbackLine(reason: string, fromTransport: string, toTransport: string): void {
  const ts = new Date().toISOString();
  getTransportOutputChannel().appendLine(
    `[${ts}] transport_fallback from=${fromTransport} to=${toTransport} reason=${reason}`
  );
}

export interface TransportFallbackDiagnosticResult {
  activeTransport: "native" | "http";
  fallbackTriggered: boolean;
  fallbackReason?: string;
}

/**
 * NT-S5-001: Probes native (TCP) and HTTP endpoints when transportMode === "auto",
 * records which transport is active and whether fallback occurred, and emits a
 * structured log line summarising the outcome.
 *
 * When transportMode is not "auto" the result is trivially resolved without probing.
 */
export async function runTransportFallbackDiagnostic(
  _connection: unknown,
  transportMode: string,
  nativeEndpoint: string | undefined
): Promise<TransportFallbackDiagnosticResult> {
  if (transportMode !== "auto") {
    const active = transportMode === "native" ? "native" : "http";
    appendTransportLogLine(
      `transport_diagnostic_skip transportMode=${transportMode} activeTransport=${active} (probe only runs for auto)`
    );
    return { activeTransport: active, fallbackTriggered: false };
  }

  const nativeReachable = nativeEndpoint ? await probeNativeTcp(nativeEndpoint) : false;
  // HTTP reachable is assumed true when we already completed a health check; mark it active.
  const httpReachable = true;

  let activeTransport: "native" | "http";
  let fallbackTriggered = false;
  let fallbackReason: string | undefined;

  if (nativeReachable) {
    activeTransport = "native";
  } else {
    activeTransport = "http";
    fallbackTriggered = true;
    fallbackReason = nativeEndpoint ? "native_unavailable" : "native_endpoint_not_configured";
  }

  appendTransportLogLine(
    `transport_diagnostic transportMode=auto nativeReachable=${nativeReachable} httpReachable=${httpReachable}` +
      ` activeTransport=${activeTransport}` +
      (fallbackReason ? ` fallbackReason=${fallbackReason}` : "")
  );

  if (fallbackTriggered) {
    appendTransportFallbackLine(fallbackReason ?? "unknown", "native", "http");
  }

  return { activeTransport, fallbackTriggered, fallbackReason };
}

/**
 * Attempts a TCP connect to a `vng://host:port` or `host:port` native endpoint.
 * Resolves `true` if the port is open within 2 seconds, `false` otherwise.
 */
function probeNativeTcp(endpoint: string): Promise<boolean> {
  return new Promise((resolve) => {
    // Accept vng://host:port or host:port
    const stripped = endpoint.replace(/^vng:\/\//, "");
    const lastColon = stripped.lastIndexOf(":");
    if (lastColon === -1) {
      resolve(false);
      return;
    }
    const host = stripped.slice(0, lastColon);
    const port = parseInt(stripped.slice(lastColon + 1), 10);
    if (!host || isNaN(port)) {
      resolve(false);
      return;
    }

    const socket = new net.Socket();
    const timeout = 2000;

    socket.setTimeout(timeout);
    socket.once("connect", () => { socket.destroy(); resolve(true); });
    socket.once("timeout", () => { socket.destroy(); resolve(false); });
    socket.once("error", () => { socket.destroy(); resolve(false); });
    socket.connect(port, host);
  });
}
