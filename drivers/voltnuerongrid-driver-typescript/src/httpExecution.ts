/** Same shape as `DriverRequest` in `index.ts` (avoid circular imports). */
export interface HttpDriverRequest {
  method: "GET" | "POST";
  url: string;
  headers: Record<string, string>;
  bodyJson?: string;
}

/** Aligns with Rust `DEFAULT_HTTP_REQUEST_TIMEOUT_MS` / driver-core-contract v1. */
export const DEFAULT_HTTP_REQUEST_TIMEOUT_MS = 30_000;

/** Aligns with Rust `DEFAULT_HTTP_MAX_RETRIES` / driver-core-contract v1. */
export const DEFAULT_HTTP_MAX_RETRIES = 2;

export type DriverErrorKind =
  | "validation"
  | "transport"
  | "http_status"
  | "serialization"
  | "timeout"
  | "cancelled";

/** Typed error envelope (driver-core-contract §6). */
export class DriverError extends Error {
  constructor(
    public readonly kind: DriverErrorKind,
    message: string,
    public readonly statusCode?: number,
    public readonly requestId?: string
  ) {
    super(message);
    this.name = "DriverError";
    Object.setPrototypeOf(this, new.target.prototype);
  }

  static validation(message: string): DriverError {
    return new DriverError("validation", message);
  }

  static transport(message: string): DriverError {
    return new DriverError("transport", message);
  }

  static httpStatus(statusCode: number, message: string, requestId?: string): DriverError {
    return new DriverError("http_status", message, statusCode, requestId);
  }

  static timeout(message: string): DriverError {
    return new DriverError("timeout", message);
  }

  static cancelled(message: string): DriverError {
    return new DriverError("cancelled", message);
  }
}

/** Matches Rust `is_retryable_http_status`. */
export function isRetryableHttpStatus(status: number): boolean {
  return [408, 425, 429, 500, 502, 503, 504].includes(status);
}

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

function sleepMs(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Executes a built [`DriverRequest`] over HTTP with timeout + retry hooks (driver-core-contract §5 hooks).
 * Idempotent GET-style calls are safe; for POST bodies ensure callers only enable retries when appropriate.
 */
export async function performDriverHttpRequest(
  req: HttpDriverRequest,
  opts?: HttpExecutionOptions
): Promise<HttpExecutionResult> {
  const timeoutMs = opts?.timeoutMs ?? DEFAULT_HTTP_REQUEST_TIMEOUT_MS;
  const maxRetries = opts?.maxRetries ?? DEFAULT_HTTP_MAX_RETRIES;
  const fetchFn = opts?.fetchFn ?? fetch;

  const outerAbort = opts?.abortSignal;
  let lastError: unknown;

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    if (outerAbort?.aborted) {
      throw DriverError.cancelled("request aborted");
    }

    const controller = new AbortController();
    const onOuterAbort = (): void => controller.abort();
    if (outerAbort) {
      outerAbort.addEventListener("abort", onOuterAbort, { once: true });
    }
    const timer = setTimeout(() => controller.abort(), timeoutMs);

    try {
      const res = await fetchFn(req.url, {
        method: req.method,
        headers: req.headers,
        body: req.method === "POST" && req.bodyJson !== undefined ? req.bodyJson : undefined,
        signal: controller.signal
      });
      clearTimeout(timer);
      if (outerAbort) {
        outerAbort.removeEventListener("abort", onOuterAbort);
      }

      const bodyText = await res.text();
      if (isRetryableHttpStatus(res.status) && attempt < maxRetries) {
        await sleepMs(Math.min(250 * 2 ** attempt, 2000));
        lastError = DriverError.httpStatus(res.status, `retryable HTTP ${res.status}`);
        continue;
      }
      return { status: res.status, bodyText };
    } catch (err: unknown) {
      clearTimeout(timer);
      if (outerAbort) {
        outerAbort.removeEventListener("abort", onOuterAbort);
      }

      if (err instanceof Error && err.name === "AbortError") {
        if (outerAbort?.aborted) {
          throw DriverError.cancelled("request aborted");
        }
        throw DriverError.timeout(`request timed out after ${timeoutMs}ms`);
      }

      lastError = err;
      const msg = err instanceof Error ? err.message : String(err);
      if (attempt < maxRetries) {
        await sleepMs(Math.min(250 * 2 ** attempt, 2000));
        continue;
      }
      throw DriverError.transport(`HTTP request failed: ${msg}`);
    }
  }

  if (lastError instanceof DriverError) {
    throw lastError;
  }
  throw DriverError.transport("exhausted HTTP retries");
}
