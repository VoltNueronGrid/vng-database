/**
 * HttpClient: HTTP communication with auth headers and error handling
 */

import { Connection } from "../models/Connection";

export interface HttpResponse<T = any> {
  status: number;
  data?: T;
  error?: string;
  headers: Record<string, string>;
}

export interface QueryRequestOptions {
  timeoutMs?: number;
  signal?: AbortSignal;
  requestId?: string;
}

export class HttpClient {
  private requestTimeout = 30000; // 30 seconds

  /**
   * Execute a query against the server
   */
  async executeQuery(
    connection: Connection,
    query: string,
    options?: QueryRequestOptions
  ): Promise<HttpResponse> {
    return this.post(
      connection,
      "/api/v1/sql/execute",
      {
      sql_batch: [query],
      request_id: options?.requestId ?? `ide-query-${Date.now()}`,
      },
      {
        timeoutMs: options?.timeoutMs,
        signal: options?.signal,
      }
    );
  }

  /**
   * Get schema registry
   */
  async getSchemaRegistry(connection: Connection): Promise<HttpResponse> {
    return this.get(connection, "/api/v1/ingest/schema/registry");
  }

  /**
   * Health check
   */
  async healthCheck(connection: Connection): Promise<HttpResponse> {
    return this.get(connection, "/health");
  }

  /**
   * Test connection
   */
  async testConnection(connection: Connection): Promise<{ isHealthy: boolean; message: string }> {
    try {
      const response = await this.healthCheck(connection);
      if (response.status === 200) {
        return { isHealthy: true, message: "Connection successful" };
      } else {
        return { isHealthy: false, message: `Server returned status ${response.status}` };
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : "Unknown error";
      return { isHealthy: false, message };
    }
  }

  /**
   * Generic GET request
   */
  private async get(connection: Connection, path: string): Promise<HttpResponse> {
    const url = `${connection.settings.baseUrl}${path}`;
    const headers = this.buildHeaders(connection, "GET");

    try {
      const response = await this.fetchWithTimeout(url, {
        method: "GET",
        headers,
      });

      const data = await this.parseResponse(response);
      return {
        status: response.status,
        data,
        headers: this.extractHeaders(response),
      };
    } catch (error) {
      return {
        status: 0,
        error: error instanceof Error ? error.message : "Unknown error",
        headers: {},
      };
    }
  }

  /**
   * Generic POST request
   */
  private async post(
    connection: Connection,
    path: string,
    body: any,
    requestOptions?: { timeoutMs?: number; signal?: AbortSignal }
  ): Promise<HttpResponse> {
    const url = `${connection.settings.baseUrl}${path}`;
    const headers = this.buildHeaders(connection, "POST");

    try {
      const response = await this.fetchWithTimeout(url, {
        method: "POST",
        headers,
        body: JSON.stringify(body),
      }, requestOptions?.timeoutMs, requestOptions?.signal);

      const data = await this.parseResponse(response);
      return {
        status: response.status,
        data,
        headers: this.extractHeaders(response),
      };
    } catch (error) {
      return {
        status: 0,
        error: error instanceof Error ? error.message : "Unknown error",
        headers: {},
      };
    }
  }

  /**
   * Build request headers with auth
   */
  private buildHeaders(connection: Connection, method: string): Record<string, string> {
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
      "User-Agent": "VoltNueronGrid-VSCode/0.3.0",
    };

    const { mode, adminKey, operatorId, tenantId, userId } = connection.settings;

    // Admin or operator mode: include admin key
    if ((mode === "admin" || mode === "operator") && adminKey) {
      headers["x-vng-admin-key"] = adminKey;
    }

    // Operator mode: include operator ID
    if (mode === "operator" && operatorId) {
      headers["x-vng-operator-id"] = operatorId;
    }

    // Tenant mode: include tenant and user IDs
    if (mode === "tenant") {
      if (tenantId) headers["x-vng-tenant-id"] = tenantId;
      if (userId) headers["x-vng-user-id"] = userId;
    }

    return headers;
  }

  /**
   * Parse response based on content-type
   */
  private async parseResponse(response: Response): Promise<any> {
    const contentType = response.headers.get("content-type") || "";

    if (contentType.includes("application/json")) {
      try {
        return await response.json();
      } catch {
        return null;
      }
    }

    if (contentType.includes("text/")) {
      return await response.text();
    }

    return null;
  }

  /**
   * Extract response headers
   */
  private extractHeaders(response: Response): Record<string, string> {
    const headers: Record<string, string> = {};
    response.headers.forEach((value, key) => {
      headers[key] = value;
    });
    return headers;
  }

  /**
   * Fetch with timeout
   */
  private fetchWithTimeout(
    url: string,
    options: RequestInit,
    timeout: number = this.requestTimeout,
    upstreamSignal?: AbortSignal
  ): Promise<Response> {
    const controller = new AbortController();
    let upstreamAbortHandler: (() => void) | undefined;

    if (upstreamSignal) {
      if (upstreamSignal.aborted) {
        controller.abort();
      } else {
        upstreamAbortHandler = () => controller.abort();
        upstreamSignal.addEventListener("abort", upstreamAbortHandler, { once: true });
      }
    }

    const timeoutId = setTimeout(() => controller.abort(new Error(`Request timeout after ${timeout}ms`)), timeout);

    return fetch(url, {
      ...options,
      signal: controller.signal,
    }).finally(() => {
      clearTimeout(timeoutId);
      if (upstreamSignal && upstreamAbortHandler) {
        upstreamSignal.removeEventListener("abort", upstreamAbortHandler);
      }
    });
  }
}

export function createHttpClient(): HttpClient {
  return new HttpClient();
}
