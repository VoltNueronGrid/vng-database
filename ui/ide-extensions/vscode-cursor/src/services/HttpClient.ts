/**
 * HttpClient: transport-aware communication for the VoltNueronGrid extension.
 *
 * S3-001: HTTP path backed by VoltNueronGridDriver request builders.
 * NT-S4-001 extension: routes to NativeClient when connection transportMode
 * is "native", or when "auto" and a nativeEndpoint is configured.
 */

import { Connection } from "../models/Connection";
import { makeVngDriver, executeDriverRequest, DriverError } from "./DriverAdapter";
import type { HttpExecutionOptions } from "./DriverAdapter";
import { NativeClient } from "./NativeClient";

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
  private readonly native = new NativeClient();

  /**
   * Resolve the effective transport mode for a connection.
   * "native" → always native socket.
   * "auto"   → native when nativeEndpoint is configured, else HTTP.
   * "http"   → always HTTP (default).
   */
  private effectiveTransport(connection: Connection): "http" | "native" {
    const mode = connection.settings.transportMode ?? "http";
    if (mode === "native") {
      return "native";
    }
    if (mode === "auto" && connection.settings.nativeEndpoint?.trim()) {
      return "native";
    }
    return "http";
  }

  /**
   * Execute a query against the server.
   * Routes to native socket transport when transportMode is "native" or
   * "auto" with a configured nativeEndpoint; otherwise uses HTTP.
   */
  async executeQuery(
    connection: Connection,
    query: string,
    options?: QueryRequestOptions
  ): Promise<HttpResponse> {
    if (this.effectiveTransport(connection) === "native") {
      return this.native.executeQuery(connection, query, { timeoutMs: options?.timeoutMs });
    }
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
   * Routes to native socket when transportMode is "native" or effective-auto-native.
   */
  async getSchemaRegistry(connection: Connection): Promise<HttpResponse> {
    if (this.effectiveTransport(connection) === "native") {
      return this.native.getSchemaRegistry(connection);
    }
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
   * Routes to native socket when transportMode is "native" or effective-auto-native;
   * otherwise HTTP with retries suppressed for fast deterministic probes.
   */
  async healthCheck(connection: Connection): Promise<HttpResponse> {
    if (this.effectiveTransport(connection) === "native") {
      return this.native.healthCheck(connection);
    }
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
   * Delegates to NativeClient.testConnection when on native transport.
   */
  async testConnection(connection: Connection): Promise<{ isHealthy: boolean; message: string }> {
    if (this.effectiveTransport(connection) === "native") {
      return this.native.testConnection(connection);
    }
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
