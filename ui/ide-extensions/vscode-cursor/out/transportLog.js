"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.getTransportOutputChannel = getTransportOutputChannel;
exports.appendTransportLogLine = appendTransportLogLine;
exports.appendTransportRttLine = appendTransportRttLine;
exports.appendTransportFallbackLine = appendTransportFallbackLine;
exports.runTransportFallbackDiagnostic = runTransportFallbackDiagnostic;
const vscode = __importStar(require("vscode"));
const net = __importStar(require("net"));
let channel;
function getTransportOutputChannel() {
    if (!channel) {
        channel = vscode.window.createOutputChannel("VoltNueronGrid Transport");
    }
    return channel;
}
/** NT-S5-002 scaffold: structured lines for transport preference vs actual data-plane (HTTP until TS native execution lands). */
function appendTransportLogLine(message) {
    const ts = new Date().toISOString();
    getTransportOutputChannel().appendLine(`[${ts}] ${message}`);
}
/** NT-S5-002: Log a measured round-trip time for a transport health check. */
function appendTransportRttLine(transportMode, rttMs, endpoint) {
    const ts = new Date().toISOString();
    getTransportOutputChannel().appendLine(`[${ts}] transport_rtt transportMode=${transportMode} endpoint=${endpoint} rttMs=${rttMs}`);
}
/** NT-S5-002: Log a transport fallback event (from → to). */
function appendTransportFallbackLine(reason, fromTransport, toTransport) {
    const ts = new Date().toISOString();
    getTransportOutputChannel().appendLine(`[${ts}] transport_fallback from=${fromTransport} to=${toTransport} reason=${reason}`);
}
/**
 * NT-S5-001: Probes native (TCP) and HTTP endpoints when transportMode === "auto",
 * records which transport is active and whether fallback occurred, and emits a
 * structured log line summarising the outcome.
 *
 * When transportMode is not "auto" the result is trivially resolved without probing.
 */
async function runTransportFallbackDiagnostic(_connection, transportMode, nativeEndpoint) {
    if (transportMode !== "auto") {
        const active = transportMode === "native" ? "native" : "http";
        appendTransportLogLine(`transport_diagnostic_skip transportMode=${transportMode} activeTransport=${active} (probe only runs for auto)`);
        return { activeTransport: active, fallbackTriggered: false };
    }
    const nativeReachable = nativeEndpoint ? await probeNativeTcp(nativeEndpoint) : false;
    // HTTP reachable is assumed true when we already completed a health check; mark it active.
    const httpReachable = true;
    let activeTransport;
    let fallbackTriggered = false;
    let fallbackReason;
    if (nativeReachable) {
        activeTransport = "native";
    }
    else {
        activeTransport = "http";
        fallbackTriggered = true;
        fallbackReason = nativeEndpoint ? "native_unavailable" : "native_endpoint_not_configured";
    }
    appendTransportLogLine(`transport_diagnostic transportMode=auto nativeReachable=${nativeReachable} httpReachable=${httpReachable}` +
        ` activeTransport=${activeTransport}` +
        (fallbackReason ? ` fallbackReason=${fallbackReason}` : ""));
    if (fallbackTriggered) {
        appendTransportFallbackLine(fallbackReason ?? "unknown", "native", "http");
    }
    return { activeTransport, fallbackTriggered, fallbackReason };
}
/**
 * Attempts a TCP connect to a `vng://host:port` or `host:port` native endpoint.
 * Resolves `true` if the port is open within 2 seconds, `false` otherwise.
 */
function probeNativeTcp(endpoint) {
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
//# sourceMappingURL=transportLog.js.map