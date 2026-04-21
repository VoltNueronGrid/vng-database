/**
 * HttpClient: HTTP communication backed by the VoltNueronGrid TypeScript driver.
 *
 * S3-001: replaced ad-hoc fetch() with VoltNueronGridDriver request builders +
 * performDriverHttpRequest so auth headers, timeout, and retry logic are owned
 * by the shared driver package, not duplicated in the extension.
 */

import { Connection } from "../models/Connection";
import { makeVngDriver, executeDriverRequest, DriverError } from "./DriverAdapter";
import type { HttpExecutionOptions } from "./DriverAdapter";

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
  /**
   * Execute a query against the server.
   * Uses VoltNueronGridDriver.buildSqlExecuteRequest so the request format
   * is owned by the shared driver-core-contract v1.
   */
  async executeQuery(
    connection: Connection,
    query: string,
    options?: QueryRequestOptions
  ): Promise<HttpResponse> {
    try {
      const driver = makeVngDriver(connection);
      const req = driver.buildSqlExecuteRequest(query);
      const execOpts: HttpExecutionOptions = {};
      if (options?.timeoutMs !== undefined) {
        execOpts.timeoutMs = options.timeoutMs;
      }
      if (options?.signal !== undefined) {
        execOpts.abortSignal = options.signal;
      }
      const result = await executeDriverRequest(req, execOpts);
      return this.toHttpResponse(result);
    } catch (err) {
      return this.toErrorResponse(err);
    }
  }

  /**
   * Get schema registry.
   * Uses VoltNueronGridDriver.buildSchemaRegistryRequest.
   */
  async getSchemaRegistry(connection: Connection): Promise<HttpResponse> {
    try {
      const driver = makeVngDriver(connection);
      const req = driver.buildSchemaRegistryRequest();
      const result = await executeDriverRequest(req);
      return this.toHttpResponse(result);
    } catch (err) {
      return this.toErrorResponse(err);
    }
  }

  /**
   * Health check.
   * Uses VoltNueronGridDriver.buildHealthRequest; retries suppressed (maxRetries=0)
   * so health probes are fast and deterministic.
   */
  async healthCheck(connection: Connection): Promise<HttpResponse> {
    try {
      const driver = makeVngDriver(connection);
      const req = driver.buildHealthRequest();
      const result = await executeDriverRequest(req, { maxRetries: 0 });
      return this.toHttpResponse(result);
    } catch (err) {
      return this.toErrorResponse(err);
    }
  }

  /**
   * Test connection: returns structured result for UI display.
   */
  async testConnection(connection: Connection): Promise<{ isHealthy: boolean; message: string }> {
    try {
      const response = await this.healthCheck(connection);
      if (response.status === 200) {
        return { isHealthy: true, message: "Connection successful" };
      }
      if (response.error) {
        return { isHealthy: false, message: response.error };
      }
      return { isHealthy: false, message: `Server returned status ${response.status}` };
    } catch (error) {
      const message = error instanceof Error ? error.message : "Unknown error";
      return { isHealthy: false, message };
    }
  }

  /**
   * Converts a driver HttpExecutionResult to the extension's HttpResponse shape.
   * Response headers are not available from the driver layer; callers that need
   * specific headers should migrate to direct driver calls.
   */
  private toHttpResponse(result: { status: number; bodyText: string }): HttpResponse {
    let data: unknown;
    if (result.bodyText) {
      try {
        data = JSON.parse(result.bodyText);
      } catch {
        data = result.bodyText;
      }
    }
    return { status: result.status, data, headers: {} };
  }

  /**
   * Wraps any thrown error (DriverError or otherwise) into an HttpResponse error shape.
   */
  private toErrorResponse(err: unknown): HttpResponse {
    if (err instanceof DriverError) {
      return { status: err.statusCode ?? 0, error: err.message, headers: {} };
    }
    const message = err instanceof Error ? err.message : "Unknown error";
    return { status: 0, error: message, headers: {} };
  }
}

export function createHttpClient(): HttpClient {
  return new HttpClient();
}
