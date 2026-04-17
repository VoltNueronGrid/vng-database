export type DriverMode = "admin" | "operator" | "tenant";

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
  return {
    method: "POST",
    url: `${config.baseUrl}${path}`,
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

  buildHealthRequest(): DriverRequest {
    return {
      method: "GET",
      url: `${this.config.baseUrl}/health`,
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
    return {
      method: "GET",
      url: `${this.config.baseUrl}/api/v1/ingest/schema/registry`,
      headers: buildHeaders(this.config),
    };
  }
}
