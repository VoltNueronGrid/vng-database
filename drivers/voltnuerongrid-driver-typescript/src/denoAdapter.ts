import type { DriverRequest } from "./index";

export interface DenoExecutionOptions {
  timeoutMs?: number;
}

export interface DenoExecutionResult {
  status: number;
  bodyText: string;
}

/**
 * Minimal Deno-compatible adapter for executing driver-generated HTTP requests.
 * This keeps the core request-building API transport-agnostic.
 */
export async function executeWithDenoFetch(
  request: DriverRequest,
  options: DenoExecutionOptions = {}
): Promise<DenoExecutionResult> {
  const controller = new AbortController();
  const timeoutMs = options.timeoutMs ?? 5000;
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(request.url, {
      method: request.method,
      headers: request.headers,
      body: request.bodyJson,
      signal: controller.signal,
    });
    const bodyText = await response.text();
    return { status: response.status, bodyText };
  } finally {
    clearTimeout(timer);
  }
}
