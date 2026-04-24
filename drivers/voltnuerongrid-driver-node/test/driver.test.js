/**
 * Node built-in test runner tests for @voltnuerongrid/driver-node.
 *
 * Run with:  node --test test/driver.test.js
 */

import { test } from "node:test";
import assert from "node:assert/strict";

import {
  VoltNueronGridDriver,
  DriverError,
  validateConfig,
  isRetryableHttpStatus,
} from "../src/index.js";

// --- Helpers ---

function adminConfig(overrides = {}) {
  return {
    baseUrl: "http://localhost:8080",
    sessionId: "test-session",
    mode: "admin",
    adminApiKey: "secret-key",
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// S10-002 required tests
// ---------------------------------------------------------------------------

test("buildHealthRequest returns correct url", () => {
  const driver = new VoltNueronGridDriver(adminConfig());
  const req = driver.buildHealthRequest();
  assert.equal(req.method, "GET");
  assert.equal(req.url, "http://localhost:8080/health");
});

test("buildSqlExecuteRequest sets method to POST", () => {
  const driver = new VoltNueronGridDriver(adminConfig());
  const req = driver.buildSqlExecuteRequest("SELECT 1");
  assert.equal(req.method, "POST");
  assert.ok(req.url.endsWith("/api/v1/sql/execute"));
  assert.ok(req.bodyJson.includes("sql_batch"));
  assert.ok(req.bodyJson.includes("SELECT 1"));
});

test("validateConfig rejects empty baseUrl", () => {
  const err = validateConfig({ baseUrl: "", sessionId: "s1", mode: "admin", adminApiKey: "k" });
  assert.ok(typeof err === "string");
  assert.ok(err.includes("baseUrl"));
});

test("isRetryableHttpStatus returns true for 503", () => {
  assert.equal(isRetryableHttpStatus(503), true);
});

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

test("isRetryableHttpStatus returns false for 200", () => {
  assert.equal(isRetryableHttpStatus(200), false);
});

test("isRetryableHttpStatus returns false for 404", () => {
  assert.equal(isRetryableHttpStatus(404), false);
});

test("isRetryableHttpStatus returns true for 429", () => {
  assert.equal(isRetryableHttpStatus(429), true);
});

test("buildSchemaRegistryRequest returns GET", () => {
  const driver = new VoltNueronGridDriver(adminConfig());
  const req = driver.buildSchemaRegistryRequest();
  assert.equal(req.method, "GET");
  assert.ok(req.url.endsWith("/api/v1/ingest/schema/registry"));
});

test("buildSqlTransactionRequest sets method to POST", () => {
  const driver = new VoltNueronGridDriver(adminConfig());
  const req = driver.buildSqlTransactionRequest(["INSERT INTO t VALUES(1)", "SELECT 1"]);
  assert.equal(req.method, "POST");
  assert.ok(req.url.endsWith("/api/v1/sql/transaction"));
  const body = JSON.parse(req.bodyJson);
  assert.deepEqual(body.statements, ["INSERT INTO t VALUES(1)", "SELECT 1"]);
});

test("validateConfig rejects admin mode without adminApiKey", () => {
  const err = validateConfig({ baseUrl: "http://localhost", sessionId: "s1", mode: "admin" });
  assert.ok(typeof err === "string");
  assert.ok(err.includes("adminApiKey"));
});

test("validateConfig accepts valid tenant config", () => {
  const err = validateConfig({
    baseUrl: "http://localhost",
    sessionId: "s1",
    mode: "tenant",
    tenantId: "t1",
  });
  assert.equal(err, null);
});

test("admin headers are set correctly", () => {
  const driver = new VoltNueronGridDriver(adminConfig());
  const req = driver.buildHealthRequest();
  assert.equal(req.headers["x-vng-admin-key"], "secret-key");
  assert.equal(req.headers["x-vng-session-id"], "test-session");
});

test("tenant headers set tenantId, not adminKey", () => {
  const driver = new VoltNueronGridDriver({
    baseUrl: "http://localhost:8080",
    sessionId: "s1",
    mode: "tenant",
    tenantId: "tenant-42",
  });
  const req = driver.buildHealthRequest();
  assert.equal(req.headers["x-vng-tenant-id"], "tenant-42");
  assert.equal(req.headers["x-vng-admin-key"], undefined);
});

test("DriverError.validation has kind validation", () => {
  const err = DriverError.validation("bad input");
  assert.equal(err.kind, "validation");
  assert.ok(err instanceof DriverError);
  assert.ok(err instanceof Error);
});

test("constructor throws DriverError for invalid config", () => {
  assert.throws(
    () => new VoltNueronGridDriver({ baseUrl: "", sessionId: "s1", mode: "admin", adminApiKey: "k" }),
    (err) => err instanceof DriverError && err.kind === "validation"
  );
});
