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
var __importStar = (this && this.__importStar) || function (mod) {
    if (mod && mod.__esModule) return mod;
    var result = {};
    if (mod != null) for (var k in mod) if (k !== "default" && Object.prototype.hasOwnProperty.call(mod, k)) __createBinding(result, mod, k);
    __setModuleDefault(result, mod);
    return result;
};
var __exportStar = (this && this.__exportStar) || function(m, exports) {
    for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports, p)) __createBinding(exports, m, p);
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.VoltNueronGridDriver = exports.DEFAULT_HTTP_DISCOVERY_PORT = void 0;
exports.selectTransportFromBaseUrl = selectTransportFromBaseUrl;
exports.parseVngHostForDiscovery = parseVngHostForDiscovery;
exports.inferHttpBaseUrlFromVngUrl = inferHttpBaseUrlFromVngUrl;
exports.parseDiscoveryHttpPortStr = parseDiscoveryHttpPortStr;
exports.discoveryHttpPortFromEnv = discoveryHttpPortFromEnv;
exports.resolveAutoTransportWithDiscovery = resolveAutoTransportWithDiscovery;
exports.resolveAutoTransport = resolveAutoTransport;
exports.validateConfig = validateConfig;
exports.httpRestBaseUrl = httpRestBaseUrl;
exports.parseHostPort = parseHostPort;
exports.probeTcpConnect = probeTcpConnect;
exports.inferTransportCapabilitiesTcp = inferTransportCapabilitiesTcp;
exports.inferTransportCapabilitiesTcpWithDiscovery = inferTransportCapabilitiesTcpWithDiscovery;
const net = __importStar(require("node:net"));
function selectTransportFromBaseUrl(baseUrl) {
    const b = baseUrl.trim().toLowerCase();
    return b.startsWith("vng://") ? "native" : "http";
}
/** Default HTTP port when inferring `http://…` from `vng://…` (matches Rust `DEFAULT_HTTP_DISCOVERY_PORT`). */
exports.DEFAULT_HTTP_DISCOVERY_PORT = 8080;
/** Host part of `vng://host[:nativePort][/…]` for building `http://host:httpPort`. */
function parseVngHostForDiscovery(vngUrl) {
    const t = vngUrl.trim();
    const rest = t.startsWith("vng://") ? t.slice("vng://".length) : "";
    if (!rest) {
        throw new Error("expected vng:// URL");
    }
    const hostPart = rest.split(/[/\?]/)[0]?.trim() ?? "";
    if (!hostPart) {
        throw new Error("vng URL host is empty");
    }
    if (hostPart.startsWith("[")) {
        const end = hostPart.indexOf("]");
        if (end <= 0) {
            throw new Error("invalid IPv6 bracket in vng URL");
        }
        return hostPart.slice(1, end);
    }
    const lastColon = hostPart.lastIndexOf(":");
    if (lastColon > 0) {
        const maybePort = hostPart.slice(lastColon + 1);
        if (/^\d+$/.test(maybePort) && !hostPart.slice(0, lastColon).includes(":")) {
            return hostPart.slice(0, lastColon);
        }
    }
    return hostPart;
}
/** Builds `http://host:httpPort` from a `vng://` URL (HTTP port is not the native wire port). */
function inferHttpBaseUrlFromVngUrl(vngUrl, httpPort) {
    if (!Number.isInteger(httpPort) || httpPort < 1 || httpPort > 65535) {
        throw new Error("http discovery port must be 1..65535");
    }
    const host = parseVngHostForDiscovery(vngUrl);
    if (host.includes(":")) {
        return `http://[${host}]:${httpPort}`;
    }
    return `http://${host}:${httpPort}`;
}
/** Parses `VNG_HTTP_DISCOVERY_PORT` into `1..65535`, else `undefined`. */
function parseDiscoveryHttpPortStr(s) {
    const t = s.trim();
    if (!t) {
        return undefined;
    }
    const n = Number.parseInt(t, 10);
    if (!Number.isInteger(n) || n < 1 || n > 65535) {
        return undefined;
    }
    return n;
}
function discoveryHttpPortFromEnv() {
    const raw = typeof process !== "undefined" && process.env ? process.env.VNG_HTTP_DISCOVERY_PORT : undefined;
    if (raw === undefined) {
        return undefined;
    }
    return parseDiscoveryHttpPortStr(raw);
}
/**
 * Like `resolveAutoTransport`, but when `httpFallbackUrl` is unset and `discoveryHttpPort` is set,
 * infers the HTTP base from `baseUrl` so dual-endpoint auto works without a second URL string.
 */
function resolveAutoTransportWithDiscovery(config, caps, discoveryHttpPort) {
    const port = discoveryHttpPort ?? discoveryHttpPortFromEnv();
    if (port === undefined) {
        return resolveAutoTransport(config, caps);
    }
    if ((config.httpFallbackUrl ?? "").trim()) {
        return resolveAutoTransport(config, caps);
    }
    const base = config.baseUrl.trim();
    if (!base.toLowerCase().startsWith("vng://")) {
        return resolveAutoTransport(config, caps);
    }
    const inferred = inferHttpBaseUrlFromVngUrl(base, port);
    return resolveAutoTransport({ ...config, httpFallbackUrl: inferred }, caps);
}
/** Dual-endpoint auto: native-first when `httpFallbackUrl` is set (see `transport-mode-cases.json`). */
function resolveAutoTransport(config, caps) {
    const dual = Boolean(config.httpFallbackUrl?.trim());
    if (dual) {
        if (caps.nativeAvailable) {
            return {
                active: "native",
                fallbackTriggered: false,
                notes: "auto: dual-endpoint; native available (native-first)"
            };
        }
        if (caps.httpAvailable) {
            return {
                active: "http",
                fallbackTriggered: true,
                fallbackReason: "native_unavailable",
                notes: "auto: dual-endpoint; fell back to httpFallbackUrl"
            };
        }
        throw new Error("no available transport: native and http are unavailable");
    }
    const base = config.baseUrl.trim();
    const isVng = base.toLowerCase().startsWith("vng://");
    if (isVng) {
        if (caps.nativeAvailable) {
            return {
                active: "native",
                fallbackTriggered: false,
                notes: "auto: single vng URL; native available"
            };
        }
        if (caps.httpAvailable) {
            throw new Error("native unavailable and no httpFallbackUrl is configured for HTTP fallback");
        }
        throw new Error("no available transport: native and http are unavailable");
    }
    if (caps.httpAvailable) {
        return {
            active: "http",
            fallbackTriggered: false,
            notes: "auto: single http(s) URL"
        };
    }
    throw new Error("no available transport: native and http are unavailable");
}
function validateConfig(config) {
    if (!config.baseUrl.trim()) {
        return "baseUrl must not be empty";
    }
    if (!config.sessionId.trim()) {
        return "sessionId must not be empty";
    }
    if (config.mode === "admin" && !config.adminApiKey?.trim()) {
        return "admin mode requires adminApiKey";
    }
    if (config.mode === "operator") {
        if (!config.adminApiKey?.trim() || !config.operatorId?.trim()) {
            return "operator mode requires adminApiKey and operatorId";
        }
    }
    if (config.mode === "tenant" && !config.tenantId?.trim()) {
        return "tenant mode requires tenantId";
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
/** Base URL for REST paths; when `baseUrl` is `vng://`, requires `httpFallbackUrl`. */
function httpRestBaseUrl(config) {
    const b = config.baseUrl.trim().toLowerCase();
    if (b.startsWith("vng://")) {
        const h = config.httpFallbackUrl?.trim();
        if (!h) {
            throw new Error("httpFallbackUrl is required when baseUrl uses vng:// (REST APIs need an http(s) endpoint)");
        }
        return h.replace(/\/$/, "");
    }
    return config.baseUrl.trim().replace(/\/$/, "");
}
function buildHeaders(config) {
    const headers = {
        "content-type": "application/json",
        "x-vng-session-id": config.sessionId,
    };
    if ((config.mode === "admin" || config.mode === "operator") && config.adminApiKey) {
        headers["x-vng-admin-key"] = config.adminApiKey;
    }
    if (config.mode === "operator" && config.operatorId) {
        headers["x-vng-operator-id"] = config.operatorId;
    }
    if (config.mode === "tenant" && config.tenantId) {
        headers["x-vng-tenant-id"] = config.tenantId;
    }
    if (config.mode === "tenant" && config.userId) {
        headers["x-vng-user-id"] = config.userId;
    }
    if (config.routeHint) {
        headers["x-vng-route-hint"] = config.routeHint;
    }
    return headers;
}
function buildPost(config, path, payload) {
    const base = httpRestBaseUrl(config);
    return {
        method: "POST",
        url: `${base}${path}`,
        headers: buildHeaders(config),
        bodyJson: JSON.stringify(payload),
    };
}
class VoltNueronGridDriver {
    config;
    constructor(config) {
        this.config = config;
        const error = validateConfig(config);
        if (error) {
            throw new Error(error);
        }
    }
    /** Resolves effective transport; `auto` uses `baseUrl` scheme (`vng://` → native). */
    resolveTransportMode(mode) {
        if (mode === "http" || mode === "native") {
            return { active: mode, usedAutoResolution: false };
        }
        const active = selectTransportFromBaseUrl(this.config.baseUrl);
        return {
            active,
            usedAutoResolution: true,
            notes: `auto: selected ${active} from baseUrl scheme`,
        };
    }
    buildHealthRequest() {
        const base = httpRestBaseUrl(this.config);
        return {
            method: "GET",
            url: `${base}/health`,
            headers: buildHeaders(this.config),
        };
    }
    buildSqlAnalyzeRequest(sqlBatch) {
        return buildPost(this.config, "/api/v1/sql/analyze", { sql_batch: sqlBatch });
    }
    buildSqlRouteRequest(sqlBatch) {
        return buildPost(this.config, "/api/v1/sql/route", { sql_batch: sqlBatch });
    }
    buildSqlExecuteRequest(sqlBatch, maxRows) {
        return buildPost(this.config, "/api/v1/sql/execute", { sql_batch: sqlBatch, max_rows: maxRows });
    }
    buildSqlTransactionRequest(statements) {
        return buildPost(this.config, "/api/v1/sql/transaction", { statements });
    }
    buildSchemaRegistryRequest() {
        const base = httpRestBaseUrl(this.config);
        return {
            method: "GET",
            url: `${base}/api/v1/ingest/schema/registry`,
            headers: buildHeaders(this.config),
        };
    }
}
exports.VoltNueronGridDriver = VoltNueronGridDriver;
/** Parses `host:port` for TCP probes (bracket IPv6 supported). */
function parseHostPort(hostPort) {
    const s = hostPort.trim();
    if (!s) {
        throw new Error("empty host:port");
    }
    if (s.startsWith("[")) {
        const end = s.indexOf("]");
        if (end <= 0) {
            throw new Error("invalid bracketed host:port");
        }
        const host = s.slice(1, end);
        const rest = s.slice(end + 1);
        if (!rest.startsWith(":")) {
            throw new Error("expected ]:port");
        }
        const port = Number.parseInt(rest.slice(1), 10);
        if (!Number.isInteger(port) || port < 1 || port > 65535) {
            throw new Error("invalid port");
        }
        return { host, port };
    }
    const lastColon = s.lastIndexOf(":");
    if (lastColon <= 0) {
        throw new Error("invalid host:port");
    }
    const host = s.slice(0, lastColon);
    const port = Number.parseInt(s.slice(lastColon + 1), 10);
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
        throw new Error("invalid port");
    }
    return { host, port };
}
function probeTcpConnect(hostPort, timeoutMs) {
    const t = hostPort.trim();
    if (!t || timeoutMs < 1) {
        return Promise.resolve(false);
    }
    let host;
    let port;
    try {
        ({ host, port } = parseHostPort(t));
    }
    catch {
        return Promise.resolve(false);
    }
    return new Promise((resolve) => {
        const socket = net.createConnection({ host, port });
        const timer = setTimeout(() => {
            socket.destroy();
            resolve(false);
        }, timeoutMs);
        socket.once("connect", () => {
            clearTimeout(timer);
            socket.end();
            resolve(true);
        });
        socket.once("error", () => {
            clearTimeout(timer);
            resolve(false);
        });
    });
}
function httpOriginHostPortForProbe(url) {
    const u = url.trim();
    const rest = u.startsWith("http://")
        ? u.slice("http://".length)
        : u.startsWith("https://")
            ? u.slice("https://".length)
            : undefined;
    if (rest === undefined) {
        return undefined;
    }
    const hostport = rest.split("/")[0]?.split("?")[0]?.trim();
    return hostport && hostport.length > 0 ? hostport : undefined;
}
function tryHttpRestBaseUrl(config) {
    const b = config.baseUrl.trim().toLowerCase();
    if (b.startsWith("vng://")) {
        const h = config.httpFallbackUrl?.trim();
        return h ? h.replace(/\/$/, "") : undefined;
    }
    return config.baseUrl.trim().replace(/\/$/, "");
}
/** TCP reachability for native + HTTP origins (Rust `infer_transport_capabilities_tcp` parity). */
async function inferTransportCapabilitiesTcp(config, nativeConnectTimeoutMs, httpConnectTimeoutMs) {
    let nativeAvailable = false;
    const base = config.baseUrl.trim();
    if (base.toLowerCase().startsWith("vng://")) {
        const hp = base.slice("vng://".length).split("/")[0]?.split("?")[0]?.trim();
        if (hp) {
            nativeAvailable = await probeTcpConnect(hp, nativeConnectTimeoutMs);
        }
    }
    let httpAvailable = false;
    const httpBase = tryHttpRestBaseUrl(config);
    if (httpBase) {
        const hp = httpOriginHostPortForProbe(httpBase);
        if (hp) {
            httpAvailable = await probeTcpConnect(hp, httpConnectTimeoutMs);
        }
    }
    return { nativeAvailable, httpAvailable };
}
/** Like `inferTransportCapabilitiesTcp`, but may infer HTTP from `vng://` + discovery port. */
async function inferTransportCapabilitiesTcpWithDiscovery(config, nativeConnectTimeoutMs, httpConnectTimeoutMs, discoveryHttpPort) {
    const port = discoveryHttpPort ?? discoveryHttpPortFromEnv();
    const effective = !config.httpFallbackUrl?.trim() &&
        port !== undefined &&
        config.baseUrl.trim().toLowerCase().startsWith("vng://")
        ? { ...config, httpFallbackUrl: inferHttpBaseUrlFromVngUrl(config.baseUrl.trim(), port) }
        : config;
    return inferTransportCapabilitiesTcp(effective, nativeConnectTimeoutMs, httpConnectTimeoutMs);
}
__exportStar(require("./nativeWire"), exports);
__exportStar(require("./nativeSession"), exports);
__exportStar(require("./httpExecution"), exports);
