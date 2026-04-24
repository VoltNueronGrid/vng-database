export type DriverMode = "admin" | "operator" | "tenant";
/** Dual-transport selector (NT-S3-002 / NT-S4-001). */
export type DriverTransportMode = "http" | "native" | "auto";
export interface TransportResolution {
    active: DriverTransportMode;
    usedAutoResolution: boolean;
    notes?: string;
}
/** Matches Rust `TransportCapabilities` / shared `transport-mode-cases.json` runtimeCapabilities. */
export interface TransportCapabilities {
    nativeAvailable: boolean;
    httpAvailable: boolean;
}
export interface AutoTransportResolution {
    active: DriverTransportMode;
    fallbackTriggered: boolean;
    fallbackReason?: string;
    notes?: string;
}
export declare function selectTransportFromBaseUrl(baseUrl: string): DriverTransportMode;
/** Default HTTP port when inferring `http://…` from `vng://…` (matches Rust `DEFAULT_HTTP_DISCOVERY_PORT`). */
export declare const DEFAULT_HTTP_DISCOVERY_PORT = 8080;
/** Host part of `vng://host[:nativePort][/…]` for building `http://host:httpPort`. */
export declare function parseVngHostForDiscovery(vngUrl: string): string;
/** Builds `http://host:httpPort` from a `vng://` URL (HTTP port is not the native wire port). */
export declare function inferHttpBaseUrlFromVngUrl(vngUrl: string, httpPort: number): string;
/** Parses `VNG_HTTP_DISCOVERY_PORT` into `1..65535`, else `undefined`. */
export declare function parseDiscoveryHttpPortStr(s: string): number | undefined;
export declare function discoveryHttpPortFromEnv(): number | undefined;
/**
 * Like `resolveAutoTransport`, but when `httpFallbackUrl` is unset and `discoveryHttpPort` is set,
 * infers the HTTP base from `baseUrl` so dual-endpoint auto works without a second URL string.
 */
export declare function resolveAutoTransportWithDiscovery(config: DriverConfig, caps: TransportCapabilities, discoveryHttpPort: number | undefined): AutoTransportResolution;
/** Dual-endpoint auto: native-first when `httpFallbackUrl` is set (see `transport-mode-cases.json`). */
export declare function resolveAutoTransport(config: DriverConfig, caps: TransportCapabilities): AutoTransportResolution;
export interface DriverConfig {
    baseUrl: string;
    /** When `baseUrl` is `vng://...`, set this to the HTTP runtime base for REST and for auto fallback. */
    httpFallbackUrl?: string;
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
export interface DriverRequest {
    method: "GET" | "POST";
    url: string;
    headers: Record<string, string>;
    bodyJson?: string;
}
export declare function validateConfig(config: DriverConfig): string | null;
/** Base URL for REST paths; when `baseUrl` is `vng://`, requires `httpFallbackUrl`. */
export declare function httpRestBaseUrl(config: DriverConfig): string;
export declare class VoltNueronGridDriver {
    private readonly config;
    constructor(config: DriverConfig);
    /** Resolves effective transport; `auto` uses `baseUrl` scheme (`vng://` → native). */
    resolveTransportMode(mode: DriverTransportMode): TransportResolution;
    buildHealthRequest(): DriverRequest;
    buildSqlAnalyzeRequest(sqlBatch: string): DriverRequest;
    buildSqlRouteRequest(sqlBatch: string): DriverRequest;
    buildSqlExecuteRequest(sqlBatch: string, maxRows?: number): DriverRequest;
    buildSqlTransactionRequest(statements: string[]): DriverRequest;
    buildSchemaRegistryRequest(): DriverRequest;
}
/** Parses `host:port` for TCP probes (bracket IPv6 supported). */
export declare function parseHostPort(hostPort: string): {
    host: string;
    port: number;
};
export declare function probeTcpConnect(hostPort: string, timeoutMs: number): Promise<boolean>;
/** TCP reachability for native + HTTP origins (Rust `infer_transport_capabilities_tcp` parity). */
export declare function inferTransportCapabilitiesTcp(config: DriverConfig, nativeConnectTimeoutMs: number, httpConnectTimeoutMs: number): Promise<TransportCapabilities>;
/** Like `inferTransportCapabilitiesTcp`, but may infer HTTP from `vng://` + discovery port. */
export declare function inferTransportCapabilitiesTcpWithDiscovery(config: DriverConfig, nativeConnectTimeoutMs: number, httpConnectTimeoutMs: number, discoveryHttpPort: number | undefined): Promise<TransportCapabilities>;
export * from "./nativeWire";
export * from "./nativeSession";
export * from "./httpExecution";
