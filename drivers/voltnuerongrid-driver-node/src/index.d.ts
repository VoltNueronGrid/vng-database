/**
 * TypeScript declarations for @voltnuerongrid/driver-node
 */

export type DriverMode = "admin" | "operator" | "tenant";

export type DriverErrorKind =
  | "validation"
  | "transport"
  | "http_status"
  | "timeout"
  | "cancelled";

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

export interface HttpExecutionOptions {
  timeoutMs?: number;
  maxRetries?: number;
  abortSignal?: AbortSignal;
}

export interface HttpExecutionResult {
  status: number;
  bodyText: string;
}

export declare class DriverError extends Error {
  readonly kind: DriverErrorKind;
  readonly statusCode: number | undefined;

  constructor(kind: DriverErrorKind, message: string, statusCode?: number);

  static validation(message: string): DriverError;
  static transport(message: string): DriverError;
  static httpStatus(statusCode: number, message: string): DriverError;
  static timeout(message: string): DriverError;
  static cancelled(message: string): DriverError;
}

export declare class VoltNueronGridDriver {
  constructor(config: DriverConfig);

  buildHealthRequest(): DriverRequest;
  buildSqlExecuteRequest(sqlBatch: string): DriverRequest;
  buildSqlAnalyzeRequest(sqlBatch: string): DriverRequest;
  buildSqlRouteRequest(sqlBatch: string): DriverRequest;
  buildSqlTransactionRequest(statements: string[]): DriverRequest;
  buildSchemaRegistryRequest(): DriverRequest;
}

export declare function validateConfig(config: DriverConfig): string | null;
export declare function isRetryableHttpStatus(status: number): boolean;
export declare function performDriverHttpRequest(
  req: DriverRequest,
  opts?: HttpExecutionOptions
): Promise<HttpExecutionResult>;

export declare const DEFAULT_HTTP_REQUEST_TIMEOUT_MS: number;
export declare const DEFAULT_HTTP_MAX_RETRIES: number;
