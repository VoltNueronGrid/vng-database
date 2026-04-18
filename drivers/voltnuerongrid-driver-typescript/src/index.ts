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

export function selectTransportFromBaseUrl(baseUrl: string): DriverTransportMode {
  const b = baseUrl.trim().toLowerCase();
  return b.startsWith("vng://") ? "native" : "http";
}

/** Dual-endpoint auto: native-first when `httpFallbackUrl` is set (see `transport-mode-cases.json`). */
export function resolveAutoTransport(
  config: DriverConfig,
  caps: TransportCapabilities
): AutoTransportResolution {
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
      throw new Error(
        "native unavailable and no httpFallbackUrl is configured for HTTP fallback"
      );
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

export function validateConfig(config: DriverConfig): string | null {
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
  return null;
}

/** Base URL for REST paths; when `baseUrl` is `vng://`, requires `httpFallbackUrl`. */
export function httpRestBaseUrl(config: DriverConfig): string {
  const b = config.baseUrl.trim().toLowerCase();
  if (b.startsWith("vng://")) {
    const h = config.httpFallbackUrl?.trim();
    if (!h) {
      throw new Error(
        "httpFallbackUrl is required when baseUrl uses vng:// (REST APIs need an http(s) endpoint)"
      );
    }
    return h.replace(/\/$/, "");
  }
  return config.baseUrl.trim().replace(/\/$/, "");
}

function buildHeaders(config: DriverConfig): Record<string, string> {
  const headers: Record<string, string> = {
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

function buildPost(config: DriverConfig, path: string, payload: unknown): DriverRequest {
  const base = httpRestBaseUrl(config);
  return {
    method: "POST",
    url: `${base}${path}`,
    headers: buildHeaders(config),
    bodyJson: JSON.stringify(payload),
  };
}

export class VoltNueronGridDriver {
  constructor(private readonly config: DriverConfig) {
    const error = validateConfig(config);
    if (error) {
      throw new Error(error);
    }
  }

  /** Resolves effective transport; `auto` uses `baseUrl` scheme (`vng://` → native). */
  resolveTransportMode(mode: DriverTransportMode): TransportResolution {
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

  buildHealthRequest(): DriverRequest {
    const base = httpRestBaseUrl(this.config);
    return {
      method: "GET",
      url: `${base}/health`,
      headers: buildHeaders(this.config),
    };
  }

  buildSqlAnalyzeRequest(sqlBatch: string): DriverRequest {
    return buildPost(this.config, "/api/v1/sql/analyze", { sql_batch: sqlBatch });
  }

  buildSqlRouteRequest(sqlBatch: string): DriverRequest {
    return buildPost(this.config, "/api/v1/sql/route", { sql_batch: sqlBatch });
  }

  buildSqlExecuteRequest(sqlBatch: string, maxRows?: number): DriverRequest {
    return buildPost(this.config, "/api/v1/sql/execute", { sql_batch: sqlBatch, max_rows: maxRows });
  }

  buildSqlTransactionRequest(statements: string[]): DriverRequest {
    return buildPost(this.config, "/api/v1/sql/transaction", { statements });
  }

  buildSchemaRegistryRequest(): DriverRequest {
    const base = httpRestBaseUrl(this.config);
    return {
      method: "GET",
      url: `${base}/api/v1/ingest/schema/registry`,
      headers: buildHeaders(this.config),
    };
  }
}
