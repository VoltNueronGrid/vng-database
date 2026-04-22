import { EndpointCheckResult, HttpResult } from "./types";
import { RuntimeConnection } from "./config";
import { Connection } from "./models/Connection";
import { DriverError, executeDriverRequest, makeVngDriver } from "./services/DriverAdapter";

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
  try {
    const driver = makeVngDriver(runtimeToManagedConnection(connection));
    const req = {
      method: options.method,
      url: `${connection.settings.baseUrl}${options.path}`,
      headers: {
        "content-type": "application/json",
      },
      body: options.body,
    };
    // Keep checks deterministic and fast by disabling retries in ad-hoc probes.
    const result = await executeDriverRequest(req, { maxRetries: 0 });
    return {
      status: result.status,
      bodyText: result.bodyText,
    };
  } catch (error: unknown) {
    if (error instanceof DriverError) {
      return {
        status: error.statusCode ?? 0,
        bodyText: error.message,
      };
    }
    return {
      status: 0,
      bodyText: error instanceof Error ? error.message : "Unknown request error",
    };
  }
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

function runtimeToManagedConnection(connection: RuntimeConnection): Connection {
  return {
    id: `runtime-${connection.settings.mode}-${connection.settings.baseUrl}`,
    settings: {
      id: `runtime-${connection.settings.mode}-${connection.settings.baseUrl}`,
      name: "Runtime",
      serverType: "voltnuerongrid",
      runtimeTarget: connection.settings.runtimeTarget,
      baseUrl: connection.settings.baseUrl,
      host: "127.0.0.1",
      port: 8080,
      mode: connection.settings.mode,
      adminKey: connection.adminApiKey,
      operatorId: connection.settings.operatorId,
      tenantId: connection.settings.tenantId,
      userId: connection.settings.userId,
      ssl: {
        enabled: false,
      },
      advanced: {
        connectionTimeout: 5000,
      },
      createdAt: Date.now(),
    },
    isActive: true,
    isConnected: false,
    state: "active",
    diagnostics: {},
  };
}
