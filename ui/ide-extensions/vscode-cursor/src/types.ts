export type ConnectionMode = "admin" | "operator" | "tenant";
export type RuntimeTarget = "local" | "docker" | "cloud" | "custom";

export interface ConnectionSettings {
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
