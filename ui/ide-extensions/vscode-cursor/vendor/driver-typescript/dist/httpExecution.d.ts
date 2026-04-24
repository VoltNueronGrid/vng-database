/** Same shape as `DriverRequest` in `index.ts` (avoid circular imports). */
export interface HttpDriverRequest {
    method: "GET" | "POST";
    url: string;
    headers: Record<string, string>;
    bodyJson?: string;
}
/** Aligns with Rust `DEFAULT_HTTP_REQUEST_TIMEOUT_MS` / driver-core-contract v1. */
export declare const DEFAULT_HTTP_REQUEST_TIMEOUT_MS = 30000;
/** Aligns with Rust `DEFAULT_HTTP_MAX_RETRIES` / driver-core-contract v1. */
export declare const DEFAULT_HTTP_MAX_RETRIES = 2;
export type DriverErrorKind = "validation" | "transport" | "http_status" | "serialization" | "timeout" | "cancelled";
/** Typed error envelope (driver-core-contract §6). */
export declare class DriverError extends Error {
    readonly kind: DriverErrorKind;
    readonly statusCode?: number | undefined;
    readonly requestId?: string | undefined;
    constructor(kind: DriverErrorKind, message: string, statusCode?: number | undefined, requestId?: string | undefined);
    static validation(message: string): DriverError;
    static transport(message: string): DriverError;
    static httpStatus(statusCode: number, message: string, requestId?: string): DriverError;
    static timeout(message: string): DriverError;
    static cancelled(message: string): DriverError;
}
/** Matches Rust `is_retryable_http_status`. */
export declare function isRetryableHttpStatus(status: number): boolean;
export interface HttpExecutionOptions {
    /** Wall-clock timeout per attempt (AbortSignal on fetch). */
    timeoutMs?: number;
    /** Retries after transient HTTP statuses or network errors (not after 4xx except retryable list). */
    maxRetries?: number;
    /** When aborted, throws `DriverError` with kind `cancelled`. */
    abortSignal?: AbortSignal;
    fetchFn?: typeof fetch;
}
export interface HttpExecutionResult {
    status: number;
    bodyText: string;
}
/**
 * Executes a built [`DriverRequest`] over HTTP with timeout + retry hooks (driver-core-contract §5 hooks).
 * Idempotent GET-style calls are safe; for POST bodies ensure callers only enable retries when appropriate.
 */
export declare function performDriverHttpRequest(req: HttpDriverRequest, opts?: HttpExecutionOptions): Promise<HttpExecutionResult>;
