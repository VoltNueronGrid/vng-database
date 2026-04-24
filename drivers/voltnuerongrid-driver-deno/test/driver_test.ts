/**
 * Deno tests for the VoltNueronGrid Deno adapter.
 *
 * Run:  deno test --allow-net test/driver_test.ts
 */

import { assertEquals, assertThrows, assert } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  VoltNueronGridDriver,
  validateConfig,
  isRetryableHttpStatus,
  DriverError,
} from "../mod.ts";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function adminConfig() {
  return {
    baseUrl: "http://localhost:8080",
    sessionId: "test-session",
    mode: "admin" as const,
    adminApiKey: "secret-key",
  };
}

// ---------------------------------------------------------------------------
// S10-004 required tests
// ---------------------------------------------------------------------------

Deno.test("buildHealthRequest returns GET", () => {
  const driver = new VoltNueronGridDriver(adminConfig());
  const req = driver.buildHealthRequest();
  assertEquals(req.method, "GET");
  assertEquals(req.url, "http://localhost:8080/health");
});

Deno.test("validateConfig rejects empty baseUrl", () => {
  const err = validateConfig({
    baseUrl: "",
    sessionId: "s1",
    mode: "admin",
    adminApiKey: "k",
  });
  assert(typeof err === "string");
  assert(err.includes("baseUrl"));
});

Deno.test("isRetryableHttpStatus 503", () => {
  assertEquals(isRetryableHttpStatus(503), true);
});

// ---------------------------------------------------------------------------
// Additional tests
// ---------------------------------------------------------------------------

Deno.test("buildHealthRequest url has /health suffix", () => {
  const driver = new VoltNueronGridDriver(adminConfig());
  const req = driver.buildHealthRequest();
  assert(req.url.endsWith("/health"));
});

Deno.test("buildSqlExecuteRequest sets method POST", () => {
  const driver = new VoltNueronGridDriver(adminConfig());
  const req = driver.buildSqlExecuteRequest("SELECT 1");
  assertEquals(req.method, "POST");
  assert(req.url.endsWith("/api/v1/sql/execute"));
  assert(req.bodyJson !== undefined);
  assert(req.bodyJson!.includes("sql_batch"));
});

Deno.test("buildSchemaRegistryRequest returns GET", () => {
  const driver = new VoltNueronGridDriver(adminConfig());
  const req = driver.buildSchemaRegistryRequest();
  assertEquals(req.method, "GET");
  assert(req.url.endsWith("/api/v1/ingest/schema/registry"));
});

Deno.test("buildSqlTransactionRequest serialises statements array", () => {
  const driver = new VoltNueronGridDriver(adminConfig());
  const req = driver.buildSqlTransactionRequest(["INSERT INTO t VALUES(1)", "SELECT 1"]);
  assertEquals(req.method, "POST");
  const body = JSON.parse(req.bodyJson!);
  assertEquals(body.statements, ["INSERT INTO t VALUES(1)", "SELECT 1"]);
});

Deno.test("isRetryableHttpStatus returns false for 200", () => {
  assertEquals(isRetryableHttpStatus(200), false);
});

Deno.test("isRetryableHttpStatus returns true for 429", () => {
  assertEquals(isRetryableHttpStatus(429), true);
});

Deno.test("DriverError.validation has correct kind", () => {
  const err = DriverError.validation("bad config");
  assertEquals(err.kind, "validation");
  assert(err instanceof DriverError);
});

Deno.test("admin mode headers include x-vng-admin-key", () => {
  const driver = new VoltNueronGridDriver(adminConfig());
  const req = driver.buildHealthRequest();
  assertEquals(req.headers["x-vng-admin-key"], "secret-key");
  assertEquals(req.headers["x-vng-session-id"], "test-session");
});

Deno.test("validateConfig rejects admin mode without adminApiKey", () => {
  const err = validateConfig({ baseUrl: "http://localhost", sessionId: "s1", mode: "admin" });
  assert(typeof err === "string");
  assert(err.includes("adminApiKey"));
});

Deno.test("validateConfig accepts valid tenant config", () => {
  const err = validateConfig({
    baseUrl: "http://localhost",
    sessionId: "s1",
    mode: "tenant",
    tenantId: "t1",
  });
  assertEquals(err, null);
});
