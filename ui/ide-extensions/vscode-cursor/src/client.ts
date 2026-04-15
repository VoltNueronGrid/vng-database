import { EndpointCheckResult, HttpResult } from "./types";
import { RuntimeConnection } from "./config";

interface RequestOptions {
  method: "GET" | "POST";
  path: string;
  body?: unknown;
}

export async function runConnectivityChecks(connection: RuntimeConnection): Promise<EndpointCheckResult[]> {
  const health = await requestRuntime(connection, {
    method: "GET",
    path: "/health",
  });

  const sql = await requestRuntime(connection, {
    method: "POST",
    path: "/api/v1/sql/execute",
    body: {
      sql_batch: ["SELECT 1;"],
      request_id: "ide-connectivity-check",
    },
  });

  const schema = await requestRuntime(connection, {
    method: "GET",
    path: "/api/v1/ingest/schema/registry",
  });

  return [
    toResult("GET", "/health", health.status, health.bodyText),
    toResult("POST", "/api/v1/sql/execute", sql.status, sql.bodyText),
    toResult("GET", "/api/v1/ingest/schema/registry", schema.status, schema.bodyText),
  ];
}

export async function executeSql(connection: RuntimeConnection, sql: string): Promise<HttpResult> {
  return requestRuntime(connection, {
    method: "POST",
    path: "/api/v1/sql/execute",
    body: {
      sql_batch: [sql],
      request_id: "ide-query-runner",
    },
  });
}

export async function analyzeSql(connection: RuntimeConnection, sql: string): Promise<HttpResult> {
  return requestRuntime(connection, {
    method: "POST",
    path: "/api/v1/sql/analyze",
    body: {
      sql,
      request_id: "ide-query-diagnostics",
    },
  });
}

export async function getSchemaRegistry(connection: RuntimeConnection): Promise<HttpResult> {
  return requestRuntime(connection, {
    method: "GET",
    path: "/api/v1/ingest/schema/registry",
  });
}

export async function requestRuntime(connection: RuntimeConnection, options: RequestOptions): Promise<HttpResult> {
  const url = `${connection.settings.baseUrl}${options.path}`;
  const headers: Record<string, string> = {
    "content-type": "application/json",
  };

  if (connection.settings.mode === "admin" || connection.settings.mode === "operator") {
    if (connection.adminApiKey) {
      headers["x-vng-admin-key"] = connection.adminApiKey;
    }
  }

  if (connection.settings.mode === "operator" && connection.settings.operatorId) {
    headers["x-vng-operator-id"] = connection.settings.operatorId;
  }

  if (connection.settings.mode === "tenant") {
    if (connection.settings.tenantId) {
      headers["x-vng-tenant-id"] = connection.settings.tenantId;
    }
    if (connection.settings.userId) {
      headers["x-vng-user-id"] = connection.settings.userId;
    }
  }

  const response = await fetch(url, {
    method: options.method,
    headers,
    body: options.body ? JSON.stringify(options.body) : undefined,
  });

  const bodyText = await response.text();
  return {
    status: response.status,
    bodyText,
  };
}

export function toPermissionMessage(status: number, mode: string): string | undefined {
  if (status === 401) {
    if (mode === "tenant") {
      return "Authentication failed. Verify tenant and user headers.";
    }
    return "Authentication failed. Verify admin key and operator headers.";
  }
  if (status === 403) {
    return "Permission denied for the selected identity and operation.";
  }
  return undefined;
}

function toResult(method: "GET" | "POST", endpoint: string, status: number, bodyText: string): EndpointCheckResult {
  const success = endpoint === "/health" ? status === 200 : status === 200 || status === 401 || status === 403;
  const detail = summarizeBody(bodyText);

  return {
    endpoint,
    method,
    ok: success,
    status,
    detail,
  };
}

function summarizeBody(bodyText: string): string {
  if (!bodyText) {
    return "(empty response)";
  }

  if (bodyText.length <= 200) {
    return bodyText;
  }

  return `${bodyText.slice(0, 200)}...`;
}
