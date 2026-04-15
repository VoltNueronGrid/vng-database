// Re-export all models from model files for easier imports
export * from "./models/Connection";
export * from "./models/Schema";
export * from "./models/QueryResult";

// Legacy compatibility exports
export type ConnectionMode = "admin" | "operator" | "tenant";
export type RuntimeTarget = "local" | "docker" | "cloud" | "custom";

// Legacy runtime connection settings used by the existing wizard/client flow.
export interface RuntimeConnectionSettings {
  baseUrl: string;
  runtimeTarget: RuntimeTarget;
  mode: ConnectionMode;
  operatorId?: string;
  tenantId?: string;
  userId?: string;
}

export interface EndpointCheckResult {
  endpoint: string;
  method: "GET" | "POST";
  ok: boolean;
  status: number;
  detail: string;
}

export interface HttpResult {
  status: number;
  bodyText: string;
}
